//! Rate limits on submitting requests, on M1-C's limiter (D37). Nothing here is new: a
//! [`KeyedLimiter`] per key and one per network, with bounded memory and an injectable clock.
//!
//! The network is remembered **in memory only**, as the limiter's key; it is never written to
//! the database, the log or an audit row.

use std::net::IpAddr;
use std::time::Duration;

use crate::access::Fingerprint;
use crate::waiting_room::{Clock, KeyedLimiter, RatePolicy, SystemClock, Verdict, client_network};

/// One key may submit a burst of 3, then one more every 10 minutes. A person fixing a typo is
/// never near it; a script is.
pub const DEFAULT_KEY_RATE: RatePolicy = RatePolicy::new(3, Duration::from_secs(600));
/// One network may submit a burst of 10, then one more every 2 minutes (30 an hour). Generous
/// for a household or a campus behind one address, ruinous for a flood.
pub const DEFAULT_NETWORK_RATE: RatePolicy = RatePolicy::new(10, Duration::from_secs(120));
/// Sources each limiter remembers before it evicts (D37).
pub const DEFAULT_TRACKED: usize = 10_000;

/// Which limit refused a submission. For the log only; the person is not told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitLimited {
    /// This key has submitted too often.
    Key,
    /// This network has submitted too often.
    Network,
}

/// The per-key and per-network limits on submitting a request.
#[derive(Debug)]
pub struct RequestGate<C = SystemClock> {
    keys: KeyedLimiter<Fingerprint, C>,
    networks: KeyedLimiter<IpAddr, C>,
}

impl RequestGate {
    /// A gate with the default limits and the system clock.
    #[must_use]
    pub fn new() -> Self {
        Self::with_rates(
            DEFAULT_KEY_RATE,
            DEFAULT_NETWORK_RATE,
            DEFAULT_TRACKED,
            &SystemClock::new(),
        )
    }
}

impl Default for RequestGate {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Clock + Clone> RequestGate<C> {
    /// A gate with explicit limits and clock (tests use a manual clock).
    #[must_use]
    pub fn with_rates(key: RatePolicy, network: RatePolicy, tracked: usize, clock: &C) -> Self {
        Self {
            keys: KeyedLimiter::new(key, tracked, clock.clone()),
            networks: KeyedLimiter::new(network, tracked, clock.clone()),
        }
    }
}

impl<C: Clock> RequestGate<C> {
    /// Count one submission by `key` from `peer` (an IPv6 address counts as its /64). Both
    /// limits are counted even when the first refuses, so one source cannot dodge the other.
    pub fn admit(&self, key: &Fingerprint, peer: Option<IpAddr>) -> Result<(), SubmitLimited> {
        let network = peer.map_or(Verdict::Allowed, |ip| {
            self.networks.check(&client_network(ip))
        });
        let per_key = self.keys.check(key);
        if !per_key.is_allowed() {
            Err(SubmitLimited::Key)
        } else if !network.is_allowed() {
            Err(SubmitLimited::Network)
        } else {
            Ok(())
        }
    }
}
