//! The editing services (M2-B2, D69): create, change and delete roles, grant and revoke roles
//! and permissions. Each takes the **actor's [`Snapshot`]** and refuses through
//! [`Snapshot::require`], so a front end cannot reach the store without the check. The rules
//! live here; the store only persists (and, where a race would break a rule, re-checks inside
//! its transaction: the last `sysop` grant, a role still granted).
//!
//! * **Rank.** The target role's rank must be strictly below the actor's top global rank. One
//!   exception, for `grant_role` and `revoke_role` only: a holder of `sysop` may grant and
//!   revoke `sysop`, because it already holds everything and nothing is escalated.
//! * **Locked roles** (`sysop`, `waiting`) are never created over, changed or deleted.
//!   `waiting` is never granted. Built-in roles may change but not be deleted.
//! * **No escalation.** A permission goes into a role, a role is granted, or a permission is
//!   granted, only if the actor holds every permission involved (on the resource, when scoped).
//!   Taking something away is never escalation.
//! * **No lockout.** The last global `sysop` grant cannot be revoked (the store re-checks in
//!   its transaction). The `everyone` group's `member` grant cannot be revoked.
//!
//! Granting to a group also needs `groups.edit`: it changes what every member of the group
//! holds. Changes apply on the next action, because nothing is cached (D33).

use std::collections::BTreeSet;

use crate::audit::Actor;

use super::groups::Kind;
use super::permission::Permission;
use super::slug::Slug;
use super::snapshot::{Denied, Granted, Scope, Snapshot, Source};
use super::store::{AccessError, AccessStore, Granting, NewGrant, RoleInfo, Subject};

/// Lowest rank a custom role can have: `0` is `waiting`'s.
pub const MIN_CUSTOM_RANK: u8 = 1;

/// Why an edit was refused. Nothing was written, and no audit row either.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    /// The actor lacks `roles.edit` (or `groups.edit` for a group grant).
    #[error("{0}")]
    Denied(#[from] Denied),
    /// The check passed for a session with no account (the waiting room); there is no one to
    /// name as the actor.
    #[error("not signed in")]
    NotSignedIn,
    /// The role's rank is not below the actor's own.
    #[error("role {role} is at or above your rank")]
    RankNotBelow {
        /// The role refused.
        role: Slug,
    },
    /// A new role's rank is outside what the actor may create: at least 1 and below their own.
    #[error("rank {rank} is not allowed: use 1 up to one below your own")]
    RankOutOfRange {
        /// The rank asked for.
        rank: u8,
    },
    /// `sysop` and `waiting` cannot be created over, changed or deleted.
    #[error("role {role} is locked and cannot be changed")]
    RoleLocked {
        /// The role refused.
        role: Slug,
    },
    /// Built-in roles can be edited but not deleted.
    #[error("role {role} is built in and cannot be deleted")]
    RoleBuiltin {
        /// The role refused.
        role: Slug,
    },
    /// The actor does not hold a permission they tried to hand out.
    #[error("you do not hold {} so you cannot give it", .permission.name())]
    Escalation {
        /// The permission the actor lacks.
        permission: Permission,
    },
    /// The `member` role on `everyone` is what makes an approved account a member.
    #[error("everyone always holds the member role; that grant cannot be revoked")]
    ProtectedGrant,
    /// The store refused, or failed.
    #[error(transparent)]
    Access(#[from] AccessError),
}

/// What a caller gives to make a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRole {
    /// The name; unique.
    pub name: Slug,
    /// Ordering only; at least [`MIN_CUSTOM_RANK`] and below the actor's top rank.
    pub rank: u8,
    /// The permissions it holds. The actor must hold each.
    pub permissions: Vec<Permission>,
}

/// The actor, from a proof that must name an account.
fn actor_of(proof: &Granted) -> Result<Actor, EditError> {
    proof
        .account()
        .map(Actor::Account)
        .ok_or(EditError::NotSignedIn)
}

/// The check every edit starts with: the actor and their top global rank. Holding `roles.edit`
/// through a role held only on one resource, or only as an allow entry, leaves no global rank,
/// so it counts as 0 and nothing is below it.
fn gate(actor: &Snapshot, permission: Permission) -> Result<(Actor, u8), EditError> {
    let proof = actor.require(permission, None)?;
    Ok((actor_of(&proof)?, actor.top_rank().unwrap_or(0)))
}

