//! The menu tree: what exists and where it leads (`docs/ui.md`, "Four separate layers").
//!
//! A [`MenuTree`] is **data**, defined once (see [`main_tree`]). It holds no text and no
//! drawing: every label is a *string-table key* that the front end resolves, and any layout
//! can render the same tree. Adding a layout does not touch this module.
//!
//! * [`MenuTree::validate`] rejects a tree a session could get stuck in or confused by:
//!   duplicate hotkeys, hotkeys that clash with the universal keys, dangling targets,
//!   unreachable or empty menus.
//! * Which items a session sees comes from **one hook**,
//!   [`PermissionCheck`](crate::access::PermissionCheck). The visible set is also the
//!   activatable set: a hotkey of an item the session cannot see does nothing.
//! * [`MenuSession`] is the navigation state (a stack of menus, the help overlay and a
//!   notice) and produces a [`MenuPage`] for a layout to draw.
//!
//! **Universal keys** are the same in every menu and cannot be taken by an item: `?` help,
//! Esc back, Ctrl-C / Ctrl-D quit ([`UniversalKey`]). Quit is a session-wide rule (it logs off
//! from any screen), so the session handles it, not the menu.

mod custom;
mod nav;
mod outline;
mod tools;
mod tree;

use crate::access::{Permission, PermissionCheck};

pub use custom::{
    Built, CustomError, ItemSpec, MAX_GROUPS, MAX_ITEMS, MAX_LABEL_CHARS, MAX_LOADS, MAX_NESTING,
    MAX_PAGES, MenuSpec, Spec, build, builtin_ids, builtin_menus, valid_page_id,
};
pub use nav::{MenuKey, MenuOutcome, MenuPage, MenuSession, PageItem};
pub use outline::{Outline, OutlineKind, OutlineLine, outline};
pub use tools::{
    Bulletin, BulletinKey, MenuToolsKey, MenuToolsListing, MenuToolsRequest, MenuToolsScreen,
    MenuToolsStep, MenuToolsView, PageRow, RoleRow,
};
pub use tree::main_tree;

/// The key that opens help in every menu.
pub const HELP_KEY: char = '?';

/// Deepest the menu stack may grow. A cycle in the tree cannot grow it without bound.
pub const MAX_DEPTH: usize = 8;

/// A key that means the same thing everywhere. Items may not use [`UniversalKey::Help`]'s
/// character; Back and Quit are not printable characters, so they cannot clash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UniversalKey {
    /// `?`: help for the current menu.
    Help,
    /// Esc: back one menu (at the top: nothing).
    Back,
    /// Ctrl-C / Ctrl-D: log off.
    Quit,
}

impl UniversalKey {
    /// Every universal key.
    pub const ALL: [Self; 3] = [Self::Help, Self::Back, Self::Quit];
}

