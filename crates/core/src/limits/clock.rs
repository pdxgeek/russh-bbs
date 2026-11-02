//! Time, injected so tests never sleep and the rules never read the wall clock themselves.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::store::UnixSeconds;

/// Seconds in a UTC day. Usage is counted per UTC day so every member's day ends together.
pub const SECONDS_PER_DAY: i64 = 86_400;

/// A day number: whole days since 1970-01-01 UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day(i64);

impl Day {
    /// The day containing `at`.
    #[must_use]
    pub const fn of(at: UnixSeconds) -> Self {
        Self(at.as_raw().div_euclid(SECONDS_PER_DAY))
    }

    /// From a stored number.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// The number to store.
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }

    /// The first second of this day.
    #[must_use]
    pub const fn start(self) -> UnixSeconds {
        UnixSeconds::from_raw(self.0.saturating_mul(SECONDS_PER_DAY))
    }

    /// The first second of the next day: when the day's counters reset.
    #[must_use]
    pub const fn end(self) -> UnixSeconds {
        UnixSeconds::from_raw(self.0.saturating_add(1).saturating_mul(SECONDS_PER_DAY))
    }

    /// The day `days` earlier.
    #[must_use]
    pub const fn minus(self, days: i64) -> Self {
        Self(self.0.saturating_sub(days))
    }
}

/// Where "now" comes from.
pub trait Clock: Send + Sync {
    /// The current time.
    fn now(&self) -> UnixSeconds;
}

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> UnixSeconds {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        UnixSeconds::from_raw(secs)
    }
}

/// A clock that moves only when told to, for tests.
#[derive(Debug, Default)]
pub struct ManualClock(AtomicI64);

impl ManualClock {
    /// A clock showing `at`.
    #[must_use]
    pub const fn new(at: UnixSeconds) -> Self {
        Self(AtomicI64::new(at.as_raw()))
    }

    /// Move the clock forward.
    pub fn advance(&self, seconds: i64) {
        self.0.fetch_add(seconds, Ordering::SeqCst);
    }

    /// Set the clock.
    pub fn set(&self, at: UnixSeconds) {
        self.0.store(at.as_raw(), Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds::from_raw(self.0.load(Ordering::SeqCst))
    }
}
