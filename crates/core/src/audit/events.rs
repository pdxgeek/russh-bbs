//! The events today's core writes make, built in one place so the two backends cannot word
//! them differently. A store calls these inside the transaction of the change.

use super::event::{Action, Actor, AuditEvent, Target};
use crate::access::{Child, Granting, GroupChange, GroupId, NewGrant, Scope, Slug, Subject};
use crate::limits::{Limit, LimitKind};
use crate::store::{AccountId, AccountState};

/// The founder claim made `account` the first sysop. The actor is the new account itself.
#[must_use]
pub fn founder_claimed(account: AccountId) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(account),
        Action::FounderClaimed,
        Target::Account(account),
    )
    .detail("role sysop")
}

/// `account` was created with `role` (besides `member`, which needs no grant) in `state`. The
/// store does not know who asked, so the actor is [`Actor::System`]; a flow that does (an
/// invite redemption, M2-D) appends its own richer event in the same transaction.
#[must_use]
pub fn account_created(account: AccountId, role: &Slug, state: AccountState) -> AuditEvent {
    AuditEvent::new(
        Actor::System,
        Action::AccountCreated,
        Target::Account(account),
    )
    .detail(&format!("role {} state {}", role.as_str(), state.as_str()))
}

/// `actor`, holding `members.edit`, changed `target`'s profile. `fields` are the **names** of the
/// fields that changed, never their values.
#[must_use]
pub fn profile_edited(actor: Actor, target: AccountId, fields: &[&str]) -> AuditEvent {
    AuditEvent::new(actor, Action::ProfileEdited, Target::Account(target))
        .detail(&format!("fields {}", fields.join(" ")))
}

/// A custom group was created.
#[must_use]
pub fn group_created(actor: Actor, group: GroupId, name: &Slug) -> AuditEvent {
    AuditEvent::new(actor, Action::GroupCreated, Target::Group(group))
        .detail(&format!("name {}", name.as_str()))
}

/// A membership or delete change. `group_name` is the group's name, when known, for the note.
#[must_use]
pub fn group_changed(actor: Actor, change: GroupChange, group_name: Option<&Slug>) -> AuditEvent {
    let child_note = |child: Child| match child {
        Child::Account(id) => format!("account {id}"),
        Child::Group(id) => format!("group {id}"),
    };
    match change {
        GroupChange::Add { group, child } => {
            AuditEvent::new(actor, Action::GroupMemberAdded, Target::Group(group))
                .detail(&child_note(child))
        }
        GroupChange::Remove { group, child } => {
            AuditEvent::new(actor, Action::GroupMemberRemoved, Target::Group(group))
                .detail(&child_note(child))
        }
        GroupChange::Delete(group) => {
            let event = AuditEvent::new(actor, Action::GroupDeleted, Target::Group(group));
            match group_name {
                Some(name) => event.detail(&format!("name {}", name.as_str())),
                None => event,
            }
        }
    }
}

/// The target (the subject) and the note ("role x" / "permission y") shared by grant and revoke.
fn grant_parts(grant: &NewGrant) -> (Target, String) {
    let target = match grant.subject {
        Subject::Account(id) => Target::Account(id),
        Subject::Group(id) => Target::Group(id),
    };
    let what = match &grant.granting {
        Granting::Role(role) => format!("role {}", role.as_str()),
        Granting::Permission(permission) => format!("permission {}", permission.name()),
    };
    (target, what)
}

/// A scoped grant names its resource on the event; a global one has none.
fn with_scope(event: AuditEvent, scope: Scope) -> AuditEvent {
    match scope {
        Scope::Global => event,
        Scope::Resource(resource) => event.on(resource),
    }
}

/// A grant was added: the target is the subject, the note says what it gives, and the grant's
/// resource (if scoped) is the event's resource. A role is `role.granted`, one permission
/// `permission.granted`.
#[must_use]
pub fn grant_added(actor: Actor, grant: &NewGrant) -> AuditEvent {
    let (target, what) = grant_parts(grant);
    let action = match grant.granting {
        Granting::Role(_) => Action::RoleGranted,
        Granting::Permission(_) => Action::PermissionGranted,
    };
    with_scope(
        AuditEvent::new(actor, action, target).detail(&what),
        grant.scope,
    )
}

/// A grant was taken away: `role.revoked` or `permission.revoked`, worded like [`grant_added`].
#[must_use]
pub fn grant_removed(actor: Actor, grant: &NewGrant) -> AuditEvent {
    let (target, what) = grant_parts(grant);
    let action = match grant.granting {
        Granting::Role(_) => Action::RoleRevoked,
        Granting::Permission(_) => Action::PermissionRevoked,
    };
    with_scope(
        AuditEvent::new(actor, action, target).detail(&what),
        grant.scope,
    )
}

/// A custom role was created with `rank` and `permissions` permissions.
#[must_use]
pub fn role_created(actor: Actor, role: &Slug, rank: u8, permissions: usize) -> AuditEvent {
    AuditEvent::new(actor, Action::RoleCreated, Target::Role(role.clone()))
        .detail(&format!("rank {rank} permissions {permissions}"))
}

/// A role's permission set was replaced: how many were added and removed (the names are in the
/// role itself; a long list would not fit the 120-character note).
#[must_use]
pub fn role_permissions_set(actor: Actor, role: &Slug, added: usize, removed: usize) -> AuditEvent {
    AuditEvent::new(
        actor,
        Action::RolePermissionsSet,
        Target::Role(role.clone()),
    )
    .detail(&format!("added {added} removed {removed}"))
}

/// A custom role was deleted.
#[must_use]
pub fn role_deleted(actor: Actor, role: &Slug) -> AuditEvent {
    AuditEvent::new(actor, Action::RoleDeleted, Target::Role(role.clone()))
}

/// A resource limit was set (`Some`) or a member override cleared (`None`). The target is the
/// role or the account; the note names the kind and the new value.
#[must_use]
pub fn limit_changed(
    actor: Actor,
    target: Target,
    kind: LimitKind,
    value: Option<Limit>,
) -> AuditEvent {
    let what = match value {
        Some(Limit::AtMost(n)) => format!("{} set to {n}", kind.name()),
        Some(Limit::Unlimited) => format!("{} set to unlimited", kind.name()),
        None => format!("{} override cleared", kind.name()),
    };
    AuditEvent::new(actor, Action::LimitChanged, target).detail(&what)
}

/// A member saved their own terminal settings.
#[must_use]
pub fn terminal_settings_changed(account: AccountId) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(account),
        Action::SettingChanged,
        Target::Account(account),
    )
    .detail("terminal settings")
}
