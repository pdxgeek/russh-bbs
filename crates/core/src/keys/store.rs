//! The storage trait for keys, codes and the notices a member reads at the next sign-in
//! (FIX-M3-11, D97). Persistence only; the rules (permission, rank, rate, expiry in the sense of
//! "which codes may be made") are in [`super::service`], and the backends run the three rules a
//! race could break inside their transactions: never remove the last way in, a code is used
//! once, and a code expires.

use std::future::Future;

use super::code::{CodeKind, LoginCodeHash};
use crate::access::{Fingerprint, NoMembers};
use crate::audit::AuditEvent;
use crate::store::{Account, AccountId, StoreError, UnixSeconds};

/// One key bound to an account, as listed. Public data only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRecord {
    /// The key's fingerprint.
    pub fingerprint: Fingerprint,
    /// When it was bound to the account.
    pub added_at: UnixSeconds,
    /// The last time it signed in, if it has since the store began recording that.
    pub last_used_at: Option<UnixSeconds>,
}

/// What happened to the account's keys that its owner is told about at the next sign-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// A key was pasted into My keys.
    KeyAdded,
    /// A key was removed in My keys.
    KeyRemoved,
    /// A key was added by giving the handle and password in the waiting room.
    KeyLinked,
    /// A key was added by a device-link code.
    DeviceLinked,
    /// A key was added and the password replaced with a staff recovery code.
    Recovered,
}

impl NoticeKind {
    /// The stored name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KeyAdded => "key_added",
            Self::KeyRemoved => "key_removed",
            Self::KeyLinked => "key_linked",
            Self::DeviceLinked => "device_linked",
            Self::Recovered => "recovered",
        }
    }

    /// Read a stored name back. `None` for text the code did not write.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::KeyAdded,
            Self::KeyRemoved,
            Self::KeyLinked,
            Self::DeviceLinked,
            Self::Recovered,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == text)
    }
}

/// One thing to tell the member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyNotice {
    /// What happened.
    pub kind: NoticeKind,
    /// When.
    pub at: UnixSeconds,
    /// The key it concerns (public), when there is one.
    pub fingerprint: Option<Fingerprint>,
}

/// The result of asking to remove a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveOutcome {
    /// The key was removed.
    Removed,
    /// The account has no such key.
    NotFound,
    /// It is the last way in: the account has no password and no other key.
    LastWayIn,
}

/// A code to record. The store stamps the times from its clock.
#[derive(Debug, Clone)]
pub struct NewCode {
    /// Link or recovery.
    pub kind: CodeKind,
    /// The account the code will bind a key to.
    pub target: AccountId,
    /// Who made it: the same account for a link code, the staff member for a recovery code.
    pub issuer: AccountId,
    /// The hash of the code (never the code).
    pub hash: LoginCodeHash,
    /// Seconds the code lives.
    pub ttl_secs: i64,
    /// Most codes of this kind the target may have made in the last hour, this one included
    /// (checked in the same transaction as the insert). `None` for no limit.
    pub per_hour: Option<u32>,
}

/// Why a code was not recorded. Nothing was written.
#[derive(Debug, thiserror::Error)]
pub enum IssueError {
    /// The account has made as many codes this hour as it may.
    #[error("too many codes this hour")]
    RateLimited {
        /// When the oldest of them stops counting.
        retry_at: UnixSeconds,
    },
    /// The target is not an active account.
    #[error("no active account")]
    NoAccount,
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Why a redemption did not bind a key. **The person is told the same thing for every one of
/// these**; a refusal writes nothing and the code stays usable (a key that belongs to an
/// account rolls the redemption back).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedeemRefusal {
    /// No such code, or it was used, voided or expired, or its account is not active.
    Code,
    /// The key already belongs to an account.
    Key,
}

