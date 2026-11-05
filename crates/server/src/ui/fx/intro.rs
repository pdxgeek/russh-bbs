//! The login intro: an original demoscene-style scene drawn on a half-block canvas. A plasma
//! field (256 colours and up), a flying starfield, and the board's name in big letters that
//! light up one by one, with a shine sweeping over them. It then dissolves cell by cell into
//! the static login screen, so the last frame is that screen.
//!
//! The scene draws only pixels from a canvas, so it is cells and colours and nothing else. The
//! board name is turned into pixels through the fixed font in [`super::font`], which has no
//! marker glyph or delimiter, so the name cannot put anything unexpected on the screen. The
//! scene covers everything above the status row and never touches the status row itself.

#![allow(
    clippy::many_single_char_names,
    reason = "pixel and colour maths use x, y, w, h, r, g, b by convention"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "pixel coordinates and time are small and bounded by the terminal size"
)]

use ratatui::buffer::Buffer;

use super::effects::{Stage, is_reserved, unit};
use super::font;
use crate::ui::canvas::{HalfBlock, Paint, Rgb, mix, scale};
use crate::ui::depth::ColorDepth;

/// How long the scene runs, in seconds (the engine's duration for the intro).
pub(crate) const SECONDS: f32 = 1.8;

/// The smallest terminal the scene is drawn on; below it the static screen is shown at once.
pub(crate) const MIN_SIZE: (u16, u16) = (40, 12);

/// The colours the scene uses, from the style's palette roles.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Colors {
    /// The style's background.
    pub surface: Rgb,
    /// The style's accent.
    pub accent: Rgb,
    /// The style's second accent.
    pub accent2: Rgb,
}

const WHITE: Rgb = (255, 255, 255);

/// The largest the scene is drawn, in cells.
const STAGE: (u16, u16) = (120, 36);

/// How often the plasma field moves, in seconds.
const PLASMA_STEP: f32 = 0.2;

/// How much of the screen the scene covers at `t`: it appears quickly, holds, then dissolves
/// away to nothing by `t = 1`.
pub(crate) fn cover(t: f32) -> f32 {
    let rise = (t / 0.08).clamp(0.0, 1.0);
    let fall = if t < 0.62 {
        1.0
    } else {
        ((1.0 - t) / 0.38).clamp(0.0, 1.0)
    };
    rise * fall
}

/// Draw the scene at `t` into `buf`, inside `stage.area`.
pub(crate) fn paint(buf: &mut Buffer, stage: &Stage, t: f32, title: &str, colors: Colors) {
    // The stage is at most 120x36 cells, centred: a bigger terminal does not get a bigger scene
    // (the cost would grow with the screen), and the login box is centred anyway.
    let full = stage.area;
    let (w, h) = (full.width.min(STAGE.0), full.height.min(STAGE.1));
    let area = ratatui::layout::Rect::new(
        full.x + (full.width - w) / 2,
        full.y + (full.height - h) / 2,
        w,
        h,
    );
    if area.width < MIN_SIZE.0 || area.height + 1 < MIN_SIZE.1 {
        return;
    }
    let cover = cover(t);
    if cover <= 0.0 {
        return;
    }
    let mut canvas = HalfBlock::new(area.width, area.height);
    let s = t * SECONDS;
    // The scene is drawn in at most 256 colours, even on a truecolor terminal: a truecolor
    // sequence is twice as long and a smooth field changes every cell on every frame, which
    // measured 5 to 20 times the bytes (see the benchmark in docs/ui.md). Posterised to the
    // palette, neighbouring cells match and the diff skips most of them.
    let depth = stage.depth.min(ColorDepth::Ansi256);
    if depth >= ColorDepth::Ansi256 && !stage.ascii {
        // The field is chunky on purpose (demoscene style, and cheap): blocks of 2x2 pixels, so
        // the two halves of a cell match (one colour sequence, not two) and neighbours match
        // too. It moves in steps of about 150 ms rather than every frame, so most cells are
        // unchanged between frames and the diff skips them.
        plasma(
            &mut canvas,
            (s / PLASMA_STEP).floor() * PLASMA_STEP,
            colors,
            2,
        );
    }
    stars(&mut canvas, s, colors);
    let plain = stage.ascii || depth == ColorDepth::Mono;
    if !plain {
        big_title(&mut canvas, t, title, colors);
    }
    canvas.render_masked(buf, area, Paint::new(depth, stage.ascii), |x, y, cell| {
        !is_reserved(cell) && unit(x, y) < cover
    });
    if plain {
        plain_title(buf, area, t, title, cover);
    }
}

