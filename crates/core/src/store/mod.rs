//! Storage traits M1 needs: accounts, the key fingerprints bound to them, and board settings.
//!
//! The SQLite implementation lives in `russh-bbs-store`; this crate only defines the shapes so
//! rules never see a row type or a SQL error (decision D6). Ids are newtypes minted by the store.
//!
//! M1 stores fingerprints and public metadata only. Nothing here holds a secret.

use std::fmt;
use std::future::Future;

use crate::access::{Fingerprint, Slug};
use crate::sanitize::Username;

mod state;

pub use state::{AccountState, AccountStateStore, StateChange, StateChangeError};

/// Store-assigned identifier of an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AccountId(i64);

impl AccountId {
    /// Wrap a raw id. Call this only where a row is mapped (the store); everyone else receives
    /// ids from the store.
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

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Seconds since the Unix epoch, as recorded by the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnixSeconds(i64);

impl UnixSeconds {
    /// Wrap a raw timestamp read from storage.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// The raw number of seconds.
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }
}

/// An account to create. The username has already passed the sanitizer.
#[derive(Debug, Clone)]
pub struct NewAccount {
    /// Validated display name; also the uniqueness key through its skeleton (D17).
    pub username: Username,
    /// Role to give besides `member`, which every active account has through the `everyone`
    /// group (D33). `member` writes no grant; any other built-in or custom role does.
    pub role: Slug,
    /// Initial state.
    pub state: AccountState,
}

/// A stored account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Store-assigned id.
    pub id: AccountId,
    /// Display name, re-validated when read.
    pub username: Username,
    /// State.
    pub state: AccountState,
    /// When the account was created.
    pub created_at: UnixSeconds,
}

/// Why a store call failed. Callers that guard access treat every error as "no".
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The key is already bound to an account (to this one or another).
    #[error("fingerprint is already bound to an account")]
    FingerprintTaken,
    /// Another account already has this username (or one that looks like it).
    #[error("username is already taken")]
    UsernameTaken,
    /// No account has this id.
    #[error("no such account")]
    AccountNotFound,
    /// A stored value is not one the code writes: the database was edited or is from the future.
    #[error("stored data is invalid: {0}")]
    Corrupt(String),
    /// The database failed (I/O, locked, migration). The cause is kept for logs.
    #[error("storage backend failed")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Accounts and the keys bound to them.
pub trait AccountStore: Send + Sync {
    /// Create an account. Fails with [`StoreError::UsernameTaken`] on a duplicate name.
    fn create_account(
        &self,
        account: NewAccount,
    ) -> impl Future<Output = Result<Account, StoreError>> + Send;

    /// Bind a key to an account. A key belongs to at most one account:
    /// [`StoreError::FingerprintTaken`] otherwise, [`StoreError::AccountNotFound`] for a bad id.
    fn attach_fingerprint(
        &self,
        account: AccountId,
        fingerprint: &Fingerprint,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The account a key is bound to, or `None` for an unknown key.
    fn account_by_fingerprint(
        &self,
        fingerprint: &Fingerprint,
    ) -> impl Future<Output = Result<Option<Account>, StoreError>> + Send;
}

/// Names of board-level settings. A closed set, so a typo cannot create a setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingKey {
    /// Who may register: a [`RegistrationMode`].
    RegistrationMode,
    /// Set to `true` once the founder claim has been used. Written by M1-D.
    FounderClaimed,
    /// The sysop's waiting-room banner text. Raw; sanitized when shown.
    WaitingRoomBanner,
    /// The sysop's waiting-room rules text. Raw; sanitized when shown.
    WaitingRoomRules,
}

impl SettingKey {
    /// The stable key stored in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RegistrationMode => "registration_mode",
            Self::FounderClaimed => "founder_claimed",
            Self::WaitingRoomBanner => "waiting_room_banner",
            Self::WaitingRoomRules => "waiting_room_rules",
        }
    }
}

/// Who may register (decision D2/D23). The sysop sets it; the default is [`Self::Invite`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RegistrationMode {
    /// Unknown keys reach only a waiting room.
    #[default]
    Invite,
    /// Anyone may apply; an account is inactive until approved.
    Apply,
    /// An unknown key may create an active account at once.
    Open,
}

impl RegistrationMode {
    /// The stable text stored in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Invite => "invite",
            Self::Apply => "apply",
            Self::Open => "open",
        }
    }

    /// Parse the stored text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "invite" => Some(Self::Invite),
            "apply" => Some(Self::Apply),
            "open" => Some(Self::Open),
            _ => None,
        }
    }
}

/// Board-level key/value settings.
pub trait SettingsStore: Send + Sync {
    /// The stored value, or `None` if never set.
    fn get_setting(
        &self,
        key: SettingKey,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send;

    /// Store a value, replacing any earlier one.
    fn set_setting(
        &self,
        key: SettingKey,
        value: &str,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

/// The registration mode: the stored value, or [`RegistrationMode::Invite`] if never set.
///
/// An unrecognised stored value is an error, never a silent fall back to a looser mode.
pub async fn registration_mode<S: SettingsStore>(
    settings: &S,
) -> Result<RegistrationMode, StoreError> {
    match settings.get_setting(SettingKey::RegistrationMode).await? {
        None => Ok(RegistrationMode::default()),
        Some(text) => RegistrationMode::parse(&text)
            .ok_or_else(|| StoreError::Corrupt("unknown registration_mode value".into())),
    }
}

/// Set the registration mode.
pub async fn set_registration_mode<S: SettingsStore>(
    settings: &S,
    mode: RegistrationMode,
) -> Result<(), StoreError> {
    settings
        .set_setting(SettingKey::RegistrationMode, mode.as_str())
        .await
}

#[cfg(test)]
mod tests;
