//! Pixel canvases drawn as ordinary styled cells (UI-8): **half-block** (two square pixels per
//! cell, each with its own colour) and **braille** (2x4 dots per cell, one colour per cell).
//!
//! **Cells only.** A canvas never writes an escape sequence, a graphics protocol (sixel, kitty,
//! iTerm) or text from anywhere: it writes a fixed set of glyphs (`▀ ▄ █`, the braille block,
//! the shade ramp, ASCII) into a ratatui [`Buffer`] with colours, so the sanitizer's guarantees
//! hold and it works over plain ssh and telnet. A canvas is **chrome**: it takes no strings.
//! It also never draws the reserved marker style (bold plus reverse); callers give it
//! [`Paint`], which carries a depth and a tier and nothing else.
//!
//! **Tiers.** Every canvas draws at all four depths and in the ASCII tier:
//!
//! | Depth / tier | Half-block | Braille |
//! |---|---|---|
//! | truecolor, 256, 16 | `▀ ▄ █` with the colour quantised to the depth | braille glyph, cell colour quantised |
//! | mono (Unicode) | shade ramp `░▒▓█` by brightness | braille glyph, no colour |
//! | ASCII tier | brightness ramp ` .:-=+*#%@` | density ramp ` .:+#` by dots set |
//!
//! Braille and block glyphs are never used in the ASCII tier (D18).

// Pixel maths: indices and brightness are small and non-negative; the casts are bounded by the
// canvas size, which callers cap.
#![allow(
    clippy::many_single_char_names,
    reason = "pixel and colour maths use x, y, w, h, r, g, b by convention"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "pixel coordinates and brightness are small, non-negative and bounded by the canvas"
)]
// The API surface is built ahead of its callers (UI-4, UI-10); it is tested here.
#![allow(dead_code, reason = "the canvas API is consumed by UI-4 and UI-10")]

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use super::depth::{ColorDepth, indexed_rgb, nearest_16};

/// 24-bit colour.
pub(crate) type Rgb = (u8, u8, u8);

/// How a canvas is drawn: the colour depth and whether the ASCII tier is in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Paint {
    /// What the client can show.
    pub depth: ColorDepth,
    /// The ASCII tier: no block or braille glyphs at all.
    pub ascii: bool,
}

impl Paint {
    /// Paint at `depth` in the Unicode or ASCII tier.
    pub(crate) const fn new(depth: ColorDepth, ascii: bool) -> Self {
        Self { depth, ascii }
    }
}

/// The colour `rgb` becomes at `depth`; `None` on a mono terminal.
pub(crate) fn quantize(rgb: Rgb, depth: ColorDepth) -> Option<Color> {
    let (r, g, b) = rgb;
    match depth {
        ColorDepth::Mono => None,
        ColorDepth::TrueColor => Some(Color::Rgb(r, g, b)),
        ColorDepth::Ansi256 => Some(Color::Indexed(cube(r, g, b))),
        ColorDepth::Ansi16 => Some(nearest_16(r, g, b)),
    }
}

/// The nearest 256-colour cell, found arithmetically (the exhaustive search in
/// [`super::depth::nearest_256`] is too slow to run for every cell of every frame). Near-greys
/// use the 24-step grey ramp, which has finer steps in the dark than the cube.
fn cube(r: u8, g: u8, b: u8) -> u8 {
    let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
    if hi - lo < 10 {
        let mean = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
        return match mean {
            0..=3 => 16,
            247..=255 => 231,
            _ => 232 + ((mean - 3) * 24 / 245).min(23) as u8,
        };
    }
    let level = |v: u8| -> u8 {
        match v {
            0..=47 => 0,
            48..=114 => 1,
            _ => (v - 35) / 40,
        }
    };
    16 + 36 * level(r) + 6 * level(g) + level(b)
}

/// The RGB a ratatui colour stands for, if the server can know it.
pub(crate) fn rgb_of(color: Color) -> Option<Rgb> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(i) => Some(indexed_rgb(i)),
        Color::Reset => None,
        named => super::depth::Tint::Named(named).rgb(),
    }
}

/// `from` moved `t` (0 to 1) of the way to `to`.
pub(crate) fn mix(from: Rgb, to: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    (lerp(from.0, to.0), lerp(from.1, to.1), lerp(from.2, to.2))
}

/// `rgb` scaled by `k` (0 is black, 1 is unchanged).
pub(crate) fn scale(rgb: Rgb, k: f32) -> Rgb {
    mix((0, 0, 0), rgb, k)
}

/// Perceived brightness, 0 to 1.
pub(crate) fn brightness(rgb: Rgb) -> f32 {
    (0.299 * f32::from(rgb.0) + 0.587 * f32::from(rgb.1) + 0.114 * f32::from(rgb.2)) / 255.0
}

