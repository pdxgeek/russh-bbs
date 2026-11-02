//! The founder claim over SQLite: one transaction that is a compare-and-set on the
//! `founder_claimed` setting, so two concurrent claims make exactly one sysop (M1-D).
//!
//! The setting row is inserted with `ON CONFLICT DO NOTHING`. The statement is the first write
//! of the transaction, so SQLite takes the write lock there; a second claimant waits for the
//! first to commit, then finds the row present and changes nothing. The account, its key and
//! the flag commit together or not at all.

use russh_bbs_core::access::{Fingerprint, MemberDirectory, RoleDirectory};
use russh_bbs_core::founder::FounderStore;
use russh_bbs_core::sanitize::Username;
use russh_bbs_core::store::{Account, AccountState, Role, SettingKey, StoreError};
use sqlx::error::ErrorKind;

use super::SqliteStore;
use crate::directory;
use crate::error::{backend, kind};
use crate::rows::AccountRow;

impl FounderStore for SqliteStore {
    async fn founder_claimed(&self) -> Result<bool, StoreError> {
        directory::founder_claimed(self).await
    }

    async fn claim_founder(
        &self,
        username: Username,
        fingerprint: &Fingerprint,
    ) -> Result<Option<Account>, StoreError> {
        let flag = SettingKey::FounderClaimed.as_str();
        let skeleton = username.skeleton();
        let name = username.as_str();
        let role = Role::Sysop.as_str();
        let state = AccountState::Active.as_str();
        let fingerprint = fingerprint.as_str();

        let mut tx = self.pool().begin().await.map_err(backend)?;
        let flagged = sqlx::query!(
            "INSERT INTO settings (key, value) VALUES (?, 'true') ON CONFLICT (key) DO NOTHING",
            flag,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        if flagged.rows_affected() == 0 {
            // Already claimed. Nothing was written; dropping the transaction rolls it back.
            return Ok(None);
        }
        let row = sqlx::query_as!(
            AccountRow,
            r#"INSERT INTO accounts (username, username_skeleton, role, state)
               VALUES (?, ?, ?, ?)
               RETURNING id AS "id!", username, role, state, created_at"#,
            name,
            skeleton,
            role,
            state,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => StoreError::UsernameTaken,
            _ => backend(error),
        })?;
        sqlx::query!(
            "INSERT INTO account_keys (fingerprint, account_id) VALUES (?, ?)",
            fingerprint,
            row.id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => StoreError::FingerprintTaken,
            _ => backend(error),
        })?;
        tx.commit().await.map_err(backend)?;
        row.into_account().map(Some)
    }
}

impl RoleDirectory for SqliteStore {
    async fn role_of(&self, key: &Fingerprint) -> Option<Role> {
        directory::role_of(self, key).await
    }
}

impl MemberDirectory for SqliteStore {
    async fn is_member(&self, key: &Fingerprint) -> bool {
        directory::is_member(self, key).await
    }
}
