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
pub(crate) mod keys;
// No screen has text long enough to page yet; the first long-text screen (the message reader,
// M3) uses this. The expectation fails the build when that happens, as a reminder to drop it.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no long-text screen exists yet (D65)")
)]
pub(crate) mod pager;
pub mod strings;

use russh_bbs_core::menu::{MenuError, main_tree};

pub use strings::{StringError, StringTable};

/// Why the UI could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UiError {
    /// The menu tree is invalid.
    #[error("menu tree: {0}")]
    Menu(#[from] MenuError),
    /// The string table is invalid.
    #[error("string table: {0}")]
    Strings(#[from] StringError),
}

/// What every screen on the framework draws with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ui {
    strings: StringTable,
}

impl Ui {
    /// The shipped table and tree. Fails only if the build itself is inconsistent, which a
    /// unit test also catches.
    pub fn standard() -> Result<Self, UiError> {
        Self::from_table(strings::DEFAULT_TOML)
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
            .collect();
        Ok(Self {
            strings: StringTable::parse(toml, &required)?,
        })
    }

    /// The validated string table.
    #[must_use]
    pub const fn strings(&self) -> &StringTable {
        &self.strings
    }
}

#[cfg(test)]
mod tests;
