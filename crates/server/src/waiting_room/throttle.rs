//! The two admission limiters of the SSH front end: per network (at accept) and per key (at
//! authentication). The policy and the bounded table live in `russh_bbs_core::waiting_room`;
//! this only owns the instances and the choice of what to key on.

use std::net::IpAddr;

use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::waiting_room::{
    Clock, KeyedLimiter, RatePolicy, SystemClock, Verdict, client_network,
};

/// Defaults for the per-network limit: a burst of 10 connections, then one every 2 seconds
/// (30 a minute). Generous for a NAT with several members, ruinous for a scanner (D31).
pub const DEFAULT_IP_RATE: RatePolicy = RatePolicy::new(10, std::time::Duration::from_secs(2));
/// Defaults for the per-key limit: a burst of 5 authentications, then one every 10 seconds
/// (6 a minute). A person retrying a login never gets near it (D31).
pub const DEFAULT_KEY_RATE: RatePolicy = RatePolicy::new(5, std::time::Duration::from_secs(10));
/// Distinct sources each limiter remembers before it evicts (D31).
pub const DEFAULT_MAX_TRACKED: usize = 10_000;

/// Rate limits for connections and authentications.
#[derive(Debug)]
pub(crate) struct Throttle<C = SystemClock> {
    networks: KeyedLimiter<IpAddr, C>,
    keys: KeyedLimiter<Fingerprint, C>,
}

impl<C: Clock + Clone> Throttle<C> {
    /// Build both limiters; they share one clock.
    pub(crate) fn new(ip: RatePolicy, key: RatePolicy, max_tracked: usize, clock: &C) -> Self {
        Self {
            networks: KeyedLimiter::new(ip, max_tracked, clock.clone()),
            keys: KeyedLimiter::new(key, max_tracked, clock.clone()),
        }
    }

    /// Count a new connection from `addr` (an IPv6 address counts as its /64).
    pub(crate) fn admit_connection(&self, addr: IpAddr) -> Verdict {
        self.networks.check(&client_network(addr))
    }

    /// Count an authentication attempt by `key`. Applied to every key before membership is
    /// looked up, so the answer cannot tell a member from a stranger.
    pub(crate) fn admit_key(&self, key: &Fingerprint) -> Verdict {
        self.keys.check(key)
    }
}

#[cfg(test)]
mod tests;
