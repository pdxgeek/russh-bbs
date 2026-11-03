//! The events today's core writes make, built in one place so the two backends cannot word
//! them differently. A store calls these inside the transaction of the change.

use super::event::{Action, Actor, AuditEvent, Target};
use crate::access::{
    Child, Granting, GroupChange, GroupId, NewGrant, ResourceRef, Scope, Slug, Subject,
};
use crate::invites::InviteId;
use crate::limits::{Limit, LimitKind};
use crate::queue::RequestId;
use crate::store::{AccountId, AccountState, UnixSeconds};

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

/// `account` set or changed its own password. `note` says which kind of session did it (a fixed
/// phrase from the passwords module). No password, hash or salt is ever passed here.
#[must_use]
pub fn password_set(account: AccountId, note: &str) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(account),
        Action::PasswordSet,
        Target::Account(account),
    )
    .detail(note)
}

/// `account` removed its own password. Worded like [`password_set`].
#[must_use]
pub fn password_cleared(account: AccountId, note: &str) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(account),
        Action::PasswordCleared,
        Target::Account(account),
    )
    .detail(note)
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

/// A member made an invite. The target is the invite's id; the code is not known here and is
/// never written. `expires_at` goes in the note.
#[must_use]
pub fn invite_created(inviter: AccountId, invite: InviteId, expires_at: UnixSeconds) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(inviter),
        Action::InviteCreated,
        Target::Invite(invite),
    )
    .detail(&format!("expires at {}", expires_at.as_raw()))
}

/// A redemption made `account`. Written beside [`invite_redeemed`] in the same transaction;
/// unlike [`account_created`] it knows who asked (the new account's own key) and who vouched.
#[must_use]
pub fn account_created_via_invite(
    account: AccountId,
    invite: InviteId,
    inviter: AccountId,
    state: AccountState,
) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(account),
        Action::AccountCreated,
        Target::Account(account),
    )
    .detail(&format!(
        "via invite {invite} by account {inviter} state {}",
        state.as_str()
    ))
}

/// An invite was redeemed. The actor is the new account; the target is the invite.
#[must_use]
pub fn invite_redeemed(account: AccountId, invite: InviteId, inviter: AccountId) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(account),
        Action::InviteRedeemed,
        Target::Invite(invite),
    )
    .detail(&format!("account {account} inviter {inviter}"))
}

/// `account` moved from `from` to `to`, by `actor`.
#[must_use]
pub fn account_state_changed(
    actor: Actor,
    account: AccountId,
    from: AccountState,
    to: AccountState,
) -> AuditEvent {
    AuditEvent::new(actor, Action::AccountStateChanged, Target::Account(account)).detail(&format!(
        "{} to {}",
        from.as_str(),
        to.as_str()
    ))
}

/// `actor` approved request `request`, which made `account`. The note has the request's id and
/// nothing the requester typed.
#[must_use]
pub fn member_approved(actor: AccountId, account: AccountId, request: RequestId) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(actor),
        Action::MemberApproved,
        Target::Account(account),
    )
    .detail(&format!("request {request}"))
}

/// The approval also made `account`: the same record every account has, with the approver as
/// the actor and the request in the note.
#[must_use]
pub fn account_created_via_request(
    actor: AccountId,
    account: AccountId,
    request: RequestId,
    state: AccountState,
) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(actor),
        Action::AccountCreated,
        Target::Account(account),
    )
    .detail(&format!("via request {request} state {}", state.as_str()))
}

/// `actor` denied request `request`. No content and no key: the request is gone, and only a
/// hash of its key remains, for a day, outside the log.
#[must_use]
pub fn member_denied(actor: AccountId, request: RequestId) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(actor),
        Action::MemberDenied,
        Target::Request(request),
    )
}

/// `voucher` vouched for request `request`.
#[must_use]
pub fn member_vouched(voucher: AccountId, request: RequestId) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(voucher),
        Action::MemberVouched,
        Target::Request(request),
    )
}

/// An account was made by `open` registration (M2-G). Nobody vouched and nobody decided: the
/// actor is the board itself, the target is the new account, and the account has no inviter.
#[must_use]
pub fn account_created_via_open(account: AccountId, state: AccountState) -> AuditEvent {
    AuditEvent::new(
        Actor::System,
        Action::AccountCreated,
        Target::Account(account),
    )
    .detail(&format!("via open state {}", state.as_str()))
}

/// A membership application was kept as request `request` (M2-G). No account exists yet, so the
/// actor is the board and the target is the request's id: no handle, no message, no key.
#[must_use]
pub fn application_submitted(request: RequestId) -> AuditEvent {
    AuditEvent::new(
        Actor::System,
        Action::ApplicationSubmitted,
        Target::Request(request),
    )
}

/// The `open` daily cap was reached on `day` (the number of whole days since 1970-01-01 UTC), so
/// the board answers with the `apply` form until the day ends (M2-G). `cap` is the cap that was
/// in force.
#[must_use]
pub fn registration_capped(day: i64, cap: u32) -> AuditEvent {
    AuditEvent::new(Actor::System, Action::RegistrationCapped, Target::None)
        .detail(&format!("day {day} cap {cap}"))
}

/// `actor` created `board`, `restricted` or open (M3-E, D83). The name is not written.
#[must_use]
pub fn board_created(actor: AccountId, board: ResourceRef, restricted: bool) -> AuditEvent {
    AuditEvent::new(Actor::Account(actor), Action::BoardCreated, Target::None)
        .on(board)
        .detail(if restricted { "restricted" } else { "open" })
}

/// `actor`, holding `forum.moderate` on `board`, removed post `post` written by `author`; when
/// `thread_removed` it was the opening post and the whole thread went (M3-E). Ids only.
#[must_use]
pub fn post_removed(
    actor: AccountId,
    author: Option<AccountId>,
    board: ResourceRef,
    post: i64,
    thread_removed: bool,
) -> AuditEvent {
    let target = author.map_or(Target::None, Target::Account);
    AuditEvent::new(Actor::Account(actor), Action::PostRemoved, target)
        .on(board)
        .detail(&format!(
            "post {post}{}",
            if thread_removed {
                " with its thread"
            } else {
                ""
            }
        ))
}
