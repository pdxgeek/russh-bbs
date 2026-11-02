//! Invites over Postgres (M2-D, D67): the same behaviour as the SQLite backend, in Postgres SQL.
//!
//! **Atomic redemption.** The first statement of the transaction is `UPDATE invite SET
//! redeemed_at ... WHERE code_hash = $2 AND redeemed_at IS NULL ...`. A concurrent redeemer of
//! the same code blocks on that row's lock until the first transaction ends, then re-evaluates
//! the `WHERE` (read committed) against the committed row, finds `redeemed_at` set and matches
//! nothing. If the first transaction rolls back (a taken handle) the second proceeds.
//!
//! **Quota.** Creation locks the inviter's `accounts` row (`FOR UPDATE`) before it counts, so
//! two creations by one inviter run one after the other and cannot both take the last slot.

use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::audit::events;
use russh_bbs_core::invites::{
    CodeHash, CreateError, InviteId, InviteRecord, InviteStore, InviteSummary, NewInvite,
    REDEEMED_STATE, RedeemError, Redeemed, RejectReason, TreeNode, month_start, next_month_start,
};
use russh_bbs_core::sanitize::Username;
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};
use sqlx::error::ErrorKind;

use super::PostgresStore;
use crate::error::{backend, kind};
use crate::invite_rows::{InviteRow, TreeRow};
use crate::rows::AccountRow;

impl InviteStore for PostgresStore {
    async fn create_invite(&self, new: &NewInvite) -> Result<InviteRecord, CreateError> {
        let now = self.clock().now().as_raw();
        let expires = now.saturating_add(new.ttl_secs);
        // The quota counts the UTC calendar month the store's clock is in.
        let window_start = month_start(UnixSeconds::from_raw(now)).as_raw();
        let resets_at = next_month_start(UnixSeconds::from_raw(now));
        let hash = new.code_hash.to_hex();
        let inviter = new.inviter.as_raw();

        let mut tx = self.pool().begin().await.map_err(backend)?;
        let locked =
            sqlx::query_scalar!("SELECT id FROM accounts WHERE id = $1 FOR UPDATE", inviter)
                .fetch_optional(&mut *tx)
                .await
                .map_err(backend)?;
        if locked.is_none() {
            return Err(CreateError::Store(StoreError::AccountNotFound));
        }
        let before = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!" FROM invite WHERE inviter_id = $1 AND created_at >= $2"#,
            inviter,
            window_start,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        if let Some(limit) = new.quota.limit()
            && before >= i64::from(limit)
        {
            return Err(CreateError::QuotaReached { limit, resets_at });
        }
        let id = sqlx::query_scalar!(
            "INSERT INTO invite (code_hash, inviter_id, created_at, expires_at)
             VALUES ($1, $2, $3, $4) RETURNING id",
            hash,
            inviter,
            now,
            expires,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        self.audit_in(
            &mut tx,
            &events::invite_created(
                new.inviter,
                InviteId::from_raw(id),
                UnixSeconds::from_raw(expires),
            ),
        )
        .await?;
        tx.commit().await.map_err(backend)?;
        Ok(InviteRecord {
            id: InviteId::from_raw(id),
            expires_at: UnixSeconds::from_raw(expires),
            used_in_month: u32::try_from(before.saturating_add(1)).unwrap_or(u32::MAX),
        })
    }

    async fn redeem_invite(
        &self,
        hash: &CodeHash,
        username: Username,
        fingerprint: &Fingerprint,
    ) -> Result<Redeemed, RedeemError> {
        let now = self.clock().now().as_raw();
        let hash = hash.to_hex();
        let skeleton = username.skeleton();
        let name = username.as_str();
        let state = REDEEMED_STATE.as_str();
        let fingerprint = fingerprint.as_str();

        let mut tx = self.pool().begin().await.map_err(backend)?;
        let claimed = sqlx::query!(
            "UPDATE invite SET redeemed_at = $1
             WHERE code_hash = $2 AND redeemed_at IS NULL AND expires_at > $1
               AND EXISTS (SELECT 1 FROM accounts a
                           WHERE a.id = invite.inviter_id AND a.state = 'active')
             RETURNING id, inviter_id",
            now,
            hash,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(claimed) = claimed else {
            return Err(RedeemError::Rejected(RejectReason::Code));
        };
        let row = sqlx::query_as!(
            AccountRow,
            "INSERT INTO accounts (username, username_skeleton, state, invited_by)
             VALUES ($1, $2, $3, $4)
             RETURNING id, username, state, created_at",
            name,
            skeleton,
            state,
            claimed.inviter_id,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => RedeemError::Rejected(RejectReason::Handle),
            _ => RedeemError::Store(backend(error)),
        })?;
        sqlx::query!(
            "UPDATE invite SET redeemed_by = $1 WHERE id = $2",
            row.id,
            claimed.id,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query!(
            "INSERT INTO account_keys (fingerprint, account_id) VALUES ($1, $2)",
            fingerprint,
            row.id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => RedeemError::Rejected(RejectReason::Key),
            _ => RedeemError::Store(backend(error)),
        })?;
        let account = row.into_account()?;
        let invite = InviteId::from_raw(claimed.id);
        let inviter = AccountId::from_raw(claimed.inviter_id);
        self.audit_in(
            &mut tx,
            &events::account_created_via_invite(account.id, invite, inviter, account.state),
        )
        .await?;
        self.audit_in(
            &mut tx,
            &events::invite_redeemed(account.id, invite, inviter),
        )
        .await?;
        tx.commit().await.map_err(backend)?;
        Ok(Redeemed {
            account,
            invite,
            inviter,
        })
    }

    async fn invites_by(
        &self,
        inviter: AccountId,
        limit: u32,
    ) -> Result<Vec<InviteSummary>, StoreError> {
        let now = self.clock().now().as_raw();
        let inviter = inviter.as_raw();
        let limit = i64::from(limit);
        let rows = sqlx::query_as!(
            InviteRow,
            r#"SELECT i.id, i.created_at, i.expires_at, i.redeemed_at,
                      a.username AS "redeemed_username?"
               FROM invite i LEFT JOIN accounts a ON a.id = i.redeemed_by
               WHERE i.inviter_id = $1
               ORDER BY i.id DESC
               LIMIT $2"#,
            inviter,
            limit,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        rows.into_iter().map(|row| row.into_summary(now)).collect()
    }

    async fn invite_tree(&self, limit: u32) -> Result<Vec<TreeNode>, StoreError> {
        let limit = i64::from(limit);
        // Active and suspended accounts: members, past or present. Pending and approved
        // accounts have not been admitted (an `apply` applicant is visible to the sysop queue
        // only), so the tree never shows them.
        let rows = sqlx::query_as!(
            TreeRow,
            "SELECT id, username, invited_by
             FROM accounts
             WHERE state IN ('active', 'suspended')
             ORDER BY id
             LIMIT $1",
            limit,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        rows.into_iter().map(TreeRow::into_node).collect()
    }
}
