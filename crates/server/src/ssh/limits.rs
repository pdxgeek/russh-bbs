//! Connection and authentication limits.
//!
//! These keep a stranger from holding connections open forever or hammering the door. They are
//! per connection, global, per network (checked at accept, before a handler exists) or per key
//! (checked at the top of `ClientHandler::auth_publickey`). The rate policy itself lives in
//! `russh_bbs_core::waiting_room`; the defaults are decision D37.

use std::time::Duration;

use russh_bbs_core::waiting_room::{DEFAULT_MAX_SUBMISSIONS, RatePolicy};

use crate::waiting_room::{DEFAULT_IP_RATE, DEFAULT_KEY_RATE, DEFAULT_MAX_TRACKED};

/// Timeouts and caps for the SSH front end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Time from TCP accept to a successful authentication. Slow or silent strangers are
    /// disconnected when it elapses.
    pub auth_timeout: Duration,
    /// Authentication attempts per connection before the server disconnects.
    pub max_auth_attempts: u32,
    /// Delay before a failed authentication is answered, so guessing is slow.
    pub auth_rejection_time: Duration,
    /// A connection with no traffic for this long is closed (applies before and after auth).
    pub idle_timeout: Duration,
    /// Interval for SSH keepalives that cull dead TCP connections.
    pub keepalive_interval: Duration,
    /// Concurrent connections, authenticated or not. Excess connections are dropped on accept.
    pub max_connections: usize,
    /// New connections per network (an IPv4 address or an IPv6 /64). Excess are dropped on
    /// accept, before any SSH work is done.
    pub ip_rate: RatePolicy,
    /// Authentication attempts per key. Excess are rejected without a membership lookup.
    pub key_rate: RatePolicy,
    /// Distinct networks and keys each limiter remembers; beyond it the oldest are evicted, so
    /// a flood of distinct sources cannot grow memory without bound.
    pub max_tracked_sources: usize,
    /// Waiting-room form submissions one session may make before it is disconnected.
    pub max_form_submissions: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            auth_timeout: Duration::from_secs(30),
            max_auth_attempts: 6,
            auth_rejection_time: Duration::from_secs(1),
            idle_timeout: Duration::from_secs(600),
            keepalive_interval: Duration::from_secs(30),
            max_connections: 256,
            ip_rate: DEFAULT_IP_RATE,
            key_rate: DEFAULT_KEY_RATE,
            max_tracked_sources: DEFAULT_MAX_TRACKED,
            max_form_submissions: DEFAULT_MAX_SUBMISSIONS,
        }
    }
}