/// Why a redemption failed.
#[derive(Debug, thiserror::Error)]
pub enum RedeemError {
    /// Refused; nothing was written.
    #[error("code refused")]
    Refused(RedeemRefusal),
    /// The store failed. Nothing was written.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Where keys, codes and notices are kept.
pub trait KeyStore: Send + Sync {
    /// The account's keys, oldest first.
    fn list_keys(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<KeyRecord>, StoreError>> + Send;

    /// Bind `fingerprint` to the account, record a [`NoticeKind::KeyAdded`] notice and write
    /// `audit`, in one transaction. [`StoreError::FingerprintTaken`] if the key belongs to an
    /// account already (to this one too); nothing is written then.
    fn add_key(
        &self,
        account: AccountId,
        fingerprint: &Fingerprint,
        audit: &AuditEvent,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Remove the key unless it is the account's last way in (no password, no other key), record
    /// a [`NoticeKind::KeyRemoved`] notice and write `audit`, in one transaction. Two removals
    /// racing for the last two keys cannot both succeed.
    fn remove_key(
        &self,
        account: AccountId,
        fingerprint: &Fingerprint,
        audit: &AuditEvent,
    ) -> impl Future<Output = Result<RemoveOutcome, StoreError>> + Send;

    /// Remember that the key signed in now. A key that is not bound changes nothing.
    fn note_key_use(
        &self,
        fingerprint: &Fingerprint,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The notices the member has not been told about, oldest first, marked as told in the same
    /// transaction, so each is shown at one sign-in only.
    fn take_notices(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<KeyNotice>, StoreError>> + Send;

    /// The active account with this handle (already lowercased), for the staff screen.
    fn active_account(
        &self,
        handle: &str,
    ) -> impl Future<Output = Result<Option<Account>, StoreError>> + Send;

    /// The rank of the highest global role the account holds (0 for none): what the staff
    /// rank rule compares (D69).
    fn account_rank(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<u8, StoreError>> + Send;

    /// Record a code and void the target's older live codes of the same kind, then write
    /// `audit`, in one transaction that first locks the target account, so the hourly limit
    /// holds under concurrency.
    fn issue_code(
        &self,
        code: &NewCode,
        audit: &AuditEvent,
    ) -> impl Future<Output = Result<UnixSeconds, IssueError>> + Send;

    /// Use a live link code: bind `fingerprint` to the code's account, record a
    /// [`NoticeKind::DeviceLinked`] notice and the audit row, and mark the code used, in one
    /// transaction. The first statement claims the code, so two redeemers cannot both win.
    fn redeem_link_code(
        &self,
        hash: &LoginCodeHash,
        fingerprint: &Fingerprint,
    ) -> impl Future<Output = Result<Account, RedeemError>> + Send;

    /// Use a live recovery code: bind `fingerprint`, set the password to `password_hash`
    /// (argon2id), record a [`NoticeKind::Recovered`] notice and the audit row, and mark the
    /// code used, in one transaction.
    fn redeem_recovery_code(
        &self,
        hash: &LoginCodeHash,
        fingerprint: &Fingerprint,
        password_hash: &str,
    ) -> impl Future<Output = Result<Account, RedeemError>> + Send;
}

/// The empty directory has no accounts, so no key or code exists.
impl KeyStore for NoMembers {
    fn list_keys(
        &self,
        _account: AccountId,
    ) -> impl Future<Output = Result<Vec<KeyRecord>, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn add_key(
        &self,
        _account: AccountId,
        _fingerprint: &Fingerprint,
        _audit: &AuditEvent,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn remove_key(
        &self,
        _account: AccountId,
        _fingerprint: &Fingerprint,
        _audit: &AuditEvent,
    ) -> impl Future<Output = Result<RemoveOutcome, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn note_key_use(
        &self,
        _fingerprint: &Fingerprint,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Ok(()))
    }

    fn take_notices(
        &self,
        _account: AccountId,
    ) -> impl Future<Output = Result<Vec<KeyNotice>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn active_account(
        &self,
        _handle: &str,
    ) -> impl Future<Output = Result<Option<Account>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn account_rank(
        &self,
        _account: AccountId,
    ) -> impl Future<Output = Result<u8, StoreError>> + Send {
        std::future::ready(Ok(0))
    }

    fn issue_code(
        &self,
        _code: &NewCode,
        _audit: &AuditEvent,
    ) -> impl Future<Output = Result<UnixSeconds, IssueError>> + Send {
        std::future::ready(Err(IssueError::NoAccount))
    }

    fn redeem_link_code(
        &self,
        _hash: &LoginCodeHash,
        _fingerprint: &Fingerprint,
    ) -> impl Future<Output = Result<Account, RedeemError>> + Send {
        std::future::ready(Err(RedeemError::Refused(RedeemRefusal::Code)))
    }

    fn redeem_recovery_code(
        &self,
        _hash: &LoginCodeHash,
        _fingerprint: &Fingerprint,
        _password_hash: &str,
    ) -> impl Future<Output = Result<Account, RedeemError>> + Send {
        std::future::ready(Err(RedeemError::Refused(RedeemRefusal::Code)))
    }
}
