//! The events today's core writes make, built in one place so the two backends cannot word
//! them differently. A store calls these inside the transaction of the change.

use super::event::{Action, Actor, AuditEvent, Target};
use crate::access::{
    Child, Fingerprint, Granting, GroupChange, GroupId, NewGrant, ResourceRef, Scope, Slug, Subject,
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

/// `account` added `fingerprint` to itself by giving its handle and password in the waiting
/// room (D94 (8)). The fingerprint is public; no password or hash is passed here.
#[must_use]
pub fn key_linked(account: AccountId, fingerprint: &Fingerprint) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(account),
        Action::KeyLinked,
        Target::Key(fingerprint.clone()),
    )
    .detail("handle and password")
}

/// `account` pasted `fingerprint` into My keys (FIX-M3-11). The fingerprint is public.
#[must_use]
pub fn key_added(account: AccountId, fingerprint: &Fingerprint) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(account),
        Action::KeyAdded,
        Target::Key(fingerprint.clone()),
    )
    .detail("my keys")
}

/// `account` removed `fingerprint` in My keys (FIX-M3-11).
#[must_use]
pub fn key_removed(account: AccountId, fingerprint: &Fingerprint) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(account),
        Action::KeyRemoved,
        Target::Key(fingerprint.clone()),
    )
    .detail("my keys")
}

/// `account` made a device-link code (FIX-M3-11). No code, no hash.
#[must_use]
pub fn device_code_issued(account: AccountId) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(account),
        Action::DeviceCodeIssued,
        Target::Account(account),
    )
}

/// A device-link code bound `fingerprint` to `account` (FIX-M3-11).
#[must_use]
pub fn device_code_redeemed(account: AccountId, fingerprint: &Fingerprint) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(account),
        Action::DeviceCodeRedeemed,
        Target::Key(fingerprint.clone()),
    )
    .detail("device link code")
}

/// `staff` made a recovery code for `account` (FIX-M3-11). No code, no hash.
#[must_use]
pub fn recovery_code_issued(staff: AccountId, account: AccountId) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(staff),
        Action::RecoveryCodeIssued,
        Target::Account(account),
    )
}

/// A recovery code bound `fingerprint` to `account` and set a new password (FIX-M3-11). No
/// password, no hash.
#[must_use]
pub fn recovery_code_redeemed(account: AccountId, fingerprint: &Fingerprint) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(account),
        Action::RecoveryCodeRedeemed,
        Target::Key(fingerprint.clone()),
    )
    .detail("recovery code, password replaced")
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

/// Staff (`actor`) removed `author`'s `kind` entry number `entry` (M3-J, D86). The text is not
/// recorded: only that a one-liner or wall line was removed, and whose it was.
#[must_use]
pub fn community_entry_removed(
    actor: AccountId,
    author: AccountId,
    kind: &str,
    entry: i64,
) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(actor),
        Action::CommunityEntryRemoved,
        Target::Account(author),
    )
    .detail(&format!("{kind} {entry}"))
}

/// Staff (`actor`) closed poll `poll`, which `creator` made (M3-J, D86). Not written when the
/// creator closes their own poll.
#[must_use]
pub fn community_poll_closed(actor: AccountId, creator: AccountId, poll: i64) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(actor),
        Action::CommunityPollClosed,
        Target::Account(creator),
    )
    .detail(&format!("poll {poll}"))
}

/// Staff (`actor`) removed poll `poll`, which `creator` made, and its votes (M3-J, D86).
#[must_use]
pub fn community_poll_removed(actor: AccountId, creator: AccountId, poll: i64) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(actor),
        Action::CommunityPollRemoved,
        Target::Account(creator),
    )
    .detail(&format!("poll {poll}"))
}

