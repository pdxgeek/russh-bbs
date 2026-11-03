//! The credit ledger over SQLite (M3-C, D79). The rules are in core's `credits`; the overdraft
//! decision is `credit_rows::apply`; this file is SQL and one transaction. The table is
//! append-only in the database itself (migration 0011); there is no update or delete here.

use russh_bbs_core::access::Slug;
use russh_bbs_core::credits::{CreditError, CreditStore, LedgerEntry, LedgerId, Posted};
use russh_bbs_core::store::{AccountId, StoreError};

use super::SqliteStore;
use crate::credit_rows::{LedgerRow, apply, balance, page};
use crate::error::backend;

impl CreditStore for SqliteStore {
    async fn append(
        &self,
        account: AccountId,
        delta: i64,
        reason: &Slug,
    ) -> Result<Posted, CreditError> {
        let id = account.as_raw();
        let reason = reason.as_str();
        let at = self.clock().now().as_raw();
        // BEGIN IMMEDIATE takes the write lock first, so the balance read and the insert are one
        // step: two spends for the last credits cannot both pass.
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let exists =
            sqlx::query_scalar!(r#"SELECT 1 AS "one!: i64" FROM accounts WHERE id = ?"#, id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(backend)?;
        if exists.is_none() {
            return Err(StoreError::AccountNotFound.into());
        }
        let current = sqlx::query_scalar!(
            r#"SELECT COALESCE(SUM(delta), 0) AS "sum!: i64" FROM credit_ledger WHERE account_id = ?"#,
            id,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        let new_balance = apply(current, delta)?;
        let row = sqlx::query_scalar!(
            r#"INSERT INTO credit_ledger (account_id, at, delta, reason) VALUES (?, ?, ?, ?)
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
            r#"SELECT COALESCE(SUM(delta), 0) AS "sum!: i64" FROM credit_ledger WHERE account_id = ?"#,
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
               WHERE account_id = ? AND id < ? ORDER BY id DESC LIMIT ?"#,
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
