//! How many wrong passwords a handle and a network may try (D100): a gentle per-handle backoff
//! and a firmer per-network one. Only wrong passwords count, and a correct one clears the
//! handle. Keys never come through here, so a key bound to an account always signs in.
//!
//! Each table keeps, per key, the number of recent failures and the time before which the next
//! attempt is refused. The wait grows by doubling once the free failures are used up and stops at
//! a ceiling; an entry that has been quiet for `forget` is dropped. Refused attempts cost
//! nothing and do not lengthen the wait, so an attacker cannot push the wait past the ceiling by
//! knocking while it runs. The tables are bounded: when full they drop idle entries, then the
//! oldest eighth. The time comes from a [`Clock`], so tests move a [`ManualClock`] instead of
//! sleeping.
//!
//! [`ManualClock`]: crate::waiting_room::ManualClock

use std::collections::HashMap;
use std::hash::Hash;
use std::net::IpAddr;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use super::service::login_handle;
use crate::waiting_room::{Clock, Verdict, client_network};

/// The shape of one backoff: some free failures, then a wait that doubles up to a ceiling.
/// Four `u32`s, so a `Limits` that holds two stays small.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    /// Wrong passwords allowed back to back with no wait.
    pub free: u32,
    /// The wait after the first failure past `free`, in milliseconds; each further failure
    /// doubles it.
    pub first_ms: u32,
    /// The longest wait there is, in milliseconds.
    pub ceiling_ms: u32,
    /// How long without a failure, in seconds, before the count starts again from zero.
    pub forget_secs: u32,
}

impl Backoff {
    /// No limit at all (for tests that are about something else).
    pub const UNLIMITED: Self = Self {
        free: u32::MAX,
        first_ms: 0,
        ceiling_ms: 0,
        forget_secs: 0,
    };

    /// The wait that follows the `failures`-th wrong password in a row.
    #[must_use]
    pub fn wait_after(&self, failures: u32) -> Duration {
        let Some(step) = failures
            .checked_sub(self.free)
            .and_then(|n| n.checked_sub(1))
        else {
            return Duration::ZERO;
        };
        // 2^20 times any u32 of milliseconds fits a u64 and is far past every ceiling.
        let millis = u64::from(self.first_ms) << step.min(20);
        Duration::from_millis(millis.min(u64::from(self.ceiling_ms)))
    }

    /// How long a quiet key is remembered.
    #[must_use]
    pub const fn forget(&self) -> Duration {
        Duration::from_secs(self.forget_secs as u64)
    }
}

/// Defaults per handle (D100): 4 free failures, then 1, 2, 4, 8, 16 and 30 s (the ceiling), and
/// forgotten after 10 quiet minutes. A person who mistypes is held for seconds at worst, and the
/// 12-character minimum makes about two guesses a minute hopeless.
pub const DEFAULT_HANDLE_BACKOFF: Backoff = Backoff {
    free: 4,
    first_ms: 1_000,
    ceiling_ms: 30_000,
    forget_secs: 600,
};

/// Defaults per network, firmer than the handle's (D100): 3 free failures across all handles,
/// then 5, 10, 20 s and on up to 5 minutes, and forgotten after 10 quiet minutes. One address
/// cannot sweep handles or keep one shut: it reaches its own wait before the handle's wait grows.
pub const DEFAULT_NETWORK_BACKOFF: Backoff = Backoff {
    free: 3,
    first_ms: 5_000,
    ceiling_ms: 300_000,
    forget_secs: 600,
};

#[derive(Debug, Clone, Copy)]
struct Entry {
    failures: u32,
    blocked_until: Duration,
    last: Duration,
}

/// Failures per key under one [`Backoff`].
#[derive(Debug)]
struct Table<K> {
    policy: Backoff,
    capacity: usize,
    state: Mutex<HashMap<K, Entry>>,
}

