//! Keys, device-link and recovery codes, and key notices over Postgres (FIX-M3-11, D97): the
//! same behaviour as the SQLite backend, in Postgres SQL.
//!
//! **Last way in.** Removal locks the account row (`FOR UPDATE`) before the one `DELETE` that
//! checks "a password or another key", so two removals for one account run one after the other
//! and cannot both take the last way in.
//!
//! **Single use.** A redemption's first statement is an `UPDATE login_code SET redeemed_at ...
//! WHERE code_hash = $2 AND redeemed_at IS NULL ...`. A concurrent redeemer of the same code
//! blocks on that row's lock until the first transaction ends, then re-evaluates the `WHERE`
//! (read committed) against the committed row and matches nothing. A key that already has an
//! account rolls the redemption back, and the code is not used up.
//!
//! **Rate limit.** Making a code locks the target's `accounts` row, then counts, so two makers
//! cannot both take the last place.

use russh_bbs_core::access::{AccessStore as _, Fingerprint};
use russh_bbs_core::audit::{AuditEvent, events};
use russh_bbs_core::keys::{
    CodeKind, IssueError, KeyNotice, KeyRecord, KeyStore, LoginCodeHash, NewCode, NoticeKind,
    RedeemError, RedeemRefusal, RemoveOutcome,
};
use russh_bbs_core::store::{Account, AccountId, StoreError, UnixSeconds};
use sqlx::PgConnection;
use sqlx::error::ErrorKind;

use super::PostgresStore;
use crate::error::{backend, kind};
use crate::key_rows::{KeyRow, NoticeRow, notices_in_order};
use crate::rows::AccountRow;

/// One hour, for the per-account code limit.
const HOUR_SECS: i64 = 3_600;

impl PostgresStore {
    /// Write a notice for `account` in the open transaction.
    async fn notice_in(
        &self,
        conn: &mut PgConnection,
        account: i64,
        notice: NoticeKind,
        fingerprint: Option<&Fingerprint>,
    ) -> Result<(), StoreError> {
        let now = self.clock().now().as_raw();
        let kind = notice.as_str();
        let fingerprint = fingerprint.map(Fingerprint::as_str);
        sqlx::query!(
            "INSERT INTO key_notice (account_id, kind, fingerprint, at) VALUES ($1, $2, $3, $4)",
            account,
            kind,
            fingerprint,
            now,
        )
        .execute(conn)
        .await
        .map_err(backend)?;
        Ok(())
    }

    /// Bind a key in the open transaction; `Err(Key)` if it belongs to an account.
    async fn bind_redeemed_key(
        conn: &mut PgConnection,
        account: i64,
        fingerprint: &Fingerprint,
    ) -> Result<(), RedeemError> {
        let fingerprint = fingerprint.as_str();
        sqlx::query!(
            "INSERT INTO account_keys (fingerprint, account_id) VALUES ($1, $2)",
            fingerprint,
            account,
        )
        .execute(conn)
        .await
        .map(|_| ())
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => RedeemError::Refused(RedeemRefusal::Key),
            _ => RedeemError::Store(backend(error)),
        })
    }

    async fn account_row(conn: &mut PgConnection, id: i64) -> Result<Account, StoreError> {
        sqlx::query_as!(
            AccountRow,
            "SELECT id, username, state, created_at FROM accounts WHERE id = $1",
            id,
        )
        .fetch_one(conn)
        .await
        .map_err(backend)?
        .into_account()
    }

    /// Claim a live code of `kind`: the account it is for, or `None`.
    async fn claim_code(
        &self,
        conn: &mut PgConnection,
        hash: &LoginCodeHash,
        code_kind: CodeKind,
    ) -> Result<Option<i64>, StoreError> {
        let now = self.clock().now().as_raw();
        let hash = hash.to_hex();
        let code_kind = code_kind.as_str();
        let claimed = sqlx::query_scalar!(
            "UPDATE login_code SET redeemed_at = $1
             WHERE code_hash = $2 AND kind = $3 AND redeemed_at IS NULL AND voided_at IS NULL
               AND expires_at > $1
               AND EXISTS (SELECT 1 FROM accounts a
                           WHERE a.id = login_code.account_id AND a.state = 'active')
             RETURNING account_id",
            now,
            hash,
            code_kind,
        )
        .fetch_optional(conn)
        .await
        .map_err(backend)?;
        Ok(claimed)
    }
}

