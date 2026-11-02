//! Backend-independent mapping for the access tables: grant rows into a core [`Snapshot`], role
//! rows into [`RoleInfo`], and the group graph loaded from rows. Each backend runs its own SQL
//! and hands the rows here, so the rules about what a row means live once (D24).

use std::collections::BTreeSet;

use russh_bbs_core::access::{
    Child, GroupGraph, GroupId, Held, Kind, Permission, ResourceId, ResourceKind, ResourceRef,
    RoleInfo, Scope, Slug, Snapshot, Source, Via,
};
use russh_bbs_core::store::{AccountId, AccountState, StoreError};
use tracing::warn;

/// One row of the snapshot query: the account's state, and one (grant x role permission) per row.
/// Every column after `state` is NULL when the account has no grant at all.
pub(crate) struct SnapshotRow {
    pub(crate) state: String,
    pub(crate) role: Option<String>,
    pub(crate) rank: Option<i64>,
    pub(crate) permission: Option<String>,
    pub(crate) resource_kind: Option<String>,
    pub(crate) resource_id: Option<i64>,
    pub(crate) group_name: Option<String>,
}

/// The snapshot of an active account. `Ok(None)` when the account is unknown or not `active`:
/// the caller then builds the `waiting` snapshot instead.
pub(crate) fn snapshot_from_rows(
    account: AccountId,
    rows: &[SnapshotRow],
) -> Result<Option<Snapshot>, StoreError> {
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    let state = AccountState::parse(&first.state)
        .ok_or_else(|| StoreError::Corrupt("stored account state is not known".into()))?;
    if !state.is_member_access() {
        return Ok(None);
    }
    let held = rows.iter().filter_map(held_from_row).collect();
    Ok(Some(Snapshot::new(Some(account), held)))
}

/// One fact, or `None` for a row that carries nothing (an empty role) or that this build does
/// not understand (a permission or role name it never wrote). Skipping fails closed.
fn held_from_row(row: &SnapshotRow) -> Option<Held> {
    let permission_text = row.permission.as_deref()?;
    let Some(permission) = Permission::parse(permission_text) else {
        warn!(
            permission = permission_text,
            "stored permission is not known; skipped"
        );
        return None;
    };
    let scope = scope_from(row.resource_kind.as_deref(), row.resource_id)?;
    let via = match row.group_name.as_deref() {
        None => Via::Account,
        Some(name) => Via::Group(parse_slug(name)?),
    };
    let source = match row.role.as_deref() {
        Some(role) => Source::Role {
            role: parse_slug(role)?,
            rank: u8::try_from(row.rank?).ok()?,
            via,
        },
        None => Source::Allow { via },
    };
    Some(Held {
        permission,
        scope,
        source,
    })
}

fn parse_slug(text: &str) -> Option<Slug> {
    let parsed = Slug::parse(text).ok();
    if parsed.is_none() {
        warn!("stored role or group name is not valid; skipped");
    }
    parsed
}

/// `('', 0)` is global; anything else must be a known kind and a positive id.
fn scope_from(kind: Option<&str>, id: Option<i64>) -> Option<Scope> {
    match (kind.unwrap_or(""), id.unwrap_or(0)) {
        ("", 0) => Some(Scope::Global),
        (kind, id) => {
            let scope = ResourceKind::parse(kind)
                .zip(ResourceId::new(id))
                .map(|(kind, id)| Scope::Resource(ResourceRef { kind, id }));
            if scope.is_none() {
                warn!("stored grant scope is not valid; skipped");
            }
            scope
        }
    }
}

/// The snapshot for an unknown key or an account that is not active: the `waiting` role's
/// permissions, nothing else.
pub(crate) fn waiting_snapshot(permissions: &[String]) -> Snapshot {
    let known = permissions.iter().filter_map(|text| {
        let parsed = Permission::parse(text);
        if parsed.is_none() {
            warn!(
                permission = text.as_str(),
                "stored permission is not known; skipped"
            );
        }
        parsed
    });
    Snapshot::from_role(None, &Slug::WAITING, 0, &Via::Waiting, known)
}