/// What picking an item does when it is not a navigation or a placeholder. Each carries the
/// permission it needs ([`Action::permission`]); adding one means adding its row to
/// `docs/access.md` and its variant to [`Permission`] in the same change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Open the forums: the board list (`forum.list`, M3-E).
    Forums,
    /// Open the mailbox: the inbox (`mail.read`, M3-I).
    Mail,
    /// Open the file areas (`files.list`, M4-F2): browse, search, details, the approval queue.
    Files,
    /// Open the terminal settings screen.
    TerminalSettings,
    /// Make an invite code and show it once (`invites.create`, M2-D).
    InviteCreate,
    /// List the member's own invites (`invites.create`, M2-D).
    InviteList,
    /// Show the invite tree (`invites.view_tree`, M2-D).
    InviteTree,
    /// Open the member list (M2-F).
    MemberList,
    /// Open the signed-in member's own profile (M2-F).
    OwnProfile,
    /// Open the form to edit the signed-in member's own profile (M2-F).
    EditOwnProfile,
    /// Open the sysop's queue of invite requests (`members.approve`, M2-E).
    SysopQueue,
    /// Open My keys: list, add and remove keys and make a device-link code (`keys.manage`,
    /// FIX-M3-11).
    MyKeys,
    /// Open the staff screen that issues a one-time recovery code (`keys.recover`, FIX-M3-11).
    RecoveryCode,
    /// Open the theme preview under Sysop tools (UI-3): the styles, layouts and art as they
    /// would draw, before a default is set. It changes nothing, so it needs only what Sysop
    /// tools itself needs (`members.approve`).
    ThemePreview,
    /// Open the theme packs screen under Sysop tools (UI-7): export and import.
    ThemePacks,
    /// Open the menus and pages screen under Sysop tools (UI-6): the effective menu for a role,
    /// the pages, and the reload. Board look and feel, so it needs `themes.manage` like the
    /// theme packs (D131).
    MenuTools,
    /// Open the vouch screen: pending requests a member can speak for (`members.vouch`, M2-E).
    VouchQueue,
    /// Open the one-liners (`community.view`, M3-J).
    Oneliners,
    /// Open the graffiti wall (`community.view`, M3-J).
    GraffitiWall,
    /// Open last callers (`community.view`, M3-J).
    LastCallers,
    /// Open the polls (`community.view`, M3-J).
    Polls,
    /// Open the chat room list (`chat.join`, M3-F).
    Chat,
    /// Open the door list (`doors.run`, M4-D0, D98): the front end hands the session to the door
    /// framework and gets it back when the member leaves.
    Doors,
    /// End the session.
    LogOff,
}

impl Action {
    /// The permission needed to see and run the action. `None` only for [`Action::LogOff`]:
    /// every signed-in session may end itself, so there is nothing to grant or withhold.
    #[must_use]
    pub const fn permission(self) -> Option<Permission> {
        match self {
            Self::Forums => Some(Permission::ForumList),
            Self::Mail => Some(Permission::MailRead),
            Self::Files => Some(Permission::FilesList),
            Self::TerminalSettings => Some(Permission::AccountEditSettings),
            Self::InviteCreate | Self::InviteList => Some(Permission::InvitesCreate),
            Self::InviteTree => Some(Permission::InvitesViewTree),
            Self::MemberList | Self::OwnProfile => Some(Permission::MembersView),
            Self::EditOwnProfile => Some(Permission::AccountEditProfile),
            Self::SysopQueue | Self::ThemePreview => Some(Permission::MembersApprove),
            Self::ThemePacks | Self::MenuTools => Some(Permission::ThemesManage),
            Self::RecoveryCode => Some(Permission::KeysRecover),
            Self::MyKeys => Some(Permission::KeysManage),
            Self::VouchQueue => Some(Permission::MembersVouch),
            Self::Oneliners | Self::GraffitiWall | Self::LastCallers | Self::Polls => {
                Some(Permission::CommunityView)
            }
            Self::Chat => Some(Permission::ChatJoin),
            Self::Doors => Some(Permission::DoorsRun),
            Self::LogOff => None,
        }
    }
}