/// `roles.edit`, and `groups.edit` when the subject is a group.
fn gate_grant(actor: &Snapshot, subject: Subject) -> Result<(Actor, u8), EditError> {
    let gated = gate(actor, Permission::RolesEdit)?;
    if matches!(subject, Subject::Group(_)) {
        actor.require(Permission::GroupsEdit, None)?;
    }
    Ok(gated)
}

fn require_rank_below(top: u8, role: &RoleInfo) -> Result<(), EditError> {
    if role.rank < top {
        Ok(())
    } else {
        Err(EditError::RankNotBelow {
            role: role.name.clone(),
        })
    }
}

/// The rank rule for granting and revoking, with the `sysop` exception.
fn require_standing(actor: &Snapshot, top: u8, role: &RoleInfo) -> Result<(), EditError> {
    if role.name == Slug::SYSOP && holds_sysop(actor) {
        return Ok(());
    }
    require_rank_below(top, role)
}

fn holds_sysop(actor: &Snapshot) -> bool {
    actor.held().iter().any(|held| {
        held.scope == Scope::Global
            && matches!(&held.source, Source::Role { role, .. } if *role == Slug::SYSOP)
    })
}

/// The actor holds `permission` where it would apply.
fn require_held(actor: &Snapshot, permission: Permission, scope: Scope) -> Result<(), EditError> {
    let resource = match scope {
        Scope::Global => None,
        Scope::Resource(resource) => Some(resource),
    };
    if actor.resolve(permission, resource).is_granted() {
        Ok(())
    } else {
        Err(EditError::Escalation { permission })
    }
}

async fn existing_role<S: AccessStore>(store: &S, name: &Slug) -> Result<RoleInfo, EditError> {
    store
        .role(name)
        .await
        .map_err(AccessError::from)?
        .ok_or_else(|| AccessError::UnknownRole.into())
}

/// Locked roles are not editable.
fn require_unlocked(role: &RoleInfo) -> Result<(), EditError> {
    match role.kind {
        Kind::Locked => Err(EditError::RoleLocked {
            role: role.name.clone(),
        }),
        Kind::Builtin | Kind::Custom => Ok(()),
    }
}

/// Create a custom role (`roles.edit`). The rank must be 1 up to one below the actor's, and the
/// actor must hold every permission they put in it.
pub async fn create_role<S: AccessStore>(
    store: &S,
    actor: &Snapshot,
    new: NewRole,
) -> Result<(), EditError> {
    let (who, top) = gate(actor, Permission::RolesEdit)?;
    if new.rank < MIN_CUSTOM_RANK || new.rank >= top {
        return Err(EditError::RankOutOfRange { rank: new.rank });
    }
    // A name that is a locked role's gets the specific refusal, not a generic "taken".
    if let Some(existing) = store.role(&new.name).await.map_err(AccessError::from)? {
        require_unlocked(&existing)?;
        return Err(AccessError::RoleNameTaken.into());
    }
    let permissions: BTreeSet<Permission> = new.permissions.into_iter().collect();
    for &permission in &permissions {
        require_held(actor, permission, Scope::Global)?;
    }
    let role = RoleInfo {
        name: new.name,
        rank: new.rank,
        kind: Kind::Custom,
        permissions: permissions.into_iter().collect(),
    };
    store.put_role(&role, who).await?;
    Ok(())
}

/// Replace a role's whole permission set (`roles.edit`). The role must be below the actor's
/// rank and not locked; the actor must hold each permission being **added** (keeping or
/// removing one is never escalation).
pub async fn set_role_permissions<S: AccessStore>(
    store: &S,
    actor: &Snapshot,
    name: &Slug,
    permissions: Vec<Permission>,
) -> Result<(), EditError> {
    let (who, top) = gate(actor, Permission::RolesEdit)?;
    let role = existing_role(store, name).await?;
    require_unlocked(&role)?;
    require_rank_below(top, &role)?;
    let wanted: BTreeSet<Permission> = permissions.into_iter().collect();
    let current: BTreeSet<Permission> = role.permissions.iter().copied().collect();
    for &permission in wanted.difference(&current) {
        require_held(actor, permission, Scope::Global)?;
    }
    let wanted: Vec<Permission> = wanted.into_iter().collect();
    store.set_role_permissions(name, &wanted, who).await?;
    Ok(())
}

