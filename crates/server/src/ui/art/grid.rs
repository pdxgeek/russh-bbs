//! The cell grid art becomes: characters with an optional foreground and background from the
//! sixteen ANSI colours, and nothing else.
//!
//! **A cell cannot carry a style attribute.** There is no bold, reverse, blink or underline in
//! a cell: the parser folds bold into the bright half of the palette and reverse into a swap of
//! the two colours, so art can never produce the reserved style (bold plus reverse) that only a
//! security marker may wear. Every character in a cell has passed the shared sanitizer.

use russh_bbs_core::sanitize::SingleLine;

use super::cp437;

/// Widest art, in columns (a full screen at the largest size the plan names).
pub const MAX_COLS: usize = 132;
/// Tallest art, in rows.
pub const MAX_ROWS: usize = 50;

/// One cell: what to show in the Unicode tiers, what to show in the ASCII tier, and the colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtCell {
    /// The character in the Unicode tiers: one column wide, sanitized.
    pub ch: char,
    /// The character in the ASCII tier: printable ASCII.
    pub ascii: char,
    /// The foreground, a palette index 0 to 15, or `None` for the style's own text colour.
    pub fg: Option<u8>,
    /// The background, a palette index 0 to 15, or `None` for the screen's own.
    pub bg: Option<u8>,
}

impl ArtCell {
    /// An empty cell.
    pub const BLANK: Self = Self {
        ch: ' ',
        ascii: ' ',
        fg: None,
        bg: None,
    };
}

/// Why art was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArtError {
    /// More bytes than the size cap.
    #[error("{0} bytes is more than the {1}-byte cap")]
    TooManyBytes(usize, usize),
    /// Wider than [`MAX_COLS`].
    #[error("wider than the {MAX_COLS}-column cap")]
    TooWide,
    /// Taller than [`MAX_ROWS`].
    #[error("taller than the {MAX_ROWS}-row cap")]
    TooTall,
    /// Nothing visible.
    #[error("the art is empty")]
    Empty,
    /// A field of an art file is wrong.
    #[error("{0}")]
    Invalid(String),
}

/// A rectangle of [`ArtCell`]s.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Grid {
    width: usize,
    height: usize,
    cells: Vec<ArtCell>,
}

impl Grid {
    /// A blank grid. Callers have already checked the size against the caps.
    pub(super) fn blank(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![ArtCell::BLANK; width * height],
        }
    }

    /// Columns.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Rows.
    #[must_use]
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Whether there is nothing to draw.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// The cell at `(x, y)`, or a blank one outside the grid.
    #[must_use]
    pub fn get(&self, x: usize, y: usize) -> ArtCell {
        if x < self.width && y < self.height {
            self.cells[y * self.width + x]
        } else {
            ArtCell::BLANK
        }
    }

    pub(super) fn set(&mut self, x: usize, y: usize, cell: ArtCell) {
        if x < self.width && y < self.height {
            self.cells[y * self.width + x] = cell;
        }
    }

    /// A row as text, in the Unicode or the ASCII tier, with trailing blanks kept (the grid is
    /// a rectangle).
    #[must_use]
    pub fn row_text(&self, y: usize, ascii: bool) -> String {
        (0..self.width)
            .map(|x| {
                let cell = self.get(x, y);
                if ascii { cell.ascii } else { cell.ch }
            })
            .collect()
    }

    /// Every cell, row by row.
    pub fn cells(&self) -> impl Iterator<Item = ArtCell> + '_ {
        self.cells.iter().copied()
    }
}

/// A character that is safe in a cell: the shared sanitizer's verdict on it, and exactly one
/// column wide. A control, an invisible or a double-width character becomes a space; a
/// private-use codepoint or a reserved marker glyph becomes U+FFFD (the sanitizer's own rule).
#[must_use]
pub(super) fn safe_char(c: char) -> char {
    let cleaned = SingleLine::new(2).sanitize(&c.to_string());
    let mut chars = cleaned.as_str().chars();
    match (chars.next(), chars.next()) {
        (Some(only), None) if column_width(only) == 1 => only,
        _ => ' ',
    }
}

/// The columns `c` takes.
fn column_width(c: char) -> usize {
    ratatui::text::Span::raw(c.to_string()).width()
}

/// A cell for `c` (any character): the sanitized character, and its ASCII stand-in.
#[must_use]
pub(super) fn cell_for(c: char, fg: Option<u8>, bg: Option<u8>) -> ArtCell {
    let ch = safe_char(c);
    ArtCell {
        ch,
        ascii: cp437::ascii_of(ch),
        fg: fg.filter(|i| *i < 16),
        bg: bg.filter(|i| *i < 16),
    }
}