/// The board name as spaced capitals on one row, letter by letter, for the tiers with no colour
/// or no block glyphs: big letters would be a smudge of ramp characters there. Only the font's
/// own letters, digits and marks are drawn (see [`font`]), in ASCII, and only into cells the
/// scene is allowed to cover.
fn plain_title(buf: &mut Buffer, area: ratatui::layout::Rect, t: f32, title: &str, cover: f32) {
    let letters: Vec<char> = font::supported(title);
    let room = usize::from(area.width).saturating_sub(4);
    // "R U S S H" is two columns a letter; drop letters that do not fit.
    let letters: Vec<char> = letters.into_iter().take(room.div_ceil(2)).collect();
    if letters.is_empty() {
        return;
    }
    let width = letters.len() * 2 - 1;
    let x0 = area.x + (area.width - u16::try_from(width).unwrap_or(0)) / 2;
    let y = area.y + area.height / 3;
    let shown = (((t - 0.10) / 0.36).clamp(0.0, 1.0) * letters.len() as f32).ceil() as usize;
    // The letters, and a blank between each pair so a star does not sit inside the word.
    for (i, ch) in letters.iter().take(shown).enumerate() {
        for (offset, symbol) in [
            (0_usize, Some(*ch)),
            (1, (i + 1 < letters.len()).then_some(' ')),
        ] {
            let Some(symbol) = symbol else { continue };
            let x = x0 + u16::try_from(i * 2 + offset).unwrap_or(0);
            if !area.contains((x, y).into()) {
                continue;
            }
            let cell = &mut buf[(x, y)];
            if is_reserved(cell) || unit(x - area.x, y - area.y) >= cover {
                continue;
            }
            let mut utf8 = [0_u8; 4];
            cell.set_symbol(symbol.encode_utf8(&mut utf8))
                .set_style(ratatui::style::Style::new());
        }
    }
}

fn plasma(canvas: &mut HalfBlock, s: f32, colors: Colors, block: u16) {
    // A pixel's block is aligned so the two halves of a cell share a colour.
    let (w, h) = (f32::from(canvas.width()), f32::from(canvas.height()));
    for py in 0..canvas.height() {
        for px in 0..canvas.width() {
            let (x, y) = (f32::from(px / block * block), f32::from(py / block * block));
            let d = (x - w / 2.0).hypot(y - h / 2.0);
            let v = ((x * 0.09 + s * 2.1).sin()
                + (y * 0.13 - s * 1.7).sin()
                + ((x + y) * 0.05 + s * 1.1).sin()
                + (d * 0.10 - s * 2.6).sin())
                / 4.0;
            // Two tones with dark seams between them, in seven steps rather than a smooth ramp:
            // mixing the two accents would pass through grey, and a cell changes colour only when
            // the field crosses a step, which is what keeps the frames small.
            let level = (v.abs() * 6.0).round() / 6.0;
            let hue = if v < 0.0 {
                colors.accent2
            } else {
                colors.accent
            };
            let c = scale(hue, 0.06 + 0.64 * level);
            canvas.set(px, py, mix(colors.surface, c, 0.92));
        }
    }
}

