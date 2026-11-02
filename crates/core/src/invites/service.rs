//! The invite actions, with their permission and quota checks (M2-D). Front ends call these;
//! none of the rules live in a front end.

use tracing::{info, warn};

use super::code::{CODE_BYTES, CodeHash, InviteCode};
use super::quota::{INVITE_TTL_SECS, invite_quota};
use super::store::{
    CreateError, InviteId, InviteStore, InviteSummary, NewInvite, RedeemError, Redeemed,
    RejectReason,
};
use super::tree::{TreeLine, order_tree};
use crate::access::{Denied, Fingerprint, Permission, Snapshot};
use crate::sanitize::Username;
use crate::store::{StoreError, UnixSeconds};

/// Most of an inviter's recent codes listed on the screen.
pub const LIST_LIMIT: u32 = 20;

/// Most accounts read for the tree. A board larger than this shows its oldest accounts.
pub const TREE_LIMIT: u32 = 2000;

/// A code the inviter just made. It is the only place the plaintext exists.
#[derive(Debug)]
pub struct CreatedInvite {
    /// The invite's id, for the log and for "your invites".
    pub id: InviteId,
    /// The code. Show it once; it cannot be read again (only its hash is stored).
    pub code: InviteCode,
    /// When it expires.
    pub expires_at: UnixSeconds,
    /// Codes left in the quota window; `None` if the quota is unlimited.
    pub remaining: Option<u32>,
}

/// Why an invite action did not happen.
#[derive(Debug, thiserror::Error)]
pub enum InviteError {
    /// The session does not hold the permission. Nothing was read or written.
    #[error(transparent)]
    Denied(#[from] Denied),
    /// The monthly quota is used up.
    #[error("invite quota reached ({limit} per month)")]
    QuotaReached {
        /// The quota.
        limit: u32,
    },
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Create an invite for the session behind `access`. Needs `invites.create`; the quota comes
/// from the account's highest role ([`invite_quota`]); `random` is [`CODE_BYTES`] bytes from the
/// OS CSPRNG. Audited in the store's transaction. The plaintext code is in the result and
/// nowhere else.
///
/// # Errors
/// [`InviteError::Denied`] without the permission, [`InviteError::QuotaReached`] at the quota,
/// [`InviteError::Store`] otherwise.
pub async fn create_invite<S: InviteStore>(
    store: &S,
    access: &Snapshot,
    random: [u8; CODE_BYTES],
) -> Result<CreatedInvite, InviteError> {
    let granted = access.require(Permission::InvitesCreate, None)?;
    let Some(inviter) = granted.account() else {
        // `invites.create` is never granted to the waiting room; fail closed if it somehow is.
        return Err(InviteError::Store(StoreError::AccountNotFound));
    };
    let quota = invite_quota(access.top_role());
    let code = InviteCode::from_bytes(random);
    let new = NewInvite {
        inviter,
        code_hash: code.hash(),
        ttl_secs: INVITE_TTL_SECS,
        quota,
    };
    match store.create_invite(&new).await {
        Ok(record) => {
            info!(inviter = %inviter, invite = %record.id, "invite created");
            let remaining = quota
                .limit()
                .map(|limit| limit.saturating_sub(record.used_in_window));
            Ok(CreatedInvite {
                id: record.id,
                code,
                expires_at: record.expires_at,
                remaining,
            })
        }
        Err(CreateError::QuotaReached { limit }) => Err(InviteError::QuotaReached { limit }),
        Err(CreateError::Store(error)) => Err(error.into()),
    }
}

/// Redeem `typed_code` for `fingerprint` under `username` (already validated by the sanitizer).
/// Needs `invite.redeem`, which the waiting room's snapshot holds. Every refusal is
/// [`RedeemError::Rejected`], which the screen words identically: a wrong code, an expired one,
/// a used one and an unknown one cannot be told apart.
///
/// # Errors
/// [`RedeemError::Rejected`] for every refusal, [`RedeemError::Store`] if the store failed.
pub async fn redeem_invite<S: InviteStore>(
    store: &S,
    access: &Snapshot,
    typed_code: &str,
    username: Username,
    fingerprint: &Fingerprint,
) -> Result<Redeemed, RedeemError> {
    if access.require(Permission::InviteRedeem, None).is_err() {
        return Err(RedeemError::Rejected(RejectReason::Code));
    }
    let hash = CodeHash::of(typed_code);
    match store.redeem_invite(&hash, username, fingerprint).await {
        Ok(redeemed) => {
            info!(
                account = %redeemed.account.id,
                inviter = %redeemed.inviter,
                invite = %redeemed.invite,
                fingerprint = %fingerprint,
                "invite redeemed"
            );
            Ok(redeemed)
        }
        Err(RedeemError::Rejected(reason)) => {
            // The reason is for the sysop's log only; the person is told one thing. The code
            // text is never logged.
            info!(fingerprint = %fingerprint, ?reason, "invite redemption refused");
            Err(RedeemError::Rejected(reason))
        }
        Err(RedeemError::Store(error)) => {
            warn!(fingerprint = %fingerprint, %error, "invite redemption failed in the store");
            Err(RedeemError::Store(error))
        }
    }
}

/// The caller's own recent codes with where each stands. Needs `invites.create`; reads only the
/// caller's own rows (the store is asked for the account the permission was granted to).
///
/// # Errors
/// [`InviteError::Denied`] or [`InviteError::Store`].
pub async fn my_invites<S: InviteStore>(
    store: &S,
    access: &Snapshot,
) -> Result<Vec<InviteSummary>, InviteError> {
    let granted = access.require(Permission::InvitesCreate, None)?;
    let Some(inviter) = granted.account() else {
        return Err(InviteError::Store(StoreError::AccountNotFound));
    };
    Ok(store.invites_by(inviter, LIST_LIMIT).await?)
}

/// Who invited whom, depth first with each person's invitees beneath them. Needs
/// `invites.view_tree` (trusted and up).
///
/// # Errors
/// [`InviteError::Denied`] or [`InviteError::Store`].
pub async fn invite_tree<S: InviteStore>(
    store: &S,
    access: &Snapshot,
) -> Result<Vec<TreeLine>, InviteError> {
    access.require(Permission::InvitesViewTree, None)?;
    Ok(order_tree(&store.invite_tree(TREE_LIMIT).await?))
}
