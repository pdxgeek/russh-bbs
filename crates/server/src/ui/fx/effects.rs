//! The effects themselves: pure functions that rewrite a finished frame's [`Buffer`] for a
//! moment in time `t` (0 at the start, 1 at the end). At `t = 1` every one of them changes
//! nothing, so the last frame of an effect is the static screen.
//!
//! **What an effect may touch.** A [`Stage`] holds the area an effect may change: the frame
//! minus its last row, which is the status bar and the security marker cell (D39). On top of
//! that, a cell that carries the reserved marker style (bold plus reverse) is skipped, so even
//! a marker drawn somewhere unexpected is left alone. Effects write only blanks, fixed ASCII
//! and block glyphs and colours they compute; they never copy text from anywhere, so nothing a
//! member typed can reach the screen through one, and they never set the reserved style.

#![allow(
    clippy::many_single_char_names,
    reason = "pixel and colour maths use x, y, w, h, r, g, b by convention"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "cell coordinates and progress are small and bounded by the terminal size"
)]
#![allow(
    dead_code,
    reason = "glow and gradient are API for UI-4 and UI-10; tested here"
)]

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

use crate::settings::Look;
use crate::ui::canvas::{Rgb, mix, quantize, rgb_of};
use crate::ui::depth::ColorDepth;

/// Where and how an effect draws.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stage {
    /// The cells an effect may change: the frame without its last row.
    pub area: Rect,
    /// What the client can show.
    pub depth: ColorDepth,
    /// The ASCII tier: no block glyphs.
    pub ascii: bool,
    /// The style's background, behind hidden cells.
    pub backdrop: Option<Color>,
    /// The colour of a frontier or cursor.
    pub edge: Option<Color>,
}

impl Stage {
    /// The stage for a frame of `frame` cells drawn with `look`. The last row (status bar and
    /// marker cell) is never part of it.
    pub(crate) fn new(frame: Rect, look: Look) -> Self {
        let area = Rect {
            height: frame.height.saturating_sub(1),
            ..frame
        };
        let palette = look.palette();
        Self {
            area,
            depth: look.depth(),
            ascii: look.ascii(),
            backdrop: look.color_of(palette.surface),
            edge: look.color_of(palette.accent),
        }
    }

    fn each(&self, buf: &mut Buffer, mut f: impl FnMut(&mut Cell, u16, u16)) {
        let area = self.area.intersection(buf.area);
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                let cell = &mut buf[(x, y)];
                if is_reserved(cell) {
                    continue;
                }
                f(cell, x - area.x, y - area.y);
            }
        }
    }
}

/// Whether a cell wears the reserved marker style (bold plus reverse). Effects leave it alone.
pub(crate) fn is_reserved(cell: &Cell) -> bool {
    cell.modifier.contains(Modifier::BOLD | Modifier::REVERSED)
}

/// A cheap deterministic number in `[0, 1)` for a cell, so a dissolve is the same every frame.
pub(crate) fn unit(x: u16, y: u16) -> f32 {
    let mut h = u32::from(x).wrapping_mul(0x9E37_79B1) ^ u32::from(y).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65_536.0
}

/// Blank a cell, keeping its background (so a panel stays a panel) or replacing it.
fn hide(cell: &mut Cell, background: Option<Color>) {
    let kept = cell.bg;
    cell.reset();
    cell.set_symbol(" ");
    match background {
        Some(bg) => {
            cell.set_bg(bg);
        }
        None => {
            if kept != Color::Reset {
                cell.set_bg(kept);
            }
        }
    }
}

/// Cells appear in a fixed random order as `t` rises, at every colour depth.
pub(crate) fn dissolve(buf: &mut Buffer, stage: &Stage, t: f32) {
    if t >= 1.0 {
        return;
    }
    stage.each(buf, |cell, x, y| {
        if unit(x, y) >= t {
            hide(cell, None);
        }
    });
}

/// Fade the screen in from `toward` (usually the backdrop colour). Needs 256 colours or
/// truecolor to blend; below that it dissolves, which reads the same on 16 colours and mono.
pub(crate) fn fade(buf: &mut Buffer, stage: &Stage, t: f32, toward: Rgb) {
    if t >= 1.0 {
        return;
    }
    if stage.depth < ColorDepth::Ansi256 {
        dissolve(buf, stage, t);
        return;
    }
    let depth = stage.depth;
    let blend = |color: Color, default: Rgb| -> Color {
        let rgb = rgb_of(color).unwrap_or(default);
        quantize(mix(toward, rgb, t), depth).unwrap_or(color)
    };
    stage.each(buf, |cell, _, _| {
        cell.fg = blend(cell.fg, (200, 200, 200));
        cell.bg = blend(cell.bg, toward);
    });
}

