//! Sysop menus and pages (UI-6, D54).
//!
//! **The files.** In the data directory (beside the host key, like `themes/` and `art/`):
//!
//! * `menus.toml`: reorder, rename, re-key, hide and group the built-in menu items, and add
//!   links to pages ([`file`]). It cannot add an action, a target or a permission.
//! * `pages/<id>.toml`: a bulletin or info screen, optional art plus sanitized text ([`page`]).
//!
//! **The rule.** The menu tree stays core data: this module only parses the file into a
//! `core::menu::Spec`, and `core::menu::build` checks it, builds the tree and runs
//! `MenuTree::validate` on it. Who sees what is still `MenuTree::item_visible`, that is
//! `core::access`. A hidden item is not in the tree, so its hotkey does nothing.
//!
//! **Bad files.** Anything wrong anywhere (the menu file, a page it links, the text, the tree)
//! refuses the whole load with one logged reason and the built-in menu stays. The board boots.
//!
//! **When it loads.** At start, and when a sysop with `themes.manage` chooses Reload on the
//! Menus and pages screen ([`service`]). A reload swaps one `Arc<MenuSetup>`; sessions already
//! signed in keep the menu they started with (their tree is `'static` data), and new sessions
//! get the new one. A page a running session opened that the new load removed shows a notice.

pub(crate) mod file;
pub(crate) mod page;
pub(crate) mod render;
pub(crate) mod service;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use russh_bbs_core::menu::{self, MenuTree, Spec, main_tree};
use russh_bbs_core::sanitize::SingleLine;
use tracing::warn;

pub use page::{Block, Page};

use crate::ui::strings::{StringTable, check_board_text};

/// What the board's menus and pages are right now.
#[derive(Debug, Clone)]
pub struct MenuSetup {
    tree: &'static MenuTree,
    pages: BTreeMap<String, Arc<Page>>,
    custom: bool,
    problem: Option<String>,
}

impl PartialEq for MenuSetup {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.tree, other.tree)
            && self.custom == other.custom
            && self.problem == other.problem
            && self.pages.len() == other.pages.len()
            && self
                .pages
                .iter()
                .zip(&other.pages)
                .all(|((a, x), (b, y))| a == b && x == y)
    }
}

impl Eq for MenuSetup {}

impl MenuSetup {
    /// The built-in menu, no pages.
    #[must_use]
    pub fn built_in() -> Self {
        Self {
            tree: main_tree(),
            pages: BTreeMap::new(),
            custom: false,
            problem: None,
        }
    }

    fn refused(reason: &str) -> Self {
        let reason = SingleLine::new(200).sanitize(reason).into_string();
        warn!(%reason, "menus.toml refused; the built-in menu stays");
        Self {
            problem: Some(reason),
            ..Self::built_in()
        }
    }

    /// The tree new sessions start from.
    #[must_use]
    pub const fn tree(&self) -> &'static MenuTree {
        self.tree
    }

    /// A page by id.
    #[must_use]
    pub fn page(&self, id: &str) -> Option<&Page> {
        self.pages.get(id).map(Arc::as_ref)
    }

    /// The loaded pages, by id.
    pub fn pages(&self) -> impl Iterator<Item = &Page> {
        self.pages.values().map(Arc::as_ref)
    }

    /// Whether a `menus.toml` is in force.
    #[must_use]
    pub const fn custom(&self) -> bool {
        self.custom
    }

    /// Why the last `menus.toml` was refused, if it was.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

/// Where the files are and what is loaded. Shared by every session through [`Ui`](crate::ui::Ui).
#[derive(Clone)]
pub(crate) struct MenusHandle {
    dir: Option<PathBuf>,
    current: Arc<RwLock<Arc<MenuSetup>>>,
}

impl MenusHandle {
    pub(crate) fn built_in() -> Self {
        Self {
            dir: None,
            current: Arc::new(RwLock::new(Arc::new(MenuSetup::built_in()))),
        }
    }

    pub(crate) fn with_dir(dir: &Path, strings: &StringTable) -> Self {
        let setup = load(dir, strings);
        Self {
            dir: Some(dir.to_path_buf()),
            current: Arc::new(RwLock::new(Arc::new(setup))),
        }
    }

    pub(crate) fn get(&self) -> Arc<MenuSetup> {
        Arc::clone(&self.current.read().unwrap_or_else(PoisonError::into_inner))
    }

    pub(crate) fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// Load again and make the result current. Returns it, and whether it differs from before.
    pub(crate) fn reload(&self, strings: &StringTable) -> Option<(Arc<MenuSetup>, bool)> {
        let dir = self.dir.as_deref()?;
        let fresh = Arc::new(load(dir, strings));
        let mut slot = self.current.write().unwrap_or_else(PoisonError::into_inner);
        let changed = **slot != *fresh;
        *slot = Arc::clone(&fresh);
        Some((fresh, changed))
    }
}

impl PartialEq for MenusHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.current, &other.current)
    }
}

impl Eq for MenusHandle {}

impl std::fmt::Debug for MenusHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenusHandle")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

/// Read `menus.toml` and the pages it links from `dir`. Never fails: a missing file is the
/// built-in menu, and a bad one is the built-in menu with the reason recorded.
#[must_use]
pub fn load(dir: &Path, strings: &StringTable) -> MenuSetup {
    let path = dir.join("menus.toml");
    let text = match file::read(&path) {
        Ok(Some(text)) => text,
        Ok(None) => return MenuSetup::built_in(),
        Err(error) => return MenuSetup::refused(&format!("menus.toml: {error}")),
    };
    let spec = match file::parse(&text) {
        Ok(spec) => spec,
        Err(error) => return MenuSetup::refused(&format!("menus.toml: {error}")),
    };
    match assemble(&spec, &dir.join("pages"), strings) {
        Ok(setup) => setup,
        Err(reason) => MenuSetup::refused(&reason),
    }
}

/// The pages a spec links, loaded, then the tree built over them.
fn assemble(spec: &Spec, pages_dir: &Path, strings: &StringTable) -> Result<MenuSetup, String> {
    let mut pages = BTreeMap::new();
    for id in file::linked_pages(spec) {
        let page = page::load(pages_dir, &id, strings).map_err(|e| format!("page {id}: {e}"))?;
        pages.insert(id, Arc::new(page));
    }
    check_spec_text(spec, strings)?;
    let title_of = |id: &str| pages.get(id).map(|page| page.title().to_owned());
    let built = menu::build(spec, &title_of).map_err(|e| e.to_string())?;
    Ok(MenuSetup {
        tree: built.tree,
        pages,
        custom: true,
        problem: None,
    })
}

/// The text rules the string table applies to every themed string: no privacy claim, and not a
/// string-table key (an unknown key draws as itself, so a label equal to a key would draw that
/// key's text).
fn check_spec_text(spec: &Spec, strings: &StringTable) -> Result<(), String> {
    let texts = spec.menus.iter().flat_map(|menu| {
        menu.title
            .iter()
            .chain(menu.items.iter().filter_map(|item| item.label.as_ref()))
    });
    for text in texts {
        let text = text.trim();
        if strings.keys().any(|key| key == text) {
            return Err("a label or title is the name of a string-table key".to_owned());
        }
        check_board_text("a label or title", text).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
