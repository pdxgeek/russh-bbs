//! The closed set of named permissions (D33): one per action, `area.verb`.
//!
//! The enum is the code half of the list in `docs/access.md`; a test keeps the two equal. A new
//! action adds its variant, its row in that table and a migration giving the built-in roles
//! their rows (`sysop` always) in the same PR. Matches over [`Permission`] have no wildcard
//! arm, so adding a variant is a compile error wherever a decision depends on it.
//!
//! The `strum` derives give the stored/doc name, parsing and the list of variants, so the names
//! are written once (D47). A name read back from the database that is not here fails to parse
//! and is skipped by the loader (fail closed).

use strum::{EnumString, IntoStaticStr, VariantArray};

/// A named permission (`area.verb`, as in `docs/access.md`).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    EnumString,
    IntoStaticStr,
    VariantArray,
)]
pub enum Permission {
    /// `waiting.view`.
    #[strum(serialize = "waiting.view")]
    WaitingView,
    /// `invite.redeem`.
    #[strum(serialize = "invite.redeem")]
    InviteRedeem,
    /// `membership.apply`.
    #[strum(serialize = "membership.apply")]
    MembershipApply,
    /// `account.edit_profile`.
    #[strum(serialize = "account.edit_profile")]
    AccountEditProfile,
    /// `account.edit_settings`.
    #[strum(serialize = "account.edit_settings")]
    AccountEditSettings,
    /// `account.set_password`.
    #[strum(serialize = "account.set_password")]
    AccountSetPassword,
    /// `members.view`.
    #[strum(serialize = "members.view")]
    MembersView,
    /// `members.approve`.
    #[strum(serialize = "members.approve")]
    MembersApprove,
    /// `members.deny`.
    #[strum(serialize = "members.deny")]
    MembersDeny,
    /// `members.vouch`.
    #[strum(serialize = "members.vouch")]
    MembersVouch,
    /// `members.suspend`.
    #[strum(serialize = "members.suspend")]
    MembersSuspend,
    /// `members.edit`.
    #[strum(serialize = "members.edit")]
    MembersEdit,
    /// `members.delete_data`.
    #[strum(serialize = "members.delete_data")]
    MembersDeleteData,
    /// `invites.create`.
    #[strum(serialize = "invites.create")]
    InvitesCreate,
    /// `invites.revoke`.
    #[strum(serialize = "invites.revoke")]
    InvitesRevoke,
    /// `invites.view_tree`.
    #[strum(serialize = "invites.view_tree")]
    InvitesViewTree,
    /// `forum.list`.
    #[strum(serialize = "forum.list")]
    ForumList,
    /// `forum.read`.
    #[strum(serialize = "forum.read")]
    ForumRead,
    /// `forum.post`.
    #[strum(serialize = "forum.post")]
    ForumPost,
    /// `forum.reply`.
    #[strum(serialize = "forum.reply")]
    ForumReply,
    /// `forum.edit_own`.
    #[strum(serialize = "forum.edit_own")]
    ForumEditOwn,
    /// `forum.moderate`.
    #[strum(serialize = "forum.moderate")]
    ForumModerate,
    /// `forum.manage`.
    #[strum(serialize = "forum.manage")]
    ForumManage,
    /// `chat.join`.
    #[strum(serialize = "chat.join")]
    ChatJoin,
    /// `chat.speak`.
    #[strum(serialize = "chat.speak")]
    ChatSpeak,
    /// `chat.whisper`.
    #[strum(serialize = "chat.whisper")]
    ChatWhisper,
    /// `chat.moderate`.
    #[strum(serialize = "chat.moderate")]
    ChatModerate,
    /// `chat.manage_rooms`.
    #[strum(serialize = "chat.manage_rooms")]
    ChatManageRooms,
    /// `mail.read`.
    #[strum(serialize = "mail.read")]
    MailRead,
    /// `mail.send`.
    #[strum(serialize = "mail.send")]
    MailSend,
    /// `files.list`.
    #[strum(serialize = "files.list")]
    FilesList,
    /// `files.download`.
    #[strum(serialize = "files.download")]
    FilesDownload,
    /// `files.upload`.
    #[strum(serialize = "files.upload")]
    FilesUpload,
    /// `files.tag`.
    #[strum(serialize = "files.tag")]
    FilesTag,
    /// `files.approve`.
    #[strum(serialize = "files.approve")]
    FilesApprove,
    /// `files.manage_areas`.
    #[strum(serialize = "files.manage_areas")]
    FilesManageAreas,
    /// `feeds.read`.
    #[strum(serialize = "feeds.read")]
    FeedsRead,
    /// `feeds.manage`.
    #[strum(serialize = "feeds.manage")]
    FeedsManage,
    /// `doors.run`.
    #[strum(serialize = "doors.run")]
    DoorsRun,
    /// `doors.manage`.
    #[strum(serialize = "doors.manage")]
    DoorsManage,
    /// `oneliners.post`.
    #[strum(serialize = "oneliners.post")]
    OnelinersPost,
    /// `graffiti.post`.
    #[strum(serialize = "graffiti.post")]
    GraffitiPost,
    /// `polls.vote`.
    #[strum(serialize = "polls.vote")]
    PollsVote,
    /// `polls.create`.
    #[strum(serialize = "polls.create")]
    PollsCreate,
    /// `events.view`.
    #[strum(serialize = "events.view")]
    EventsView,
    /// `events.manage`.
    #[strum(serialize = "events.manage")]
    EventsManage,
    /// `sysop.page`.
    #[strum(serialize = "sysop.page")]
    SysopPage,
    /// `sysop.chat`.
    #[strum(serialize = "sysop.chat")]
    SysopChat,
    /// `sessions.view`.
    #[strum(serialize = "sessions.view")]
    SessionsView,
    /// `sessions.disconnect`.
    #[strum(serialize = "sessions.disconnect")]
    SessionsDisconnect,
    /// `audit.view`.
    #[strum(serialize = "audit.view")]
    AuditView,
    /// `settings.edit`.
    #[strum(serialize = "settings.edit")]
    SettingsEdit,
    /// `roles.edit`.
    #[strum(serialize = "roles.edit")]
    RolesEdit,
    /// `groups.edit`.
    #[strum(serialize = "groups.edit")]
    GroupsEdit,
    /// `limits.edit`.
    #[strum(serialize = "limits.edit")]
    LimitsEdit,
    /// `themes.manage`.
    #[strum(serialize = "themes.manage")]
    ThemesManage,
    /// `backups.run`.
    #[strum(serialize = "backups.run")]
    BackupsRun,
}

impl Permission {
    /// Every permission, in the order of `docs/access.md`.
    pub const ALL: &'static [Self] = <Self as VariantArray>::VARIANTS;

    /// The `area.verb` name used in `docs/access.md` and stored in the database.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.into()
    }

    /// Parse a stored or documented name. `None` for anything that is not a permission.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        text.parse().ok()
    }
}
