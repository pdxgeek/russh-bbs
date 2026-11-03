//! Explicit terminal cell widths for every non-ASCII character the registry uses.
//!
//! A generic `wcwidth` is not trusted: private-use characters are "ambiguous" and fonts
//! disagree. Policy: printable ASCII is 1 cell; plain-Unicode tier symbols are East Asian
//! Width *Neutral* or *Narrow* (narrow everywhere, including CJK locales) and are 1 cell; Nerd Font
//! glyphs are **reserved at 2 cells** and rendered with one trailing space, which is
//! correct whether the terminal draws the glyph in one cell or overflows into a second.

/// Cells for a character, as a closed enum so only 1 and 2 exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphWidth {
    /// One cell.
    One,
    /// Two cells (reserved for Nerd Font glyphs).
    Two,
}

impl GlyphWidth {
    /// The width as a number of cells.
    #[must_use]
    pub const fn cells(self) -> usize {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }
}

/// The width table. Every non-ASCII character in the registry must appear here.
pub(super) const TABLE: &[(char, GlyphWidth)] = &[
    // Nerd Font (Font Awesome range), reserved at two cells.
    ('\u{f086}', GlyphWidth::Two),
    ('\u{f084}', GlyphWidth::Two),
    ('\u{f111}', GlyphWidth::Two),
    ('\u{f09e}', GlyphWidth::Two),
    ('\u{f0e0}', GlyphWidth::Two),
    ('\u{f234}', GlyphWidth::Two),
    ('\u{f06e}', GlyphWidth::Two),
    ('\u{f023}', GlyphWidth::Two),
    ('\u{f0a3}', GlyphWidth::Two),
    ('\u{f071}', GlyphWidth::Two),
    ('\u{f09c}', GlyphWidth::Two),
    ('\u{f132}', GlyphWidth::Two),
    // Plain Unicode tier, East Asian Width Neutral.
    ('\u{2263}', GlyphWidth::One),
    ('\u{25c9}', GlyphWidth::One),
    ('\u{223f}', GlyphWidth::One),
    ('\u{2709}', GlyphWidth::One),
    ('\u{2731}', GlyphWidth::One),
    ('\u{2298}', GlyphWidth::One),
    ('\u{2299}', GlyphWidth::One),
    ('\u{27e6}', GlyphWidth::One),
    ('\u{27e7}', GlyphWidth::One),
    ('\u{229b}', GlyphWidth::One),
    ('\u{270e}', GlyphWidth::One),
    ('\u{26a0}', GlyphWidth::One),
    ('\u{2297}', GlyphWidth::One),
    ('\u{2296}', GlyphWidth::One),
];

/// The cell width of `c`, or `None` if the table has no entry (printable ASCII is always 1).
#[must_use]
pub fn char_width(c: char) -> Option<GlyphWidth> {
    if c.is_ascii() {
        return (' '..='~').contains(&c).then_some(GlyphWidth::One);
    }
    TABLE.iter().find(|(k, _)| *k == c).map(|(_, w)| *w)
}

/// Total cells of `s`, or `None` if any character has no entry.
pub(super) fn str_width(s: &str) -> Option<usize> {
    s.chars()
        .try_fold(0, |acc, c| char_width(c).map(|w| acc + w.cells()))
}
