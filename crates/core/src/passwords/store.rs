//! The storage trait for password hashes: persistence only. The rules (who may set one, what a
//! login must do) are in [`super::service`].
//!
//! A hash lives in its own table (`account_password`), never in a row that a list or a profile
//! reads, so no query for a member's data can return it by accident. Nothing here takes or
//! returns a plain-text password.

use std::future::Future;

use crate::access::Fingerprint;
use crate::audit::AuditEvent;
use crate::store::{Account, AccountId, StoreError};

/// A stored account and its hash, as a login needs them.
#[derive(Debug, Clone)]
pub struct LoginRecord {
    /// The account behind the handle.
    pub account: Account,
    /// Its argon2id hash, if it set a password.
    pub hash: Option<String>,
}

/// Where password hashes are kept.
pub trait PasswordStore: Send + Sync {
    /// The account whose handle is `handle` (already lowercased), with its hash. `None` for
    /// no such account.
    fn login_record(
        &self,
        handle: &str,
    ) -> impl Future<Output = Result<Option<LoginRecord>, StoreError>> + Send;

    /// The account's hash, `None` if it has no password. [`StoreError::AccountNotFound`] for
    /// an id with no account.
    fn stored_hash(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send;

    /// Whether an SSH key is bound to the account. An account made over telnet has none
    /// (D94), and then its password is its only way in.
    fn has_key(&self, account: AccountId) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// Bind `fingerprint` to the account and write `audit` in the same transaction (D94 (8)).
    /// [`StoreError::FingerprintTaken`] if the key belongs to an account already (nothing is
    /// written), [`StoreError::AccountNotFound`] for an id with no account.
    fn link_key(
        &self,
        account: AccountId,
        fingerprint: &Fingerprint,
        audit: &AuditEvent,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Store `hash` for the account, or remove its password with `None`, and write `audit` in
    /// the same transaction. Removing a password the account does not have changes nothing and
    /// writes no audit row. [`StoreError::AccountNotFound`] for an id with no account.
    fn set_hash(
        &self,
        account: AccountId,
        hash: Option<&str>,
        audit: &AuditEvent,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Replace `old` with `new` only if `old` is still the stored hash (a login upgrading its
    /// parameters must not undo a password change that happened meanwhile). No audit row: the
    /// password did not change. Whether it replaced anything.
    fn upgrade_hash(
        &self,
        account: AccountId,
        old: &str,
        new: &str,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;
}

/// The empty directory has no accounts, so no password exists and none can be set.
impl PasswordStore for crate::access::NoMembers {
    fn login_record(
        &self,
        _handle: &str,
    ) -> impl Future<Output = Result<Option<LoginRecord>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn stored_hash(
        &self,
        _account: AccountId,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn has_key(
        &self,
        _account: AccountId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn link_key(
        &self,
        _account: AccountId,
        _fingerprint: &Fingerprint,
        _audit: &AuditEvent,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn set_hash(
        &self,
        _account: AccountId,
        _hash: Option<&str>,
        _audit: &AuditEvent,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn upgrade_hash(
        &self,
        _account: AccountId,
        _old: &str,
        _new: &str,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(false))
    }
}
