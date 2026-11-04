//! The screen behind the panels: a style's background colour and, where the colour depth and
//! the tier allow it, a texture (scanlines, stars, a grid) (UI-2).
//!
//! **Chrome only, and never near a marker.** The backdrop is painted into the area the frame
//! hands out *above* the status bar, before anything else is drawn, and the status bar clears its
//! own row, so the marker cell never carries a colour or a glyph from here. Panels clear their
//! own area, so the texture never shows through text. Every glyph is fixed ASCII in this file.
//! Nothing is drawn on a mono terminal or in the ASCII tier: the texture is decoration, and a
//! plain screen is the readable one.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Block;

use super::depth::{ColorDepth, Tint};
use super::theme::Backdrop;
use super::theme::skin::blend;
use crate::settings::Look;

/// Paint the style's background and texture over `area`.
pub(crate) fn paint(frame: &mut Frame<'_>, area: Rect, look: Look) {
    let base = look
        .fg(look.palette().text)
        .patch(look.bg(look.palette().surface));
    if base != Style::new() {
        frame.render_widget(Block::default().style(base), area);
    }
    if !texture_drawn(look) {
        return;
    }
    let palette = look.palette();
    let surface = palette.surface;
    let buffer = frame.buffer_mut();
    match look.skin().backdrop {
        Backdrop::None => {}
        // Scanlines are a pass over the finished screen ([`finish`]), not a background.
        Backdrop::Scanlines => {}
        Backdrop::Stars => {
            let Some(dim) = blend(surface, palette.text, 110) else {
                return;
            };
            let faint = look.fg(dim);
            for y in area.y..area.y + area.height {
                for x in area.x..area.x + area.width {
                    let h = hash(x, y);
                    let symbol = match h % 61 {
                        0 => "*",
                        1..=3 => ".",
                        _ => continue,
                    };
                    buffer[(x, y)].set_symbol(symbol).set_style(faint);
                }
            }
        }
        Backdrop::Grid => {
            let Some(dim) = blend(surface, palette.accent, 90) else {
                return;
            };
            let faint = look.fg(dim);
            for y in (area.y..area.y + area.height).filter(|y| (y - area.y) % 3 == 2) {
                for x in (area.x..area.x + area.width).filter(|x| (x - area.x) % 6 == 3) {
                    buffer[(x, y)].set_symbol(".").set_style(faint);
                }
            }
        }
    }
}

/// A texture is drawn only where there is colour to draw it with, and not in the ASCII tier.
fn texture_drawn(look: Look) -> bool {
    look.depth() >= ColorDepth::Ansi256 && !look.ascii()
}

/// A cheap deterministic hash of a cell, so the same stars are in the same places every frame
/// (a changing field would redraw the whole screen).
fn hash(x: u16, y: u16) -> u32 {
    let mut h = u32::from(x).wrapping_mul(0x9E37_79B1) ^ u32::from(y).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}

/// The last pass of a frame, after the screen has drawn: the **scanline** texture. Every other
/// row whose background is one of the style's own (the glass, a panel or a popup) gets a shade
/// darker, so the lines run through panels and text alike. Cells with any other background (the
/// bars, the selected row, a table header) are left alone, and the last row, where the status bar
/// and its marker cell live, is never touched. Does nothing for other backdrops, on a mono or
/// 16-colour terminal, or in the ASCII tier.
pub(crate) fn finish(frame: &mut Frame<'_>, look: Look) {
    if look.skin().backdrop != Backdrop::Scanlines || !texture_drawn(look) {
        return;
    }
    let palette = look.palette();
    let black = Tint::Named(ratatui::style::Color::Black);
    let shades: Vec<(ratatui::style::Color, ratatui::style::Color)> =
        [palette.surface, palette.panel, palette.popup]
            .into_iter()
            .filter_map(|tint| {
                let base = look.color_of(tint)?;
                let dark = look.color_of(blend(tint, black, 88)?)?;
                Some((base, dark))
            })
            .collect();
    if shades.is_empty() {
        return;
    }
    let area = frame.area();
    let buffer = frame.buffer_mut();
    for y in (area.y..area.y + area.height.saturating_sub(1)).filter(|y| (y - area.y) % 2 == 1) {
        for x in area.x..area.x + area.width {
            let cell = &mut buffer[(x, y)];
            if let Some((_, dark)) = shades.iter().find(|(base, _)| *base == cell.bg) {
                cell.bg = *dark;
            }
        }
    }
}
