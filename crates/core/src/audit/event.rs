//! The audit event: a closed set of actions and typed fields only.

use strum::{EnumString, IntoStaticStr, VariantArray};

use crate::access::{Fingerprint, GroupId, ResourceKind, ResourceRef, Slug};
use crate::invites::InviteId;
use crate::queue::RequestId;
use crate::sanitize::{Sanitized, SingleLine};
use crate::store::AccountId;

/// Longest [`Detail`], in characters. The database also checks a looser bound.
pub const DETAIL_MAX_CHARS: usize = 120;

/// What happened. A closed set: a new audited write adds a variant here, so the list of
/// everything the log can say is reviewable in one place (`docs/audit.md`). The stored name
/// is `area.verb`, like permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, IntoStaticStr, VariantArray)]
pub enum Action {
    /// The one-time founder claim created the first sysop.
    #[strum(serialize = "founder.claimed")]
    FounderClaimed,
    /// An account was created.
    #[strum(serialize = "account.created")]
    AccountCreated,
    /// A custom group was created.
    #[strum(serialize = "group.created")]
    GroupCreated,
    /// An account or group was put inside a group.
    #[strum(serialize = "group.member_added")]
    GroupMemberAdded,
    /// An account or group was taken out of a group.
    #[strum(serialize = "group.member_removed")]
    GroupMemberRemoved,
    /// A group was deleted.
    #[strum(serialize = "group.deleted")]
    GroupDeleted,
    /// A custom role was created (M2-B2).
    #[strum(serialize = "role.created")]
    RoleCreated,
    /// A role's permission set was replaced (M2-B2).
    #[strum(serialize = "role.permissions_set")]
    RolePermissionsSet,
    /// A custom role was deleted (M2-B2).
    #[strum(serialize = "role.deleted")]
    RoleDeleted,
    /// A role was granted to an account or group, globally or on a resource.
    #[strum(serialize = "role.granted")]
    RoleGranted,
    /// A role grant was taken away (M2-B2).
    #[strum(serialize = "role.revoked")]
    RoleRevoked,
    /// One permission was granted to an account or group: an allow-list entry.
    #[strum(serialize = "permission.granted")]
    PermissionGranted,
    /// An allow-list entry was taken away (M2-B2).
    #[strum(serialize = "permission.revoked")]
    PermissionRevoked,
    /// A setting was changed (the member's own terminal settings today).
    #[strum(serialize = "setting.changed")]
    SettingChanged,
    /// A member made an invite code (M2-D). The target is the invite's id, never the code.
    #[strum(serialize = "invite.created")]
    InviteCreated,
    /// An invite code was redeemed and made an account (M2-D).
    #[strum(serialize = "invite.redeemed")]
    InviteRedeemed,
    /// An account moved from one state to another (M2-D).
    #[strum(serialize = "account.state_changed")]
    AccountStateChanged,
    /// A resource limit was set or an override cleared (M2-H).
    #[strum(serialize = "limit.changed")]
    LimitChanged,
    /// Staff edited another member's profile (M2-F). The owner's own edits are not audited.
    #[strum(serialize = "profile.edited")]
    ProfileEdited,
    /// A queue request was approved and became an account (M2-E). The target is the account.
    #[strum(serialize = "member.approved")]
    MemberApproved,
    /// A queue request was denied (M2-E). The target is the request's id; nothing of its
    /// content, and not the key, is written.
    #[strum(serialize = "member.denied")]
    MemberDenied,
    /// A member vouched for a queue request (M2-E). The target is the request's id.
    #[strum(serialize = "member.vouched")]
    MemberVouched,
    /// A stranger's membership application was kept in the sysop queue (M2-G). The target is
    /// the request's id; nothing the applicant typed is written.
    #[strum(serialize = "application.submitted")]
    ApplicationSubmitted,
    /// `open` registration used up its daily cap, so the board answers with the `apply` form
    /// for the rest of that UTC day (M2-G). Written once per day, by the account that took the
    /// last place (or by the first refusal, if the cap was lowered below the day's count).
    #[strum(serialize = "registration.capped")]
    RegistrationCapped,
    /// A member set or changed their own password (M3-D, D79). The target is the account; the
    /// note says only which kind of session did it. No secret and no hash is written.
    #[strum(serialize = "password.set")]
    PasswordSet,
    /// A member removed their own password (M3-D, D79). Written only when there was one.
    #[strum(serialize = "password.cleared")]
    PasswordCleared,
}

impl Action {
    /// Every action, for tests and documentation.
    pub const ALL: &'static [Self] = <Self as VariantArray>::VARIANTS;

    /// The stored `area.verb` name.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.into()
    }

    /// Parse a stored name.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        text.parse().ok()
    }
}

/// Who did it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Actor {
    /// The board itself: start-up, a scheduled job, a rule firing with no person behind it.
    System,
    /// A member (or sysop) account.
    Account(AccountId),
}