impl<K: Eq + Hash + Clone> Table<K> {
    fn new(policy: Backoff, capacity: usize) -> Self {
        Self {
            policy,
            capacity: capacity.max(1),
            state: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<K, Entry>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How long `key` must still wait, if at all.
    fn remaining(&self, key: &K, now: Duration) -> Option<Duration> {
        let entry = *self.lock().get(key)?;
        entry
            .blocked_until
            .checked_sub(now)
            .filter(|left| !left.is_zero())
    }

    fn fail(&self, key: &K, now: Duration) {
        let mut state = self.lock();
        let quiet = |entry: &Entry| now.saturating_sub(entry.last) >= self.policy.forget();
        let before = match state.get(key) {
            Some(entry) if !quiet(entry) => entry.failures,
            _ => 0,
        };
        if before == 0 && !state.contains_key(key) && state.len() >= self.capacity {
            self.make_room(&mut state, now);
        }
        let failures = before.saturating_add(1);
        state.insert(
            key.clone(),
            Entry {
                failures,
                blocked_until: now.saturating_add(self.policy.wait_after(failures)),
                last: now,
            },
        );
    }

    fn clear(&self, key: &K) {
        self.lock().remove(key);
    }

    /// Free a slot: drop quiet entries, then if still full the oldest eighth.
    fn make_room(&self, state: &mut HashMap<K, Entry>, now: Duration) {
        state.retain(|_, entry| now.saturating_sub(entry.last) < self.policy.forget());
        if state.len() < self.capacity {
            return;
        }
        let mut by_age: Vec<(Duration, K)> =
            state.iter().map(|(k, e)| (e.last, k.clone())).collect();
        by_age.sort_unstable_by_key(|(last, _)| *last);
        for (_, key) in by_age.into_iter().take(self.capacity / 8 + 1) {
            state.remove(&key);
        }
    }

    fn tracked(&self) -> usize {
        self.lock().len()
    }
}

/// The one gate every password attempt goes through, on both transports: a sign-in by password,
/// "I already have an account", the code forms and a password session giving its current
/// password again. Ask [`admit`](Self::admit) before hashing anything, then report the outcome
/// with [`failed`](Self::failed) or [`succeeded`](Self::succeeded).
#[derive(Debug)]
pub struct AttemptGate<C> {
    handles: Table<String>,
    networks: Table<IpAddr>,
    clock: C,
}

impl<C: Clock> AttemptGate<C> {
    /// A gate with `handle` and `network` policies tracking at most `capacity` keys each.
    #[must_use]
    pub fn new(handle: Backoff, network: Backoff, capacity: usize, clock: C) -> Self {
        Self {
            handles: Table::new(handle, capacity),
            networks: Table::new(network, capacity),
            clock,
        }
    }

    /// The key a typed user name is limited as. Unknown names get one exactly like members', so
    /// a limited reply says nothing about who exists; text that cannot be a handle shares one key.
    #[must_use]
    pub fn handle_key(user: &str) -> String {
        login_handle(user).unwrap_or_default()
    }

    /// May `peer` try a password for `key` now? Counts nothing. The network is asked first, then
    /// the handle; either way the caller gives the same plain refusal.
    pub fn admit(&self, peer: Option<IpAddr>, key: &str) -> Verdict {
        let now = self.clock.now();
        let network = peer.and_then(|ip| self.networks.remaining(&client_network(ip), now));
        let handle = self.handles.remaining(&key.to_owned(), now);
        network
            .or(handle)
            .map_or(Verdict::Allowed, |retry_after| Verdict::Limited {
                retry_after,
            })
    }

    /// A wrong password was tried by `peer` for `key`.
    pub fn failed(&self, peer: Option<IpAddr>, key: &str) {
        let now = self.clock.now();
        if let Some(ip) = peer {
            self.networks.fail(&client_network(ip), now);
        }
        self.handles.fail(&key.to_owned(), now);
    }

    /// The right password was given for `key`: its failures are forgotten. The network's are not,
    /// or an attacker with an account of their own could wipe their record with every login.
    pub fn succeeded(&self, key: &str) {
        self.handles.clear(&key.to_owned());
    }

    /// How many handles and networks are tracked (for tests).
    #[must_use]
    pub fn tracked(&self) -> (usize, usize) {
        (self.handles.tracked(), self.networks.tracked())
    }
}
