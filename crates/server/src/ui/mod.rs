//! The screen framework's drawing side (M2-A): the **classic hotkey layout** over the menu
//! tree in `russh_bbs_core::menu`, the reserved **status row** for security markers, and the
//! themeable **string table**.
//!
//! The four layers of `docs/ui.md` map to code like this: the menu tree is core data, the layout
//! is [`classic`], the strings (and, later, themes) are [`strings`], and the icon tier comes
//! from the member's settings. Adding a layout later means a new module beside [`classic`]; the
//! tree does not change.
//!
//! [`Ui`] is built once at boot. It checks that the shipped menu tree is valid and that the
//! string table holds exactly the keys the tree and the layout use, so a bad table stops the
//! server with a message naming the key instead of drawing blanks.

pub(crate) mod chrome;
pub(crate) mod classic;
pub(crate) mod compact;
pub mod editor;
pub(crate) mod keys;
pub(crate) mod layout;
pub(crate) mod lightbar;
pub(crate) mod pager;
pub mod strings;
pub mod theme;

use std::path::Path;

use russh_bbs_core::menu::{MenuError, main_tree};
use russh_bbs_core::terminal_settings::ThemeName;

pub use strings::{StringError, StringTable};
pub use theme::{Palette, Theme, ThemeError, ThemeSet};

/// Why the UI could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UiError {
    /// The menu tree is invalid.
    #[error("menu tree: {0}")]
    Menu(#[from] MenuError),
    /// The string table is invalid.
    #[error("string table: {0}")]
    Strings(#[from] StringError),
    /// A shipped theme is invalid (a build bug; sysop themes never produce this, they are
    /// skipped with a warning).
    #[error("shipped theme: {0}")]
    Theme(#[from] ThemeError),
}

/// What every screen on the framework draws with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ui {
    themes: ThemeSet,
}

impl Ui {
    /// The shipped table, tree and themes. Fails only if the build itself is inconsistent,
    /// which a unit test also catches.
    pub fn standard() -> Result<Self, UiError> {
        Self::from_table(strings::DEFAULT_TOML)
    }

    /// Like [`Ui::standard`], with the sysop's themes from `dir` added. A bad theme file never
    /// fails this: it is skipped with a logged warning (see [`theme`]).
    pub fn standard_with_themes(dir: &Path) -> Result<Self, UiError> {
        let mut ui = Self::standard()?;
        let base = ui.strings().clone();
        ui.themes.add_dir(dir, &base);
        Ok(ui)
    }

    /// Build from a table in TOML (the loader a theme or translation uses), validated against
    /// the menu tree and the layout's own keys.
    pub fn from_table(toml: &str) -> Result<Self, UiError> {
        let tree = main_tree();
        tree.validate()?;
        let required: Vec<&str> = keys::FRAMEWORK
            .iter()
            .copied()
            .chain(tree.text_keys())
            .chain(russh_bbs_core::profiles::TEXT_KEYS.iter().copied())
            .chain(russh_bbs_core::queue::keys::TEXT_KEYS.iter().copied())
            .chain(
                russh_bbs_core::terminal_settings::strings::TEXT_KEYS
                    .iter()
                    .copied(),
            )
            .collect();
        let strings = StringTable::parse(toml, &required)?;
        Ok(Self {
            themes: ThemeSet::shipped(&strings)?,
        })
    }

    /// The default theme's validated string table.
    #[must_use]
    pub fn strings(&self) -> &StringTable {
        self.themes.default_theme().strings()
    }

    /// The theme called `name`; the default if none is loaded under that name.
    #[must_use]
    pub fn theme(&self, name: ThemeName) -> &Theme {
        self.themes.get(name)
    }

    /// The names of the loaded themes, the default first.
    #[must_use]
    pub fn theme_names(&self) -> Vec<ThemeName> {
        self.themes.names()
    }
}

#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod tests;
