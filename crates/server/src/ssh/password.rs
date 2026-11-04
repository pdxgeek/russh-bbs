//! SSH password login: the opt-in switch, its two failure limiters and the pool the slow
//! hashing runs on (M3-D, D28, D81). The rules themselves (what a login must check, the dummy
//! hash, the upgrade) are `russh_bbs_core::passwords`; this owns only what is per-server.

use std::future::Future;
use std::net::IpAddr;
use std::sync::Arc;

use russh_bbs_core::passwords::{AttemptGate, Offload, Unavailable};
use russh_bbs_core::waiting_room::{SystemClock, Verdict};
use tokio::sync::Semaphore;

use super::limits::Limits;

/// Most argon2id hashes running at once. Each holds about 19 MiB, so this is also the memory
/// ceiling of password login (about 76 MiB). A login that finds the pool busy waits its turn
/// inside the authentication deadline.
const MAX_CONCURRENT_HASHES: usize = 4;

/// Runs hashing on tokio's blocking pool, a few at a time.
#[derive(Debug, Clone)]
pub(crate) struct HashPool {
    permits: Arc<Semaphore>,
}

impl HashPool {
    fn new() -> Self {
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        Self {
            permits: Arc::new(Semaphore::new(cores.clamp(1, MAX_CONCURRENT_HASHES))),
        }
    }
}

impl Offload for HashPool {
    fn run<T, F>(&self, work: F) -> impl Future<Output = Result<T, Unavailable>> + Send
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let permits = Arc::clone(&self.permits);
        async move {
            let _turn = permits.acquire_owned().await.map_err(|_| Unavailable)?;
            tokio::task::spawn_blocking(work)
                .await
                .map_err(|_| Unavailable)
        }
    }
}

/// Everything per-server that password login needs.
#[derive(Debug)]
pub(crate) struct PasswordLogin {
    enabled: bool,
    /// The backoff for wrong passwords, per handle and per network (D100). A handle that does
    /// not exist is limited exactly like a member's, so a limited reply says nothing about who
    /// exists. Keys never come through here.
    gate: AttemptGate<SystemClock>,
    pool: HashPool,
}

impl PasswordLogin {
    pub(crate) fn new(limits: &Limits) -> Self {
        Self {
            enabled: limits.allow_password_login,
            gate: AttemptGate::new(
                limits.password_handle_backoff,
                limits.password_network_backoff,
                limits.max_tracked_sources,
                SystemClock::new(),
            ),
            pool: HashPool::new(),
        }
    }

    /// Whether the sysop turned password login on. When off, the method is not offered at all.
    pub(crate) const fn enabled(&self) -> bool {
        self.enabled
    }

    /// May `peer` try a password for `user` now? Counts nothing: the attempt is counted by
    /// [`failed`](Self::failed) once it is known to be wrong. An attempt over either limit is
    /// refused before any hashing.
    pub(crate) fn admit(&self, peer: Option<IpAddr>, user: &str) -> Verdict {
        self.gate
            .admit(peer, &AttemptGate::<SystemClock>::handle_key(user))
    }

    /// `peer` tried a wrong password for `user`.
    pub(crate) fn failed(&self, peer: Option<IpAddr>, user: &str) {
        self.gate
            .failed(peer, &AttemptGate::<SystemClock>::handle_key(user));
    }

    /// The right password was given for `user`: the handle's failures are forgotten.
    pub(crate) fn succeeded(&self, user: &str) {
        self.gate
            .succeeded(&AttemptGate::<SystemClock>::handle_key(user));
    }

    /// Where the slow hashing runs.
    pub(crate) const fn pool(&self) -> &HashPool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use std::net::IpAddr;

    use russh_bbs_core::waiting_room::Verdict;

    use super::{Limits, PasswordLogin};

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn a_network_hammering_one_handle_hits_the_network_limit_before_the_handle_waits() {
        let login = PasswordLogin::new(&Limits::default());
        let attacker = Some(ip("198.51.100.7"));
        let owner = Some(ip("203.0.113.5"));
        // The default network allowance is 3 free failures, the handle's 4.
        for _ in 0..4 {
            assert!(login.admit(attacker, "Carol").is_allowed());
            login.failed(attacker, "Carol");
        }
        assert!(
            !login.admit(attacker, "carol").is_allowed(),
            "network waits"
        );
        assert!(
            login.admit(owner, "carol").is_allowed(),
            "the handle has not started to wait, so the owner from elsewhere signs in"
        );
    }

    #[test]
    fn a_limited_answer_is_the_same_for_a_known_and_an_unknown_name() {
        let login = PasswordLogin::new(&Limits::default());
        let peer = Some(ip("198.51.100.7"));
        for _ in 0..4 {
            login.failed(peer, "carol");
            login.failed(peer, "ghost");
        }
        let (Verdict::Limited { .. }, Verdict::Limited { .. }) =
            (login.admit(peer, "carol"), login.admit(peer, "ghost"))
        else {
            panic!("both limited");
        };
    }
}
