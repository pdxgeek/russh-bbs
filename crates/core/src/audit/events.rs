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

/// A grant was added: the target is the subject, the note says what it gives, and the grant's
/// resource (if scoped) is the event's resource.
#[must_use]
pub fn grant_added(actor: Actor, grant: &NewGrant) -> AuditEvent {
    let target = match grant.subject {
        Subject::Account(id) => Target::Account(id),
        Subject::Group(id) => Target::Group(id),
    };
    let what = match &grant.granting {
        Granting::Role(role) => format!("role {}", role.as_str()),
        Granting::Permission(permission) => format!("permission {}", permission.name()),
    };
    let event = AuditEvent::new(actor, Action::GrantAdded, target).detail(&what);
    match grant.scope {
        Scope::Global => event,
        Scope::Resource(resource) => event.on(resource),
    }
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
