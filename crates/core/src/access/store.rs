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
    /// A role with this name already exists.
    #[error("a role with that name already exists")]
    RoleNameTaken,
    /// The role is still granted to someone, so it was not deleted. Revoke its grants first.
    #[error("the role is still granted")]
    RoleInUse,
    /// The change would remove the last global `sysop` grant, which would lock the board out
    /// of its own administration.
    #[error("that is the last sysop grant")]
    LastSysop,
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

    /// One role with its permission set, or `None`.
    fn role(
        &self,
        name: &Slug,
    ) -> impl Future<Output = Result<Option<RoleInfo>, StoreError>> + Send {
        async move {
            Ok(self
                .roles()
                .await?
                .into_iter()
                .find(|role| role.name == *name))
        }
    }

    /// Insert a new role with its permission set (a plain write: the rank and escalation rules
    /// are the editing services'). A name already taken is [`AccessError::RoleNameTaken`].
    /// Audited in the same transaction (`role.created`, by `actor`).
    fn put_role(
        &self,
        role: &RoleInfo,
        actor: Actor,
    ) -> impl Future<Output = Result<(), AccessError>> + Send;

    /// Replace a role's whole permission set. An unknown role is [`AccessError::UnknownRole`].
    /// Audited in the same transaction (`role.permissions_set`, by `actor`); a set equal to the
    /// stored one changes nothing and writes no row.
    fn set_role_permissions(
        &self,
        name: &Slug,
        permissions: &[Permission],
        actor: Actor,
    ) -> impl Future<Output = Result<(), AccessError>> + Send;

    /// Delete a role and its permission set. Refused with [`AccessError::RoleInUse`] while any
    /// grant names it (decision D69: refuse, never cascade away someone's access). The check and
    /// the delete are one transaction. Audited (`role.deleted`, by `actor`).
    fn delete_role(
        &self,
        name: &Slug,
        actor: Actor,
    ) -> impl Future<Output = Result<(), AccessError>> + Send;

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
    /// it, and the rank rules, belong to the editing services. A grant that is new is audited
    /// in the same transaction (`role.granted` or `permission.granted`, by `actor`); one that
    /// already existed changed nothing and writes no row.
    fn add_grant(
        &self,
        grant: &NewGrant,
        actor: Actor,
    ) -> impl Future<Output = Result<(), AccessError>> + Send;

    /// Remove a grant. Removing one that is not there is not an error and writes no row.
    /// Removing the last global `sysop` grant is refused ([`AccessError::LastSysop`]) in the
    /// same transaction as the delete, so two concurrent revokes cannot both pass. Audited
    /// (`role.revoked` or `permission.revoked`, by `actor`).
    fn remove_grant(
        &self,
        grant: &NewGrant,
        actor: Actor,
    ) -> impl Future<Output = Result<(), AccessError>> + Send;
}

/// The empty directory holds no roles, groups or grants: the snapshot is empty (nothing is
/// held) and every write is refused.
impl AccessStore for super::NoMembers {
    fn snapshot(
        &self,
        _account: Option<AccountId>,
    ) -> impl Future<Output = Result<Snapshot, StoreError>> + Send {
        std::future::ready(Ok(Snapshot::empty()))
    }

    fn roles(&self) -> impl Future<Output = Result<Vec<RoleInfo>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn put_role(
        &self,
        _role: &RoleInfo,
        _actor: Actor,
    ) -> impl Future<Output = Result<(), AccessError>> + Send {
        std::future::ready(Err(AccessError::UnknownAccount))
    }

    fn set_role_permissions(
        &self,
        _name: &Slug,
        _permissions: &[Permission],
        _actor: Actor,
    ) -> impl Future<Output = Result<(), AccessError>> + Send {
        std::future::ready(Err(AccessError::UnknownRole))
    }

    fn delete_role(
        &self,
        _name: &Slug,
        _actor: Actor,
    ) -> impl Future<Output = Result<(), AccessError>> + Send {
        std::future::ready(Err(AccessError::UnknownRole))
    }

    fn groups(&self) -> impl Future<Output = Result<Vec<GroupInfo>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn create_group(
        &self,
        _name: &Slug,
        _actor: Actor,
    ) -> impl Future<Output = Result<GroupId, AccessError>> + Send {
        std::future::ready(Err(AccessError::UnknownAccount))
    }

    fn change_groups(
        &self,
        _change: GroupChange,
        _actor: Actor,
    ) -> impl Future<Output = Result<(), AccessError>> + Send {
        std::future::ready(Err(AccessError::UnknownAccount))
    }

    fn add_grant(
        &self,
        _grant: &NewGrant,
        _actor: Actor,
    ) -> impl Future<Output = Result<(), AccessError>> + Send {
        std::future::ready(Err(AccessError::UnknownAccount))
    }

    fn remove_grant(
        &self,
        _grant: &NewGrant,
        _actor: Actor,
    ) -> impl Future<Output = Result<(), AccessError>> + Send {
        std::future::ready(Err(AccessError::UnknownAccount))
    }
}
