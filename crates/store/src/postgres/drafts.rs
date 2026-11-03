//! Drafts over Postgres (M3-B): one row per member per context. The rules are in core's
//! `drafts`; this file is SQL and one transaction (the capped write).

use russh_bbs_core::drafts::{Body, ContextKey, DraftPut, DraftStore, DraftSummary, StoredDraft};
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

use super::PostgresStore;
use crate::draft_rows::{cap, stored, summary};
use crate::error::backend;

impl DraftStore for PostgresStore {
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
        let mut tx = self.pool().begin().await.map_err(backend)?;
        // Lock the account row first: two saves for one member queue here, so the count below
        // cannot go stale before the INSERT. NO KEY UPDATE does not block other tables'
        // foreign keys into the account.
        let locked = sqlx::query_scalar!(
            "SELECT id FROM accounts WHERE id = $1 FOR NO KEY UPDATE",
            id,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if locked.is_none() {
            return Err(StoreError::AccountNotFound);
        }
        let replaced = sqlx::query!(
            "UPDATE draft SET body = $1, updated_at = $2 WHERE account_id = $3 AND context = $4",
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
             SELECT $1, $2, $3, $4 WHERE (SELECT count(*) FROM draft WHERE account_id = $1) < $5",
            id,
            context,
            text,
            at,
            max,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?
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
            "SELECT context, body, updated_at FROM draft WHERE account_id = $1 AND context = $2",
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
            "DELETE FROM draft WHERE account_id = $1 AND context = $2",
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
            r#"SELECT context, updated_at, char_length(body)::bigint AS "chars!"
               FROM draft WHERE account_id = $1 ORDER BY updated_at DESC, context"#,
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