/// One `role LEFT JOIN role_permission` row.
pub(crate) struct RoleRow {
    pub(crate) name: String,
    pub(crate) rank: i64,
    pub(crate) kind: String,
    pub(crate) permission: Option<String>,
}

/// Fold role rows (ordered by role) into one [`RoleInfo`] each.
pub(crate) fn roles_from_rows(rows: Vec<RoleRow>) -> Result<Vec<RoleInfo>, StoreError> {
    let mut roles: Vec<RoleInfo> = Vec::new();
    for row in rows {
        if roles
            .last()
            .is_none_or(|last| last.name.as_str() != row.name)
        {
            roles.push(RoleInfo {
                name: Slug::parse(&row.name)
                    .map_err(|_| StoreError::Corrupt("stored role name is not valid".into()))?,
                rank: u8::try_from(row.rank)
                    .map_err(|_| StoreError::Corrupt("stored role rank is out of range".into()))?,
                kind: Kind::parse(&row.kind)
                    .ok_or_else(|| StoreError::Corrupt("stored role kind is not known".into()))?,
                permissions: Vec::new(),
            });
        }
        if let (Some(role), Some(text)) = (roles.last_mut(), row.permission.as_deref())
            && let Some(permission) = Permission::parse(text)
        {
            role.permissions.push(permission);
        }
    }
    for role in &mut roles {
        role.permissions.sort_unstable();
    }
    Ok(roles)
}

/// A group row.
pub(crate) struct GroupRow {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) kind: String,
}

/// A direct membership row: exactly one of `account_id` and `child_group_id` is set.
#[allow(clippy::struct_field_names)] // the column names
pub(crate) struct EdgeRow {
    pub(crate) group_id: i64,
    pub(crate) account_id: Option<i64>,
    pub(crate) child_group_id: Option<i64>,
}

/// Build the in-memory graph from the rows read inside the changing transaction.
pub(crate) fn graph_from_rows(
    groups: &[GroupRow],
    edges: &[EdgeRow],
) -> Result<GroupGraph, StoreError> {
    let mut kinds = Vec::with_capacity(groups.len());
    for group in groups {
        let kind = Kind::parse(&group.kind)
            .ok_or_else(|| StoreError::Corrupt("stored group kind is not known".into()))?;
        kinds.push((GroupId::from_raw(group.id), kind));
    }
    let mut direct = Vec::with_capacity(edges.len());
    for edge in edges {
        let child = match (edge.account_id, edge.child_group_id) {
            (Some(account), None) => Child::Account(AccountId::from_raw(account)),
            (None, Some(group)) => Child::Group(GroupId::from_raw(group)),
            _ => {
                return Err(StoreError::Corrupt(
                    "stored group member is not valid".into(),
                ));
            }
        };
        direct.push((GroupId::from_raw(edge.group_id), child));
    }
    Ok(GroupGraph::new(kinds, direct))
}

/// What to do to `group_closure` so it equals `graph.closure()`.
pub(crate) struct ClosureDiff {
    pub(crate) remove: Vec<(i64, i64)>,
    pub(crate) add: Vec<(i64, i64)>,
}

/// Compare the stored closure rows with the graph's.
pub(crate) fn closure_diff(old: &[(i64, i64)], graph: &GroupGraph) -> ClosureDiff {
    let old: BTreeSet<(i64, i64)> = old.iter().copied().collect();
    let new: BTreeSet<(i64, i64)> = graph
        .closure()
        .into_iter()
        .map(|(group, account)| (group.as_raw(), account.as_raw()))
        .collect();
    ClosureDiff {
        remove: old.difference(&new).copied().collect(),
        add: new.difference(&old).copied().collect(),
    }
}
