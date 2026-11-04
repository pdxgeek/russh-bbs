//! Password hashes over SQLite (M3-D, D81). The table holds argon2id PHC strings only; nothing
//! here ever sees a plain-text password.

use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::audit::AuditEvent;
use russh_bbs_core::keys::NoticeKind;
use russh_bbs_core::passwords::{LoginRecord, PasswordStore};
use russh_bbs_core::store::{AccountId, StoreError};
use sqlx::error::ErrorKind;

use super::SqliteStore;
use crate::error::{backend, kind};
use crate::password_rows::LoginRow;

impl PasswordStore for SqliteStore {
    async fn login_record(&self, handle: &str) -> Result<Option<LoginRecord>, StoreError> {
        // Handles are ASCII and unique ignoring case, so `lower()` is the comparison.
        let row = sqlx::query_as!(
            LoginRow,
            r#"SELECT a.id AS "id!", a.username AS "username!", a.state AS "state!",
                      a.created_at AS "created_at!", p.hash AS "hash?"
               FROM accounts a LEFT JOIN account_password p ON p.account_id = a.id
               WHERE lower(a.username) = ?"#,
            handle,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        row.map(LoginRow::into_record).transpose()
    }

    async fn stored_hash(&self, account: AccountId) -> Result<Option<String>, StoreError> {
        let id = account.as_raw();
        let row = sqlx::query!(
            r#"SELECT a.id AS "id!", p.hash AS "hash?"
               FROM accounts a LEFT JOIN account_password p ON p.account_id = a.id
               WHERE a.id = ?"#,
            id,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        row.map(|found| found.hash)
            .ok_or(StoreError::AccountNotFound)
    }

    async fn has_key(&self, account: AccountId) -> Result<bool, StoreError> {
        let id = account.as_raw();
        let keys = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM account_keys WHERE account_id = ?"#,
            id,
        )
        .fetch_one(self.pool())
        .await
        .map_err(backend)?;
        Ok(keys > 0)
    }

    async fn link_key(
        &self,
        account: AccountId,
        fingerprint: &Fingerprint,
        audit: &AuditEvent,
    ) -> Result<(), StoreError> {
        let id = account.as_raw();
        let fingerprint = fingerprint.as_str();
        let now = self.clock().now().as_raw();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        sqlx::query!(
            "INSERT INTO account_keys (fingerprint, account_id, created_at) VALUES (?, ?, ?)",
            fingerprint,
            id,
            now,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => StoreError::FingerprintTaken,
            Some(ErrorKind::ForeignKeyViolation) => StoreError::AccountNotFound,
            _ => backend(error),
        })?;
        // The member is told at the next sign-in (FIX-M3-11).
        let (kind, at) = (NoticeKind::KeyLinked.as_str(), now);
        sqlx::query!(
            "INSERT INTO key_notice (account_id, kind, fingerprint, at) VALUES (?, ?, ?, ?)",
            id,
            kind,
            fingerprint,
            at,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        self.audit_in(&mut tx, audit).await?;
        tx.commit().await.map_err(backend)
    }

    async fn set_hash(
        &self,
        account: AccountId,
        hash: Option<&str>,
        audit: &AuditEvent,
    ) -> Result<(), StoreError> {
        let id = account.as_raw();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let changed = if let Some(hash) = hash {
            sqlx::query!(
                "INSERT INTO account_password (account_id, hash) VALUES (?, ?)
                 ON CONFLICT (account_id) DO UPDATE SET
                     hash = excluded.hash, updated_at = unixepoch()",
                id,
                hash,
            )
            .execute(&mut *tx)
            .await
            .map_err(|error| match kind(&error) {
                Some(ErrorKind::ForeignKeyViolation) => StoreError::AccountNotFound,
                _ => backend(error),
            })?;
            true
        } else {
            let removed = sqlx::query!("DELETE FROM account_password WHERE account_id = ?", id)
                .execute(&mut *tx)
                .await
                .map_err(backend)?
                .rows_affected();
            if removed == 0 {
                let exists = sqlx::query_scalar!(
                    r#"SELECT count(*) AS "n!: i64" FROM accounts WHERE id = ?"#,
                    id
                )
                .fetch_one(&mut *tx)
                .await
                .map_err(backend)?;
                if exists == 0 {
                    return Err(StoreError::AccountNotFound);
                }
            }
            removed > 0
        };
        if changed {
            self.audit_in(&mut tx, audit).await?;
        }
        tx.commit().await.map_err(backend)
    }

    async fn upgrade_hash(
        &self,
        account: AccountId,
        old: &str,
        new: &str,
    ) -> Result<bool, StoreError> {
        let id = account.as_raw();
        let done = sqlx::query!(
            "UPDATE account_password SET hash = ? WHERE account_id = ? AND hash = ?",
            new,
            id,
            old,
        )
        .execute(self.pool())
        .await
        .map_err(backend)?;
        Ok(done.rows_affected() > 0)
    }
}