/// What it was done to. Ids, validated names and public fingerprints only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Nothing in particular.
    None,
    /// An account.
    Account(AccountId),
    /// A group.
    Group(GroupId),
    /// A role.
    Role(Slug),
    /// A key, by its public SHA256 fingerprint. Never key material.
    Key(Fingerprint),
    /// An invite, by its row id. Never the code, which is stored only as a hash (M2-D).
    Invite(InviteId),
    /// A queue request, by its row id. Never its text and never the key (M2-E).
    Request(RequestId),
}

impl Target {
    /// The two stored columns: a kind name and a reference. [`Target::None`] is two empty
    /// strings.
    #[must_use]
    pub fn parts(&self) -> (&'static str, String) {
        match self {
            Self::None => ("", String::new()),
            Self::Account(id) => ("account", id.to_string()),
            Self::Group(id) => ("group", id.to_string()),
            Self::Role(name) => ("role", name.as_str().to_owned()),
            Self::Key(fingerprint) => ("key", fingerprint.as_str().to_owned()),
            Self::Invite(id) => ("invite", id.to_string()),
            Self::Request(id) => ("request", id.to_string()),
        }
    }

    /// Rebuild from the stored columns. `None` for a pair the code could not have written.
    #[must_use]
    pub fn from_parts(kind: &str, reference: &str) -> Option<Self> {
        match kind {
            "" if reference.is_empty() => Some(Self::None),
            "account" => reference
                .parse()
                .ok()
                .map(AccountId::from_raw)
                .map(Self::Account),
            "group" => reference
                .parse()
                .ok()
                .map(GroupId::from_raw)
                .map(Self::Group),
            "role" => Slug::parse(reference).ok().map(Self::Role),
            "key" => Fingerprint::parse(reference).ok().map(Self::Key),
            "invite" => reference
                .parse()
                .ok()
                .map(InviteId::from_raw)
                .map(Self::Invite),
            "request" => reference
                .parse()
                .ok()
                .map(RequestId::from_raw)
                .map(Self::Request),
            _ => None,
        }
    }
}

/// A short note on an event, already through the shared sanitizer and cut to
/// [`DETAIL_MAX_CHARS`]. The only constructor takes raw text and sanitizes it, so a detail
/// cannot hold a control character, an escape sequence or a security-marker codepoint. Callers
/// still must not put a secret in it; the typed fields are the place for ids.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Detail(Sanitized);

impl Detail {
    /// Sanitize `text` as one line and cap it.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self(SingleLine::new(DETAIL_MAX_CHARS).sanitize(text))
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// One thing that happened. Built with [`AuditEvent::new`] and the `on` and `detail` setters;
/// timestamped by the store's clock when written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    /// Who did it.
    pub actor: Actor,
    /// What happened.
    pub action: Action,
    /// What it happened to.
    pub target: Target,
    /// The resource it concerned, if any.
    pub resource: Option<ResourceRef>,
    /// A short note.
    pub detail: Detail,
}

impl AuditEvent {
    /// An event with no resource and no detail.
    #[must_use]
    pub fn new(actor: Actor, action: Action, target: Target) -> Self {
        Self {
            actor,
            action,
            target,
            resource: None,
            detail: Detail::default(),
        }
    }

    /// Say which resource it concerned.
    #[must_use]
    pub fn on(mut self, resource: ResourceRef) -> Self {
        self.resource = Some(resource);
        self
    }

    /// Add a short note (sanitized and capped). The parameter is `&str`, so a secret type
    /// (a setup token, key material) is not accepted by accident:
    ///
    /// ```compile_fail
    /// use russh_bbs_core::audit::{Action, Actor, AuditEvent, Target};
    /// use russh_bbs_core::founder::{SetupToken, TOKEN_BYTES};
    ///
    /// let token = SetupToken::from_bytes([0; TOKEN_BYTES]);
    /// let _ = AuditEvent::new(Actor::System, Action::FounderClaimed, Target::None).detail(&token);
    /// ```
    #[must_use]
    pub fn detail(mut self, text: &str) -> Self {
        self.detail = Detail::new(text);
        self
    }

    /// Rebuild a resource from its stored columns. `None` for a pair the code could not have
    /// written; `Some(None)` for "no resource".
    #[must_use]
    pub fn resource_from_parts(kind: &str, id: i64) -> Option<Option<ResourceRef>> {
        if kind.is_empty() && id == 0 {
            return Some(None);
        }
        let kind = ResourceKind::parse(kind)?;
        let id = crate::access::ResourceId::new(id)?;
        Some(Some(ResourceRef { kind, id }))
    }

    /// The stored resource columns: kind name (empty for none) and id (0 for none).
    #[must_use]
    pub fn resource_parts(&self) -> (&'static str, i64) {
        self.resource
            .map_or(("", 0), |r| (r.kind.name(), r.id.as_raw()))
    }
}
