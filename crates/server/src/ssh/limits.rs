//! Connection and authentication limits.
//!
//! These keep a stranger from holding connections open forever or hammering the door. They are
//! per connection, global, per network (checked at accept, before a handler exists) or per key
//! (checked at the top of `ClientHandler::auth_publickey`). The per-session budgets after login
//! (redraw rate, request and input rates, the output stall) are decision D49. The rate policy itself lives in
//! `russh_bbs_core::waiting_room`; the defaults are decision D37.

use std::time::Duration;

use russh_bbs_core::waiting_room::{DEFAULT_MAX_SUBMISSIONS, RatePolicy};

use super::terminal::RenderCap;
use crate::waiting_room::{DEFAULT_IP_RATE, DEFAULT_KEY_RATE, DEFAULT_MAX_TRACKED};

/// Defaults for password attempts per handle: a burst of 5, then one every 30 seconds. A person
/// who mistypes a few times is never blocked; a guesser gets about 2 tries a minute per handle,
/// and the 12-character minimum makes that hopeless (D81).
pub const DEFAULT_PASSWORD_ACCOUNT_RATE: RatePolicy = RatePolicy::new(5, Duration::from_secs(30));
/// Defaults for password attempts per network: a burst of 10, then one every 6 seconds (10 a
/// minute), so one address cannot sweep many handles (D81).
pub const DEFAULT_PASSWORD_NETWORK_RATE: RatePolicy = RatePolicy::new(10, Duration::from_secs(6));

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
    /// With `render_cap` this is the sysop's memory bill: each session may hold
    /// `render_cap.bytes_per_session()` of render buffers (D53).
    pub max_connections: usize,
    /// Widest and tallest area one session draws into; a bigger window is centered on it (D53).
    pub render_cap: RenderCap,
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
    /// Shortest time between two redraws of one session. A session redraws at most
    /// `1 / redraw_interval` times a second however much it sends (D49).
    pub redraw_interval: Duration,
    /// Requests per second one session may send after login (data packets and window changes
    /// together). Excess data packets are dropped; excess window changes are folded into the
    /// latest size.
    pub requests_per_second: u32,
    /// Input bytes per second one session may send. Excess bytes are dropped.
    pub input_bytes_per_second: u32,
    /// Over-budget events (an excess request, or each started KiB of excess input) a session may
    /// collect before it is disconnected.
    pub flood_strikes: u32,
    /// How long a peer may leave the server unable to hand it output (its window or socket
    /// full) before the connection is dropped instead of queueing without bound.
    pub write_stall: Duration,
    /// Whether the SSH `password` method is offered (D94 (10)). On by default: ssh is the
    /// transport, the account is handle plus password. Off, only a key bound to a handle signs
    /// in and the server never reads a password over ssh.
    pub allow_password_login: bool,
    /// Password attempts per handle, whether or not the handle exists (D81).
    pub password_account_rate: RatePolicy,
    /// Password attempts per network (an IPv4 address or an IPv6 /64), whatever the handle (D81).
    pub password_network_rate: RatePolicy,
    /// Whether the board is in low-security mode: the sysop turned the telnet transport on
    /// (D27, M3-H). Staff screens then carry the `LOW-SECURITY` marker. It changes nothing else
    /// about an SSH session.
    pub low_security: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            auth_timeout: Duration::from_secs(30),
            max_auth_attempts: 6,
            auth_rejection_time: Duration::from_secs(1),
            idle_timeout: Duration::from_secs(600),
            keepalive_interval: Duration::from_secs(30),
            max_connections: 64,
            render_cap: RenderCap::default(),
            ip_rate: DEFAULT_IP_RATE,
            key_rate: DEFAULT_KEY_RATE,
            max_tracked_sources: DEFAULT_MAX_TRACKED,
            max_form_submissions: DEFAULT_MAX_SUBMISSIONS,
            redraw_interval: Duration::from_millis(50),
            requests_per_second: 200,
            input_bytes_per_second: 8192,
            flood_strikes: 1000,
            write_stall: Duration::from_secs(10),
            allow_password_login: true,
            password_account_rate: DEFAULT_PASSWORD_ACCOUNT_RATE,
            password_network_rate: DEFAULT_PASSWORD_NETWORK_RATE,
            low_security: false,
        }
    }
}
