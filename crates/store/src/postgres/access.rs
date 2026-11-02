//! Roles, groups and grants over Postgres (M2-B, D33): the per-action snapshot, the group
//! closure rewrite, and plain grant writes. The rules (cycles, depth, what a row means) are in
//! core and `access_rows`; this file is SQL and a transaction.

use russh_bbs_core::access::{
    AccessError, AccessStore, Child, Granting, GroupChange, GroupId, GroupInfo, NewGrant,
    Permission, RoleInfo, Scope, Slug, Snapshot, Subject,
};
use russh_bbs_core::audit::{Actor, events};
use russh_bbs_core::store::{AccountId, StoreError};
use sqlx::error::ErrorKind;

use super::PostgresStore;
use crate::access_rows::{
    ClosureDiff, EdgeRow, GrantColumns, GroupRow, RoleRow, SnapshotRow, closure_diff,
    deleted_group_name, graph_from_rows, permission_diff, roles_from_rows, snapshot_from_rows,
    waiting_snapshot,
};
use crate::error::{backend, kind};

impl AccessStore for PostgresStore {
    async fn snapshot(&self, account: Option<AccountId>) -> Result<Snapshot, StoreError> {
        if let Some(account) = account {
            let id = account.as_raw();
            // One statement: the account's state, then one row per (grant x role permission)
            // for the account, the groups it is in through the closure, and `everyone`.
            let rows = sqlx::query_as!(
                SnapshotRow,
                r#"SELECT a.state AS "state!",
                          g.role AS "role?",
                          r.rank::bigint AS "rank?",
                          COALESCE(g.permission, rp.permission) AS "permission?",
                          g.resource_kind AS "resource_kind?",
                          g.resource_id AS "resource_id?",
                          mg.name AS "group_name?"
                   FROM accounts a
                   LEFT JOIN access_grant g
                          ON g.account_id = a.id
                          OR g.group_id IN (SELECT c.group_id FROM group_closure c
                                            WHERE c.account_id = a.id)
                          OR g.group_id = (SELECT e.id FROM member_group e WHERE e.name = 'everyone')
                   LEFT JOIN member_group mg ON mg.id = g.group_id
                   LEFT JOIN role r ON r.name = g.role
                   LEFT JOIN role_permission rp ON rp.role = g.role
                   WHERE a.id = $1"#,
                id,
            )
            .fetch_all(self.pool())
            .await
            .map_err(backend)?;
            if let Some(snapshot) = snapshot_from_rows(account, &rows)? {
                return Ok(snapshot);
            }
        }
        let permissions = sqlx::query_scalar!(
            "SELECT permission FROM role_permission WHERE role = 'waiting' -- postgres"
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        Ok(waiting_snapshot(&permissions))
    }

    async fn roles(&self) -> Result<Vec<RoleInfo>, StoreError> {
        let rows = sqlx::query_as!(
            RoleRow,
            r#"SELECT r.name AS "name!", r.rank::bigint AS "rank!", r.kind AS "kind!",
                      rp.permission AS "permission?"
               FROM role r LEFT JOIN role_permission rp ON rp.role = r.name
               ORDER BY r.name, rp.permission"#,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        roles_from_rows(rows)
    }

    async fn groups(&self) -> Result<Vec<GroupInfo>, StoreError> {
        let rows = sqlx::query_as!(
            GroupRow,
            r#"SELECT id AS "id!", name, kind FROM member_group ORDER BY id -- postgres"#
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        rows.into_iter()
            .map(|row| {
                Ok(GroupInfo {
                    id: GroupId::from_raw(row.id),
                    name: Slug::parse(&row.name).map_err(|_| {
                        StoreError::Corrupt("stored group name is not valid".into())
                    })?,
                    kind: russh_bbs_core::access::Kind::parse(&row.kind).ok_or_else(|| {
                        StoreError::Corrupt("stored group kind is not known".into())
                    })?,
                })
            })
            .collect()
    }

    async fn create_group(&self, name: &Slug, actor: Actor) -> Result<GroupId, AccessError> {
        let text = name.as_str();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let id = sqlx::query_scalar!(
            r#"INSERT INTO member_group (name, kind) VALUES ($1, 'custom') RETURNING id AS "id!""#,
            text,
        )
        .fetch_one(&mut *tx)
        .await
        .map(GroupId::from_raw)
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => AccessError::GroupNameTaken,
            _ => backend(error).into(),
        })?;
        self.audit_in(&mut tx, &events::group_created(actor, id, name))
            .await?;
        tx.commit().await.map_err(backend)?;
        Ok(id)
    }

    async fn change_groups(&self, change: GroupChange, actor: Actor) -> Result<(), AccessError> {
        let mut tx = self.pool().begin().await.map_err(backend)?;
        // Exclusive on the edges: two concurrent changes cannot both pass the cycle check
        // against the same old state. Readers of the closure are not blocked.
        sqlx::query("LOCK TABLE group_member IN EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        let groups = sqlx::query_as!(
            GroupRow,
            r#"SELECT id AS "id!", name, kind FROM member_group -- postgres"#
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        let edges = sqlx::query_as!(
            EdgeRow,
            r#"SELECT group_id AS "group_id!", account_id, child_group_id FROM group_member -- postgres"#
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        let old: Vec<(i64, i64)> = sqlx::query!(
            r#"SELECT group_id AS "group_id!", account_id AS "account_id!" FROM group_closure -- postgres"#
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?
        .into_iter()
        .map(|row| (row.group_id, row.account_id))
        .collect();

        let mut graph = graph_from_rows(&groups, &edges)?;
        graph.apply(change)?;

        let deleted_name = deleted_group_name(change, &groups);
        // Adding a member that is already there, or removing one that is not, changes nothing
        // and writes no audit row.
        let mut changed = true;
        match change {
            GroupChange::Add { group, child } => {
                let group = group.as_raw();
                let (account, inner) = split(child);
                let done = sqlx::query!(
                    "INSERT INTO group_member (group_id, account_id, child_group_id)
                     VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
                    group,
                    account,
                    inner,
                )
                .execute(&mut *tx)
                .await
                .map_err(|error| match kind(&error) {
                    Some(ErrorKind::ForeignKeyViolation) => AccessError::UnknownAccount,
                    _ => backend(error).into(),
                })?;
                changed = done.rows_affected() > 0;
            }
            GroupChange::Remove { group, child } => {
                let group = group.as_raw();
                let (account, inner) = split(child);
                let done = sqlx::query!(
                    "DELETE FROM group_member WHERE group_id = $1
                       AND COALESCE(account_id, 0) = COALESCE($2::bigint, 0)
                       AND COALESCE(child_group_id, 0) = COALESCE($3::bigint, 0)",
                    group,
                    account,
                    inner,
                )
                .execute(&mut *tx)
                .await
                .map_err(backend)?;
                changed = done.rows_affected() > 0;
            }
            GroupChange::Delete(group) => {
                let group = group.as_raw();
                sqlx::query!("DELETE FROM member_group WHERE id = $1", group)
                    .execute(&mut *tx)
                    .await
                    .map_err(backend)?;
            }
        }

        rewrite_closure(&mut tx, closure_diff(&old, &graph)).await?;
        if changed {
            self.audit_in(
                &mut tx,
                &events::group_changed(actor, change, deleted_name.as_ref()),
            )
            .await?;
        }
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn add_grant(&self, grant: &NewGrant, actor: Actor) -> Result<(), AccessError> {
        if matches!(&grant.granting, Granting::Role(role) if *role == Slug::WAITING) {
            return Err(AccessError::RoleNotGrantable);
        }
        let (account, group) = match grant.subject {
            Subject::Account(id) => (Some(id.as_raw()), None),
            Subject::Group(id) => (None, Some(id.as_raw())),
        };
        let (role, permission) = match &grant.granting {
            Granting::Role(role) => (Some(role.as_str()), None),
            Granting::Permission(permission) => (None, Some(permission.name())),
        };
        let (resource_kind, resource_id) = match grant.scope {
            Scope::Global => ("", 0),
            Scope::Resource(r) => (r.kind.name(), r.id.as_raw()),
        };
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let result = sqlx::query!(
            "INSERT INTO access_grant
                 (account_id, group_id, role, permission, resource_kind, resource_id)
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
            account,
            group,
            role,
            permission,
            resource_kind,
            resource_id,
        )
        .execute(&mut *tx)
        .await;
        match result {
            Ok(done) => {
                // A grant that already existed changed nothing and writes no audit row.
                if done.rows_affected() > 0 {
                    self.audit_in(&mut tx, &events::grant_added(actor, grant))
                        .await?;
                }
                tx.commit().await.map_err(backend)?;
                Ok(())
            }
            Err(error) if kind(&error) == Some(ErrorKind::ForeignKeyViolation) => {
                drop(tx);
                Err(self.unknown_target(grant).await)
            }
            Err(error) => Err(backend(error).into()),
        }
    }
    async fn put_role(&self, role: &RoleInfo, actor: Actor) -> Result<(), AccessError> {
        let name = role.name.as_str();
        let rank = i16::from(role.rank);
        let mut tx = self.pool().begin().await.map_err(backend)?;
        sqlx::query!(
            "INSERT INTO role (name, rank, kind) VALUES ($1, $2, 'custom')",
            name,
            rank,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => AccessError::RoleNameTaken,
            _ => backend(error).into(),
        })?;
        for permission in &role.permissions {
            let permission = permission.name();
            sqlx::query!(
                "INSERT INTO role_permission (role, permission) VALUES ($1, $2)
                 ON CONFLICT DO NOTHING",
                name,
                permission,
            )
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        }
        let event = events::role_created(actor, &role.name, role.rank, role.permissions.len());
        self.audit_in(&mut tx, &event).await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn set_role_permissions(
        &self,
        name: &Slug,
        permissions: &[Permission],
        actor: Actor,
    ) -> Result<(), AccessError> {
        let text = name.as_str();
        // The write lock first, so the diff is against what is really stored.
        let mut tx = self.pool().begin().await.map_err(backend)?;
        if sqlx::query_scalar!(
            "SELECT name FROM role WHERE name = $1 FOR NO KEY UPDATE",
            text
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?
        .is_none()
        {
            return Err(AccessError::UnknownRole);
        }
        let stored: Vec<String> = sqlx::query_scalar!(
            "SELECT permission FROM role_permission WHERE role = $1",
            text
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        let diff = permission_diff(&stored, permissions);
        if diff.is_empty() {
            return Ok(());
        }
        for permission in &diff.removed {
            sqlx::query!(
                "DELETE FROM role_permission WHERE role = $1 AND permission = $2",
                text,
                permission,
            )
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        }
        for permission in &diff.added {
            let permission = permission.name();
            sqlx::query!(
                "INSERT INTO role_permission (role, permission) VALUES ($1, $2)",
                text,
                permission,
            )
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        }
        let event = events::role_permissions_set(actor, name, diff.added.len(), diff.removed.len());
        self.audit_in(&mut tx, &event).await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn delete_role(&self, name: &Slug, actor: Actor) -> Result<(), AccessError> {
        let text = name.as_str();
        // The write lock first: no grant can slip in between the check and the delete, which
        // would otherwise be cascaded away with the role.
        let mut tx = self.pool().begin().await.map_err(backend)?;
        // FOR UPDATE conflicts with the key-share lock a grant insert takes on its role, so a
        // grant is either seen by the count below or waits and then fails on the missing role.
        if sqlx::query_scalar!("SELECT name FROM role WHERE name = $1 FOR UPDATE", text)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?
            .is_none()
        {
            return Err(AccessError::UnknownRole);
        }
        let granted = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!" FROM access_grant WHERE role = $1"#,
            text
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        if granted > 0 {
            return Err(AccessError::RoleInUse);
        }
        sqlx::query!("DELETE FROM role WHERE name = $1", text)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        self.audit_in(&mut tx, &events::role_deleted(actor, name))
            .await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn remove_grant(&self, grant: &NewGrant, actor: Actor) -> Result<(), AccessError> {
        let cols = GrantColumns::of(grant);
        // The write lock first, so two revokes cannot both see "another sysop grant remains".
        let mut tx = self.pool().begin().await.map_err(backend)?;
        if cols.is_global_sysop() {
            // One revoke of a sysop grant at a time, so two cannot both see another remain.
            sqlx::query_scalar!(
                "SELECT name FROM role WHERE name = 'sysop' FOR NO KEY UPDATE -- postgres"
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
        }
        let done = sqlx::query!(
            "DELETE FROM access_grant
              WHERE COALESCE(account_id, 0) = COALESCE($1::bigint, 0)
                AND COALESCE(group_id, 0) = COALESCE($2::bigint, 0)
                AND COALESCE(role, '') = COALESCE($3::text, '')
                AND COALESCE(permission, '') = COALESCE($4::text, '')
                AND resource_kind = $5 AND resource_id = $6",
            cols.account,
            cols.group,
            cols.role,
            cols.permission,
            cols.resource_kind,
            cols.resource_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        if done.rows_affected() == 0 {
            return Ok(());
        }
        if cols.is_global_sysop() {
            let left = sqlx::query_scalar!(
                r#"SELECT COUNT(*) AS "count!" FROM access_grant
                    WHERE role = 'sysop' AND resource_id = 0 -- postgres"#
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
            if left == 0 {
                return Err(AccessError::LastSysop);
            }
        }
        self.audit_in(&mut tx, &events::grant_removed(actor, grant))
            .await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }
}

impl PostgresStore {
    /// A foreign key failed on `add_grant`: say which of the role or the subject is missing.
    async fn unknown_target(&self, grant: &NewGrant) -> AccessError {
        if let Granting::Role(role) = &grant.granting {
            let role = role.as_str();
            match sqlx::query_scalar!(r#"SELECT 1 AS "one!" FROM role WHERE name = $1"#, role)
                .fetch_optional(self.pool())
                .await
            {
                Ok(None) => return AccessError::UnknownRole,
                Ok(Some(_)) => {}
                Err(error) => return backend(error).into(),
            }
        }
        match grant.subject {
            Subject::Account(_) => AccessError::UnknownAccount,
            Subject::Group(_) => russh_bbs_core::access::GroupError::UnknownGroup.into(),
        }
    }
}

/// A membership child as the two nullable columns.
const fn split(child: Child) -> (Option<i64>, Option<i64>) {
    match child {
        Child::Account(account) => (Some(account.as_raw()), None),
        Child::Group(group) => (None, Some(group.as_raw())),
    }
}

/// Rewrite the cached closure to match the new graph, in the change's own transaction.
async fn rewrite_closure(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    diff: ClosureDiff,
) -> Result<(), AccessError> {
    for (group, account) in diff.remove {
        sqlx::query!(
            "DELETE FROM group_closure WHERE group_id = $1 AND account_id = $2",
            group,
            account,
        )
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    }
    for (group, account) in diff.add {
        sqlx::query!(
            "INSERT INTO group_closure (group_id, account_id) VALUES ($1, $2)
                 ON CONFLICT DO NOTHING",
            group,
            account,
        )
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    }
    Ok(())
}
