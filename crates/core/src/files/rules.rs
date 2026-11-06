//! Who may see and use a file area (D33, D141).
//!
//! The same shape as forums: `member` holds the `files.*` permissions **globally**, so a global
//! holding reaches every area, and a **restricted** area needs the permission held for that area
//! *and* either scoped to it (an allow entry, or a role granted on it) or `files.manage_areas`
//! held globally. Every screen, search and SFTP path calls [`can`] or [`reachable`].

use crate::access::{Permission, Reach, ResourceKind, Scope, Snapshot};

use super::ids::AreaId;
use super::store::AreaRow;

/// May the session use `permission` on `area`?
#[must_use]
pub fn can(snapshot: &Snapshot, permission: Permission, area: &AreaRow) -> bool {
    let Some(resource) = area.id.resource() else {
        return false;
    };
    if !snapshot.resolve(permission, Some(resource)).is_granted() {
        return false;
    }
    !area.restricted
        || snapshot
            .held()
            .iter()
            .any(|h| h.permission == permission && h.scope == Scope::Resource(resource))
        || snapshot
            .resolve(Permission::FilesManageAreas, None)
            .is_granted()
}

/// The areas among `areas` the session may use `permission` on, in order.
#[must_use]
pub fn reachable<'a>(
    snapshot: &Snapshot,
    permission: Permission,
    areas: &'a [AreaRow],
) -> Vec<&'a AreaRow> {
    let reach = snapshot.reach(permission, ResourceKind::FileArea);
    if reach == Reach::None {
        return Vec::new();
    }
    areas
        .iter()
        .filter(|area| {
            area.id
                .resource()
                .is_some_and(|resource| reach.contains(resource.id))
        })
        .filter(|area| can(snapshot, permission, area))
        .collect()
}

/// The ids of [`reachable`] areas.
#[must_use]
pub fn reachable_ids(
    snapshot: &Snapshot,
    permission: Permission,
    areas: &[AreaRow],
) -> Vec<AreaId> {
    reachable(snapshot, permission, areas)
        .into_iter()
        .map(|area| area.id)
        .collect()
}