/// A left-to-right sweep: everything right of the front is dimmed and the front itself is a bright
/// bar, so the screen seems to light up behind it. **No symbol ever changes**, so a client that
/// reads the text mid-sweep (a script, a screen reader, a slow link) sees the whole screen; only
/// the colours move. From 256 colours the dimming is a blend toward the backdrop and the front
/// is an accent-tinted column; below that the dimming is the terminal's dim attribute and there
/// is no front bar. `t = 1` has moved the front off the right edge.
pub(crate) fn wipe(buf: &mut Buffer, stage: &Stage, t: f32) {
    if t >= 1.0 {
        return;
    }
    let width = f32::from(stage.area.width);
    let front = (t * (width + 2.0)) as u16;
    let depth = stage.depth;
    let toward = stage.backdrop.and_then(rgb_of).unwrap_or((0, 0, 0));
    let edge = stage.edge.and_then(rgb_of);
    stage.each(buf, |cell, x, _| {
        if x < front.saturating_sub(1) {
            return;
        }
        if depth >= ColorDepth::Ansi256 {
            if x == front || x + 1 == front {
                if let Some(color) = edge
                    .and_then(|e| quantize(mix(rgb_of(cell.bg).unwrap_or(toward), e, 0.45), depth))
                {
                    cell.set_bg(color);
                }
            } else if x > front
                && let Some(rgb) = rgb_of(cell.fg)
                && let Some(color) = quantize(mix(rgb, toward, 0.6), depth)
            {
                cell.set_fg(color);
            }
        } else if x > front {
            cell.modifier.insert(Modifier::DIM);
        }
    });
}

/// A typewriter: the text appears in reading order with a cursor at the head. Only cells that
/// show something count, so blank space does not eat the time.
pub(crate) fn typewriter(buf: &mut Buffer, stage: &Stage, t: f32) {
    if t >= 1.0 {
        return;
    }
    let mut total = 0_usize;
    stage.each(buf, |cell, _, _| {
        if cell.symbol() != " " {
            total += 1;
        }
    });
    let shown = (t * total as f32) as usize;
    let cursor = if stage.ascii { "_" } else { "▌" };
    let mut seen = 0_usize;
    stage.each(buf, |cell, _, _| {
        if cell.symbol() == " " {
            return;
        }
        if seen == shown {
            hide(cell, None);
            cell.set_symbol(cursor);
            if let Some(edge) = stage.edge {
                cell.set_fg(edge);
            }
        } else if seen > shown {
            hide(cell, None);
        }
        seen += 1;
    });
}

/// A soft pulse: every cell of `area` has its background moved toward `tint` by `level` (0 to 1),
/// for the item under a cursor. `level` is the caller's pulse, for example `0.5 + 0.5 * sin`.
/// Only backgrounds change; text and glyphs are untouched. Does nothing below 256 colours
/// (sixteen colours would jump a whole step) or on a mono terminal.
pub(crate) fn glow(buf: &mut Buffer, area: Rect, depth: ColorDepth, tint: Rgb, level: f32) {
    if depth < ColorDepth::Ansi256 {
        return;
    }
    let area = area.intersection(buf.area);
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            if is_reserved(cell) {
                continue;
            }
            let base = rgb_of(cell.bg).unwrap_or((0, 0, 0));
            if let Some(color) = quantize(mix(base, tint, level * 0.6), depth) {
                cell.set_bg(color);
            }
        }
    }
}

/// A horizontal colour gradient over the text of `area`: every cell that shows something gets a
/// foreground from `from` (left) to `to` (right). Below 256 colours it keeps the flat colour it
/// has, since sixteen colours would band.
pub(crate) fn gradient_text(buf: &mut Buffer, area: Rect, depth: ColorDepth, from: Rgb, to: Rgb) {
    if depth < ColorDepth::Ansi256 {
        return;
    }
    let area = area.intersection(buf.area);
    let span = f32::from(area.width.saturating_sub(1).max(1));
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            if is_reserved(cell) || cell.symbol() == " " {
                continue;
            }
            let at = f32::from(x - area.x) / span;
            if let Some(color) = quantize(mix(from, to, at), depth) {
                cell.set_fg(color);
            }
        }
    }
}
