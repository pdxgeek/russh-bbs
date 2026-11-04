//! The sysop queue over SQLite (M2-E, D71, D72).
//!
//! **Atomic decisions.** Approve and deny start with `DELETE FROM invite_request WHERE id = ? ..
//! RETURNING ..`. It is a write, so the transaction takes SQLite's write lock there, and a second
//! decider of the same request waits, then finds no row. If a later step fails (a taken handle)
//! the transaction rolls back and the request is back. A submission's first statement is the
//! purge, also a write, so the cap and the one-per-key rule are checked under the lock and two
//! submissions cannot both take the last slot.

use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::audit::events;
use russh_bbs_core::queue::{
    ApproveError, DecideError, NewRequest, Purged, Queue, QueueStore, Refusal, RequestId,
    RequestKind, SubmitTerms, Submitted, VouchError, cooldown_key,
};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountId, AccountState, SettingKey, SettingsStore as _, StoreError, UnixSeconds,
};
use sqlx::SqliteConnection;
use sqlx::error::ErrorKind;

use super::SqliteStore;
use super::binding::{BindFailure, bind};
use crate::binding::Binding;
use crate::error::{backend, kind};
use crate::queue_rows::{QueueRow, group};
use crate::rows::AccountRow;

/// Delete what has expired. A write even when nothing matches, which is what takes the lock.
async fn purge_in(conn: &mut SqliteConnection, now: i64) -> Result<Purged, StoreError> {
    let requests = sqlx::query!("DELETE FROM invite_request WHERE expires_at <= ?", now)
        .execute(&mut *conn)
        .await
        .map_err(backend)?
        .rows_affected();
    let cooldowns = sqlx::query!("DELETE FROM request_denial WHERE expires_at <= ?", now)
        .execute(&mut *conn)
        .await
        .map_err(backend)?
        .rows_affected();
    Ok(Purged {
        requests,
        cooldowns,
    })
}