impl KeyStore for PostgresStore {
    async fn list_keys(&self, account: AccountId) -> Result<Vec<KeyRecord>, StoreError> {
        let id = account.as_raw();
        sqlx::query_as!(
            KeyRow,
            "SELECT fingerprint, created_at, last_used_at
             FROM account_keys WHERE account_id = $1 ORDER BY created_at, fingerprint",
            id,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?
        .into_iter()
        .map(KeyRow::into_record)
        .collect()
    }

    async fn add_key(
        &self,
        account: AccountId,
        fingerprint: &Fingerprint,
        audit: &AuditEvent,
    ) -> Result<(), StoreError> {
        let id = account.as_raw();
        let text = fingerprint.as_str();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        sqlx::query!(
            "INSERT INTO account_keys (fingerprint, account_id) VALUES ($1, $2)",
            text,
            id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => StoreError::FingerprintTaken,
            Some(ErrorKind::ForeignKeyViolation) => StoreError::AccountNotFound,
            _ => backend(error),
        })?;
        self.notice_in(&mut tx, id, NoticeKind::KeyAdded, Some(fingerprint))
            .await?;
        self.audit_in(&mut tx, audit).await?;
        tx.commit().await.map_err(backend)
    }

    async fn remove_key(
        &self,
        account: AccountId,
        fingerprint: &Fingerprint,
        audit: &AuditEvent,
    ) -> Result<RemoveOutcome, StoreError> {
        let id = account.as_raw();
        let text = fingerprint.as_str();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        // Serialize removals (and password changes that read the same rows) for this account.
        sqlx::query_scalar!("SELECT id FROM accounts WHERE id = $1 FOR UPDATE", id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?
            .ok_or(StoreError::AccountNotFound)?;
        let removed = sqlx::query!(
            "DELETE FROM account_keys
             WHERE fingerprint = $1 AND account_id = $2
               AND (EXISTS (SELECT 1 FROM account_password p WHERE p.account_id = $2)
                    OR EXISTS (SELECT 1 FROM account_keys k
                               WHERE k.account_id = $2 AND k.fingerprint <> $1))",
            text,
            id,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?
        .rows_affected();
        if removed == 0 {
            let present = sqlx::query_scalar!(
                r#"SELECT count(*) AS "n!" FROM account_keys
                   WHERE fingerprint = $1 AND account_id = $2"#,
                text,
                id,
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
            return Ok(if present > 0 {
                RemoveOutcome::LastWayIn
            } else {
                RemoveOutcome::NotFound
            });
        }
        self.notice_in(&mut tx, id, NoticeKind::KeyRemoved, Some(fingerprint))
            .await?;
        self.audit_in(&mut tx, audit).await?;
        tx.commit().await.map_err(backend)?;
        Ok(RemoveOutcome::Removed)
    }

    async fn note_key_use(&self, fingerprint: &Fingerprint) -> Result<(), StoreError> {
        let now = self.clock().now().as_raw();
        let text = fingerprint.as_str();
        sqlx::query!(
            "UPDATE account_keys SET last_used_at = $1 WHERE fingerprint = $2",
            now,
            text,
        )
        .execute(self.pool())
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn take_notices(&self, account: AccountId) -> Result<Vec<KeyNotice>, StoreError> {
        let id = account.as_raw();
        let now = self.clock().now().as_raw();
        let rows = sqlx::query_as!(
            NoticeRow,
            "UPDATE key_notice SET seen_at = $1
             WHERE account_id = $2 AND seen_at IS NULL
             RETURNING id, kind, fingerprint, at",
            now,
            id,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        notices_in_order(rows)
    }

    async fn active_account(&self, handle: &str) -> Result<Option<Account>, StoreError> {
        sqlx::query_as!(
            AccountRow,
            "SELECT id, username, state, created_at FROM accounts
             WHERE lower(username) = $1 AND state = 'active'",
            handle,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?
        .map(AccountRow::into_account)
        .transpose()
    }

    async fn account_rank(&self, account: AccountId) -> Result<u8, StoreError> {
        Ok(self
            .snapshot(Some(account))
            .await?
            .top_rank()
            .unwrap_or(0))
    }

    async fn issue_code(
        &self,
        code: &NewCode,
        audit: &AuditEvent,
    ) -> Result<UnixSeconds, IssueError> {
        let now = self.clock().now().as_raw();
        let expires_at = now.saturating_add(code.ttl_secs);
        let target = code.target.as_raw();
        let issuer = code.issuer.as_raw();
        let hash = code.hash.to_hex();
        let code_kind = code.kind.as_str();
        let window = now.saturating_sub(HOUR_SECS);

        let mut tx = self.pool().begin().await.map_err(backend)?;
        // Lock the target so two makers run one after the other; none if it is not active.
        let active = sqlx::query_scalar!(
            "SELECT id FROM accounts WHERE id = $1 AND state = 'active' FOR UPDATE",
            target,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if active.is_none() {
            return Err(IssueError::NoAccount);
        }
        if let Some(limit) = code.per_hour {
            let recent = sqlx::query!(
                r#"SELECT count(*) AS "n!", min(created_at) AS "oldest?"
                   FROM login_code WHERE account_id = $1 AND kind = $2 AND created_at > $3"#,
                target,
                code_kind,
                window,
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
            if recent.n >= i64::from(limit) {
                let retry_at = recent
                    .oldest
                    .map_or(now, |oldest| oldest.saturating_add(HOUR_SECS));
                return Err(IssueError::RateLimited {
                    retry_at: UnixSeconds::from_raw(retry_at),
                });
            }
        }
        sqlx::query!(
            "UPDATE login_code SET voided_at = $1
             WHERE account_id = $2 AND kind = $3 AND redeemed_at IS NULL AND voided_at IS NULL
               AND expires_at > $1",
            now,
            target,
            code_kind,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query!(
            "INSERT INTO login_code (kind, code_hash, account_id, issued_by, created_at, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
            code_kind,
            hash,
            target,
            issuer,
            now,
            expires_at,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        self.audit_in(&mut tx, audit).await?;
        tx.commit().await.map_err(backend)?;
        Ok(UnixSeconds::from_raw(expires_at))
    }

    async fn redeem_link_code(
        &self,
        hash: &LoginCodeHash,
        fingerprint: &Fingerprint,
    ) -> Result<Account, RedeemError> {
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let Some(account) = self.claim_code(&mut tx, hash, CodeKind::Link).await? else {
            return Err(RedeemError::Refused(RedeemRefusal::Code));
        };
        Self::bind_redeemed_key(&mut tx, account, fingerprint).await?;
        self.notice_in(&mut tx, account, NoticeKind::DeviceLinked, Some(fingerprint))
            .await?;
        let row = Self::account_row(&mut tx, account).await?;
        self.audit_in(&mut tx, &events::device_code_redeemed(row.id, fingerprint))
            .await?;
        tx.commit().await.map_err(backend)?;
        Ok(row)
    }

    async fn redeem_recovery_code(
        &self,
        hash: &LoginCodeHash,
        fingerprint: &Fingerprint,
        password_hash: &str,
    ) -> Result<Account, RedeemError> {
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let Some(account) = self.claim_code(&mut tx, hash, CodeKind::Recovery).await? else {
            return Err(RedeemError::Refused(RedeemRefusal::Code));
        };
        Self::bind_redeemed_key(&mut tx, account, fingerprint).await?;
        sqlx::query!(
            "INSERT INTO account_password (account_id, hash) VALUES ($1, $2)
             ON CONFLICT (account_id) DO UPDATE SET
                 hash = excluded.hash, updated_at = extract(epoch FROM now())::bigint",
            account,
            password_hash,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        self.notice_in(&mut tx, account, NoticeKind::Recovered, Some(fingerprint))
            .await?;
        let row = Self::account_row(&mut tx, account).await?;
        self.audit_in(
            &mut tx,
            &events::recovery_code_redeemed(row.id, fingerprint),
        )
        .await?;
        tx.commit().await.map_err(backend)?;
        Ok(row)
    }
}
