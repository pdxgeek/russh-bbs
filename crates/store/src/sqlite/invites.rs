//! Invites over SQLite (M2-D, D67): create with the quota check, redeem atomically, list, tree.
//!
//! **Atomic redemption.** The first statement of the transaction is an `UPDATE invite SET
//! redeemed_at ... WHERE code_hash = ? AND redeemed_at IS NULL AND expires_at > now AND the
//! inviter is active`. It is a write, so SQLite takes the database write lock there; a second
//! redeemer of the same code waits for the first to commit, then finds `redeemed_at` set and
//! matches no row. The account, its key, the inviter link and both audit rows are written after
//! it in the same transaction, so a failure (a taken handle, a key that already has an account)
//! rolls the redemption back and the code is not consumed.

use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::audit::events;
use russh_bbs_core::invites::{
    CodeHash, CreateError, InviteId, InviteRecord, InviteStore, InviteSummary, NewInvite,
    REDEEMED_STATE, RedeemError, Redeemed, RejectReason, TreeNode, month_start, next_month_start,
};
use russh_bbs_core::sanitize::Username;
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};
use sqlx::error::ErrorKind;

use super::SqliteStore;
use super::binding::{BindFailure, bind};
use crate::binding::Binding;
use crate::error::{backend, kind};
use crate::invite_rows::{InviteRow, TreeRow};
use crate::rows::AccountRow;

impl SqliteStore {
    /// Redeem the code, binding the new account to `binding` (a key or a password, D94).
    async fn redeem_bound(
        &self,
        hash: &CodeHash,
        username: Username,
        binding: Binding<'_>,
    ) -> Result<Redeemed, RedeemError> {
        let now = self.clock().now().as_raw();
        let hash = hash.to_hex();
        let skeleton = username.skeleton();
        let name = username.as_str();
        let state = REDEEMED_STATE.as_str();

        let mut tx = self.pool().begin().await.map_err(backend)?;
        let claimed = sqlx::query!(
            r#"UPDATE invite SET redeemed_at = ?1
               WHERE code_hash = ?2 AND redeemed_at IS NULL AND expires_at > ?1
                 AND EXISTS (SELECT 1 FROM accounts a
                             WHERE a.id = invite.inviter_id AND a.state = 'active')
               RETURNING id AS "id!", inviter_id"#,
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
            r#"INSERT INTO accounts (username, username_skeleton, state, invited_by)
               VALUES (?, ?, ?, ?)
               RETURNING id AS "id!", username, state, created_at"#,
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
            "UPDATE invite SET redeemed_by = ? WHERE id = ?",
            row.id,
            claimed.id,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        bind(&mut tx, row.id, binding)
            .await
            .map_err(|failure| match failure {
                BindFailure::KeyTaken => RedeemError::Rejected(RejectReason::Key),
                BindFailure::Store(error) => RedeemError::Store(error),
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
}

impl InviteStore for SqliteStore {
    async fn create_invite(&self, new: &NewInvite) -> Result<InviteRecord, CreateError> {
        let now = self.clock().now().as_raw();
        let expires = now.saturating_add(new.ttl_secs);
        // The quota counts the UTC calendar month the store's clock is in.
        let window_start = month_start(UnixSeconds::from_raw(now)).as_raw();
        let resets_at = next_month_start(UnixSeconds::from_raw(now));
        let hash = new.code_hash.to_hex();
        let inviter = new.inviter.as_raw();
        let limit = new.quota.limit().map(i64::from);

        let mut tx = self.pool().begin().await.map_err(backend)?;
        // One statement counts and inserts, and it is the transaction's first write, so two
        // callers cannot both take the last slot. No row means the inviter is missing or the
        // quota is used; the next query says which.
        let id = sqlx::query_scalar!(
            r#"INSERT INTO invite (code_hash, inviter_id, created_at, expires_at)
               SELECT ?1, a.id, ?2, ?3 FROM accounts a
               WHERE a.id = ?4
                 AND (?5 IS NULL
                      OR (SELECT count(*) FROM invite
                          WHERE inviter_id = a.id AND created_at >= ?6) < ?5)
               RETURNING id AS "id!""#,
            hash,
            now,
            expires,
            inviter,
            limit,
            window_start,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(id) = id else {
            let exists =
                sqlx::query_scalar!(r#"SELECT id AS "id!" FROM accounts WHERE id = ?"#, inviter)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(backend)?
                    .is_some();
            return Err(match (exists, new.quota.limit()) {
                (true, Some(limit)) => CreateError::QuotaReached { limit, resets_at },
                (true, None) | (false, _) => CreateError::Store(StoreError::AccountNotFound),
            });
        };
        let used = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM invite
               WHERE inviter_id = ? AND created_at >= ?"#,
            inviter,
            window_start,
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
            used_in_month: u32::try_from(used).unwrap_or(u32::MAX),
        })
    }

    async fn redeem_invite(
        &self,
        hash: &CodeHash,
        username: Username,
        fingerprint: &Fingerprint,
    ) -> Result<Redeemed, RedeemError> {
        self.redeem_bound(hash, username, Binding::Key(fingerprint.as_str()))
            .await
    }

    async fn redeem_invite_password(
        &self,
        hash: &CodeHash,
        username: Username,
        password_hash: &str,
    ) -> Result<Redeemed, RedeemError> {
        self.redeem_bound(hash, username, Binding::Password(password_hash))
            .await
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
            r#"SELECT i.id AS "id!", i.created_at, i.expires_at, i.redeemed_at,
                      a.username AS "redeemed_username?"
               FROM invite i LEFT JOIN accounts a ON a.id = i.redeemed_by
               WHERE i.inviter_id = ?
               ORDER BY i.id DESC
               LIMIT ?"#,
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
            r#"SELECT id AS "id!", username, invited_by
               FROM accounts
               WHERE state IN ('active', 'suspended')
               ORDER BY id
               LIMIT ?"#,
            limit,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        rows.into_iter().map(TreeRow::into_node).collect()
    }
}