impl QueueStore for SqliteStore {
    async fn submit_request(
        &self,
        new: &NewRequest,
        terms: SubmitTerms,
    ) -> Result<Submitted, StoreError> {
        let now = self.clock().now().as_raw();
        let expires = now.saturating_add(terms.ttl_secs);
        let fingerprint = new.fingerprint.as_str();
        let denial = cooldown_key(&new.fingerprint);
        let handle = new.handle.as_str();
        let message = new.message.as_str();
        let cap = i64::from(terms.max_pending);
        let kind_text = new.kind.as_str();

        let mut tx = self.pool().begin().await.map_err(backend)?;
        purge_in(&mut tx, now).await?;
        let has_account = sqlx::query_scalar!(
            r#"SELECT account_id AS "id!" FROM account_keys WHERE fingerprint = ?"#,
            fingerprint,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?
        .is_some();
        let cooling = sqlx::query_scalar!(
            r#"SELECT expires_at AS "until!" FROM request_denial WHERE key_hash = ?"#,
            denial,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?
        .is_some();
        let pending = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM invite_request WHERE fingerprint = ?"#,
            fingerprint,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?
        .is_some();
        let waiting = sqlx::query_scalar!(r#"SELECT count(*) AS "n!: i64" FROM invite_request"#)
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
        let refusal = if has_account {
            Some(Refusal::KeyHasAccount)
        } else if cooling {
            Some(Refusal::Cooling)
        } else if pending {
            Some(Refusal::AlreadyPending)
        } else if waiting >= cap {
            Some(Refusal::QueueFull)
        } else {
            None
        };
        if let Some(refusal) = refusal {
            // The purge above is kept; nothing else was written.
            tx.commit().await.map_err(backend)?;
            return Ok(Submitted::Refused(refusal));
        }
        let id = sqlx::query_scalar!(
            r#"INSERT INTO invite_request
                   (fingerprint, handle, message, created_at, expires_at, kind, password_hash)
               VALUES (?, ?, ?, ?, ?, ?, ?)
               RETURNING id AS "id!""#,
            fingerprint,
            handle,
            message,
            now,
            expires,
            kind_text,
            new.password_hash,
        )
        .fetch_one(&mut *tx)
        .await;
        match id {
            Ok(id) => {
                let id = RequestId::from_raw(id);
                if new.kind == RequestKind::Application {
                    // The application is the registration act, so it is audited in the same
                    // transaction (ids only). Invite requests are not (D71).
                    self.audit_in(&mut tx, &events::application_submitted(id))
                        .await?;
                }
                tx.commit().await.map_err(backend)?;
                Ok(Submitted::Stored(id))
            }
            // Cannot happen under the write lock; if it ever does, it is the one-per-key rule.
            Err(error) if kind(&error) == Some(ErrorKind::UniqueViolation) => {
                Ok(Submitted::Refused(Refusal::AlreadyPending))
            }
            Err(error) => Err(backend(error)),
        }
    }

    async fn pending_requests(&self, limit: u32) -> Result<Queue, StoreError> {
        let now = self.clock().now().as_raw();
        let limit = i64::from(limit);
        let mut tx = self.pool().begin().await.map_err(backend)?;
        purge_in(&mut tx, now).await?;
        // Active vouchers only: a suspended member's name is not shown.
        let rows = sqlx::query_as!(
            QueueRow,
            r#"SELECT r.id AS "id!", r.handle, r.message, r.created_at, r.expires_at, r.kind,
                      v.account_id AS "voucher_id?", a.username AS "voucher_handle?"
               FROM invite_request r
               LEFT JOIN invite_request_vouch v ON v.request_id = r.id
               LEFT JOIN accounts a ON a.id = v.account_id AND a.state = 'active'
               WHERE r.id IN (SELECT id FROM invite_request ORDER BY id LIMIT ?)
               ORDER BY r.id, v.created_at, v.account_id"#,
            limit,
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(Queue {
            now: UnixSeconds::from_raw(now),
            requests: group(rows)?,
        })
    }

    async fn approve_request(
        &self,
        id: RequestId,
        actor: AccountId,
    ) -> Result<Account, ApproveError> {
        let now = self.clock().now().as_raw();
        let id = id.as_raw();
        let state = AccountState::Active;
        let state_text = state.as_str();

        let mut tx = self.pool().begin().await.map_err(backend)?;
        let claimed = sqlx::query!(
            r#"DELETE FROM invite_request WHERE id = ?1 AND expires_at > ?2
               RETURNING fingerprint, handle, password_hash"#,
            id,
            now,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(claimed) = claimed else {
            return Err(ApproveError::NotFound);
        };
        let username = sanitize_username(&claimed.handle)
            .map_err(|_| StoreError::Corrupt("stored request handle is not valid".into()))?;
        let skeleton = username.skeleton();
        let name = username.as_str();
        let inviter = actor.as_raw();
        let row = sqlx::query_as!(
            AccountRow,
            r#"INSERT INTO accounts (username, username_skeleton, state, invited_by)
               VALUES (?, ?, ?, ?)
               RETURNING id AS "id!", username, state, created_at"#,
            name,
            skeleton,
            state_text,
            inviter,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => ApproveError::HandleTaken,
            _ => ApproveError::Store(backend(error)),
        })?;
        // A request made with a password (telnet, D94) binds the hash it carried and no key.
        let binding = match claimed.password_hash.as_deref() {
            Some(hash) => Binding::Password(hash),
            None => Binding::Key(claimed.fingerprint.as_str()),
        };
        bind(&mut tx, row.id, binding)
            .await
            .map_err(|failure| match failure {
                BindFailure::KeyTaken => ApproveError::KeyHasAccount,
                BindFailure::Store(error) => ApproveError::Store(error),
            })?;
        let account = row.into_account()?;
        let request = RequestId::from_raw(id);
        self.audit_in(
            &mut tx,
            &events::account_created_via_request(actor, account.id, request, account.state),
        )
        .await?;
        self.audit_in(
            &mut tx,
            &events::member_approved(actor, account.id, request),
        )
        .await?;
        tx.commit().await.map_err(backend)?;
        Ok(account)
    }

    async fn deny_request(
        &self,
        id: RequestId,
        actor: AccountId,
        cooldown_secs: i64,
    ) -> Result<(), DecideError> {
        let now = self.clock().now().as_raw();
        let until = now.saturating_add(cooldown_secs);
        let raw = id.as_raw();

        let mut tx = self.pool().begin().await.map_err(backend)?;
        let claimed = sqlx::query_scalar!(
            r#"DELETE FROM invite_request WHERE id = ?1 AND expires_at > ?2
               RETURNING fingerprint"#,
            raw,
            now,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(fingerprint) = claimed else {
            return Err(DecideError::NotFound);
        };
        let fingerprint = Fingerprint::parse(&fingerprint)
            .map_err(|_| StoreError::Corrupt("stored request fingerprint is not valid".into()))?;
        let denial = cooldown_key(&fingerprint);
        sqlx::query!(
            "INSERT INTO request_denial (key_hash, expires_at) VALUES (?, ?)
             ON CONFLICT (key_hash) DO UPDATE SET expires_at = excluded.expires_at",
            denial,
            until,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        self.audit_in(&mut tx, &events::member_denied(actor, id))
            .await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn vouch_request(&self, id: RequestId, voucher: AccountId) -> Result<(), VouchError> {
        let now = self.clock().now().as_raw();
        let raw = id.as_raw();
        let who = voucher.as_raw();

        let mut tx = self.pool().begin().await.map_err(backend)?;
        let inserted = sqlx::query!(
            "INSERT INTO invite_request_vouch (request_id, account_id, created_at)
             SELECT r.id, ?1, ?2 FROM invite_request r WHERE r.id = ?3 AND r.expires_at > ?2",
            who,
            now,
            raw,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => VouchError::AlreadyVouched,
            Some(ErrorKind::ForeignKeyViolation) => VouchError::Store(StoreError::AccountNotFound),
            _ => VouchError::Store(backend(error)),
        })?;
        if inserted.rows_affected() == 0 {
            return Err(VouchError::NotFound);
        }
        self.audit_in(&mut tx, &events::member_vouched(voucher, id))
            .await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn max_pending_setting(&self) -> Result<Option<String>, StoreError> {
        self.get_setting(SettingKey::QueueMaxPending).await
    }

    async fn purge_expired(&self) -> Result<Purged, StoreError> {
        let now = self.clock().now().as_raw();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let purged = purge_in(&mut tx, now).await?;
        tx.commit().await.map_err(backend)?;
        Ok(purged)
    }
}
