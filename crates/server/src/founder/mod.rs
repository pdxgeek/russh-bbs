//! The founder claim in the server: issuing the one-time setup token at start-up, the claim
//! rate limits, and the bundle the SSH handler uses (M1-D, decision D41).
//!
//! The rules (token, hash, constant-time check, the atomic claim) are in
//! `russh_bbs_core::founder`. This module only generates the randomness, prints the token once
//! and holds the limiters.
//!
//! **The token is printed exactly once per process start**, through the writer the caller
//! passes (stdout in the binary, a buffer in tests), between clear markers. It is never given
//! to `tracing`, never stored, and a restart while unclaimed issues a new one.

use std::io::{self, Write};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use rand::TryRng as _;
use rand::rngs::SysRng;
use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::founder::{FounderGate, FounderStore, SetupToken, TOKEN_BYTES};
use russh_bbs_core::store::StoreError;
use russh_bbs_core::waiting_room::{
    Clock, KeyedLimiter, RatePolicy, SystemClock, Verdict, client_network,
};
use tracing::info;

/// The SSH user name that opens the claim prompt: `ssh claim@host`. It is not a secret and
/// the prompt is the same on a claimed board (D41).
pub const CLAIM_USER: &str = "claim";

/// Token attempts one key may make: a burst of 3, then one more every 30 seconds.
pub const DEFAULT_CLAIM_KEY_RATE: RatePolicy = RatePolicy::new(3, Duration::from_secs(30));
/// Token attempts one network may make: a burst of 10, then one more every 10 seconds.
pub const DEFAULT_CLAIM_IP_RATE: RatePolicy = RatePolicy::new(10, Duration::from_secs(10));
/// Sources each claim limiter remembers.
const MAX_TRACKED: usize = 10_000;

/// Why the founder state could not be set up. A start-up error.
#[derive(Debug, thiserror::Error)]
pub enum FounderError {
    /// The store could not say whether the board is claimed.
    #[error("cannot read the founder state")]
    Store(#[from] StoreError),
    /// The operating system's random source failed. No token is issued.
    #[error("the operating system random source failed")]
    Random,
    /// The announcement could not be written.
    #[error("cannot write the setup token announcement")]
    Announce(#[from] io::Error),
}

/// Everything the SSH handler needs for the claim: the gate and the limiters.
#[derive(Debug)]
pub struct Founder<C = SystemClock> {
    gate: FounderGate,
    keys: KeyedLimiter<Fingerprint, C>,
    networks: KeyedLimiter<IpAddr, C>,
}

impl Founder {
    /// A founder service with the default claim rate limits.
    #[must_use]
    pub fn new(gate: FounderGate) -> Self {
        Self::with_rates(
            gate,
            DEFAULT_CLAIM_KEY_RATE,
            DEFAULT_CLAIM_IP_RATE,
            &SystemClock::new(),
        )
    }

    /// A founder service that is closed: no token is live. Used when no claim is possible.
    #[must_use]
    pub fn closed() -> Self {
        Self::new(FounderGate::closed())
    }
}

impl<C: Clock + Clone> Founder<C> {
    /// Build with explicit limits and clock (tests use a manual clock).
    #[must_use]
    pub fn with_rates(gate: FounderGate, key: RatePolicy, ip: RatePolicy, clock: &C) -> Self {
        Self {
            gate,
            keys: KeyedLimiter::new(key, MAX_TRACKED, clock.clone()),
            networks: KeyedLimiter::new(ip, MAX_TRACKED, clock.clone()),
        }
    }
}

impl<C: Clock> Founder<C> {
    /// The gate, for [`russh_bbs_core::founder::claim_founder`].
    #[must_use]
    pub fn gate(&self) -> &FounderGate {
        &self.gate
    }

    /// Count a token attempt by `key` from `peer`. A limited attempt is answered with the same
    /// generic failure as a wrong token, so the limit is not an oracle.
    #[must_use]
    pub fn admit_attempt(&self, key: &Fingerprint, peer: Option<IpAddr>) -> Verdict {
        let network = peer.map_or(Verdict::Allowed, |ip| {
            self.networks.check(&client_network(ip))
        });
        // Both are counted even if the first limits, so one source cannot dodge the other.
        let per_key = self.keys.check(key);
        if network.is_allowed() {
            per_key
        } else {
            network
        }
    }
}

/// Decide the founder state at start-up and, if the board is unclaimed, issue and announce a
/// token on `out`. Returns the service to hand to the SSH server.
///
/// Claimed board: the gate is closed and nothing is printed. Unclaimed: 20 bytes from the OS
/// CSPRNG become the token; only its hash is kept.
pub async fn start<S: FounderStore>(
    store: &S,
    out: &mut impl Write,
) -> Result<Founder, FounderError> {
    if store.founder_claimed().await? {
        info!("founder already claimed; no setup token issued");
        return Ok(Founder::closed());
    }
    let token = generate_token()?;
    announce(out, &token)?;
    info!("board is unclaimed; a one-time setup token was printed to the console");
    Ok(Founder::new(FounderGate::open_with(&token)))
}

/// A new token from the operating system's random source.
fn generate_token() -> Result<SetupToken, FounderError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| FounderError::Random)?;
    Ok(SetupToken::from_bytes(bytes))
}

/// The single deliberate place the token is shown.
fn announce(out: &mut impl Write, token: &SetupToken) -> io::Result<()> {
    writeln!(out)?;
    writeln!(
        out,
        "=== russh-bbs FIRST-RUN SETUP TOKEN (shown once; not logged) ==="
    )?;
    writeln!(
        out,
        "This board has no sysop yet. The first person to present this token"
    )?;
    writeln!(
        out,
        "over SSH becomes the sysop:   ssh -p <port> claim@<host>"
    )?;
    writeln!(
        out,
        "Anyone who can read this console or its logs can claim the board."
    )?;
    writeln!(
        out,
        "A restart before the claim issues a new token and voids this one."
    )?;
    writeln!(out)?;
    writeln!(out, "    {}", token.reveal())?;
    writeln!(out)?;
    writeln!(out, "=== end of setup token ===")?;
    out.flush()
}

/// A shareable founder service.
pub type SharedFounder = Arc<Founder>;

#[cfg(test)]
mod tests;