fn stars(canvas: &mut HalfBlock, s: f32, colors: Colors) {
    let (w, h) = (f32::from(canvas.width()), f32::from(canvas.height()));
    let max = (w / 2.0).hypot(h / 2.0);
    for i in 0..36_u16 {
        let angle = f32::from(i) * 2.399_963;
        let speed = 0.35 + 0.65 * unit(i, 7);
        let r = (unit(i, 3) + s * speed * 0.6).fract();
        let dist = r * r * max;
        let (dx, dy) = (angle.cos(), angle.sin());
        let (x, y) = (w / 2.0 + dx * dist, h / 2.0 + dy * dist);
        let c = mix(colors.accent2, WHITE, r);
        canvas.set_i(x as i32, y as i32, c);
        if r > 0.7 {
            // A short streak toward the centre.
            canvas.set_i((x - dx) as i32, (y - dy) as i32, scale(c, 0.6));
        }
    }
}

fn big_title(canvas: &mut HalfBlock, t: f32, title: &str, colors: Colors) {
    let (w, h) = (i32::from(canvas.width()), i32::from(canvas.height()));
    let all = renderable(title);
    // The biggest size at which every renderable letter fits: 3x3, 2x2, then 1 wide by 2 tall
    // (a tall, narrow letter for an 80-column screen) and 1x1 (truncated) as the last resort.
    let mut choice = None;
    for (kx, ky) in [(3_i32, 3_i32), (2, 2), (1, 2), (1, 1)] {
        let room = u16::try_from(((w - 4) / kx).max(0)).unwrap_or(0);
        let (letters, width) = font::layout(title, room);
        if letters.is_empty() || 5 * ky + 4 > h / 2 + 1 {
            continue;
        }
        if letters.len() == all || (kx, ky) == (1, 1) {
            choice = Some((kx, ky, letters, i32::from(width)));
            break;
        }
    }
    let Some((kx, ky, letters, width)) = choice else {
        return;
    };
    let (total_w, total_h) = (width * kx, 5 * ky);
    let x0 = (w - total_w) / 2;
    let y0 = ((h - total_h) / 2 - h / 12).max(1);
    let count = letters.len().max(1) as f32;
    let shine = x0 as f32 + (t - 0.40) / 0.25 * (total_w as f32 + 20.0) - 10.0;
    for pass in 0..2 {
        for (i, letter) in letters.iter().enumerate() {
            let appears = 0.10 + 0.36 * (i as f32 / count);
            if t < appears {
                continue;
            }
            let flash = (1.0 - (t - appears) / 0.09).clamp(0.0, 1.0);
            for cy in 0..5_u16 {
                for cx in 0..5_u16 {
                    if !letter.lit(cx, cy) {
                        continue;
                    }
                    for sy in 0..ky {
                        for sx in 0..kx {
                            let px = x0 + (i32::from(letter.x) + i32::from(cx)) * kx + sx;
                            let py = y0 + i32::from(cy) * ky + sy;
                            if pass == 0 {
                                canvas.set_i(px + 1, py + 1, scale(colors.surface, 0.3));
                                continue;
                            }
                            let along = (px - x0) as f32 / total_w.max(1) as f32;
                            let mut c = mix(colors.accent, colors.accent2, along);
                            if (px as f32 - shine).abs() < 3.0 {
                                c = mix(c, WHITE, 0.75);
                            }
                            canvas.set_i(px, py, mix(c, WHITE, flash));
                        }
                    }
                }
            }
        }
    }
    // An underline that draws itself under the letters.
    let grow = ((t - 0.30) / 0.25).clamp(0.0, 1.0);
    let line = (total_w as f32 * grow) as i32;
    for dx in 0..line {
        canvas.set_i(
            x0 + dx,
            y0 + total_h + 2,
            mix(
                colors.accent2,
                colors.accent,
                dx as f32 / total_w.max(1) as f32,
            ),
        );
    }
}

/// How many letters of `title` the font can draw.
fn renderable(title: &str) -> usize {
    font::layout(title, u16::MAX).0.len()
}
