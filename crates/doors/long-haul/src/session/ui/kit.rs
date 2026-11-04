//! The drawing kit every Long Haul screen shares: the palette (the theme's roles plus three
//! accents of the ship's own), bars and pips, panels, key hints, and a braille raster.
//!
//! Nothing here knows a rule. It turns numbers into shapes, and it keeps the two ways the
//! shapes degrade honest: the board brings each colour down to the member's depth and swaps
//! icon-like characters for ASCII in the ASCII tier, and where that swap would be too crude
//! (bars, the chart) the kit draws plain ASCII itself.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    reason = "screen geometry: values are small, clamped to the area, and never money"
)]

use russh_bbs_door::DoorFrame;
use russh_bbs_door::DoorStyle;
use russh_bbs_door::buffer::Buffer;
use russh_bbs_door::layout::Rect;
use russh_bbs_door::style::{Color, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Block, BorderType, Widget};

/// Hull grey: borders and structure.
pub(super) const HULL: Color = Color::Rgb(0x8d, 0x96, 0xa0);
/// Engine amber: the ship's own lights, the cursor and the route.
pub(super) const AMBER: Color = Color::Rgb(0xe8, 0xa3, 0x3d);
/// Starfield blue: space itself.
pub(super) const STAR: Color = Color::Rgb(0x5b, 0x8d, 0xe0);
/// Far stars.
pub(super) const DEEP: Color = Color::Rgb(0x2f, 0x4a, 0x80);

/// One accent per faction, in standing order (League, Miners, Free Captains, Guild, Church).
const FACTION_COLORS: [Color; 5] = [
    Color::Rgb(0x4f, 0xa3, 0xe8),
    Color::Rgb(0xd9, 0x7a, 0x3c),
    Color::Rgb(0xd2, 0x4d, 0x57),
    Color::Rgb(0x3d, 0xc4, 0xb0),
    Color::Rgb(0xa9, 0x7b, 0xd8),
];

/// Every style a screen draws with. Built once per frame from the theme's roles.
#[derive(Clone, Copy)]
pub(super) struct Pal {
    pub plain: Style,
    pub dim: Style,
    pub strong: Style,
    pub accent: Style,
    pub good: Style,
    pub bad: Style,
    pub sel: Style,
    pub hull: Style,
    pub amber: Style,
    pub star: Style,
    pub deep: Style,
    /// The member is in the ASCII tier: draw plain ASCII where the swap would be crude.
    pub ascii: bool,
}

impl Pal {
    pub(super) fn new(frame: &DoorFrame<'_>, ascii: bool) -> Self {
        Self {
            plain: frame.style(DoorStyle::Plain),
            dim: frame.style(DoorStyle::Dim),
            strong: frame.style(DoorStyle::Strong),
            accent: frame.style(DoorStyle::Accent),
            good: frame.style(DoorStyle::Good),
            bad: frame.style(DoorStyle::Bad),
            sel: frame.style(DoorStyle::Selected),
            hull: Style::new().fg(HULL),
            amber: Style::new().fg(AMBER),
            star: Style::new().fg(STAR),
            deep: Style::new().fg(DEEP),
            ascii,
        }
    }

    /// Green, amber or red for how full a gauge is that you want full (fuel, hull). Words
    /// always travel with it: colour is never the only cue.
    pub(super) fn level(&self, pct: u32) -> Style {
        match pct {
            50.. => self.good,
            25..=49 => self.amber,
            _ => self.bad,
        }
    }

    /// The reverse of [`Pal::level`]: red when high (wear, danger).
    pub(super) fn danger(&self, pct: u32) -> Style {
        self.level(100u32.saturating_sub(pct))
    }

    /// A faction's accent.
    pub(super) fn faction(&self, ix: usize) -> Style {
        Style::new().fg(FACTION_COLORS[ix % FACTION_COLORS.len()])
    }

    /// A style for text drawn in the given [`DoorStyle`] role.
    pub(super) fn role(&self, style: DoorStyle) -> Style {
        match style {
            DoorStyle::Plain => self.plain,
            DoorStyle::Dim => self.dim,
            DoorStyle::Strong => self.strong,
            DoorStyle::Accent => self.accent,
            DoorStyle::Good => self.good,
            DoorStyle::Bad => self.bad,
            DoorStyle::Selected => self.sel,
        }
    }
}

/// Cut `text` to at most `width` columns, ending in `~` when it was cut.
pub(super) fn trunc(text: &str, width: usize) -> String {
    if Line::raw(text).width() <= width {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = Span::raw(c.to_string()).width();
        if used + w + 1 > width {
            break;
        }
        out.push(c);
        used += w;
    }
    if width > 0 {
        out.push('~');
    }
    out
}

/// Text padded on the right to exactly `width` columns (cut if longer).
pub(super) fn pad(text: &str, width: usize) -> String {
    let cut = trunc(text, width);
    let have = Line::raw(cut.as_str()).width();
    format!("{cut}{}", " ".repeat(width.saturating_sub(have)))
}

