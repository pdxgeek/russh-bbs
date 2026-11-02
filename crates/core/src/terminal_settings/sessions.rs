//! What the SSH front end needs from the store for a signed-in session.
//!
//! [`TerminalSettingsStore`] is the narrow storage trait the backends implement and test.
//! [`MemberSessions`] is the one the front end calls; it is implemented once, here, for every
//! store that has accounts and terminal settings, so a new backend (M1-G) gets it for free.

use std::future::Future;

use crate::access::{Fingerprint, NoMembers};
use crate::sanitize::Username;
use crate::store::{Account, AccountId, AccountStore, Role, StoreError, UnixSeconds};

use super::settings::TerminalSettings;

/// Per-account terminal settings and the last-login time.
pub trait TerminalSettingsStore: Send + Sync {
    /// The account's settings, or [`TerminalSettings::default`] if none were ever saved.
    fn terminal_settings(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<TerminalSettings, StoreError>> + Send;

    /// Save the account's settings, replacing any earlier ones.
    /// [`StoreError::AccountNotFound`] for an id with no account.
    fn set_terminal_settings(
        &self,
        account: AccountId,
        settings: &TerminalSettings,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Record a login now and return the time of the previous one (`None` for the first).
    fn record_login(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<UnixSeconds>, StoreError>> + Send;
}

/// What a signed-in session starts from.
#[derive(Debug, Clone)]
pub struct MemberStart {
    /// The account, for saving settings back.
    pub account: AccountId,
    /// The validated handle (also run through the sanitizer before it is drawn).
    pub handle: Username,
    /// The stored role.
    pub role: Role,
    /// The stored settings, or the defaults.
    pub settings: TerminalSettings,
    /// When the account last signed in, before this session.
    pub previous_login: Option<UnixSeconds>,
}

/// Start and update a signed-in session. Errors are the caller's to handle; the SSH front end
/// logs them and fails closed (back to the waiting room).
pub trait MemberSessions: Send + Sync {
    /// Begin a session for the account behind `key`. `None` if the key has no account.
    fn start_for_key(
        &self,
        key: &Fingerprint,
    ) -> impl Future<Output = Result<Option<MemberStart>, StoreError>> + Send;

    /// Begin a session for an account just created (the founder claim).
    fn start_for_account(
        &self,
        account: &Account,
    ) -> impl Future<Output = Result<MemberStart, StoreError>> + Send;

    /// Save settings changed during the session.
    fn save_settings(
        &self,
        account: AccountId,
        settings: &TerminalSettings,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

impl<S: AccountStore + TerminalSettingsStore> MemberSessions for S {
    async fn start_for_key(&self, key: &Fingerprint) -> Result<Option<MemberStart>, StoreError> {
        match self.account_by_fingerprint(key).await? {
            Some(account) if account.state.is_member_access() => {
                self.start_for_account(&account).await.map(Some)
            }
            Some(_) | None => Ok(None),
        }
    }

    async fn start_for_account(&self, account: &Account) -> Result<MemberStart, StoreError> {
        let settings = self.terminal_settings(account.id).await?;
        let previous_login = self.record_login(account.id).await?;
        Ok(MemberStart {
            account: account.id,
            handle: account.username.clone(),
            role: account.role,
            settings,
            previous_login,
        })
    }

    async fn save_settings(
        &self,
        account: AccountId,
        settings: &TerminalSettings,
    ) -> Result<(), StoreError> {
        self.set_terminal_settings(account, settings).await
    }
}

/// The empty directory has no accounts, so no session ever starts.
impl MemberSessions for NoMembers {
    fn start_for_key(
        &self,
        _key: &Fingerprint,
    ) -> impl Future<Output = Result<Option<MemberStart>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn start_for_account(
        &self,
        _account: &Account,
    ) -> impl Future<Output = Result<MemberStart, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn save_settings(
        &self,
        _account: AccountId,
        _settings: &TerminalSettings,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }
}
