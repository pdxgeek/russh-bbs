//! The storage trait for invites and the shapes it returns. Persistence only: the rules
//! (permission, quota, expiry) are in [`super::service`] and the backends run them inside
//! their transactions.

use std::fmt;
use std::future::Future;

use super::code::CodeHash;
use super::quota::Quota;
use crate::access::{Fingerprint, NoMembers};
use crate::sanitize::Username;
use crate::store::{Account, AccountId, AccountState, StoreError, UnixSeconds};

/// The state an account redeemed from an invite starts in (D67): `active`. The inviter vouched
/// for the person; the invite tree records who, and the sysop can suspend. `pending` is for the
/// `apply` path, where nobody has vouched.
pub const REDEEMED_STATE: AccountState = AccountState::Active;

/// Store-assigned id of an invite. This is what the audit log and screens name; the code itself
/// is never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InviteId(i64);

impl InviteId {
    /// Wrap a raw id. Call this only where a row is mapped (the store).
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// The raw id, for the store to bind into a query.
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }
}

impl fmt::Display for InviteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A code to record. The store stamps the times from its clock.
#[derive(Debug, Clone)]
pub struct NewInvite {
    /// Who the code is bound to.
    pub inviter: AccountId,
    /// The hash of the code (never the code).
    pub code_hash: CodeHash,
    /// Seconds the code lives.
    pub ttl_secs: i64,
    /// The inviter's quota; the store checks it in the same transaction as the insert.
    pub quota: Quota,
}

/// A code that was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InviteRecord {
    /// Its id.
    pub id: InviteId,
    /// When it expires.
    pub expires_at: UnixSeconds,
    /// How many codes the inviter has created in the quota window, this one included.
    pub used_in_window: u32,
}

/// Why a code was not recorded. Nothing was written.
#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    /// The inviter has already created `limit` codes in the window.
    #[error("invite quota reached ({limit} per month)")]
    QuotaReached {
        /// The quota that was hit.
        limit: u32,
    },
    /// The store failed, or the inviter does not exist.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Why a redemption was refused, for the log and for tests. **The person is told the same
/// thing for every one of these** (no oracle), and a refusal writes nothing: the code stays
/// usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// No such code, or it was used, or it expired, or its inviter is no longer active. One
    /// query cannot and need not tell these apart.
    Code,
    /// The handle is taken (or looks like one that is).
    Handle,
    /// The key is already bound to an account.
    Key,
}

/// Why a redemption did not make an account.
#[derive(Debug, thiserror::Error)]
pub enum RedeemError {
    /// Refused; nothing was written.
    #[error("invite refused")]
    Rejected(RejectReason),
    /// The store failed. Nothing was written.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// A redemption that made an account.
#[derive(Debug, Clone)]
pub struct Redeemed {
    /// The new account, in [`REDEEMED_STATE`], bound to the presenting key.
    pub account: Account,
    /// The invite that was used.
    pub invite: InviteId,
    /// The account that created it, recorded as the new account's inviter.
    pub inviter: AccountId,
}

/// Where an invite stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InviteStatus {
    /// Unused and not yet expired.
    Open,
    /// Used. The account that used it, if it still exists.
    Redeemed(Option<Username>),
    /// Unused and past its expiry.
    Expired,
}

/// One of an inviter's codes, as listed. No code, no hash: only what the inviter may see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteSummary {
    /// Its id.
    pub id: InviteId,
    /// When it was created.
    pub created_at: UnixSeconds,
    /// When it expires (or expired).
    pub expires_at: UnixSeconds,
    /// Where it stands.
    pub status: InviteStatus,
}

/// One account in the invite tree, as the store returns it (unordered).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    /// The account.
    pub account: AccountId,
    /// Its handle.
    pub handle: Username,
    /// Who invited it. `None` for the founder and for accounts made some other way.
    pub invited_by: Option<AccountId>,
}

/// Invite persistence.
pub trait InviteStore: Send + Sync {
    /// Record a code for `new.inviter`. In one transaction: lock the inviter, count the codes
    /// created in the quota window, refuse with [`CreateError::QuotaReached`] if the quota is
    /// used, insert, and write the `invite.created` audit row (actor: the inviter, target: the
    /// invite id). Concurrent callers cannot both take the last slot.
    fn create_invite(
        &self,
        new: &NewInvite,
    ) -> impl Future<Output = Result<InviteRecord, CreateError>> + Send;

    /// Redeem the code with `hash`, binding `fingerprint` to a new account named `username` in
    /// [`REDEEMED_STATE`], with the code's inviter recorded on it. **Atomic**: one transaction
    /// marks the code used (only if it is unused, unexpired and its inviter is `active`),
    /// creates the account and its key, and writes the `account.created` and `invite.redeemed`
    /// audit rows. If any step fails the code is not consumed. Two concurrent redemptions of one
    /// code produce exactly one account.
    fn redeem_invite(
        &self,
        hash: &CodeHash,
        username: Username,
        fingerprint: &Fingerprint,
    ) -> impl Future<Output = Result<Redeemed, RedeemError>> + Send;

    /// The most recent `limit` codes `inviter` created, newest first.
    fn invites_by(
        &self,
        inviter: AccountId,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<InviteSummary>, StoreError>> + Send;

    /// Up to `limit` accounts with who invited each, oldest first (ids ascend).
    fn invite_tree(
        &self,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<TreeNode>, StoreError>> + Send;
}

/// The empty directory has no accounts: nothing can be created or redeemed, every list is empty.
impl InviteStore for NoMembers {
    fn create_invite(
        &self,
        _new: &NewInvite,
    ) -> impl Future<Output = Result<InviteRecord, CreateError>> + Send {
        std::future::ready(Err(CreateError::Store(StoreError::AccountNotFound)))
    }

    fn redeem_invite(
        &self,
        _hash: &CodeHash,
        _username: Username,
        _fingerprint: &Fingerprint,
    ) -> impl Future<Output = Result<Redeemed, RedeemError>> + Send {
        std::future::ready(Err(RedeemError::Rejected(RejectReason::Code)))
    }

    fn invites_by(
        &self,
        _inviter: AccountId,
        _limit: u32,
    ) -> impl Future<Output = Result<Vec<InviteSummary>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn invite_tree(
        &self,
        _limit: u32,
    ) -> impl Future<Output = Result<Vec<TreeNode>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }
}
