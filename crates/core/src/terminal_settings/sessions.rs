//! What the SSH front end needs from the store for a signed-in session.
//!
//! [`TerminalSettingsStore`] is the narrow storage trait the backends implement and test.
//! [`MemberSessions`] is the one the front end calls; it is implemented once, here, for every
//! store that has accounts and terminal settings, so a new backend (M1-G) gets it for free.

use std::future::Future;

use crate::access::{
    AccessStore, Fingerprint, Granted, NoMembers, Permission, Slug, Snapshot, Via,
};
use crate::sanitize::Username;
use crate::store::{Account, AccountId, AccountStore, StoreError, UnixSeconds};

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
    /// What the account holds, loaded as the session starts (D33).
    pub access: Snapshot,
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

    /// The handle of the active member `key` is bound to, or `None` for any other key. Unlike
    /// [`MemberSessions::start_for_key`] it **records no login**: the SSH key check uses it to
    /// see whether a key names its handle, and the shell then starts the one session.
    fn handle_for_key(
        &self,
        key: &Fingerprint,
    ) -> impl Future<Output = Result<Option<Username>, StoreError>> + Send;

    /// The id of the active member `key` is bound to, or `None` for any other key. Like
    /// [`MemberSessions::handle_for_key`] it records no login: the SFTP subsystem (M4-F2) uses it
    /// to learn who is connected, and then asks [`MemberSessions::access_for`] for each action.
    fn account_for_key(
        &self,
        key: &Fingerprint,
    ) -> impl Future<Output = Result<Option<AccountId>, StoreError>> + Send;

    /// Begin a session for an account just created (the founder claim).
    fn start_for_account(
        &self,
        account: &Account,
    ) -> impl Future<Output = Result<MemberStart, StoreError>> + Send;

    /// What the account holds now; `None` is a key with no account (the waiting room). The
    /// front end calls this at the start of every action (D33), so a role or group change
    /// applies on the next one.
    fn access_for(
        &self,
        account: Option<AccountId>,
    ) -> impl Future<Output = Result<Snapshot, StoreError>> + Send;

    /// Save settings changed during the session. `granted` must be the proof of
    /// `account.edit_settings` from [`Snapshot::require`]; it also names the account, so a
    /// session can only ever save its own.
    fn save_settings(
        &self,
        granted: &Granted,
        settings: &TerminalSettings,
    ) -> impl Future<Output = Result<(), SaveError>> + Send;
}

/// Why settings were not saved.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    /// The proof was not for editing the signed-in account's settings.
    #[error("not permitted to save these settings")]
    NotPermitted,
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl<S: AccountStore + TerminalSettingsStore + AccessStore> MemberSessions for S {
    async fn start_for_key(&self, key: &Fingerprint) -> Result<Option<MemberStart>, StoreError> {
        match self.account_by_fingerprint(key).await? {
            Some(account) if account.state.is_member_access() => {
                self.start_for_account(&account).await.map(Some)
            }
            Some(_) | None => Ok(None),
        }
    }

    async fn handle_for_key(&self, key: &Fingerprint) -> Result<Option<Username>, StoreError> {
        Ok(self
            .account_by_fingerprint(key)
            .await?
            .filter(|account| account.state.is_member_access())
            .map(|account| account.username))
    }

    async fn account_for_key(&self, key: &Fingerprint) -> Result<Option<AccountId>, StoreError> {
        Ok(self
            .account_by_fingerprint(key)
            .await?
            .filter(|account| account.state.is_member_access())
            .map(|account| account.id))
    }

    async fn start_for_account(&self, account: &Account) -> Result<MemberStart, StoreError> {
        let settings = self.terminal_settings(account.id).await?;
        let previous_login = self.record_login(account.id).await?;
        let access = self.snapshot(Some(account.id)).await?;
        Ok(MemberStart {
            account: account.id,
            handle: account.username.clone(),
            access,
            settings,
            previous_login,
        })
    }

    async fn access_for(&self, account: Option<AccountId>) -> Result<Snapshot, StoreError> {
        self.snapshot(account).await
    }

    async fn save_settings(
        &self,
        granted: &Granted,
        settings: &TerminalSettings,
    ) -> Result<(), SaveError> {
        match (granted.permission(), granted.account()) {
            (Permission::AccountEditSettings, Some(account)) => {
                Ok(self.set_terminal_settings(account, settings).await?)
            }
            _ => Err(SaveError::NotPermitted),
        }
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

    fn handle_for_key(
        &self,
        _key: &Fingerprint,
    ) -> impl Future<Output = Result<Option<Username>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn account_for_key(
        &self,
        _key: &Fingerprint,
    ) -> impl Future<Output = Result<Option<AccountId>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn start_for_account(
        &self,
        _account: &Account,
    ) -> impl Future<Output = Result<MemberStart, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    /// The empty directory still has the waiting room: the built-in `waiting` role's
    /// permissions, as the stores seed them. Nobody else exists.
    fn access_for(
        &self,
        account: Option<AccountId>,
    ) -> impl Future<Output = Result<Snapshot, StoreError>> + Send {
        std::future::ready(match account {
            None => Ok(Snapshot::from_role(
                None,
                &Slug::WAITING,
                0,
                &Via::Waiting,
                [
                    Permission::WaitingView,
                    Permission::InviteRedeem,
                    Permission::MembershipApply,
                    Permission::MembershipOpen,
                ],
            )),
            Some(_) => Err(StoreError::AccountNotFound),
        })
    }

    fn save_settings(
        &self,
        _granted: &Granted,
        _settings: &TerminalSettings,
    ) -> impl Future<Output = Result<(), SaveError>> + Send {
        std::future::ready(Err(SaveError::NotPermitted))
    }
}
