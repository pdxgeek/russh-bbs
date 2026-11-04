//! The founder claim over SQLite: one transaction that is a compare-and-set on the
//! `founder_claimed` setting, so two concurrent claims make exactly one sysop (M1-D).
//!
//! The setting row is inserted with `ON CONFLICT DO NOTHING`. The statement is the first write
//! of the transaction, so SQLite takes the write lock there; a second claimant waits for the
//! first to commit, then finds the row present and changes nothing. The account, its key and
//! the flag commit together or not at all.

use russh_bbs_core::access::{Fingerprint, MemberDirectory};
use russh_bbs_core::audit::events;
use russh_bbs_core::founder::FounderStore;
use russh_bbs_core::sanitize::Username;
use russh_bbs_core::store::{Account, AccountState, SettingKey, StoreError};
use sqlx::error::ErrorKind;

use super::SqliteStore;
use super::binding::{BindFailure, bind};
use crate::binding::Binding;
use crate::directory;
use crate::error::{backend, kind};
use crate::rows::AccountRow;

impl SqliteStore {
    /// Make the founder, bound to `binding`, if and only if nobody has claimed (D41, D94).
    async fn claim_bound(
        &self,
        username: Username,
        binding: Binding<'_>,
    ) -> Result<Option<Account>, StoreError> {
        let flag = SettingKey::FounderClaimed.as_str();
        let skeleton = username.skeleton();
        let name = username.as_str();
        let state = AccountState::Active.as_str();

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
            r#"INSERT INTO accounts (username, username_skeleton, state)
               VALUES (?, ?, ?)
               RETURNING id AS "id!", username, state, created_at"#,
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
            "INSERT INTO access_grant (account_id, role) VALUES (?, 'sysop')",
            row.id,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        bind(&mut tx, row.id, binding)
            .await
            .map_err(|failure| match failure {
                BindFailure::KeyTaken => StoreError::FingerprintTaken,
                BindFailure::Store(error) => error,
            })?;
        let account = row.into_account()?;
        // The claim and its audit row commit together or not at all.
        self.audit_in(&mut tx, &events::founder_claimed(account.id))
            .await?;
        tx.commit().await.map_err(backend)?;
        Ok(Some(account))
    }
}

impl FounderStore for SqliteStore {
    async fn founder_claimed(&self) -> Result<bool, StoreError> {
        directory::founder_claimed(self).await
    }
    async fn claim_founder(
        &self,
        username: Username,
        fingerprint: &Fingerprint,
    ) -> Result<Option<Account>, StoreError> {
        self.claim_bound(username, Binding::Key(fingerprint.as_str()))
            .await
    }

    async fn claim_founder_password(
        &self,
        username: Username,
        password_hash: &str,
        fingerprint: Option<&Fingerprint>,
    ) -> Result<Option<Account>, StoreError> {
        let binding = match fingerprint {
            Some(fingerprint) => Binding::KeyAndPassword(fingerprint.as_str(), password_hash),
            None => Binding::Password(password_hash),
        };
        self.claim_bound(username, binding).await
    }
}

impl MemberDirectory for SqliteStore {
    async fn is_member(&self, key: &Fingerprint) -> bool {
        directory::is_member(self, key).await
    }
}
