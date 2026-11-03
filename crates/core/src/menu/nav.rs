//! Navigation state over a [`MenuTree`]: which menu is open, the help overlay, a notice.

use super::{Action, MAX_DEPTH, MenuTree, Target};
use crate::access::PermissionCheck;

/// A key as the menu sees it. Quit is not here: logging off from anywhere is the session's
/// rule, applied before a menu is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    /// A printable character: an item's hotkey, or [`HELP_KEY`](super::HELP_KEY).
    Char(char),
    /// Esc.
    Back,
    /// Move the cursor to the previous item (a layout with a cursor, such as the lightbar).
    Up,
    /// Move the cursor to the next item.
    Down,
    /// Pick the item under the cursor, as if its hotkey had been typed.
    Select,
}

/// What the front end must do after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOutcome {
    /// Redraw.
    Stay,
    /// Carry out the action, then redraw.
    Action(Action),
}

/// One item as a layout draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageItem {
    /// The key that picks it.
    pub hotkey: char,
    /// String-table key of the label.
    pub label: &'static str,
    /// Whether it leads to an area that does not exist yet.
    pub placeholder: bool,
}

/// What a layout needs to draw the open menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuPage {
    /// String-table key of the title.
    pub title: &'static str,
    /// How many menus deep the session is (0 at the root).
    pub depth: usize,
    /// The items this session sees.
    pub items: Vec<PageItem>,
    /// String-table key of a notice, such as a placeholder's "not built yet".
    pub notice: Option<&'static str>,
    /// Whether the help overlay is showing.
    pub help: bool,
    /// Index into [`MenuPage::items`] of the item under the cursor. Every layout may ignore it
    /// (the classic hotkey layout does); the lightbar highlights it. Always in range, or 0 for
    /// a menu with no visible items.
    pub cursor: usize,
}

/// Where a session is in a menu tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSession {
    tree: &'static MenuTree,
    stack: Vec<&'static str>,
    help: bool,
    notice: Option<&'static str>,
    /// Index of the item under the cursor among the visible items of the open menu. Reset to the
    /// first item whenever the open menu changes. Clamped when read, because the visible set
    /// can shrink between actions (a permission goes away).
    cursor: usize,
}

impl MenuSession {
    /// Start at the root of `tree`.
    #[must_use]
    pub fn new(tree: &'static MenuTree) -> Self {
        Self {
            tree,
            stack: vec![tree.root],
            help: false,
            notice: None,
            cursor: 0,
        }
    }

    /// Id of the open menu.
    #[must_use]
    pub fn current(&self) -> &'static str {
        self.stack.last().copied().unwrap_or(self.tree.root)
    }

    /// Whether the help overlay is showing.
    #[must_use]
    pub const fn help_open(&self) -> bool {
        self.help
    }

    /// Handle a key. Any key closes the help overlay; Esc then also stays put.
    pub fn press(&mut self, access: &dyn PermissionCheck, key: MenuKey) -> MenuOutcome {
        self.notice = None;
        if self.help {
            self.help = false;
            return MenuOutcome::Stay;
        }
        match key {
            MenuKey::Back => {
                if self.stack.len() > 1 {
                    self.stack.pop();
                    self.cursor = 0;
                }
                MenuOutcome::Stay
            }
            MenuKey::Up => {
                self.cursor = self.cursor_in(access).saturating_sub(1);
                MenuOutcome::Stay
            }
            MenuKey::Down => {
                let last = self.visible_count(access).saturating_sub(1);
                self.cursor = (self.cursor_in(access) + 1).min(last);
                MenuOutcome::Stay
            }
            MenuKey::Select => self.select(access),
            MenuKey::Char(c) if c == super::HELP_KEY => {
                self.help = true;
                MenuOutcome::Stay
            }
            MenuKey::Char(c) => self.pick(access, c),
        }
    }

    fn visible_count(&self, access: &dyn PermissionCheck) -> usize {
        self.tree
            .menu(self.current())
            .map_or(0, |menu| MenuTree::visible_items(menu, access).count())
    }

    /// The cursor, kept inside the visible items.
    fn cursor_in(&self, access: &dyn PermissionCheck) -> usize {
        self.cursor
            .min(self.visible_count(access).saturating_sub(1))
    }

    fn pick(&mut self, access: &dyn PermissionCheck, typed: char) -> MenuOutcome {
        let Some(menu) = self.tree.menu(self.current()) else {
            return MenuOutcome::Stay;
        };
        // Only visible items can be picked: hiding an item also makes it unreachable.
        let Some(index) = MenuTree::visible_items(menu, access)
            .position(|item| item.hotkey.eq_ignore_ascii_case(&typed))
        else {
            return MenuOutcome::Stay;
        };
        self.cursor = index;
        self.activate(access, index)
    }

    fn select(&mut self, access: &dyn PermissionCheck) -> MenuOutcome {
        let index = self.cursor_in(access);
        self.activate(access, index)
    }

    /// Carry out the `index`th visible item of the open menu.
    fn activate(&mut self, access: &dyn PermissionCheck, index: usize) -> MenuOutcome {
        let Some(menu) = self.tree.menu(self.current()) else {
            return MenuOutcome::Stay;
        };
        let Some(item) = MenuTree::visible_items(menu, access).nth(index) else {
            return MenuOutcome::Stay;
        };
        match item.target {
            Target::Menu(id) => {
                if self.stack.len() < MAX_DEPTH {
                    self.stack.push(id);
                    self.cursor = 0;
                }
                MenuOutcome::Stay
            }
            Target::Action(action) => MenuOutcome::Action(action),
            Target::Placeholder(key) => {
                self.notice = Some(key);
                MenuOutcome::Stay
            }
        }
    }

    #[cfg(test)]
    pub(super) const fn notice_for_tests(&self) -> Option<&'static str> {
        self.notice
    }

    /// The open menu as a layout draws it, for a session with `access`.
    #[must_use]
    pub fn page(&self, access: &dyn PermissionCheck) -> MenuPage {
        let menu = self.tree.menu(self.current());
        MenuPage {
            title: menu.map_or("", |m| m.title),
            depth: self.stack.len() - 1,
            items: menu
                .map(|m| {
                    MenuTree::visible_items(m, access)
                        .map(|item| PageItem {
                            hotkey: item.hotkey,
                            label: item.label,
                            placeholder: matches!(item.target, Target::Placeholder(_)),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            notice: self.notice,
            help: self.help,
            cursor: self.cursor_in(access),
        }
    }
}
