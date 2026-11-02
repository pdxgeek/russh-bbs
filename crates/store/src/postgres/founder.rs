//! The founder claim over Postgres: one transaction that is a compare-and-set on the
//! `founder_claimed` setting, so two concurrent claims make exactly one sysop (M1-D).
//!
//! The setting row is inserted with `ON CONFLICT DO NOTHING`. The statement is the first write
//! of the transaction; a second claimant's insert of the same key waits on the unique index
//! until the first transaction commits, then conflicts and changes nothing (`rows_affected` is
//! 0). If the first rolls back, the waiter proceeds and claims. The account, its key and the
//! flag commit together or not at all. No explicit isolation level is needed: the unique index
//! is the compare-and-set.

use russh_bbs_core::access::{Fingerprint, MemberDirectory};
use russh_bbs_core::audit::events;
use russh_bbs_core::founder::FounderStore;
use russh_bbs_core::sanitize::Username;
use russh_bbs_core::store::{Account, AccountState, SettingKey, StoreError};
use sqlx::error::ErrorKind;

use super::PostgresStore;
use crate::directory;
use crate::error::{backend, kind};
use crate::rows::AccountRow;

impl FounderStore for PostgresStore {
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
        let state = AccountState::Active.as_str();
        let fingerprint = fingerprint.as_str();

        let mut tx = self.pool().begin().await.map_err(backend)?;
        let flagged = sqlx::query!(
            "INSERT INTO settings (key, value) VALUES ($1, 'true') ON CONFLICT (key) DO NOTHING",
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
            "INSERT INTO accounts (username, username_skeleton, state)
             VALUES ($1, $2, $3)
             RETURNING id, username, state, created_at",
            name,
            skeleton,
            state,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => StoreError::UsernameTaken,
            _ => backend(error),
        })?;
        sqlx::query!(
            "INSERT INTO access_grant (account_id, role) VALUES ($1, 'sysop')",
            row.id,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query!(
            "INSERT INTO account_keys (fingerprint, account_id) VALUES ($1, $2)",
            fingerprint,
            row.id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => StoreError::FingerprintTaken,
            _ => backend(error),
        })?;
        let account = row.into_account()?;
        // The claim and its audit row commit together or not at all.
        self.audit_in(&mut tx, &events::founder_claimed(account.id))
            .await?;
        tx.commit().await.map_err(backend)?;
        Ok(Some(account))
    }
}

impl MemberDirectory for PostgresStore {
    async fn is_member(&self, key: &Fingerprint) -> bool {
        directory::is_member(self, key).await
    }
}
