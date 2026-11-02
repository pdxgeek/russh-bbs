//! Named permissions and the one question a menu asks: "may this session do that?".
//!
//! **This is a minimal placeholder for M2-B.** The closed enum of every permission in
//! `docs/access.md`, roles as permission sets, groups and per-resource policies are M2-B's
//! (D33). This file holds only the permissions the M2-A menu framework names, so a menu item
//! can carry one, plus the single hook ([`PermissionCheck`]) the menu code calls. M2-B replaces
//! [`RolePermissions`] with the real resolution and adds the remaining variants; menu code does
//! not change, because it only ever calls [`PermissionCheck::allows`].

use crate::store::Role;

/// A named permission (`area.verb`, as in `docs/access.md`). Closed on purpose: an action
/// cannot be offered without a name here and a row in `docs/access.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permission {
    /// `account.edit_settings`: change one's own terminal settings.
    AccountEditSettings,
    /// `members.view`: see the members list.
    MembersView,
    /// `members.approve`: approve a membership request.
    MembersApprove,
    /// `forum.list`: see that forums exist.
    ForumList,
    /// `mail.read`: open one's mailbox.
    MailRead,
    /// `files.list`: see file areas.
    FilesList,
}

impl Permission {
    /// Every permission this placeholder knows.
    pub const ALL: [Self; 6] = [
        Self::AccountEditSettings,
        Self::MembersView,
        Self::MembersApprove,
        Self::ForumList,
        Self::MailRead,
        Self::FilesList,
    ];

    /// The `area.verb` name used in `docs/access.md`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AccountEditSettings => "account.edit_settings",
            Self::MembersView => "members.view",
            Self::MembersApprove => "members.approve",
            Self::ForumList => "forum.list",
            Self::MailRead => "mail.read",
            Self::FilesList => "files.list",
        }
    }
}

/// The single access hook for menus: may this session use something that needs `permission`?
///
/// Menu code asks nothing else. Visibility and activation use the same answer, so an item a
/// session cannot see is also not reachable by its hotkey.
pub trait PermissionCheck {
    /// Whether the session holds `permission`.
    fn allows(&self, permission: Permission) -> bool;
}

/// Today's stand-in: permissions follow the stored [`Role`]. A `member` holds everything that
/// exists except `members.approve`; the `sysop` holds everything (`docs/access.md`, "Default
/// roles"). M2-B replaces this with the resolved permission set of the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RolePermissions(Role);

impl RolePermissions {
    /// The permissions of a session whose account has `role`.
    #[must_use]
    pub const fn new(role: Role) -> Self {
        Self(role)
    }
}

impl PermissionCheck for RolePermissions {
    fn allows(&self, permission: Permission) -> bool {
        match self.0 {
            Role::Sysop => true,
            Role::Member => !matches!(permission, Permission::MembersApprove),
        }
    }
}