/// Where an item leads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Another menu, by id.
    Menu(&'static str),
    /// An action the front end carries out.
    Action(Action),
    /// An area that does not exist yet. Picking it shows the string-table text `key`, which
    /// must say honestly that it is not built.
    Placeholder(&'static str),
    /// A sysop page (UI-6): a bulletin or info screen the sysop wrote, by id. Only a menu file
    /// can add one. It grants nothing and leads nowhere else: picking it shows the page's text,
    /// and the item is visible to whoever can see the menu that holds it (plus `requires`).
    Page(&'static str),
}

/// One entry of a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    /// The key that picks it (ASCII letter or digit, case-insensitive).
    pub hotkey: char,
    /// String-table key of the label.
    pub label: &'static str,
    /// Where it leads.
    pub target: Target,
    /// For a menu or placeholder target, the permission needed to see it. An action target
    /// uses [`Action::permission`] instead and this stays `None`.
    pub requires: Option<Permission>,
    /// A second permission that must also be held: the gate of the menu this item came from, when
    /// a sysop's menu file moved it to another menu (UI-6). Moving an item can therefore only
    /// narrow who reaches it. Always `None` in the built-in tree.
    pub also: Option<Permission>,
}

impl Item {
    /// The permission that gates this item, if any.
    #[must_use]
    pub const fn permission(&self) -> Option<Permission> {
        match self.target {
            Target::Action(action) => action.permission(),
            Target::Menu(_) | Target::Placeholder(_) | Target::Page(_) => self.requires,
        }
    }

    /// Whether `access` holds the permission that gates this item. A menu item without a
    /// permission of its own (a sysop group, UI-6) is also gated by what it leads to: see
    /// [`MenuTree::item_visible`], which is what menus use.
    #[must_use]
    pub fn permitted(&self, access: &dyn PermissionCheck) -> bool {
        self.permission().is_none_or(|p| access.allows(p))
            && self.also.is_none_or(|p| access.allows(p))
    }
}

/// A menu: a title and its items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Menu {
    /// Unique id other menus target.
    pub id: &'static str,
    /// String-table key of the title.
    pub title: &'static str,
    /// The items, in display order.
    pub items: &'static [Item],
}

/// A whole tree of menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuTree {
    /// Id of the menu a session starts at.
    pub root: &'static str,
    /// Every menu.
    pub menus: &'static [Menu],
}