/// Text padded on the left (right-aligned) to `width` columns.
pub(super) fn rpad(text: &str, width: usize) -> String {
    let cut = trunc(text, width);
    let have = Line::raw(cut.as_str()).width();
    format!("{}{cut}", " ".repeat(width.saturating_sub(have)))
}

/// Credits with thousands separators.
pub(super) fn credits(n: i64) -> String {
    super::super::cr(n)
}

/// Eighths of a block, for the fractional end of a bar.
const EIGHTHS: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];

/// A horizontal bar. Unicode tier: full blocks, a fractional end and a shaded track. ASCII
/// tier: `#` and `-`. The fill is drawn with the foreground, so it shows without colour.
pub(super) struct Bar {
    /// Fill in per-mille, 0 to 1000.
    pub pm: u32,
    pub fill: Style,
    pub track: Style,
    pub ascii: bool,
    /// Per-mille positions of tick marks drawn over the bar (a threshold).
    pub ticks: Vec<u32>,
}

impl Bar {
    pub(super) fn new(pm: u32, fill: Style, pal: &Pal) -> Self {
        Self {
            pm: pm.min(1000),
            fill,
            track: pal.dim,
            ascii: pal.ascii,
            ticks: Vec::new(),
        }
    }

    pub(super) fn ticks(mut self, ticks: Vec<u32>) -> Self {
        self.ticks = ticks;
        self
    }
}

impl Widget for Bar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let w = usize::from(area.width);
        let total = (self.pm as usize * w * 8).div_ceil(1000);
        let (full, rem) = (total / 8, total % 8);
        for i in 0..w {
            let (ch, style) = if self.ascii {
                if i < full || (i == full && rem >= 4) {
                    ('#', self.fill)
                } else {
                    ('-', self.track)
                }
            } else if i < full {
                ('█', self.fill)
            } else if i == full && rem > 0 {
                (EIGHTHS[rem], self.fill)
            } else {
                ('░', self.track)
            };
            let x = area.x + i as u16;
            for y in area.y..area.y + area.height {
                buf[(x, y)].set_char(ch).set_style(style);
            }
        }
        for t in &self.ticks {
            let i = (*t as usize * w / 1000).min(w - 1);
            let x = area.x + i as u16;
            buf[(x, area.y)].set_char(if self.ascii { '|' } else { '┃' });
        }
    }
}

/// A bar that runs both ways from a middle notch (a mood or a standing from -max to +max).
pub(super) struct Centered {
    /// Value from -`max` to `max`.
    pub value: i32,
    pub max: i32,
    pub neg: Style,
    pub pos: Style,
    pub track: Style,
    pub ascii: bool,
}

impl Widget for Centered {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 || self.max <= 0 {
            return;
        }
        let w = i32::from(area.width);
        let mid = w / 2;
        let span = (self.value.abs() * mid / self.max).min(mid);
        for i in 0..w {
            let (ch, style) = if i == mid {
                (if self.ascii { '|' } else { '│' }, self.track)
            } else if self.value > 0 && i > mid && i <= mid + span {
                (if self.ascii { '#' } else { '█' }, self.pos)
            } else if self.value < 0 && i < mid && i >= mid - span {
                (if self.ascii { '#' } else { '█' }, self.neg)
            } else {
                (if self.ascii { '-' } else { '░' }, self.track)
            };
            let x = area.x + i as u16;
            buf[(x, area.y)].set_char(ch).set_style(style);
        }
    }
}

/// Lines of text placed at fixed cells (relative to the area): labels over a chart.
pub(super) struct Strings(pub Vec<(u16, u16, String, Style)>);

impl Widget for Strings {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for (x, y, text, style) in self.0 {
            if x < area.width && y < area.height {
                buf.set_stringn(
                    area.x + x,
                    area.y + y,
                    &text,
                    usize::from(area.width - x),
                    style,
                );
            }
        }
    }
}

/// `n` of `max` pips: `●●●○○` (`***..` in ASCII).
pub(super) fn pips(n: u32, max: u32, ascii: bool) -> String {
    let on = if ascii { '*' } else { '●' };
    let off = if ascii { '.' } else { '○' };
    (0..max).map(|i| if i < n { on } else { off }).collect()
}

/// A panel with a hull-grey border and a title. `hot` makes the border engine amber.
pub(super) fn panel<'a>(pal: &Pal, title: &str, hot: bool) -> Block<'a> {
    let border = if hot { pal.amber } else { pal.hull };
    let shown = if title.is_empty() {
        Line::raw("")
    } else {
        Line::from(vec![
            Span::styled(" ", border),
            Span::styled(title.to_owned(), pal.accent),
            Span::styled(" ", border),
        ])
    };
    Block::bordered()
        .border_type(if pal.ascii {
            BorderType::Plain
        } else {
            BorderType::Rounded
        })
        .border_style(border)
        .title(shown)
}

