//! The sysop queue over Postgres (M2-E, D71, D72): the same behaviour as the SQLite backend, in
//! Postgres SQL.
//!
//! **Atomic decisions.** Approve and deny start with `DELETE FROM invite_request WHERE id = $1 ..
//! RETURNING ..`. A concurrent decider of the same request blocks on that row until the first
//! transaction ends, then re-evaluates against the committed state and finds no row; if the
//! first rolled back (a taken handle) it proceeds.
//!
//! **Caps.** A submission takes `SHARE ROW EXCLUSIVE` on `invite_request` first. That mode
//! conflicts with itself and with every writer, so two submissions (and a decision) run one
//! after the other and cannot both take the last slot or the same key; readers are not blocked.

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
use sqlx::PgConnection;
use sqlx::error::ErrorKind;

use super::PostgresStore;
use super::binding::{BindFailure, bind};
use crate::binding::Binding;
use crate::error::{backend, kind};
use crate::queue_rows::{QueueRow, group};
use crate::rows::AccountRow;

/// Delete what has expired.
async fn purge_in(conn: &mut PgConnection, now: i64) -> Result<Purged, StoreError> {
    let requests = sqlx::query!("DELETE FROM invite_request WHERE expires_at <= $1", now)
        .execute(&mut *conn)
        .await
        .map_err(backend)?
        .rows_affected();
    let cooldowns = sqlx::query!("DELETE FROM request_denial WHERE expires_at <= $1", now)
        .execute(&mut *conn)
        .await
        .map_err(backend)?
        .rows_affected();
    Ok(Purged {
        requests,
        cooldowns,
    })
}

impl QueueStore for PostgresStore {
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
        sqlx::query("LOCK TABLE invite_request IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        purge_in(&mut tx, now).await?;
        let has_account = sqlx::query_scalar!(
            "SELECT account_id FROM account_keys WHERE fingerprint = $1",
            fingerprint,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?
        .is_some();
        let cooling = sqlx::query_scalar!(
            "SELECT expires_at FROM request_denial WHERE key_hash = $1",
            denial,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?
        .is_some();
        let pending = sqlx::query_scalar!(
            "SELECT id FROM invite_request WHERE fingerprint = $1",
            fingerprint,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?
        .is_some();
        let waiting = sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM invite_request"#)
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
            "INSERT INTO invite_request
                 (fingerprint, handle, message, created_at, expires_at, kind, password_hash, key_bound)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id",
            fingerprint,
            handle,
            message,
            now,
            expires,
            kind_text,
            new.password_hash,
            new.key_bound,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
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

    async fn pending_requests(&self, limit: u32) -> Result<Queue, StoreError> {
        let now = self.clock().now().as_raw();
        let limit = i64::from(limit);
        let mut tx = self.pool().begin().await.map_err(backend)?;
        purge_in(&mut tx, now).await?;
        // Active vouchers only: a suspended member's name is not shown.
        let rows = sqlx::query_as!(
            QueueRow,
            r#"SELECT r.id, r.handle, r.message, r.created_at, r.expires_at, r.kind,
                      v.account_id AS "voucher_id?", a.username AS "voucher_handle?"
               FROM invite_request r
               LEFT JOIN invite_request_vouch v ON v.request_id = r.id
               LEFT JOIN accounts a ON a.id = v.account_id AND a.state = 'active'
               WHERE r.id IN (SELECT id FROM invite_request ORDER BY id LIMIT $1)
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
            "DELETE FROM invite_request WHERE id = $1 AND expires_at > $2
             RETURNING fingerprint, handle, password_hash, key_bound",
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
            "INSERT INTO accounts (username, username_skeleton, state, invited_by)
             VALUES ($1, $2, $3, $4)
             RETURNING id, username, state, created_at",
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
        // A request made with a password binds the hash it carried; it binds its fingerprint
        // too when that is a real SSH key and not a keyless request's stand-in (D94, D96).
        let binding = match (claimed.password_hash.as_deref(), claimed.key_bound) {
            (Some(hash), true) => Binding::KeyAndPassword(claimed.fingerprint.as_str(), hash),
            (Some(hash), false) => Binding::Password(hash),
            (None, _) => Binding::Key(claimed.fingerprint.as_str()),
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
            "DELETE FROM invite_request WHERE id = $1 AND expires_at > $2
             RETURNING fingerprint",
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
            "INSERT INTO request_denial (key_hash, expires_at) VALUES ($1, $2)
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
             SELECT r.id, $1, $2 FROM invite_request r WHERE r.id = $3 AND r.expires_at > $2",
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