/// Why a tree is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MenuError {
    /// The root id names no menu.
    #[error("menu tree root `{0}` does not exist")]
    MissingRoot(&'static str),
    /// Two menus share an id.
    #[error("menu id `{0}` is used twice")]
    DuplicateMenu(&'static str),
    /// A menu has no items, so a session entering it could only leave by the universal keys.
    #[error("menu `{0}` has no items")]
    EmptyMenu(&'static str),
    /// Two items of one menu share a hotkey (compared ignoring case).
    #[error("menu `{menu}` uses hotkey `{hotkey}` twice")]
    DuplicateHotkey {
        /// The menu.
        menu: &'static str,
        /// The repeated hotkey.
        hotkey: char,
    },
    /// A hotkey is not an ASCII letter or digit, which also keeps it off the universal keys.
    #[error("menu `{menu}` has hotkey `{hotkey}`, which is not an ASCII letter or digit")]
    BadHotkey {
        /// The menu.
        menu: &'static str,
        /// The offending hotkey.
        hotkey: char,
    },
    /// An item targets a menu that does not exist.
    #[error("menu `{menu}` item `{hotkey}` leads to missing menu `{target}`")]
    DanglingTarget {
        /// The menu holding the item.
        menu: &'static str,
        /// The item's hotkey.
        hotkey: char,
        /// The id that does not exist.
        target: &'static str,
    },
    /// A menu cannot be reached from the root.
    #[error("menu `{0}` cannot be reached from the root")]
    Unreachable(&'static str),
    /// An action item names a permission of its own, which would be ignored.
    #[error("menu `{menu}` item `{hotkey}` is an action; its permission comes from the action")]
    ActionWithRequires {
        /// The menu holding the item.
        menu: &'static str,
        /// The item's hotkey.
        hotkey: char,
    },
}

impl MenuTree {
    /// The menu with `id`.
    #[must_use]
    pub fn menu(&self, id: &str) -> Option<&'static Menu> {
        self.menus.iter().find(|menu| menu.id == id)
    }

    /// Check the tree. Run at boot and in tests; a session never sees an invalid tree.
    pub fn validate(&self) -> Result<(), MenuError> {
        let mut seen: Vec<&'static str> = Vec::new();
        for menu in self.menus {
            if seen.contains(&menu.id) {
                return Err(MenuError::DuplicateMenu(menu.id));
            }
            seen.push(menu.id);
        }
        if self.menu(self.root).is_none() {
            return Err(MenuError::MissingRoot(self.root));
        }
        for menu in self.menus {
            self.validate_menu(menu)?;
        }
        self.check_reachable()
    }

    fn validate_menu(&self, menu: &Menu) -> Result<(), MenuError> {
        if menu.items.is_empty() {
            return Err(MenuError::EmptyMenu(menu.id));
        }
        let mut hotkeys: Vec<char> = Vec::new();
        for item in menu.items {
            if !item.hotkey.is_ascii_alphanumeric() {
                return Err(MenuError::BadHotkey {
                    menu: menu.id,
                    hotkey: item.hotkey,
                });
            }
            let folded = item.hotkey.to_ascii_lowercase();
            if hotkeys.contains(&folded) {
                return Err(MenuError::DuplicateHotkey {
                    menu: menu.id,
                    hotkey: item.hotkey,
                });
            }
            hotkeys.push(folded);
            match item.target {
                Target::Menu(target) if self.menu(target).is_none() => {
                    return Err(MenuError::DanglingTarget {
                        menu: menu.id,
                        hotkey: item.hotkey,
                        target,
                    });
                }
                Target::Action(_) if item.requires.is_some() => {
                    return Err(MenuError::ActionWithRequires {
                        menu: menu.id,
                        hotkey: item.hotkey,
                    });
                }
                Target::Menu(_) | Target::Action(_) | Target::Placeholder(_) | Target::Page(_) => {}
            }
        }
        Ok(())
    }

    fn check_reachable(&self) -> Result<(), MenuError> {
        let mut reached: Vec<&'static str> = vec![self.root];
        let mut next = 0;
        while let Some(&id) = reached.get(next) {
            next += 1;
            let Some(menu) = self.menu(id) else { continue };
            for item in menu.items {
                if let Target::Menu(target) = item.target
                    && !reached.contains(&target)
                {
                    reached.push(target);
                }
            }
        }
        match self.menus.iter().find(|menu| !reached.contains(&menu.id)) {
            Some(menu) => Err(MenuError::Unreachable(menu.id)),
            None => Ok(()),
        }
    }

    /// Whether a session with `access` sees (and may pick) `item`. The item's own permission
    /// must be held. A link to a submenu with no permission of its own (a group a sysop made,
    /// UI-6) is shown only when something inside it is visible too, so an empty group never
    /// appears and its name leaks nothing. This one function is the visible set and the
    /// activatable set (D54).
    #[must_use]
    pub fn item_visible(&self, item: &Item, access: &dyn PermissionCheck) -> bool {
        self.item_visible_at(item, access, 0)
    }

    fn item_visible_at(&self, item: &Item, access: &dyn PermissionCheck, depth: usize) -> bool {
        if !item.permitted(access) {
            return false;
        }
        match item.target {
            Target::Menu(id) if item.requires.is_none() => {
                depth < MAX_DEPTH
                    && self.menu(id).is_some_and(|menu| {
                        menu.items
                            .iter()
                            .any(|inner| self.item_visible_at(inner, access, depth + 1))
                    })
            }
            Target::Menu(_) | Target::Action(_) | Target::Placeholder(_) | Target::Page(_) => true,
        }
    }

    /// The items of `menu` that a session with `access` sees, in order.
    pub fn visible_items<'a>(
        &'a self,
        menu: &'a Menu,
        access: &'a dyn PermissionCheck,
    ) -> impl Iterator<Item = &'a Item> {
        menu.items
            .iter()
            .filter(move |item| self.item_visible(item, access))
    }

    /// Every string-table key the tree uses (titles, labels, placeholder texts), so the table
    /// can be checked against the tree at boot.
    pub fn text_keys(&self) -> impl Iterator<Item = &'static str> {
        self.menus.iter().flat_map(|menu| {
            std::iter::once(menu.title).chain(menu.items.iter().flat_map(|item| {
                let note = match item.target {
                    Target::Placeholder(key) => Some(key),
                    Target::Menu(_) | Target::Action(_) | Target::Page(_) => None,
                };
                std::iter::once(item.label).chain(note)
            }))
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod custom_tests;
