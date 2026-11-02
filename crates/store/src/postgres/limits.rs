//! Limits over Postgres (M2-H): role values, member overrides and per-day usage. The rules are in
//! core's `limits`; this file is SQL and one transaction (the capped increment).

use russh_bbs_core::access::Slug;
use russh_bbs_core::limits::{
    Day, Limit, LimitKind, LimitStore, LimitWriteError, MemberLimit, RoleLimit, UsageOutcome,
};
use russh_bbs_core::store::{AccountId, StoreError};
use sqlx::error::ErrorKind;

use super::PostgresStore;
use crate::error::{backend, kind};
use crate::limit_rows::{
    MemberLimitRow, RoleLimitRow, amount, member_limits, role_limits, stored, to_i64,
};

impl LimitStore for PostgresStore {
    async fn role_limits(&self, roles: &[Slug]) -> Result<Vec<RoleLimit>, StoreError> {
        // A handful of rows (roles x kinds): read them all and pick in Rust, the same on both
        // backends.
        let rows = sqlx::query_as!(
            RoleLimitRow,
            r#"SELECT role, kind, value FROM role_limit -- postgres"#
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        role_limits(rows, roles)
    }

    async fn member_limits(&self, account: AccountId) -> Result<Vec<MemberLimit>, StoreError> {
        let id = account.as_raw();
        let rows = sqlx::query_as!(
            MemberLimitRow,
            "SELECT kind, value FROM member_limit WHERE account_id = $1",
            id,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        member_limits(rows)
    }

    async fn set_role_limit(
        &self,
        role: &Slug,
        kind_: LimitKind,
        limit: Limit,
    ) -> Result<(), LimitWriteError> {
        let role = role.as_str();
        let name = kind_.name();
        let value = stored(limit)?;
        sqlx::query!(
            "INSERT INTO role_limit (role, kind, value) VALUES ($1, $2, $3)
             ON CONFLICT (role, kind) DO UPDATE SET value = excluded.value",
            role,
            name,
            value,
        )
        .execute(self.pool())
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::ForeignKeyViolation) => LimitWriteError::UnknownRole,
            _ => backend(error).into(),
        })?;
        Ok(())
    }

    async fn set_member_limit(
        &self,
        account: AccountId,
        kind_: LimitKind,
        limit: Option<Limit>,
    ) -> Result<(), LimitWriteError> {
        let id = account.as_raw();
        let name = kind_.name();
        match limit {
            Some(limit) => {
                let value = stored(limit)?;
                sqlx::query!(
                    "INSERT INTO member_limit (account_id, kind, value) VALUES ($1, $2, $3)
                     ON CONFLICT (account_id, kind) DO UPDATE SET value = excluded.value",
                    id,
                    name,
                    value,
                )
                .execute(self.pool())
                .await
                .map_err(|error| match kind(&error) {
                    Some(ErrorKind::ForeignKeyViolation) => LimitWriteError::UnknownAccount,
                    _ => backend(error).into(),
                })?;
            }
            None => {
                sqlx::query!(
                    "DELETE FROM member_limit WHERE account_id = $1 AND kind = $2",
                    id,
                    name,
                )
                .execute(self.pool())
                .await
                .map_err(backend)?;
            }
        }
        Ok(())
    }

    async fn usage(
        &self,
        account: AccountId,
        kind_: LimitKind,
        day: Day,
    ) -> Result<u64, StoreError> {
        let id = account.as_raw();
        let name = kind_.name();
        let day = day.as_raw();
        let used = sqlx::query_scalar!(
            "SELECT amount FROM limit_usage WHERE account_id = $1 AND kind = $2 AND day = $3",
            id,
            name,
            day,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        used.map_or(Ok(0), amount)
    }

    async fn add_usage(
        &self,
        account: AccountId,
        kind_: LimitKind,
        day: Day,
        add: u64,
        cap: Option<u64>,
    ) -> Result<UsageOutcome, StoreError> {
        let id = account.as_raw();
        let name = kind_.name();
        let day = day.as_raw();
        let add = to_i64(add)?;
        let cap = cap.map_or(Ok(i64::MAX), to_i64)?;
        // The UPDATE locks the row, so a concurrent add waits and then re-checks the cap.
        let mut tx = self.pool().begin().await.map_err(backend)?;
        sqlx::query!(
            "INSERT INTO limit_usage (account_id, kind, day, amount) VALUES ($1, $2, $3, 0)
             ON CONFLICT (account_id, kind, day) DO NOTHING",
            id,
            name,
            day,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::ForeignKeyViolation) => StoreError::AccountNotFound,
            _ => backend(error),
        })?;
        let changed = sqlx::query!(
            "UPDATE limit_usage SET amount = amount + $4::bigint
             WHERE account_id = $1 AND kind = $2 AND day = $3 AND amount <= $5::bigint - $4::bigint",
            id,
            name,
            day,
            add,
            cap,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?
        .rows_affected();
        let total = sqlx::query_scalar!(
            "SELECT amount FROM limit_usage WHERE account_id = $1 AND kind = $2 AND day = $3",
            id,
            name,
            day,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let total = amount(total)?;
        Ok(if changed == 0 {
            UsageOutcome::Full(total)
        } else {
            UsageOutcome::Added(total)
        })
    }

    async fn prune_usage(&self, account: AccountId, before: Day) -> Result<(), StoreError> {
        let id = account.as_raw();
        let before = before.as_raw();
        sqlx::query!(
            "DELETE FROM limit_usage WHERE account_id = $1 AND day < $2",
            id,
            before,
        )
        .execute(self.pool())
        .await
        .map_err(backend)?;
        Ok(())
    }
}
