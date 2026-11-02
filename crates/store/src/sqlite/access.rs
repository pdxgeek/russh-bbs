//! Roles, groups and grants over SQLite (M2-B, D33): the per-action snapshot, the group
//! closure rewrite, and plain grant writes. The rules (cycles, depth, what a row means) are in
//! core and `access_rows`; this file is SQL and a transaction.

use russh_bbs_core::access::{
    AccessError, AccessStore, Child, Granting, GroupChange, GroupId, GroupInfo, NewGrant, RoleInfo,
    Scope, Slug, Snapshot, Subject,
};
use russh_bbs_core::audit::{Actor, events};
use russh_bbs_core::store::{AccountId, StoreError};
use sqlx::error::ErrorKind;

use super::SqliteStore;
use crate::access_rows::{
    EdgeRow, GroupRow, RoleRow, SnapshotRow, closure_diff, graph_from_rows, roles_from_rows,
    snapshot_from_rows, waiting_snapshot,
};
use crate::error::{backend, kind};

impl AccessStore for SqliteStore {
    async fn snapshot(&self, account: Option<AccountId>) -> Result<Snapshot, StoreError> {
        if let Some(account) = account {
            let id = account.as_raw();
            // One statement: the account's state, then one row per (grant x role permission)
            // for the account, the groups it is in through the closure, and `everyone`.
            let rows = sqlx::query_as!(
                SnapshotRow,
                r#"SELECT a.state AS "state!",
                          g.role AS "role?",
                          r.rank AS "rank?",
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
                   WHERE a.id = ?"#,
                id,
            )
            .fetch_all(self.pool())
            .await
            .map_err(backend)?;
            if let Some(snapshot) = snapshot_from_rows(account, &rows)? {
                return Ok(snapshot);
            }
        }
        let permissions =
            sqlx::query_scalar!("SELECT permission FROM role_permission WHERE role = 'waiting'")
                .fetch_all(self.pool())
                .await
                .map_err(backend)?;
        Ok(waiting_snapshot(&permissions))
    }

    async fn roles(&self) -> Result<Vec<RoleInfo>, StoreError> {
        let rows = sqlx::query_as!(
            RoleRow,
            r#"SELECT r.name AS "name!", r.rank AS "rank!", r.kind AS "kind!",
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
            r#"SELECT id AS "id!", name, kind FROM member_group ORDER BY id"#
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
            r#"INSERT INTO member_group (name, kind) VALUES (?, 'custom') RETURNING id AS "id!""#,
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
        // BEGIN IMMEDIATE takes the write lock before the reads, so two changes cannot both
        // pass the cycle check against the same old state.
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let groups = sqlx::query_as!(
            GroupRow,
            r#"SELECT id AS "id!", name, kind FROM member_group"#
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        let edges = sqlx::query_as!(
            EdgeRow,
            r#"SELECT group_id AS "group_id!", account_id, child_group_id FROM group_member"#
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        let old: Vec<(i64, i64)> = sqlx::query!(
            r#"SELECT group_id AS "group_id!", account_id AS "account_id!" FROM group_closure"#
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?
        .into_iter()
        .map(|row| (row.group_id, row.account_id))
        .collect();

        let mut graph = graph_from_rows(&groups, &edges)?;
        graph.apply(change)?;

        // Naming a deleted group in its audit row needs the name before it is gone.
        let deleted_name = match change {
            GroupChange::Delete(group) => groups
                .iter()
                .find(|row| row.id == group.as_raw())
                .and_then(|row| Slug::parse(&row.name).ok()),
            _ => None,
        };
        // Adding a member that is already there, or removing one that is not, changes nothing
        // and writes no audit row.
        let mut changed = true;
        match change {
            GroupChange::Add { group, child } => {
                let group = group.as_raw();
                let (account, inner) = split(child);
                let done = sqlx::query!(
                    "INSERT INTO group_member (group_id, account_id, child_group_id)
                     VALUES (?, ?, ?) ON CONFLICT DO NOTHING",
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
                    "DELETE FROM group_member WHERE group_id = ?
                       AND COALESCE(account_id, 0) = COALESCE(?, 0)
                       AND COALESCE(child_group_id, 0) = COALESCE(?, 0)",
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
                sqlx::query!("DELETE FROM member_group WHERE id = ?", group)
                    .execute(&mut *tx)
                    .await
                    .map_err(backend)?;
            }
        }

        let diff = closure_diff(&old, &graph);
        for (group, account) in diff.remove {
            sqlx::query!(
                "DELETE FROM group_closure WHERE group_id = ? AND account_id = ?",
                group,
                account,
            )
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        }
        for (group, account) in diff.add {
            sqlx::query!(
                "INSERT INTO group_closure (group_id, account_id) VALUES (?, ?)
                 ON CONFLICT DO NOTHING",
                group,
                account,
            )
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        }
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
             VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT DO NOTHING",
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
}

impl SqliteStore {
    /// A foreign key failed on `add_grant`: say which of the role or the subject is missing.
    async fn unknown_target(&self, grant: &NewGrant) -> AccessError {
        if let Granting::Role(role) = &grant.granting {
            let role = role.as_str();
            match sqlx::query_scalar!("SELECT name FROM role WHERE name = ?", role)
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
