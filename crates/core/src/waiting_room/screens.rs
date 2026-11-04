//! Which screens exist in each registration mode, and how to reach them.
//!
//! This is the rule (D2, D23); the SSH front end only draws what it is told. The sets are:
//!
//! | Mode | Menu on the home page |
//! |---|---|
//! | `invite` | rules, request an invite, redeem an invite |
//! | `apply` | rules, apply for membership |
//! | `open` | rules, create an account |
//!
//! The banner is the home page's header in every mode.

use super::strings;
use crate::store::RegistrationMode;

/// A screen in the waiting room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    /// Banner and menu.
    Home,
    /// The sysop's rules.
    Rules,
    /// Ask for an invite (`invite` mode).
    RequestInvite,
    /// Enter an invite code (`invite` mode).
    RedeemInvite,
    /// Submit a membership application (`apply` mode).
    Apply,
    /// Create an account at once (`open` mode).
    CreateAccount,
    /// "I already have an account": a handle and password that bind this key to it (a room
    /// built [`WaitingRoom::with_key_link`](super::WaitingRoom::with_key_link), D94 (8)).
    LinkKey,
    /// "Link this device": a code made on a signed-in session binds this connection's key
    /// (FIX-M3-11, D97).
    LinkDevice,
    /// "Recovery code": a code from staff binds this key and sets a new password (FIX-M3-11).
    Recover,
}

impl Page {
    /// The page's heading.
    #[must_use]
    pub const fn heading(self) -> &'static str {
        match self {
            Self::Home => strings::HOME_HEADING,
            Self::Rules => strings::RULES_HEADING,
            Self::RequestInvite => strings::REQUEST_INVITE_HEADING,
            Self::RedeemInvite => strings::REDEEM_INVITE_HEADING,
            Self::Apply => strings::APPLY_HEADING,
            Self::CreateAccount => strings::CREATE_ACCOUNT_HEADING,
            Self::LinkKey => strings::LINK_KEY_HEADING,
            Self::LinkDevice => crate::keys::strings::LINK_DEVICE_HEADING,
            Self::Recover => crate::keys::strings::RECOVER_HEADING,
        }
    }

    /// Whether the page has a form.
    #[must_use]
    pub const fn is_form(self) -> bool {
        !matches!(self, Self::Home | Self::Rules)
    }
}

/// One line of the home menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuEntry {
    /// The key that selects it (lowercase; matching ignores case).
    pub key: char,
    /// What the user reads.
    pub label: &'static str,
    /// Where it leads.
    pub page: Page,
}

const RULES: MenuEntry = MenuEntry {
    key: 'r',
    label: strings::MENU_RULES,
    page: Page::Rules,
};

const INVITE_MENU: &[MenuEntry] = &[
    RULES,
    MenuEntry {
        key: 'i',
        label: strings::MENU_REQUEST_INVITE,
        page: Page::RequestInvite,
    },
    MenuEntry {
        key: 'c',
        label: strings::MENU_REDEEM_INVITE,
        page: Page::RedeemInvite,
    },
];

const APPLY_MENU: &[MenuEntry] = &[
    RULES,
    MenuEntry {
        key: 'a',
        label: strings::MENU_APPLY,
        page: Page::Apply,
    },
];

const OPEN_MENU: &[MenuEntry] = &[
    RULES,
    MenuEntry {
        key: 'n',
        label: strings::MENU_CREATE_ACCOUNT,
        page: Page::CreateAccount,
    },
];

/// The home menu for a registration mode.
#[must_use]
pub const fn menu(mode: RegistrationMode) -> &'static [MenuEntry] {
    match mode {
        RegistrationMode::Invite => INVITE_MENU,
        RegistrationMode::Apply => APPLY_MENU,
        RegistrationMode::Open => OPEN_MENU,
    }
}

/// The key of the "I already have an account" entry, on rooms that offer it.
pub const LINK_KEY: char = 'e';

/// The key of the "Link this device" entry, on rooms that offer it (a visitor with a key).
pub const LINK_DEVICE_KEY: char = 'd';

/// The key of the "Recovery code" entry, on rooms that offer it (a visitor with a key).
pub const RECOVER_KEY: char = 'y';

/// The key that disconnects from the home page.
pub const QUIT_KEY: char = 'q';

/// The entry selected by `key` in `mode`, if any.
#[must_use]
pub fn entry_for(mode: RegistrationMode, key: char) -> Option<&'static MenuEntry> {
    let key = key.to_ascii_lowercase();
    menu(mode).iter().find(|entry| entry.key == key)
}

/// Every page a mode can reach, home included.
#[must_use]
pub fn pages(mode: RegistrationMode) -> Vec<Page> {
    let mut all = vec![Page::Home];
    all.extend(menu(mode).iter().map(|entry| entry.page));
    all
}
