//! Connection and authentication limits.
//!
//! These keep a stranger from holding connections open forever. They are per connection or
//! global. **Per-IP and per-key rate limits are M1-C**: they plug in at the admission point in
//! `server::accept_loop` (before a handler exists) and in `ClientHandler::auth_publickey`
//! (per key), both marked in the code.

use std::time::Duration;

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
        }
    }
}
