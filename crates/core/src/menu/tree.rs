//! The board's menus, defined once as data. Layouts render this; they do not redefine it.
//!
//! Placeholders stand for areas that do not exist yet and say so when picked (their texts are
//! in the string table). They appear only for sessions that would hold the area's permission,
//! so a placeholder never promises something the session could not later open.

use super::{Action, Item, Menu, MenuTree, Target};
use crate::access::Permission;

const MAIN: &str = "main";
const SYSOP: &str = "sysop";
const INVITES: &str = "invites";

const fn item(hotkey: char, label: &'static str, target: Target) -> Item {
    Item {
        hotkey,
        label,
        target,
        requires: None,
    }
}

const fn gated(hotkey: char, label: &'static str, target: Target, requires: Permission) -> Item {
    Item {
        hotkey,
        label,
        target,
        requires: Some(requires),
    }
}

static MAIN_ITEMS: [Item; 8] = [
    gated(
        'f',
        "menu.main.item.forums",
        Target::Placeholder("placeholder.forums"),
        Permission::ForumList,
    ),
    gated(
        'm',
        "menu.main.item.mail",
        Target::Placeholder("placeholder.mail"),
        Permission::MailRead,
    ),
    gated(
        'a',
        "menu.main.item.files",
        Target::Placeholder("placeholder.files"),
        Permission::FilesList,
    ),
    gated(
        'u',
        "menu.main.item.members",
        Target::Placeholder("placeholder.members"),
        Permission::MembersView,
    ),
    gated(
        'i',
        "menu.main.item.invites",
        Target::Menu(INVITES),
        Permission::InvitesCreate,
    ),
    item(
        's',
        "menu.main.item.settings",
        Target::Action(Action::TerminalSettings),
    ),
    gated(
        'o',
        "menu.main.item.sysop",
        Target::Menu(SYSOP),
        Permission::MembersApprove,
    ),
    item('l', "menu.main.item.logoff", Target::Action(Action::LogOff)),
];

static SYSOP_ITEMS: [Item; 1] = [gated(
    'q',
    "menu.sysop.item.queue",
    Target::Placeholder("placeholder.queue"),
    Permission::MembersApprove,
)];

static INVITES_ITEMS: [Item; 3] = [
    item(
        'c',
        "menu.invites.item.create",
        Target::Action(Action::InviteCreate),
    ),
    item(
        'y',
        "menu.invites.item.list",
        Target::Action(Action::InviteList),
    ),
    item(
        't',
        "menu.invites.item.tree",
        Target::Action(Action::InviteTree),
    ),
];

static MENUS: [Menu; 3] = [
    Menu {
        id: MAIN,
        title: "menu.main.title",
        items: &MAIN_ITEMS,
    },
    Menu {
        id: SYSOP,
        title: "menu.sysop.title",
        items: &SYSOP_ITEMS,
    },
    Menu {
        id: INVITES,
        title: "menu.invites.title",
        items: &INVITES_ITEMS,
    },
];

static MAIN_TREE: MenuTree = MenuTree {
    root: MAIN,
    menus: &MENUS,
};

/// The main menu tree every signed-in session starts from.
#[must_use]
pub fn main_tree() -> &'static MenuTree {
    &MAIN_TREE
}