/// Delete a custom role (`roles.edit`): below the actor's rank, not built in or locked, and not
/// granted to anyone (revoke its grants first).
pub async fn delete_role<S: AccessStore>(
    store: &S,
    actor: &Snapshot,
    name: &Slug,
) -> Result<(), EditError> {
    let (who, top) = gate(actor, Permission::RolesEdit)?;
    let role = existing_role(store, name).await?;
    require_unlocked(&role)?;
    if role.kind == Kind::Builtin {
        return Err(EditError::RoleBuiltin { role: role.name });
    }
    require_rank_below(top, &role)?;
    store.delete_role(name, who).await?;
    Ok(())
}

/// Give an account or group a role, globally or on one resource (`roles.edit`, and
/// `groups.edit` for a group). Rank rule, plus the actor must hold every permission of the role
/// where it will apply. `waiting` is never granted.
pub async fn grant_role<S: AccessStore>(
    store: &S,
    actor: &Snapshot,
    subject: Subject,
    role: &Slug,
    scope: Scope,
) -> Result<(), EditError> {
    let (who, top) = gate_grant(actor, subject)?;
    let info = existing_role(store, role).await?;
    if info.name == Slug::WAITING {
        return Err(AccessError::RoleNotGrantable.into());
    }
    require_standing(actor, top, &info)?;
    for &permission in &info.permissions {
        require_held(actor, permission, scope)?;
    }
    let grant = NewGrant {
        subject,
        granting: Granting::Role(role.clone()),
        scope,
    };
    store.add_grant(&grant, who).await?;
    Ok(())
}

/// Take a role away from an account or group (`roles.edit`, and `groups.edit` for a group).
/// Rank rule. Refused for `everyone`'s `member` grant and for the last global `sysop` grant.
pub async fn revoke_role<S: AccessStore>(
    store: &S,
    actor: &Snapshot,
    subject: Subject,
    role: &Slug,
    scope: Scope,
) -> Result<(), EditError> {
    let (who, top) = gate_grant(actor, subject)?;
    let info = existing_role(store, role).await?;
    require_standing(actor, top, &info)?;
    if *role == Slug::MEMBER && scope == Scope::Global && is_everyone(store, subject).await? {
        return Err(EditError::ProtectedGrant);
    }
    let grant = NewGrant {
        subject,
        granting: Granting::Role(role.clone()),
        scope,
    };
    store.remove_grant(&grant, who).await?;
    Ok(())
}

/// Add an allow-list entry (`roles.edit`, and `groups.edit` for a group). The actor must hold
/// the permission where it will apply.
pub async fn grant_permission<S: AccessStore>(
    store: &S,
    actor: &Snapshot,
    subject: Subject,
    permission: Permission,
    scope: Scope,
) -> Result<(), EditError> {
    let (who, _) = gate_grant(actor, subject)?;
    require_held(actor, permission, scope)?;
    let grant = NewGrant {
        subject,
        granting: Granting::Permission(permission),
        scope,
    };
    store.add_grant(&grant, who).await?;
    Ok(())
}

/// Remove an allow-list entry (`roles.edit`, and `groups.edit` for a group).
pub async fn revoke_permission<S: AccessStore>(
    store: &S,
    actor: &Snapshot,
    subject: Subject,
    permission: Permission,
    scope: Scope,
) -> Result<(), EditError> {
    let (who, _) = gate_grant(actor, subject)?;
    let grant = NewGrant {
        subject,
        granting: Granting::Permission(permission),
        scope,
    };
    store.remove_grant(&grant, who).await?;
    Ok(())
}

async fn is_everyone<S: AccessStore>(store: &S, subject: Subject) -> Result<bool, EditError> {
    let Subject::Group(id) = subject else {
        return Ok(false);
    };
    let groups = store.groups().await.map_err(AccessError::from)?;
    Ok(groups
        .iter()
        .any(|group| group.id == id && group.name == Slug::EVERYONE))
}