/// Key hints as lines wrapped to `width`: `[j]` in amber, the label dim. Returns at most
/// `max_lines` lines; items that do not fit are dropped from the end, so list the most
/// important first.
pub(super) fn key_lines(
    pal: &Pal,
    items: &[(&str, &str)],
    width: u16,
    max_lines: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    let width = usize::from(width);
    for (key, label) in items {
        let need = key.chars().count() + label.chars().count() + 3;
        if used > 0 && used + need > width {
            lines.push(Line::from(std::mem::take(&mut spans)));
            used = 0;
            if lines.len() >= max_lines {
                return lines;
            }
        }
        spans.push(Span::styled(format!("[{key}]"), pal.amber));
        spans.push(Span::styled(format!("{label}  "), pal.dim));
        used += need;
    }
    if !spans.is_empty() && lines.len() < max_lines {
        lines.push(Line::from(spans));
    }
    lines
}

/// Dots per cell, across and down, in a braille cell.
const DOTS_X: i32 = 2;
const DOTS_Y: i32 = 4;

/// A grid of braille dots with a layer per cell, so a lane, a ring and a route that share a
/// cell take the colour of the most important one.
pub(super) struct Raster {
    cols: u16,
    rows: u16,
    bits: Vec<u8>,
    layer: Vec<u8>,
}

impl Raster {
    pub(super) fn new(cols: u16, rows: u16) -> Self {
        let n = usize::from(cols) * usize::from(rows);
        Self {
            cols,
            rows,
            bits: vec![0; n],
            layer: vec![0; n],
        }
    }

    pub(super) fn dots_w(&self) -> i32 {
        i32::from(self.cols) * DOTS_X
    }

    pub(super) fn dots_h(&self) -> i32 {
        i32::from(self.rows) * DOTS_Y
    }

    /// Raise the dot at (`x`, `y`) in dot units, on `layer`. Outside the raster is ignored.
    pub(super) fn plot(&mut self, x: i32, y: i32, layer: u8) {
        if x < 0 || y < 0 || x >= self.dots_w() || y >= self.dots_h() {
            return;
        }
        let (cx, cy) = (x / DOTS_X, y / DOTS_Y);
        let (dx, dy) = ((x % DOTS_X) as usize, (y % DOTS_Y) as usize);
        const MAP: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
        let i = cy as usize * usize::from(self.cols) + cx as usize;
        self.bits[i] |= MAP[dy][dx];
        self.layer[i] = self.layer[i].max(layer);
    }

    /// A line, dotted when `dash` is `(on, off)`.
    pub(super) fn line(
        &mut self,
        from: (i32, i32),
        to: (i32, i32),
        layer: u8,
        dash: Option<(u32, u32)>,
    ) {
        let (mut x, mut y) = from;
        let (dx, dy) = ((to.0 - x).abs(), -(to.1 - y).abs());
        let (sx, sy) = (if x < to.0 { 1 } else { -1 }, if y < to.1 { 1 } else { -1 });
        let mut err = dx + dy;
        let mut step = 0u32;
        // Bounded: a line cannot be longer than the two axes together.
        for _ in 0..(dx.abs() + dy.abs() + 2).min(20_000) {
            let on = dash.is_none_or(|(on, off)| step % (on + off) < on);
            if on {
                self.plot(x, y, layer);
            }
            step += 1;
            if (x, y) == to {
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

    /// A circle of radius `r` dots, dotted when `dash` is given.
    pub(super) fn circle(&mut self, c: (i32, i32), r: i32, layer: u8, dash: Option<(u32, u32)>) {
        if r <= 0 || r > 4000 {
            return;
        }
        // Walk the circle by angle with a step fine enough to leave no gaps.
        let n = (r * 8).clamp(16, 8000);
        for k in 0..n {
            let a = f64::from(k) * std::f64::consts::TAU / f64::from(n);
            let on = dash.is_none_or(|(on, off)| (k as u32) % (on + off) < on);
            if on {
                self.plot(
                    c.0 + (f64::from(r) * a.cos()).round() as i32,
                    c.1 + (f64::from(r) * a.sin()).round() as i32,
                    layer,
                );
            }
        }
    }

    /// The raster as a widget; `styles[layer]` colours each cell.
    pub(super) fn widget<'a>(&'a self, styles: &'a [Style]) -> RasterWidget<'a> {
        RasterWidget { r: self, styles }
    }
}

/// See [`Raster::widget`].
pub(super) struct RasterWidget<'a> {
    r: &'a Raster,
    styles: &'a [Style],
}

impl Widget for RasterWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for cy in 0..self.r.rows.min(area.height) {
            for cx in 0..self.r.cols.min(area.width) {
                let i = usize::from(cy) * usize::from(self.r.cols) + usize::from(cx);
                let bits = self.r.bits[i];
                if bits == 0 {
                    continue;
                }
                let style = self
                    .styles
                    .get(usize::from(self.r.layer[i]))
                    .copied()
                    .unwrap_or_default();
                if let Some(ch) = char::from_u32(0x2800 + u32::from(bits)) {
                    buf[(area.x + cx, area.y + cy)]
                        .set_char(ch)
                        .set_style(style);
                }
            }
        }
    }
}

/// A stable hash for scattering stars and picking art.
pub(super) fn mix(a: u64, b: u64) -> u64 {
    let mut z = a
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(b.wrapping_mul(0xD1B5_4A32_D192_ED03))
        .wrapping_add(0x1234_5678_9ABC_DEF1);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
