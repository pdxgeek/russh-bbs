//! Rate limiting for strangers: a keyed limiter with bounded memory and an injectable clock.
//!
//! The algorithm is GCRA (the "generic cell rate algorithm", a token bucket in disguise): each
//! source stores one number, the *theoretical arrival time* of its next conforming request. A
//! request is allowed while that time is no more than `(burst - 1) * interval` in the future.
//! One duration per key keeps the state small and the arithmetic integer-only.
//!
//! **Bounded memory.** The table never holds more than `capacity` sources. When it is full a
//! new source first purges the entries that are idle (an idle entry is the same as an absent
//! one), and if that is not enough drops the oldest eighth. The cost of that last step is that
//! a flood of distinct sources can reset some limited sources early; the memory bound matters
//! more than a perfect memory (decision D37).
//!
//! **Time.** The limiter reads a [`Clock`], so tests advance a [`ManualClock`] instead of
//! sleeping.

use std::collections::HashMap;
use std::hash::Hash;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// A monotonic clock, as time since an arbitrary origin.
pub trait Clock: Send + Sync {
    /// Time elapsed since the clock's origin. Never goes backwards.
    fn now(&self) -> Duration;
}

/// The real clock: monotonic time since it was created.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// A clock whose origin is now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

/// A clock that only moves when told to. Clones share the same time.
#[derive(Debug, Clone, Default)]
pub struct ManualClock(Arc<AtomicU64>);

impl ManualClock {
    /// A clock at time zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Move time forward.
    pub fn advance(&self, by: Duration) {
        let millis = u64::try_from(by.as_millis()).unwrap_or(u64::MAX);
        self.0.fetch_add(millis, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::SeqCst))
    }
}

/// How fast one source may act: a burst allowance that refills at one action per `interval`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RatePolicy {
    /// Actions allowed back to back. At least 1.
    pub burst: u32,
    /// Time to earn back one action. At least 1 ms.
    pub interval: Duration,
}

impl RatePolicy {
    /// A policy of `burst` actions, refilling one per `interval`.
    #[must_use]
    pub const fn new(burst: u32, interval: Duration) -> Self {
        Self { burst, interval }
    }
}

/// The answer to "may this source act now?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Go ahead; the action was counted.
    Allowed,
    /// Too fast; nothing was counted. Retry after the given wait.
    Limited {
        /// How long until one action would be allowed.
        retry_after: Duration,
    },
}

impl Verdict {
    /// Whether the action may proceed.
    #[must_use]
    pub fn is_allowed(self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// A rate limiter with one independent allowance per key.
#[derive(Debug)]
pub struct KeyedLimiter<K, C> {
    interval: Duration,
    /// How far ahead of now a source's arrival time may run: `(burst - 1) * interval`.
    tolerance: Duration,
    capacity: usize,
    clock: C,
    /// Theoretical arrival time per source. A std mutex, taken only in non-async code.
    state: Mutex<HashMap<K, Duration>>,
}

impl<K: Eq + Hash + Clone, C: Clock> KeyedLimiter<K, C> {
    /// A limiter for `policy` that tracks at most `capacity` sources (at least 1).
    #[must_use]
    pub fn new(policy: RatePolicy, capacity: usize, clock: C) -> Self {
        let interval = policy.interval.max(Duration::from_millis(1));
        let extra = policy.burst.max(1) - 1;
        Self {
            interval,
            tolerance: interval.saturating_mul(extra),
            capacity: capacity.max(1),
            clock,
            state: Mutex::new(HashMap::new()),
        }
    }

    /// Count one action by `key` if the policy allows it.
    pub fn check(&self, key: &K) -> Verdict {
        let now = self.clock.now();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let arrival = state.get(key).copied().unwrap_or(now).max(now);
        let ahead = arrival.saturating_sub(now);
        if ahead > self.tolerance {
            return Verdict::Limited {
                retry_after: ahead.saturating_sub(self.tolerance),
            };
        }
        if !state.contains_key(key) && state.len() >= self.capacity {
            make_room(&mut state, now, self.capacity);
        }
        state.insert(key.clone(), arrival.saturating_add(self.interval));
        Verdict::Allowed
    }

    /// How many sources are tracked right now (for tests and metrics).
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// Free at least one slot: drop idle entries, then if still full the oldest eighth.
fn make_room<K: Eq + Hash + Clone>(
    state: &mut HashMap<K, Duration>,
    now: Duration,
    capacity: usize,
) {
    // An entry whose arrival time has passed is back at full burst: same as having no entry.
    state.retain(|_, arrival| *arrival > now);
    if state.len() < capacity {
        return;
    }
    let drop_count = capacity / 8 + 1;
    let mut by_age: Vec<(Duration, K)> = state.iter().map(|(k, t)| (*t, k.clone())).collect();
    by_age.sort_unstable_by_key(|(arrival, _)| *arrival);
    for (_, key) in by_age.into_iter().take(drop_count) {
        state.remove(&key);
    }
}

/// The network a client address is limited as: an IPv4 address on its own, an IPv6 address by
/// its /64 (one customer commonly holds a whole /64, so per-address limits would be free to
/// evade). IPv4-mapped IPv6 addresses count as the IPv4 address.
#[must_use]
pub fn client_network(addr: IpAddr) -> IpAddr {
    match addr.to_canonical() {
        IpAddr::V4(v4) => IpAddr::V4(v4),
        IpAddr::V6(v6) => {
            let high = u128::from(v6) & !u128::from(u64::MAX);
            IpAddr::V6(Ipv6Addr::from(high))
        }
    }
}

#[cfg(test)]
mod tests;
