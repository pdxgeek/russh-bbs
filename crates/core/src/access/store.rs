//! The storage trait for access data: persistence only. The rules are in [`super::snapshot`]
//! (checks) and [`super::groups`] (nesting); a backend runs them inside its transactions.

use std::future::Future;

use crate::audit::Actor;
use crate::store::{AccountId, StoreError};

use super::groups::{GroupChange, GroupError, GroupId, GroupInfo, Kind};
use super::permission::Permission;
use super::slug::Slug;
use super::snapshot::{Scope, Snapshot};

/// A role as stored: its name, rank and complete permission set (no inheritance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleInfo {
    /// The role's name.
    pub name: Slug,
    /// Ordering only (`docs/access.md`); never unlocks anything by itself.
    pub rank: u8,
    /// How much of it may change.
    pub kind: Kind,
    /// Everything the role holds, sorted.
    pub permissions: Vec<Permission>,
}

/// Who a grant is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    /// One account.
    Account(AccountId),
    /// Every account in a group, directly or nested.
    Group(GroupId),
}

/// What a grant gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Granting {
    /// A role: all its permissions, on the grant's scope.
    Role(Slug),
    /// One permission: an allow-list entry.
    Permission(Permission),
}

/// A row of the one grant table: a subject gets a role or a permission, globally or on one
/// resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGrant {
    /// Who gets it.
    pub subject: Subject,
    /// What they get.
    pub granting: Granting,
    /// Where it applies.
    pub scope: Scope,
}

/// Why an access write failed.
#[derive(Debug, thiserror::Error)]
pub enum AccessError {
    /// The storage backend failed, or a stored value was invalid.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The group change was refused. Nothing was written.
    #[error(transparent)]
    Group(#[from] GroupError),
    /// The role named does not exist.
    #[error("no such role")]
    UnknownRole,
    /// The account named does not exist.
    #[error("no such account")]
    UnknownAccount,
    /// A group with this name already exists.
    #[error("a group with that name already exists")]
    GroupNameTaken,
    /// The `waiting` role is only for unknown and inactive keys and cannot be granted.
    #[error("that role cannot be granted")]
    RoleNotGrantable,
}

/// Roles, groups, grants and the per-action snapshot.
pub trait AccessStore: Send + Sync {
    /// What `account` holds right now, loaded with one query. `None`, an unknown id, or an
    /// account that is not `active` all give the `waiting` role's snapshot (so suspension and
    /// the waiting room need no special case in `resolve`).
    fn snapshot(
        &self,
        account: Option<AccountId>,
    ) -> impl Future<Output = Result<Snapshot, StoreError>> + Send;

    /// Every role with its permission set.
    fn roles(&self) -> impl Future<Output = Result<Vec<RoleInfo>, StoreError>> + Send;

    /// Every group.
    fn groups(&self) -> impl Future<Output = Result<Vec<GroupInfo>, StoreError>> + Send;

    /// Create a custom group. Audited in the same transaction (`group.created`, by `actor`).
    fn create_group(
        &self,
        name: &Slug,
        actor: Actor,
    ) -> impl Future<Output = Result<GroupId, AccessError>> + Send;

    /// Change group membership or delete a group. One transaction: validate with
    /// [`super::GroupGraph`] (cycles and depth), write, rebuild the closure, and write the
    /// audit row (`group.member_added`, `group.member_removed` or `group.deleted`, by `actor`).
    fn change_groups(
        &self,
        change: GroupChange,
        actor: Actor,
    ) -> impl Future<Output = Result<(), AccessError>> + Send;

    /// Add a grant. Adding one that exists is not an error. This is a plain write: who may call
    /// it, and the rank rules, belong to the editing services (M2-B2). A grant that is new is
    /// audited in the same transaction (`grant.added`, by `actor`); one that already existed
    /// changed nothing and writes no row.
    fn add_grant(
        &self,
        grant: &NewGrant,
        actor: Actor,
    ) -> impl Future<Output = Result<(), AccessError>> + Send;
}
