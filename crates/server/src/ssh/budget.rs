//! Per-session budgets for what a connected client may ask of the server (D49).
//!
//! The connection-setup limits (D22, D37) stop at authentication. After it, every request a
//! client sends costs the server something, and an unknown key is authenticated by design. These
//! budgets bound that cost: requests per second, input bytes per second, and a strike count that
//! ends a session which keeps pushing past both.
//!
//! The arithmetic is pure and takes the time as an argument, so tests drive it with a fake clock.

use std::time::{Duration, Instant};

use super::limits::Limits;

/// A bucket holds this many seconds of its rate, so a legitimate burst (a paste, a window drag)
/// goes through and a sustained flood does not.
const BURST_SECONDS: u64 = 2;

/// Tokens are counted in millionths so refilling by elapsed microseconds loses nothing.
const MICRO: u64 = 1_000_000;

/// Dropped input is charged one strike per started kibibyte.
const STRIKE_BYTES: usize = 1024;

/// Strikes are forgotten after this long without a new one, so a long session is not punished
/// for an old burst.
const STRIKE_MEMORY: Duration = Duration::from_secs(10);

/// A token bucket that starts full and refills at a fixed rate.
#[derive(Debug)]
struct TokenBucket {
    rate: u64,
    capacity: u64,
    tokens: u64,
    last: Instant,
}

impl TokenBucket {
    fn new(rate_per_second: u32, now: Instant) -> Self {
        let rate = u64::from(rate_per_second);
        let capacity = rate.saturating_mul(BURST_SECONDS).saturating_mul(MICRO);
        Self {
            rate,
            capacity,
            tokens: capacity,
            last: now,
        }
    }

    /// Take up to `want` whole tokens and return how many were granted.
    fn take_up_to(&mut self, now: Instant, want: u64) -> u64 {
        if let Some(elapsed) = now.checked_duration_since(self.last) {
            let earned = u64::try_from(elapsed.as_micros())
                .unwrap_or(u64::MAX)
                .saturating_mul(self.rate);
            self.tokens = self.tokens.saturating_add(earned).min(self.capacity);
            self.last = now;
        }
        let granted = (self.tokens / MICRO).min(want);
        self.tokens -= granted * MICRO;
        granted
    }
}

/// One session's request and input budgets, and its strike count.
#[derive(Debug)]
pub(super) struct SessionBudget {
    requests: TokenBucket,
    input: TokenBucket,
    strikes: u32,
    strike_limit: u32,
    last_strike: Option<Instant>,
    dropped_bytes: u64,
}

impl SessionBudget {
    pub(super) fn new(limits: &Limits, now: Instant) -> Self {
        Self {
            requests: TokenBucket::new(limits.requests_per_second, now),
            input: TokenBucket::new(limits.input_bytes_per_second, now),
            strikes: 0,
            strike_limit: limits.flood_strikes,
            last_strike: None,
            dropped_bytes: 0,
        }
    }

    /// Charge one request (a data packet or a window-change). `false` means it is over budget;
    /// the caller drops a data packet and keeps only the latest size of a window change.
    pub(super) fn request(&mut self, now: Instant) -> bool {
        let allowed = self.requests.take_up_to(now, 1) == 1;
        if !allowed {
            self.strike(now, 1);
        }
        allowed
    }

    /// Charge `len` bytes of input and return how many of them may be used.
    pub(super) fn input(&mut self, now: Instant, len: usize) -> usize {
        let want = u64::try_from(len).unwrap_or(u64::MAX);
        let granted = self.input.take_up_to(now, want);
        let dropped = want - granted;
        if dropped > 0 {
            self.dropped_bytes = self.dropped_bytes.saturating_add(dropped);
            let kib = usize::try_from(dropped)
                .unwrap_or(usize::MAX)
                .div_ceil(STRIKE_BYTES);
            self.strike(now, u32::try_from(kib).unwrap_or(u32::MAX));
        }
        usize::try_from(granted).unwrap_or(len)
    }

    /// Whether the session has pushed past its budgets often enough to be dropped.
    pub(super) fn exceeded(&self) -> bool {
        self.strikes >= self.strike_limit
    }

    /// Input bytes dropped so far.
    pub(super) fn dropped_bytes(&self) -> u64 {
        self.dropped_bytes
    }

    fn strike(&mut self, now: Instant, count: u32) {
        if self
            .last_strike
            .is_some_and(|at| now.saturating_duration_since(at) > STRIKE_MEMORY)
        {
            self.strikes = 0;
        }
        self.last_strike = Some(now);
        self.strikes = self.strikes.saturating_add(count);
    }
}

#[cfg(test)]
mod tests;
