//! SSH password login: the opt-in switch, its two failure limiters and the pool the slow
//! hashing runs on (M3-D, D28, D79). The rules themselves (what a login must check, the dummy
//! hash, the upgrade) are `russh_bbs_core::passwords`; this owns only what is per-server.

use std::future::Future;
use std::net::IpAddr;
use std::sync::Arc;

use russh_bbs_core::passwords::{Offload, Unavailable, login_handle};
use russh_bbs_core::waiting_room::{KeyedLimiter, SystemClock, Verdict, client_network};
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
    /// Attempts per handle, typed or not: an unknown name is limited exactly like a member's, so
    /// a limited reply says nothing about who exists.
    accounts: KeyedLimiter<String, SystemClock>,
    networks: KeyedLimiter<IpAddr, SystemClock>,
    pool: HashPool,
}

impl PasswordLogin {
    pub(crate) fn new(limits: &Limits) -> Self {
        let clock = SystemClock::new();
        Self {
            enabled: limits.allow_password_login,
            accounts: KeyedLimiter::new(
                limits.password_account_rate,
                limits.max_tracked_sources,
                clock,
            ),
            networks: KeyedLimiter::new(
                limits.password_network_rate,
                limits.max_tracked_sources,
                clock,
            ),
            pool: HashPool::new(),
        }
    }

    /// Whether the sysop turned password login on. When off, the method is not offered at all.
    pub(crate) const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Count one attempt by `peer` for `user`. The network is charged first, then the handle;
    /// an attempt over either limit is refused before any hashing.
    pub(crate) fn admit(&self, peer: Option<IpAddr>, user: &str) -> Verdict {
        if let Some(ip) = peer
            && let limited @ Verdict::Limited { .. } = self.networks.check(&client_network(ip))
        {
            return limited;
        }
        self.accounts.check(&login_handle(user).unwrap_or_default())
    }

    /// Where the slow hashing runs.
    pub(crate) const fn pool(&self) -> &HashPool {
        &self.pool
    }
}
