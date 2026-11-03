//! The row read for a password login, mapped once to core's types.

use russh_bbs_core::passwords::LoginRecord;
use russh_bbs_core::store::StoreError;

use crate::rows::AccountRow;

/// An `accounts` row with its `account_password` hash, if any.
pub(crate) struct LoginRow {
    pub(crate) id: i64,
    pub(crate) username: String,
    pub(crate) state: String,
    pub(crate) created_at: i64,
    pub(crate) hash: Option<String>,
}

impl LoginRow {
    /// Validate the stored text; anything the code could not have written is `Corrupt`.
    pub(crate) fn into_record(self) -> Result<LoginRecord, StoreError> {
        let account = AccountRow {
            id: self.id,
            username: self.username,
            state: self.state,
            created_at: self.created_at,
        }
        .into_account()?;
        Ok(LoginRecord {
            account,
            hash: self.hash,
        })
    }
}