/// Brightness ramps for the tiers with no colour or no block glyphs.
const ASCII_RAMP: &[u8] = b" .:-=+*#%@";
const SHADES: [&str; 5] = [" ", "░", "▒", "▓", "█"];

fn ramp_ascii(level: f32) -> &'static str {
    const TABLE: [&str; 10] = [" ", ".", ":", "-", "=", "+", "*", "#", "%", "@"];
    let i = ((level.clamp(0.0, 1.0) * (ASCII_RAMP.len() - 1) as f32).round() as usize)
        .min(TABLE.len() - 1);
    TABLE[i]
}

fn ramp_shade(level: f32) -> &'static str {
    let i = (level.clamp(0.0, 1.0) * (SHADES.len() - 1) as f32).round() as usize;
    SHADES[i.min(SHADES.len() - 1)]
}

/// A half-block canvas: `w` cells wide and `h` cells tall, so `w` by `2h` square pixels. A pixel
/// is `None` (not drawn: whatever is under it shows) or a colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HalfBlock {
    w: u16,
    h: u16,
    px: Vec<Option<Rgb>>,
}

impl HalfBlock {
    /// A blank canvas of `w` by `h` cells.
    pub(crate) fn new(w: u16, h: u16) -> Self {
        Self {
            w,
            h,
            px: vec![None; usize::from(w) * usize::from(h) * 2],
        }
    }

    /// Width in pixels (equal to cells).
    pub(crate) const fn width(&self) -> u16 {
        self.w
    }

    /// Height in pixels (twice the cells).
    pub(crate) const fn height(&self) -> u16 {
        self.h * 2
    }

    /// Width and height in cells.
    pub(crate) const fn cells(&self) -> (u16, u16) {
        (self.w, self.h)
    }

    fn index(&self, x: u16, y: u16) -> Option<usize> {
        (x < self.w && y < self.h * 2)
            .then(|| usize::from(y) * usize::from(self.w) + usize::from(x))
    }

    /// Set a pixel; one outside the canvas is ignored.
    pub(crate) fn set(&mut self, x: u16, y: u16, rgb: Rgb) {
        if let Some(i) = self.index(x, y) {
            self.px[i] = Some(rgb);
        }
    }

    /// Set a pixel from signed coordinates; one outside the canvas is ignored.
    pub(crate) fn set_i(&mut self, x: i32, y: i32, rgb: Rgb) {
        if let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) {
            self.set(x, y, rgb);
        }
    }

    /// The pixel at `x`, `y`.
    pub(crate) fn get(&self, x: u16, y: u16) -> Option<Rgb> {
        self.index(x, y).and_then(|i| self.px[i])
    }

    /// Fill a rectangle of pixels.
    pub(crate) fn fill(&mut self, x: u16, y: u16, w: u16, h: u16, rgb: Rgb) {
        for yy in y..y.saturating_add(h) {
            for xx in x..x.saturating_add(w) {
                self.set(xx, yy, rgb);
            }
        }
    }

    /// Draw the canvas into `buf` at `area`'s top-left, clipped to `area`. A cell with no pixel
    /// set is left as it is.
    pub(crate) fn render(&self, buf: &mut Buffer, area: Rect, paint: Paint) {
        self.render_masked(buf, area, paint, |_, _, _| true);
    }

    /// Like [`HalfBlock::render`], but a cell is drawn only when `keep(cx, cy, existing)` says
    /// so (`existing` is the cell already in the buffer). Effects use it to dissolve a scene
    /// away and to leave reserved cells alone.
    pub(crate) fn render_masked(
        &self,
        buf: &mut Buffer,
        area: Rect,
        paint: Paint,
        keep: impl Fn(u16, u16, &ratatui::buffer::Cell) -> bool,
    ) {
        let area = area.intersection(buf.area);
        for cy in 0..self.h.min(area.height) {
            for cx in 0..self.w.min(area.width) {
                if !keep(cx, cy, &buf[(area.x + cx, area.y + cy)]) {
                    continue;
                }
                let top = self.get(cx, cy * 2);
                let bottom = self.get(cx, cy * 2 + 1);
                if top.is_none() && bottom.is_none() {
                    continue;
                }
                let cell = &mut buf[(area.x + cx, area.y + cy)];
                let (symbol, style) = half_cell(top, bottom, paint);
                cell.set_symbol(symbol).set_style(style);
            }
        }
    }
}

/// The glyph and style for a pair of pixels.
fn half_cell(top: Option<Rgb>, bottom: Option<Rgb>, paint: Paint) -> (&'static str, Style) {
    if paint.ascii || paint.depth == ColorDepth::Mono {
        let level = f32::midpoint(top.map_or(0.0, brightness), bottom.map_or(0.0, brightness));
        let symbol = if paint.ascii {
            ramp_ascii(level)
        } else {
            ramp_shade(level)
        };
        return (symbol, Style::new());
    }
    let pick = |rgb: Rgb| quantize(rgb, paint.depth);
    match (top, bottom) {
        (Some(a), Some(b)) if a == b => ("█", style_fg(pick(a))),
        (Some(a), Some(b)) => ("▀", style_fg(pick(a)).bg(pick(b).unwrap_or(Color::Reset))),
        (Some(a), None) => ("▀", style_fg(pick(a))),
        (None, Some(b)) => ("▄", style_fg(pick(b))),
        (None, None) => (" ", Style::new()),
    }
}

