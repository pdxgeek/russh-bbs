//! Keys, device-link and recovery codes, and key notices over SQLite (FIX-M3-11, D97).
//!
//! **Last way in.** A key is removed by one `DELETE` whose `WHERE` also says "the account has a
//! password or another key". A single statement is atomic in SQLite, so two removals racing for
//! the last two keys cannot both pass the check.
//!
//! **Single use.** A redemption's first statement is an `UPDATE login_code SET redeemed_at ...
//! WHERE code_hash = ? AND redeemed_at IS NULL AND voided_at IS NULL AND expires_at > now AND
//! the account is active`. It is a write, so SQLite takes the write lock there; a second
//! redeemer waits, then matches no row. The key, the notice and the audit row come after it in
//! the same transaction, so a key that already has an account rolls the redemption back and the
//! code is not used up.

use russh_bbs_core::access::{AccessStore as _, Fingerprint};
use russh_bbs_core::audit::{AuditEvent, events};
use russh_bbs_core::keys::{
    CodeKind, IssueError, KeyNotice, KeyRecord, KeyStore, LoginCodeHash, NewCode, NoticeKind,
    RedeemError, RedeemRefusal, RemoveOutcome,
};
use russh_bbs_core::store::{Account, AccountId, StoreError, UnixSeconds};
use sqlx::SqliteConnection;
use sqlx::error::ErrorKind;

use super::SqliteStore;
use crate::error::{backend, kind};
use crate::key_rows::{KeyRow, NoticeRow, notices_in_order};
use crate::rows::AccountRow;

/// One hour, for the per-account code limit.
const HOUR_SECS: i64 = 3_600;

impl SqliteStore {
    /// Write a notice for `account` in the open transaction.
    async fn notice_in(
        &self,
        conn: &mut SqliteConnection,
        account: i64,
        notice: NoticeKind,
        fingerprint: Option<&Fingerprint>,
    ) -> Result<(), StoreError> {
        let now = self.clock().now().as_raw();
        let kind = notice.as_str();
        let fingerprint = fingerprint.map(Fingerprint::as_str);
        sqlx::query!(
            "INSERT INTO key_notice (account_id, kind, fingerprint, at) VALUES (?, ?, ?, ?)",
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
        conn: &mut SqliteConnection,
        account: i64,
        fingerprint: &Fingerprint,
    ) -> Result<(), RedeemError> {
        let fingerprint = fingerprint.as_str();
        sqlx::query!(
            "INSERT INTO account_keys (fingerprint, account_id) VALUES (?, ?)",
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

    async fn account_row(conn: &mut SqliteConnection, id: i64) -> Result<Account, StoreError> {
        sqlx::query_as!(
            AccountRow,
            r#"SELECT id AS "id!", username, state, created_at FROM accounts WHERE id = ?"#,
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
        conn: &mut SqliteConnection,
        hash: &LoginCodeHash,
        code_kind: CodeKind,
    ) -> Result<Option<i64>, StoreError> {
        let now = self.clock().now().as_raw();
        let hash = hash.to_hex();
        let code_kind = code_kind.as_str();
        let claimed = sqlx::query_scalar!(
            r#"UPDATE login_code SET redeemed_at = ?1
               WHERE code_hash = ?2 AND kind = ?3 AND redeemed_at IS NULL AND voided_at IS NULL
                 AND expires_at > ?1
                 AND EXISTS (SELECT 1 FROM accounts a
                             WHERE a.id = login_code.account_id AND a.state = 'active')
               RETURNING account_id AS "account_id!""#,
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

impl KeyStore for SqliteStore {
    async fn list_keys(&self, account: AccountId) -> Result<Vec<KeyRecord>, StoreError> {
        let id = account.as_raw();
        sqlx::query_as!(
            KeyRow,
            r#"SELECT fingerprint, created_at, last_used_at
               FROM account_keys WHERE account_id = ? ORDER BY created_at, fingerprint"#,
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
            "INSERT INTO account_keys (fingerprint, account_id) VALUES (?, ?)",
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
        let removed = sqlx::query!(
            r#"DELETE FROM account_keys
               WHERE fingerprint = ?1 AND account_id = ?2
                 AND (EXISTS (SELECT 1 FROM account_password p WHERE p.account_id = ?2)
                      OR EXISTS (SELECT 1 FROM account_keys k
                                 WHERE k.account_id = ?2 AND k.fingerprint <> ?1))"#,
            text,
            id,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?
        .rows_affected();
        if removed == 0 {
            let present = sqlx::query_scalar!(
                r#"SELECT count(*) AS "n!: i64" FROM account_keys
                   WHERE fingerprint = ? AND account_id = ?"#,
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
            "UPDATE account_keys SET last_used_at = ? WHERE fingerprint = ?",
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
            r#"UPDATE key_notice SET seen_at = ?
               WHERE account_id = ? AND seen_at IS NULL
               RETURNING id AS "id!", kind, fingerprint, at"#,
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
            r#"SELECT id AS "id!", username, state, created_at FROM accounts
               WHERE lower(username) = ? AND state = 'active'"#,
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
        // The first statement is a write (voiding older live codes of this kind), so the write
        // lock is taken before the count: two makers cannot both pass the limit. If the limit
        // refuses, the transaction is dropped and the voiding is rolled back.
        sqlx::query!(
            r#"UPDATE login_code SET voided_at = ?
               WHERE account_id = ? AND kind = ? AND redeemed_at IS NULL AND voided_at IS NULL
                 AND expires_at > ?1"#,
            now,
            target,
            code_kind,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        let active = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM accounts WHERE id = ? AND state = 'active'"#,
            target,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        if active == 0 {
            return Err(IssueError::NoAccount);
        }
        if let Some(limit) = code.per_hour {
            let recent = sqlx::query!(
                r#"SELECT count(*) AS "n!: i64", min(created_at) AS "oldest?: i64"
                   FROM login_code WHERE account_id = ? AND kind = ? AND created_at > ?"#,
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
            "INSERT INTO login_code (kind, code_hash, account_id, issued_by, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?)",
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
            "INSERT INTO account_password (account_id, hash) VALUES (?, ?)
             ON CONFLICT (account_id) DO UPDATE SET
                 hash = excluded.hash, updated_at = unixepoch()",
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
