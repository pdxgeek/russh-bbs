//! Choosing what fills a slot and drawing it.
//!
//! **Order.** A piece that fits; else the best-matching piece's own fallback text; else the
//! board's name in large letters (when the slot has the room for them); else nothing, and the
//! header bar's board name stands alone. In the ASCII tier a piece with no faithful ASCII form is
//! skipped for large letters.
//!
//! **Where art may go.** Only inside the rectangle a screen hands over, and never on the last row
//! of the frame, where the status bar and the security-marker cell live: [`draw`] clips to the
//! frame minus that row whatever it is given, so a piece cannot cover the marker cell even if a
//! caller passes a bad rectangle.
//!
//! **How it is styled.** A cell gets a foreground and background colour and nothing else: no
//! modifier is ever set, so art cannot produce the reserved style (bold plus reverse) and
//! nothing it draws can be mistaken for a marker. Colours go through the member's colour depth;
//! on a mono terminal only the characters remain.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::terminal_settings::ThemeName;

use super::font;
use super::grid::{ArtCell, Grid};
use super::piece::Slot;
use super::set::ArtSet;
use crate::settings::Look;
use crate::ui::depth::Tint;
use crate::ui::theme::skin::{blend, gradient_depth};

/// What a screen needs to ask for art: the pieces, the style, the board's name and whether art
/// is on at all (`board.art`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ArtCtx<'a> {
    pub(crate) set: &'a ArtSet,
    pub(crate) style: ThemeName,
    pub(crate) board: &'a str,
    pub(crate) enabled: bool,
}

/// What fills a slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Filling<'a> {
    /// A piece of art.
    Piece(&'a super::piece::ArtPiece),
    /// The board's name in large letters.
    Letters(Grid),
    /// One line of plain text.
    Text(String),
}

impl Filling<'_> {
    /// Rows used.
    pub(crate) fn height(&self) -> usize {
        match self {
            Self::Piece(piece) => piece.grid().height(),
            Self::Letters(grid) => grid.height(),
            Self::Text(_) => 1,
        }
    }
}

/// Spare rows large letters want above what they replace, so they are not jammed against it.
const LETTER_SPACE: usize = 2;

impl<'a> ArtCtx<'a> {
    /// The grid of the style's login banner, for the animated intro to reveal: the best login
    /// piece (the same order the static screen uses), skipping one with no faithful ASCII form
    /// when `ascii`. `None` when art is off or the board has no login piece. Size is not
    /// checked: the intro clips and centres.
    pub(crate) fn login_banner(&self, ascii: bool) -> Option<&'a Grid> {
        if !self.enabled {
            return None;
        }
        self.set
            .candidates(Slot::Login, self.style)
            .into_iter()
            .find(|p| !ascii || p.ascii_safe())
            .map(super::piece::ArtPiece::grid)
    }

    /// What fills `slot` in `width` x `height` cells at `tier`, or `None` if nothing does.
    pub(crate) fn fill(
        &self,
        slot: Slot,
        tier: Tier,
        width: u16,
        height: u16,
    ) -> Option<Filling<'a>> {
        let mut candidates = if self.enabled {
            self.set.candidates(slot, self.style)
        } else {
            Vec::new()
        };
        // A board with no logoff art says goodbye under the login banner.
        if slot == Slot::Logoff && candidates.is_empty() && self.enabled {
            candidates = self.set.candidates(Slot::Login, self.style);
        }
        let ascii = tier == Tier::Ascii;
        if let Some(piece) = candidates
            .iter()
            .find(|p| p.fits(width, height) && (!ascii || p.ascii_safe()))
        {
            return Some(Filling::Piece(piece));
        }
        // The fallback text of the best-matching piece, if it names one.
        if let Some(text) = candidates.iter().find_map(|p| p.fallback()) {
            let text = text.replace("{board}", self.board);
            if text.chars().count() <= usize::from(width) && height >= 1 {
                return Some(Filling::Text(text));
            }
        }
        let wide = font::width_of(self.board);
        let room = usize::from(height) >= font::HEIGHT + LETTER_SPACE;
        if room && wide <= usize::from(width) {
            let ink = if ascii { '#' } else { '\u{2588}' };
            return font::render(self.board, ink, None).map(Filling::Letters);
        }
        None
    }
}

