//! The credit ledger over Postgres (M3-C, D79). The rules are in core's `credits`; the overdraft
//! decision is `credit_rows::apply`; this file is SQL and one transaction. The table is
//! append-only in the database itself (migration 0011); there is no update or delete here.

use russh_bbs_core::access::Slug;
use russh_bbs_core::credits::{CreditError, CreditStore, LedgerEntry, LedgerId, Posted};
use russh_bbs_core::store::{AccountId, StoreError};

use super::PostgresStore;
use crate::credit_rows::{LedgerRow, apply, balance, page};
use crate::error::backend;

impl CreditStore for PostgresStore {
    async fn append(
        &self,
        account: AccountId,
        delta: i64,
        reason: &Slug,
    ) -> Result<Posted, CreditError> {
        let id = account.as_raw();
        let reason = reason.as_str();
        let at = self.clock().now().as_raw();
        // Locking the account row serializes writers for this account, so the balance read and
        // the insert are one step: two spends for the last credits cannot both pass. (NO KEY
        // UPDATE conflicts with itself but not with the key-share locks foreign keys take.)
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let exists = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM accounts WHERE id = $1 FOR NO KEY UPDATE"#,
            id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if exists.is_none() {
            return Err(StoreError::AccountNotFound.into());
        }
        let current = sqlx::query_scalar!(
            r#"SELECT COALESCE(SUM(delta), 0)::bigint AS "sum!" FROM credit_ledger WHERE account_id = $1"#,
            id,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        let new_balance = apply(current, delta)?;
        let row = sqlx::query_scalar!(
            r#"INSERT INTO credit_ledger (account_id, at, delta, reason) VALUES ($1, $2, $3, $4)
               RETURNING id AS "id!""#,
            id,
            at,
            delta,
            reason,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(Posted {
            id: LedgerId::from_raw(row),
            balance: new_balance,
        })
    }

    async fn balance(&self, account: AccountId) -> Result<u64, StoreError> {
        let id = account.as_raw();
        let sum = sqlx::query_scalar!(
            r#"SELECT COALESCE(SUM(delta), 0)::bigint AS "sum!" FROM credit_ledger WHERE account_id = $1"#,
            id,
        )
        .fetch_one(self.pool())
        .await
        .map_err(backend)?;
        balance(sum)
    }

    async fn entries(
        &self,
        account: AccountId,
        before: Option<LedgerId>,
        limit: u32,
    ) -> Result<Vec<LedgerEntry>, StoreError> {
        let id = account.as_raw();
        let before = before.map_or(i64::MAX, LedgerId::as_raw);
        let limit = page(limit);
        let rows = sqlx::query_as!(
            LedgerRow,
            r#"SELECT id AS "id!", account_id, at, delta, reason FROM credit_ledger
               WHERE account_id = $1 AND id < $2 ORDER BY id DESC LIMIT $3"#,
            id,
            before,
            limit,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        rows.into_iter().map(LedgerRow::into_entry).collect()
    }
}
