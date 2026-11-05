//! The sysop's Menus and pages screen (UI-6): the effective menu for a chosen role, the pages,
//! and the reload.
//!
//! The files and the validation belong to the server (it owns the loader); this is the state the
//! member flow keeps so keys, the `themes.manage` re-check and the way back to Sysop tools work
//! like every other page. A key becomes a [`MenuToolsRequest`]; the server carries it out, audits
//! a reload and answers with [`MenuToolsScreen::listed`].

use super::OutlineLine;
use crate::sanitize::SingleLine;

/// Longest message line, in characters.
const MESSAGE_MAX: usize = 200;

/// What the server is asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuToolsRequest {
    /// Show the current state.
    List,
    /// Read `menus.toml` and the pages again. Audited when it changes what members see.
    Reload,
}

/// A role and the menu a member with it gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRow {
    /// The role's name.
    pub name: String,
    /// The effective tree for exactly that role's permissions.
    pub lines: Vec<OutlineLine>,
}

/// A page the board has loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRow {
    /// Its id (the file name without `.toml`).
    pub id: String,
    /// Its title, sanitized.
    pub title: String,
}

/// What the server found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuToolsListing {
    /// The board uses a `menus.toml` (otherwise the built-in menu).
    pub custom: bool,
    /// Why the last file was refused, when it was (no secrets; sanitized by the screen).
    pub problem: Option<String>,
    /// One row per role.
    pub roles: Vec<RoleRow>,
    /// The loaded pages.
    pub pages: Vec<PageRow>,
    /// The board was started without a data directory.
    pub unavailable: bool,
}

/// A key on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuToolsKey {
    /// Up: the previous page, or scroll up in a page.
    Up,
    /// Down: the next page, or scroll down.
    Down,
    /// Next role.
    Role,
    /// Open the page under the cursor as a member would see it.
    View,
    /// Read the files again.
    Reload,
    /// Back (out of a page, then out of the screen).
    Back,
}

/// What a key asks of the flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuToolsStep {
    /// Redraw.
    Stay,
    /// Ask the server.
    Ask(MenuToolsRequest),
    /// Leave the screen.
    Leave,
}

/// The screen's state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuToolsScreen {
    listing: Option<MenuToolsListing>,
    role: usize,
    row: usize,
    /// The page being viewed, by id.
    viewing: Option<String>,
    scroll: usize,
    max_scroll: usize,
    message: Option<String>,
}

impl MenuToolsScreen {
    /// A screen waiting for its first listing.
    #[must_use]
    pub fn open() -> Self {
        Self::default()
    }

    /// The listing, once the server has answered.
    #[must_use]
    pub const fn listing(&self) -> Option<&MenuToolsListing> {
        self.listing.as_ref()
    }

    /// The role whose menu is shown.
    #[must_use]
    pub const fn role(&self) -> usize {
        self.role
    }

    /// The page under the cursor.
    #[must_use]
    pub const fn row(&self) -> usize {
        self.row
    }

    /// The id of the page being viewed.
    #[must_use]
    pub fn viewing(&self) -> Option<&str> {
        self.viewing.as_deref()
    }

    /// How far a viewed page is scrolled.
    #[must_use]
    pub const fn scroll(&self) -> usize {
        self.scroll
    }

    /// The last outcome, as one sanitized line.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Tell the screen how far a viewed page can scroll, so Up never has to undo overshoot.
    pub fn set_extent(&mut self, max_scroll: usize) {
        self.max_scroll = max_scroll;
        self.scroll = self.scroll.min(max_scroll);
    }

    /// The server's answer: the listing, and a line saying what happened.
    pub fn listed(&mut self, listing: MenuToolsListing, message: Option<&str>) {
        self.role = self.role.min(listing.roles.len().saturating_sub(1));
        self.row = self.row.min(listing.pages.len().saturating_sub(1));
        if let Some(id) = &self.viewing
            && !listing.pages.iter().any(|page| &page.id == id)
        {
            self.viewing = None;
        }
        self.listing = Some(listing);
        self.message =
            message.map(|text| SingleLine::new(MESSAGE_MAX).sanitize(text).into_string());
    }

    /// Apply `key`.
    pub fn press(&mut self, key: MenuToolsKey) -> MenuToolsStep {
        if self.viewing.is_some() {
            match key {
                MenuToolsKey::Up => self.scroll = self.scroll.saturating_sub(1),
                MenuToolsKey::Down => self.scroll = (self.scroll + 1).min(self.max_scroll),
                MenuToolsKey::Back | MenuToolsKey::View => {
                    self.viewing = None;
                    self.scroll = 0;
                }
                MenuToolsKey::Role | MenuToolsKey::Reload => {}
            }
            return MenuToolsStep::Stay;
        }
        let Some(listing) = &self.listing else {
            return match key {
                MenuToolsKey::Back => MenuToolsStep::Leave,
                _ => MenuToolsStep::Stay,
            };
        };
        match key {
            MenuToolsKey::Back => return MenuToolsStep::Leave,
            MenuToolsKey::Up => self.row = self.row.saturating_sub(1),
            MenuToolsKey::Down => {
                self.row = (self.row + 1).min(listing.pages.len().saturating_sub(1));
            }
            MenuToolsKey::Role => {
                if !listing.roles.is_empty() {
                    self.role = (self.role + 1) % listing.roles.len();
                }
            }
            MenuToolsKey::View => {
                if let Some(page) = listing.pages.get(self.row) {
                    self.viewing = Some(page.id.clone());
                    self.scroll = 0;
                    self.max_scroll = 0;
                }
            }
            MenuToolsKey::Reload => return MenuToolsStep::Ask(MenuToolsRequest::Reload),
        }
        MenuToolsStep::Stay
    }
}

/// A key on a sysop page a member opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulletinKey {
    /// One line up.
    Up,
    /// One line down.
    Down,
    /// Back to the menu.
    Back,
}

/// A sysop page a member is reading: which one and how far down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bulletin {
    id: &'static str,
    scroll: usize,
    max_scroll: usize,
}

impl Bulletin {
    /// Open page `id` at the top.
    #[must_use]
    pub const fn open(id: &'static str) -> Self {
        Self {
            id,
            scroll: 0,
            max_scroll: 0,
        }
    }

    /// The page's id.
    #[must_use]
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// Lines scrolled off the top.
    #[must_use]
    pub const fn scroll(&self) -> usize {
        self.scroll
    }

    /// Set how far the page can scroll (the draw knows the text and the room).
    pub fn set_extent(&mut self, max_scroll: usize) {
        self.max_scroll = max_scroll;
        self.scroll = self.scroll.min(max_scroll);
    }

    /// Apply `key`; `true` when the member leaves the page.
    pub fn press(&mut self, key: BulletinKey) -> bool {
        match key {
            BulletinKey::Up => self.scroll = self.scroll.saturating_sub(1),
            BulletinKey::Down => self.scroll = (self.scroll + 1).min(self.max_scroll),
            BulletinKey::Back => return true,
        }
        false
    }
}