/// Draw `filling` at the top of `slot`, centred across it. Clipped to the frame without its last
/// row (the status bar), and to the slot.
pub(crate) fn draw(
    frame: &mut Frame<'_>,
    slot: Rect,
    filling: &Filling<'_>,
    look: Look,
    tier: Tier,
) {
    let area = frame.area();
    let allowed = Rect {
        height: area.height.saturating_sub(1),
        ..area
    };
    let bounds = slot.intersection(allowed);
    let ascii = tier == Tier::Ascii;
    let palette = look.palette();
    match filling {
        Filling::Piece(piece) => paint(
            frame,
            bounds,
            piece.grid(),
            look,
            ascii,
            palette.text,
            false,
        ),
        Filling::Letters(grid) => paint(
            frame,
            bounds,
            grid,
            look,
            ascii,
            palette.accent,
            look.skin().gradient,
        ),
        Filling::Text(text) => {
            let grid = text_grid(text);
            paint(frame, bounds, &grid, look, ascii, palette.accent, false);
        }
    }
}

fn text_grid(text: &str) -> Grid {
    let width = text.chars().count().max(1);
    let mut grid = Grid::blank(width, 1);
    for (x, c) in text.chars().enumerate() {
        grid.set(x, 0, super::grid::cell_for(c, None, None));
    }
    grid
}

fn paint(
    frame: &mut Frame<'_>,
    bounds: Rect,
    grid: &Grid,
    look: Look,
    ascii: bool,
    base: Tint,
    gradient: bool,
) {
    let Ok(width) = u16::try_from(grid.width()) else {
        return;
    };
    let Ok(height) = u16::try_from(grid.height()) else {
        return;
    };
    if width == 0 || height == 0 || width > bounds.width || height > bounds.height {
        return;
    }
    let x0 = bounds.x + (bounds.width - width) / 2;
    let palette = look.palette();
    let last = u32::from(width.saturating_sub(1)).max(1);
    let gradient = gradient && gradient_depth(look.depth());
    let buffer = frame.buffer_mut();
    for dy in 0..height {
        for dx in 0..width {
            let cell = grid.get(usize::from(dx), usize::from(dy));
            let tint = if gradient {
                let step = u8::try_from(u32::from(dx) * 255 / last).unwrap_or(u8::MAX);
                blend(palette.accent, palette.accent2, step).unwrap_or(base)
            } else {
                base
            };
            let target = &mut buffer[(x0 + dx, bounds.y + dy)];
            target.reset();
            let symbol = if ascii { cell.ascii } else { cell.ch };
            let mut utf8 = [0_u8; 4];
            target.set_symbol(symbol.encode_utf8(&mut utf8));
            target.set_style(cell_style(cell, look, tint));
        }
    }
}

/// A cell's style: colours only. The foreground is the cell's own palette colour, or `tint`
/// where the cell has none; the background is the cell's own or the screen's.
pub(crate) fn cell_style(cell: ArtCell, look: Look, tint: Tint) -> Style {
    let fg = match cell.fg {
        Some(i) => look.fg(Tint::Named(vga(i))),
        None => look.fg(tint),
    };
    let bg = cell
        .bg
        .map_or_else(Style::new, |i| look.bg(Tint::Named(vga(i))));
    fg.patch(bg)
}

/// A VGA palette index (0 to 15) as a named colour.
pub(crate) const fn vga(index: u8) -> Color {
    match index {
        0 => Color::Black,
        1 => Color::Blue,
        2 => Color::Green,
        3 => Color::Cyan,
        4 => Color::Red,
        5 => Color::Magenta,
        6 => Color::Yellow,
        7 => Color::Gray,
        8 => Color::DarkGray,
        9 => Color::LightBlue,
        10 => Color::LightGreen,
        11 => Color::LightCyan,
        12 => Color::LightRed,
        13 => Color::LightMagenta,
        14 => Color::LightYellow,
        _ => Color::White,
    }
}
