//! The claim itself: token check, then one atomic store step.

use std::future::Future;

use tracing::{info, warn};

use super::gate::FounderGate;
use crate::access::Fingerprint;
use crate::sanitize::{Username, sanitize_username};
use crate::store::{Account, StoreError};

/// The name the founder's account gets. It is deliberately not `sysop`: that name is reserved
/// against member registration (FIX-M0-1), and the claim goes through the same
/// `sanitize_username` as everyone, with no bypass, so it must be an ordinary valid handle.
/// The sysop can rename the account later (M2).
pub const FOUNDER_USERNAME: &str = "founder";

/// The store half of the claim.
pub trait FounderStore: Send + Sync {
    /// Whether the founder has already been claimed.
    fn founder_claimed(&self) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// **Atomically** claim the board: if and only if no one has claimed it, create an active
    /// account with the `sysop` role, bind `fingerprint` to it and record the claim,
    /// all in one transaction. Returns `Ok(None)` when it was already claimed (nothing is
    /// written) and the account otherwise. Two concurrent calls yield exactly one `Some`.
    fn claim_founder(
        &self,
        username: Username,
        fingerprint: &Fingerprint,
    ) -> impl Future<Output = Result<Option<Account>, StoreError>> + Send;
}

/// The result of one claim attempt. There is deliberately one failure: a wrong token, a token
/// presented after the claim, a lost race and a store error look the same to the caller (and so
/// to the person typing), so the answer is not an oracle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimOutcome {
    /// The key now belongs to the new sysop account.
    Claimed(Account),
    /// The attempt failed. No reason is given.
    Rejected,
}

/// Try to claim the board for `fingerprint` with `candidate`.
///
/// The token is checked first, in constant time. Only a correct token reaches the store, whose
/// compare-and-set decides between two correct callers. The first success closes the gate, so
/// the token never works again.
pub async fn claim_founder<S: FounderStore>(
    gate: &FounderGate,
    store: &S,
    candidate: &str,
    fingerprint: &Fingerprint,
) -> ClaimOutcome {
    if !gate.accepts(candidate) {
        // Wrong token or already claimed: one branch, one log line, no token text.
        info!(fingerprint = %fingerprint, "founder claim rejected");
        return ClaimOutcome::Rejected;
    }
    let Ok(username) = sanitize_username(FOUNDER_USERNAME) else {
        warn!("built-in founder username is invalid; claim refused");
        return ClaimOutcome::Rejected;
    };
    match store.claim_founder(username, fingerprint).await {
        Ok(Some(account)) => {
            gate.close();
            info!(fingerprint = %fingerprint, "founder claimed by fingerprint {fingerprint}");
            ClaimOutcome::Claimed(account)
        }
        Ok(None) => {
            // Someone else won the race (or another process did). The token is spent.
            gate.close();
            info!(fingerprint = %fingerprint, "founder claim lost: already claimed");
            ClaimOutcome::Rejected
        }
        Err(error) => {
            warn!(fingerprint = %fingerprint, %error, "founder claim failed in the store");
            ClaimOutcome::Rejected
        }
    }
}

/// The empty directory has no store to claim in: the board counts as claimed and every attempt
/// is rejected. Used by tests and by servers started without a store.
impl FounderStore for crate::access::NoMembers {
    fn founder_claimed(&self) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(true))
    }

    fn claim_founder(
        &self,
        _username: Username,
        _fingerprint: &Fingerprint,
    ) -> impl Future<Output = Result<Option<Account>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }
}
