//! A piece of art and the slot it is made for.

use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::terminal_settings::ThemeName;

use super::grid::{ArtError, Grid};

/// A fixed place on a screen where art may go. Art is drawn only inside its slot's rectangle,
/// which the screen takes out of its body before anything else is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    /// The signed-in login screen's banner.
    Login,
    /// The header above the main menu.
    Menu,
    /// The header of a top-level area (forums, mail, ...). The slot and the format exist; the
    /// area screens draw it when they move onto the new frame (UI-4).
    Area,
    /// The logoff screen.
    Logoff,
}

impl Slot {
    /// Every slot.
    pub const ALL: [Self; 4] = [Self::Login, Self::Menu, Self::Area, Self::Logoff];

    /// The name used in art files.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Menu => "menu",
            Self::Area => "area",
            Self::Logoff => "logoff",
        }
    }

    /// The slot called `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|slot| slot.name() == name)
    }

    /// The largest art for this slot, as `(columns, rows)`: a header is at most 80x25 and a
    /// full-screen piece at most 132x50 (docs/ui-refresh.md).
    #[must_use]
    pub const fn cap(self) -> (usize, usize) {
        match self {
            Self::Login | Self::Logoff => (132, 50),
            Self::Menu | Self::Area => (80, 25),
        }
    }
}

/// One drawable piece: a grid for a slot, perhaps made for one style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtPiece {
    pub(super) slot: Slot,
    /// The style it was made for, or `None` for any style.
    pub(super) style: Option<ThemeName>,
    /// The art set's name (a file name or a bundled set).
    pub(super) set: String,
    /// Who made it.
    pub(super) credit: String,
    /// Text to show instead when the piece does not fit, with `{board}` for the board's name.
    pub(super) fallback: Option<String>,
    pub(super) grid: Grid,
    /// The file gave an ASCII version of its own.
    pub(super) ascii_given: bool,
    /// Whether it came from the sysop's `art/` directory (these win over bundled art).
    pub(super) sysop: bool,
}

impl ArtPiece {
    /// The slot it is for.
    #[must_use]
    pub const fn slot(&self) -> Slot {
        self.slot
    }

    /// The style it was made for, if one.
    #[must_use]
    pub const fn style(&self) -> Option<ThemeName> {
        self.style
    }

    /// The set it belongs to.
    #[must_use]
    pub fn set(&self) -> &str {
        &self.set
    }

    /// The credit line.
    #[must_use]
    pub fn credit(&self) -> &str {
        &self.credit
    }

    /// The fallback text, if the piece names one.
    #[must_use]
    pub fn fallback(&self) -> Option<&str> {
        self.fallback.as_deref()
    }

    /// The picture.
    #[must_use]
    pub const fn grid(&self) -> &Grid {
        &self.grid
    }

    /// Whether the sysop supplied it.
    #[must_use]
    pub const fn is_sysop(&self) -> bool {
        self.sysop
    }

    /// Whether the ASCII tier can show it faithfully: the file gave its own ASCII version, or
    /// every character has a plain stand-in. Otherwise the ASCII tier draws large letters.
    #[must_use]
    pub fn ascii_safe(&self) -> bool {
        self.ascii_given || !self.grid.cells().any(|c| c.ascii == '?' && c.ch != '?')
    }

    /// Whether it fits in `width` columns and `height` rows.
    #[must_use]
    pub fn fits(&self, width: u16, height: u16) -> bool {
        self.grid.width() <= usize::from(width) && self.grid.height() <= usize::from(height)
    }

    /// Check `grid` against `slot`'s caps.
    pub(super) fn check(slot: Slot, grid: &Grid) -> Result<(), ArtError> {
        let (cols, rows) = slot.cap();
        if grid.is_empty() {
            return Err(ArtError::Empty);
        }
        if grid.width() > cols {
            return Err(ArtError::Invalid(format!(
                "{} columns is wider than the {cols}-column cap for the {} slot",
                grid.width(),
                slot.name()
            )));
        }
        if grid.height() > rows {
            return Err(ArtError::Invalid(format!(
                "{} rows is taller than the {rows}-row cap for the {} slot",
                grid.height(),
                slot.name()
            )));
        }
        Ok(())
    }
}

/// Longest credit or fallback text, in characters.
pub(super) const TEXT_MAX: usize = 80;

/// Check a credit or fallback line: bounded, nothing the sanitizer would change, and no claim of
/// privacy (the same rule as every sysop string).
pub(super) fn check_text(what: &str, raw: &str) -> Result<String, ArtError> {
    let value = raw.trim();
    if value.chars().count() > TEXT_MAX {
        return Err(ArtError::Invalid(format!(
            "{what} is longer than {TEXT_MAX} characters"
        )));
    }
    if SingleLine::new(TEXT_MAX + 1).sanitize(value).as_str() != value {
        return Err(ArtError::Invalid(format!(
            "{what} contains control, invisible or reserved characters"
        )));
    }
    if !value.is_empty() {
        crate::ui::strings::check_board_text(what, value)
            .map_err(|error| ArtError::Invalid(error.to_string()))?;
    }
    Ok(value.to_owned())
}