/// Staff opened the chat room `room` named `name`. Only the name is written; nothing is ever
/// said in a room that the log could hold.
#[must_use]
pub fn chat_room_created(actor: AccountId, room: ResourceRef, name: &Slug) -> AuditEvent {
    AuditEvent::new(Actor::Account(actor), Action::ChatRoomCreated, Target::None)
        .on(room)
        .detail(&format!("name {}", name.as_str()))
}

/// Staff closed the chat room `room`.
#[must_use]
pub fn chat_room_closed(actor: AccountId, room: ResourceRef) -> AuditEvent {
    AuditEvent::new(Actor::Account(actor), Action::ChatRoomClosed, Target::None).on(room)
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

/// `actor` added a feed whose items appear on the new read-only `board` (M3-G, D89). Whether
/// plain http was allowed is written; the address is not.
#[must_use]
pub fn feed_added(actor: AccountId, board: ResourceRef, allow_http: bool) -> AuditEvent {
    AuditEvent::new(Actor::Account(actor), Action::FeedAdded, Target::None)
        .on(board)
        .detail(if allow_http { "http allowed" } else { "https" })
}

/// `actor` removed the feed (and so the board, now gone) with id `board` (M3-G). The board is
/// named in the detail by id because the resource no longer exists.
#[must_use]
pub fn feed_removed(actor: AccountId, board: i64) -> AuditEvent {
    AuditEvent::new(Actor::Account(actor), Action::FeedRemoved, Target::None)
        .detail(&format!("board {board}"))
}

/// `actor` made `board` readable as RSS by outside readers, or stopped that (M3-G, D89).
#[must_use]
pub fn board_published(actor: AccountId, board: ResourceRef, public: bool) -> AuditEvent {
    let action = if public {
        Action::BoardPublished
    } else {
        Action::BoardUnpublished
    };
    AuditEvent::new(Actor::Account(actor), action, Target::None).on(board)
}

/// `reporter` reported mail message `message` (sent by `sender`, when known) to staff as report
/// `report` (M3-I, D90). Ids only: never a subject, a text or a handle.
#[must_use]
pub fn mail_reported(
    reporter: AccountId,
    sender: Option<AccountId>,
    report: i64,
    message: i64,
) -> AuditEvent {
    let target = sender.map_or(Target::None, Target::Account);
    AuditEvent::new(Actor::Account(reporter), Action::MailReported, target)
        .detail(&format!("report {report} message {message}"))
}

/// Staff member `actor` closed mail report `report` (M3-I).
#[must_use]
pub fn mail_report_closed(actor: AccountId, report: i64) -> AuditEvent {
    AuditEvent::new(
        Actor::Account(actor),
        Action::MailReportClosed,
        Target::None,
    )
    .detail(&format!("report {report}"))
}

/// Staff deleted `player`'s save in `door` (M4-D0, D98). The door id is a fixed slug, never
/// player text; the save's contents are not written.
#[must_use]
pub fn door_player_reset(actor: Actor, player: AccountId, door: &str) -> AuditEvent {
    AuditEvent::new(actor, Action::DoorPlayerReset, Target::Account(player))
        .detail(&format!("door {door}"))
}

/// Staff ended the season of `door` (M4-D0).
#[must_use]
pub fn door_season_ended(actor: Actor, door: &str) -> AuditEvent {
    AuditEvent::new(actor, Action::DoorSeasonEnded, Target::None).detail(&format!("door {door}"))
}

/// Staff barred `player` from `door` (M4-D0).
#[must_use]
pub fn door_player_barred(actor: Actor, player: AccountId, door: &str) -> AuditEvent {
    AuditEvent::new(actor, Action::DoorPlayerBarred, Target::Account(player))
        .detail(&format!("door {door}"))
}

/// Staff lifted the bar on `player` for `door` (M4-D0).
#[must_use]
pub fn door_player_unbarred(actor: Actor, player: AccountId, door: &str) -> AuditEvent {
    AuditEvent::new(actor, Action::DoorPlayerUnbarred, Target::Account(player))
        .detail(&format!("door {door}"))
}
