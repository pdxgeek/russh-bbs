//! Drafts over SQLite (M3-B): one row per member per context. The rules are in core's
//! `drafts`; this file is SQL and one transaction (the capped write).

use russh_bbs_core::drafts::{Body, ContextKey, DraftPut, DraftStore, DraftSummary, StoredDraft};
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};
use sqlx::error::ErrorKind;

use super::SqliteStore;
use crate::draft_rows::{cap, stored, summary};
use crate::error::{backend, kind};

impl DraftStore for SqliteStore {
    async fn put_draft(
        &self,
        account: AccountId,
        context: &ContextKey,
        body: &Body,
        now: UnixSeconds,
        max_drafts: usize,
    ) -> Result<DraftPut, StoreError> {
        let id = account.as_raw();
        let context = context.as_str();
        let text = body.as_str();
        let at = now.as_raw();
        let max = cap(max_drafts);
        // The UPDATE comes first because it takes SQLite's write lock: a second save waits for
        // this transaction, so the count below cannot go stale before the INSERT.
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let replaced = sqlx::query!(
            "UPDATE draft SET body = ?, updated_at = ? WHERE account_id = ? AND context = ?",
            text,
            at,
            id,
            context,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?
        .rows_affected();
        if replaced > 0 {
            tx.commit().await.map_err(backend)?;
            return Ok(DraftPut::Replaced);
        }
        let created = sqlx::query!(
            "INSERT INTO draft (account_id, context, body, updated_at)
             SELECT ?, ?, ?, ? WHERE (SELECT count(*) FROM draft WHERE account_id = ?) < ?",
            id,
            context,
            text,
            at,
            id,
            max,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::ForeignKeyViolation) => StoreError::AccountNotFound,
            _ => backend(error),
        })?
        .rows_affected();
        tx.commit().await.map_err(backend)?;
        Ok(if created > 0 {
            DraftPut::Created
        } else {
            DraftPut::Full
        })
    }

    async fn get_draft(
        &self,
        account: AccountId,
        context: &ContextKey,
    ) -> Result<Option<StoredDraft>, StoreError> {
        let id = account.as_raw();
        let context = context.as_str();
        sqlx::query!(
            "SELECT context, body, updated_at FROM draft WHERE account_id = ? AND context = ?",
            id,
            context,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?
        .map(|row| stored(&row.context, row.body, row.updated_at))
        .transpose()
    }

    async fn delete_draft(
        &self,
        account: AccountId,
        context: &ContextKey,
    ) -> Result<bool, StoreError> {
        let id = account.as_raw();
        let context = context.as_str();
        let deleted = sqlx::query!(
            "DELETE FROM draft WHERE account_id = ? AND context = ?",
            id,
            context,
        )
        .execute(self.pool())
        .await
        .map_err(backend)?
        .rows_affected();
        Ok(deleted > 0)
    }

    async fn list_drafts(&self, account: AccountId) -> Result<Vec<DraftSummary>, StoreError> {
        let id = account.as_raw();
        sqlx::query!(
            r#"SELECT context, updated_at, length(body) AS "chars!: i64"
               FROM draft WHERE account_id = ? ORDER BY updated_at DESC, context"#,
            id,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?
        .into_iter()
        .map(|row| summary(&row.context, row.updated_at, row.chars))
        .collect()
    }
}
