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
const MEMBERS: &str = "members";
const COMMUNITY: &str = "community";

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

static MAIN_ITEMS: [Item; 10] = [
    item('f', "menu.main.item.forums", Target::Action(Action::Forums)),
    item('m', "menu.main.item.mail", Target::Action(Action::Mail)),
    gated(
        'a',
        "menu.main.item.files",
        Target::Placeholder("placeholder.files"),
        Permission::FilesList,
    ),
    item('c', "menu.main.item.chat", Target::Action(Action::Chat)),
    gated(
        'u',
        "menu.main.item.members",
        Target::Menu(MEMBERS),
        Permission::MembersView,
    ),
    gated(
        'y',
        "menu.main.item.community",
        Target::Menu(COMMUNITY),
        Permission::CommunityView,
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

static SYSOP_ITEMS: [Item; 2] = [
    item(
        'q',
        "menu.sysop.item.queue",
        Target::Action(Action::SysopQueue),
    ),
    item(
        'r',
        "menu.sysop.item.recovery",
        Target::Action(Action::RecoveryCode),
    ),
];

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

static MEMBERS_ITEMS: [Item; 5] = [
    item(
        'l',
        "menu.members.item.list",
        Target::Action(Action::MemberList),
    ),
    item(
        'p',
        "menu.members.item.profile",
        Target::Action(Action::OwnProfile),
    ),
    item(
        'e',
        "menu.members.item.edit",
        Target::Action(Action::EditOwnProfile),
    ),
    item(
        'v',
        "menu.members.item.vouch",
        Target::Action(Action::VouchQueue),
    ),
    item(
        'k',
        "menu.members.item.keys",
        Target::Action(Action::MyKeys),
    ),
];

static COMMUNITY_ITEMS: [Item; 4] = [
    item(
        'o',
        "menu.community.item.oneliners",
        Target::Action(Action::Oneliners),
    ),
    item(
        'g',
        "menu.community.item.graffiti",
        Target::Action(Action::GraffitiWall),
    ),
    item(
        'l',
        "menu.community.item.callers",
        Target::Action(Action::LastCallers),
    ),
    item(
        'p',
        "menu.community.item.polls",
        Target::Action(Action::Polls),
    ),
];

static MENUS: [Menu; 5] = [
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
    Menu {
        id: MEMBERS,
        title: "menu.members.title",
        items: &MEMBERS_ITEMS,
    },
    Menu {
        id: COMMUNITY,
        title: "menu.community.title",
        items: &COMMUNITY_ITEMS,
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
