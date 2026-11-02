//! Row types and their one-time mapping to core's domain types. The core never sees these.

use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{Account, AccountId, AccountState, StoreError, UnixSeconds};

/// An `accounts` row as read by the checked queries.
pub(crate) struct AccountRow {
    pub(crate) id: i64,
    pub(crate) username: String,
    pub(crate) state: String,
    pub(crate) created_at: i64,
}

impl AccountRow {
    /// Validate the stored text; anything the code could not have written is `Corrupt`.
    pub(crate) fn into_account(self) -> Result<Account, StoreError> {
        let username = sanitize_username(&self.username)
            .map_err(|_| StoreError::Corrupt("stored username is not valid".into()))?;
        let state = AccountState::parse(&self.state)
            .ok_or_else(|| StoreError::Corrupt("stored account state is not known".into()))?;
        Ok(Account {
            id: AccountId::from_raw(self.id),
            username,
            state,
            created_at: UnixSeconds::from_raw(self.created_at),
        })
    }
}