fn style_fg(color: Option<Color>) -> Style {
    color.map_or_else(Style::new, |c| Style::new().fg(c))
}

/// Braille dot numbering: column 0 is bits 0, 1, 2 then 6; column 1 is bits 3, 4, 5 then 7.
const BIT: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

/// A braille canvas: `w` by `h` cells of 2x4 dots, so `2w` by `4h` dots. A cell has one colour
/// (the last dot set in it wins), as a terminal cell has one foreground.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Braille {
    w: u16,
    h: u16,
    bits: Vec<u8>,
    colors: Vec<Option<Rgb>>,
}

impl Braille {
    /// A blank canvas of `w` by `h` cells.
    pub(crate) fn new(w: u16, h: u16) -> Self {
        let n = usize::from(w) * usize::from(h);
        Self {
            w,
            h,
            bits: vec![0; n],
            colors: vec![None; n],
        }
    }

    /// Width in dots.
    pub(crate) const fn dot_width(&self) -> u16 {
        self.w * 2
    }

    /// Height in dots.
    pub(crate) const fn dot_height(&self) -> u16 {
        self.h * 4
    }

    /// Set a dot (`x` in `0..2w`, `y` in `0..4h`); one outside the canvas is ignored.
    pub(crate) fn set(&mut self, x: i32, y: i32, rgb: Rgb) {
        let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
            return;
        };
        if x >= self.w * 2 || y >= self.h * 4 {
            return;
        }
        let cell = usize::from(y / 4) * usize::from(self.w) + usize::from(x / 2);
        self.bits[cell] |= BIT[usize::from(x % 2)][usize::from(y % 4)];
        self.colors[cell] = Some(rgb);
    }

    /// Whether the dot at `x`, `y` is set.
    pub(crate) fn is_set(&self, x: u16, y: u16) -> bool {
        if x >= self.w * 2 || y >= self.h * 4 {
            return false;
        }
        let cell = usize::from(y / 4) * usize::from(self.w) + usize::from(x / 2);
        self.bits[cell] & BIT[usize::from(x % 2)][usize::from(y % 4)] != 0
    }

    /// A line of dots from `a` to `b` (Bresenham).
    pub(crate) fn line(&mut self, a: (i32, i32), b: (i32, i32), rgb: Rgb) {
        let (mut x, mut y) = a;
        let dx = (b.0 - a.0).abs();
        let dy = -(b.1 - a.1).abs();
        let (sx, sy) = (
            if a.0 < b.0 { 1 } else { -1 },
            if a.1 < b.1 { 1 } else { -1 },
        );
        let mut err = dx + dy;
        // The canvas is small and callers clip; this bound stops a hostile coordinate from
        // looping for long.
        for _ in 0..(dx.max(-dy) + 1).min(10_000) {
            self.set(x, y, rgb);
            if (x, y) == b {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Fill from `y` down to the bottom of the canvas in column `x` (an area chart's body).
    pub(crate) fn fill_down(&mut self, x: i32, y: i32, rgb: Rgb) {
        for yy in y.max(0)..i32::from(self.dot_height()) {
            self.set(x, yy, rgb);
        }
    }

    /// Draw the canvas into `buf` at `area`'s top-left, clipped to `area`. A cell with no dot is
    /// left as it is.
    pub(crate) fn render(&self, buf: &mut Buffer, area: Rect, paint: Paint) {
        let area = area.intersection(buf.area);
        for cy in 0..self.h.min(area.height) {
            for cx in 0..self.w.min(area.width) {
                let i = usize::from(cy) * usize::from(self.w) + usize::from(cx);
                let bits = self.bits[i];
                if bits == 0 {
                    continue;
                }
                let cell = &mut buf[(area.x + cx, area.y + cy)];
                if paint.ascii {
                    let symbol = match bits.count_ones() {
                        0..=2 => ".",
                        3..=4 => ":",
                        5..=6 => "+",
                        _ => "#",
                    };
                    cell.set_symbol(symbol);
                } else {
                    let mut utf8 = [0_u8; 4];
                    let glyph = char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' ');
                    cell.set_symbol(glyph.encode_utf8(&mut utf8));
                }
                let color = self.colors[i].and_then(|rgb| quantize(rgb, paint.depth));
                cell.set_style(style_fg(color));
            }
        }
    }
}

#[cfg(test)]
mod tests;
