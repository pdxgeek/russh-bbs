//! Who may see and use a board (D33, D83).
//!
//! The access model is additive and `member` holds the forum permissions **globally**, so a
//! global holding reaches every board. A **restricted** board therefore cannot be expressed as
//! "everyone minus someone". Its rule is: the permission must be held for that board *and*
//! either held **scoped to that board** (an allow entry, or a role granted on it, to the account
//! or one of its groups) or the account holds `forum.manage` globally (staff who run the
//! boards). An open board needs only the permission. Every screen, search and by-id path calls
//! [`can`] or [`reachable`], so there is one rule.

use crate::access::{Permission, Reach, ResourceKind, ResourceRef, Scope, Snapshot};

use super::ids::BoardId;
use super::store::BoardRow;

fn scoped_to(snapshot: &Snapshot, permission: Permission, resource: ResourceRef) -> bool {
    snapshot
        .held()
        .iter()
        .any(|h| h.permission == permission && h.scope == Scope::Resource(resource))
}

/// May the session use `permission` on `board`?
#[must_use]
pub fn can(snapshot: &Snapshot, permission: Permission, board: &BoardRow) -> bool {
    let Some(resource) = board.id.resource() else {
        return false;
    };
    // A feed board is read-only for everyone (M3-G, D89): its items come from the feed.
    if board.feed && matches!(permission, Permission::ForumPost | Permission::ForumReply) {
        return false;
    }
    if !snapshot.resolve(permission, Some(resource)).is_granted() {
        return false;
    }
    !board.restricted
        || scoped_to(snapshot, permission, resource)
        || snapshot.resolve(Permission::ForumManage, None).is_granted()
}

/// The boards among `boards` the session may use `permission` on, in order. Uses
/// [`Snapshot::reach`] first, so a session that holds the permission nowhere never looks at a
/// board.
#[must_use]
pub fn reachable<'a>(
    snapshot: &Snapshot,
    permission: Permission,
    boards: &'a [BoardRow],
) -> Vec<&'a BoardRow> {
    let reach = snapshot.reach(permission, ResourceKind::Forum);
    if reach == Reach::None {
        return Vec::new();
    }
    boards
        .iter()
        .filter(|board| {
            board
                .id
                .resource()
                .is_some_and(|resource| reach.contains(resource.id))
        })
        .filter(|board| can(snapshot, permission, board))
        .collect()
}

/// The ids of [`reachable`] boards.
#[must_use]
pub fn reachable_ids(
    snapshot: &Snapshot,
    permission: Permission,
    boards: &[BoardRow],
) -> Vec<BoardId> {
    reachable(snapshot, permission, boards)
        .into_iter()
        .map(|board| board.id)
        .collect()
}
