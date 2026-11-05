//! Chart widgets for the dashboard and the screens UI-4 and UI-10 build (UI-8): a **sparkline**,
//! a horizontal **bar chart**, a braille **line / area chart** and a **gauge**.
//!
//! **Palette roles, not colours.** A chart reads its colours from the style's palette roles
//! through [`ChartTheme::of`] (accent for the line and bars, muted for the track, success,
//! warning and error for a gauge's tone), so every style draws its own charts and every depth
//! degrades the way the rest of the UI does (D109, D117). A chart has no way to ask for bold or
//! reverse, so it can never draw the reserved marker style (D39).
//!
//! **Labels are content.** A chart draws numbers, which are chrome, and optional labels, which
//! may be member text (a board name, a handle). Every label goes through the shared sanitizer
//! ([`SingleLine`]) and is clipped to its column; a chart never draws a label raw.
//!
//! **Tiers.** Unicode draws eighth-blocks and braille; the ASCII tier draws `#`, `:` and `.`
//! (and `-` for a gauge's track), with the same layout, so a chart reads the same in text. Mono
//! draws the same glyphs with no colour.

#![allow(
    clippy::many_single_char_names,
    reason = "pixel and colour maths use x, y, w, h, r, g, b by convention"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "chart geometry is small, non-negative and bounded by the area"
)]
#![allow(
    dead_code,
    reason = "the widgets (UI-10) use Sparkline, Meter and LineChart; BarChart and Gauge wait for UI-4"
)]

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;
use russh_bbs_core::sanitize::SingleLine;

use super::canvas::{Braille, Paint, Rgb, mix};
use super::depth::Tint;
use crate::settings::Look;

/// Eighth blocks from one eighth to a full cell.
const EIGHTHS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
/// Left-aligned eighth blocks, for horizontal bars.
const ACROSS: [&str; 9] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];

/// What a gauge or bar is telling you.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    /// Ordinary: the accent colour.
    Plain,
    /// Healthy: the success colour.
    Ok,
    /// Getting full: the warning colour.
    Warn,
    /// Over the limit: the error colour.
    Bad,
}

impl Tone {
    /// The tone for a ratio with the usual thresholds: warn from `warn_at`, bad from `bad_at`.
    pub(crate) fn for_ratio(ratio: f64, warn_at: f64, bad_at: f64) -> Self {
        if ratio >= bad_at {
            Self::Bad
        } else if ratio >= warn_at {
            Self::Warn
        } else {
            Self::Ok
        }
    }
}

/// The colours and tier a chart draws with, taken from the style's palette roles.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ChartTheme {
    paint: Paint,
    plain: Style,
    ok: Style,
    warn: Style,
    bad: Style,
    track: Style,
    text: Style,
    line: Rgb,
    fill: Rgb,
}

impl ChartTheme {
    /// The theme for `look` (its palette, depth and tier).
    pub(crate) fn of(look: Look) -> Self {
        let palette = look.palette();
        let rgb = |tint: Tint, fallback: Rgb| tint.rgb().unwrap_or(fallback);
        let line = rgb(palette.accent, (120, 200, 255));
        let surface = rgb(palette.surface, (0, 0, 0));
        Self {
            paint: Paint::new(look.depth(), look.ascii()),
            plain: look.fg(palette.accent),
            ok: look.fg(palette.success),
            warn: look.fg(palette.warning),
            bad: look.fg(palette.error),
            track: look.muted(),
            text: look.fg(palette.text),
            line,
            fill: mix(surface, line, 0.35),
        }
    }

    fn tone(&self, tone: Tone) -> Style {
        match tone {
            Tone::Plain => self.plain,
            Tone::Ok => self.ok,
            Tone::Warn => self.warn,
            Tone::Bad => self.bad,
        }
    }

    /// This theme forced to the ASCII forms when `ascii` is set (the widgets' text form draws
    /// seven-bit characters whatever the member's tier).
    #[must_use]
    pub(crate) const fn with_ascii(mut self, ascii: bool) -> Self {
        self.paint.ascii = self.paint.ascii || ascii;
        self
    }

    const fn ascii(&self) -> bool {
        self.paint.ascii
    }
}

/// A label made safe to draw: sanitized as one line and clipped to `width` columns.
fn clean(label: &str, width: usize) -> String {
    let text = SingleLine::new(80).sanitize(label);
    text.as_str().chars().take(width).collect()
}

fn put(buf: &mut Buffer, x: u16, y: u16, symbol: &str, style: Style) {
    if buf.area.contains((x, y).into()) {
        buf[(x, y)].set_symbol(symbol).set_style(style);
    }
}

fn put_text(buf: &mut Buffer, x: u16, y: u16, text: &str, max: u16, style: Style) {
    for (i, ch) in text.chars().take(usize::from(max)).enumerate() {
        let mut utf8 = [0_u8; 4];
        put(buf, x + i as u16, y, ch.encode_utf8(&mut utf8), style);
    }
}

/// A sparkline: one column per value, the newest on the right, as tall as the area.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sparkline<'a> {
    data: &'a [u64],
    max: Option<u64>,
    theme: ChartTheme,
    tone: Tone,
}

impl<'a> Sparkline<'a> {
    /// A sparkline of `data`, scaled to its own maximum.
    pub(crate) const fn new(data: &'a [u64], theme: ChartTheme) -> Self {
        Self {
            data,
            max: None,
            theme,
            tone: Tone::Plain,
        }
    }

    /// Scale to `max` instead of the data's own maximum (so two sparklines compare).
    #[must_use]
    pub(crate) const fn max(mut self, max: u64) -> Self {
        self.max = Some(max);
        self
    }

    /// Draw in `tone`.
    #[must_use]
    pub(crate) const fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

impl Widget for Sparkline<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        let shown = &self.data[self.data.len().saturating_sub(usize::from(area.width))..];
        let max = self
            .max
            .unwrap_or_else(|| shown.iter().copied().max().unwrap_or(0))
            .max(1);
        let style = self.theme.tone(self.tone);
        // Right-aligned: the newest value is the last column.
        let offset = area.width - shown.len() as u16;
        for (i, value) in shown.iter().enumerate() {
            let x = area.x + offset + i as u16;
            let eighths = (u128::from((*value).min(max)) * u128::from(area.height) * 8
                / u128::from(max)) as u64;
            if eighths == 0 {
                let low = if self.theme.ascii() { "_" } else { "▁" };
                put(buf, x, area.y + area.height - 1, low, self.theme.track);
                continue;
            }
            for row in 0..area.height {
                let rem = eighths.saturating_sub(u64::from(row) * 8).min(8) as usize;
                if rem == 0 {
                    break;
                }
                let y = area.y + area.height - 1 - row;
                put(buf, x, y, sparkline_glyph(rem, self.theme.ascii()), style);
            }
        }
    }
}

fn sparkline_glyph(eighths: usize, ascii: bool) -> &'static str {
    if ascii {
        match eighths {
            1..=2 => ".",
            3..=4 => ":",
            5..=6 => "|",
            _ => "#",
        }
    } else {
        EIGHTHS[eighths.min(8)]
    }
}

/// A horizontal bar chart: one row per entry, a label, the bar and the value.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BarChart<'a> {
    rows: &'a [(&'a str, u64)],
    max: Option<u64>,
    theme: ChartTheme,
}

impl<'a> BarChart<'a> {
    /// A bar chart of `rows` (label, value), scaled to the largest value.
    pub(crate) const fn new(rows: &'a [(&'a str, u64)], theme: ChartTheme) -> Self {
        Self {
            rows,
            max: None,
            theme,
        }
    }

    /// Scale to `max` instead of the largest value.
    #[must_use]
    pub(crate) const fn max(mut self, max: u64) -> Self {
        self.max = Some(max);
        self
    }
}

impl Widget for BarChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.width < 8 || area.is_empty() {
            return;
        }
        let rows = &self.rows[..self.rows.len().min(usize::from(area.height))];
        let max = self
            .max
            .unwrap_or_else(|| rows.iter().map(|r| r.1).max().unwrap_or(0))
            .max(1);
        let labels: Vec<String> = rows
            .iter()
            .map(|(label, _)| clean(label, usize::from(area.width / 3)))
            .collect();
        let label_w = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
        let value_w = rows
            .iter()
            .map(|r| r.1.to_string().len())
            .max()
            .unwrap_or(1) as u16;
        let bar_w = area.width.saturating_sub(label_w + value_w + 2).max(1);
        for (i, ((_, value), label)) in rows.iter().zip(&labels).enumerate() {
            let y = area.y + i as u16;
            put_text(buf, area.x, y, label, label_w, self.theme.text);
            let bar_x = area.x + label_w + 1;
            let eighths =
                (u128::from((*value).min(max)) * u128::from(bar_w) * 8 / u128::from(max)) as u64;
            for cell in 0..bar_w {
                let rem = eighths.saturating_sub(u64::from(cell) * 8).min(8) as usize;
                let (symbol, style) = if rem == 0 {
                    (if self.theme.ascii() { "-" } else { "·" }, self.theme.track)
                } else if self.theme.ascii() {
                    (if rem == 8 { "#" } else { "=" }, self.theme.plain)
                } else {
                    (ACROSS[rem], self.theme.plain)
                };
                put(buf, bar_x + cell, y, symbol, style);
            }
            let text = value.to_string();
            let pad = value_w.saturating_sub(text.len() as u16);
            put_text(
                buf,
                bar_x + bar_w + 1 + pad,
                y,
                &text,
                value_w,
                self.theme.text,
            );
        }
    }
}

/// A line chart in braille (2x4 dots per cell), optionally filled below the line (an area
/// chart). The newest value is on the right.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LineChart<'a> {
    data: &'a [f64],
    range: Option<(f64, f64)>,
    filled: bool,
    theme: ChartTheme,
}

impl<'a> LineChart<'a> {
    /// A line chart of `data`, scaled to its own minimum and maximum.
    pub(crate) const fn new(data: &'a [f64], theme: ChartTheme) -> Self {
        Self {
            data,
            range: None,
            filled: false,
            theme,
        }
    }

    /// Fill below the line (an area chart).
    #[must_use]
    pub(crate) const fn filled(mut self) -> Self {
        self.filled = true;
        self
    }

    /// Scale to `min..=max` instead of the data's own range.
    #[must_use]
    pub(crate) const fn range(mut self, min: f64, max: f64) -> Self {
        self.range = Some((min, max));
        self
    }
}

impl Widget for LineChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() || self.data.is_empty() {
            return;
        }
        let mut canvas = Braille::new(area.width, area.height);
        let (dw, dh) = (
            i32::from(canvas.dot_width()),
            i32::from(canvas.dot_height()),
        );
        let finite = self.data.iter().copied().filter(|v| v.is_finite());
        let (lo, hi) = self.range.unwrap_or_else(|| {
            finite.fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
        });
        let (lo, hi) = if lo.is_finite() && hi.is_finite() && hi > lo {
            (lo, hi)
        } else {
            (lo.min(0.0), lo.max(1.0))
        };
        let n = self.data.len();
        let y_of = |v: f64| -> i32 {
            let v = if v.is_finite() { v } else { lo };
            let unit = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
            ((1.0 - unit) * f64::from(dh - 1)).round() as i32
        };
        let point = |dx: i32| -> (i32, f64) {
            let idx = if n == 1 || dw == 1 {
                n - 1
            } else {
                ((f64::from(dx) / f64::from(dw - 1)) * (n - 1) as f64).round() as usize
            };
            (dx, self.data[idx.min(n - 1)])
        };
        let mut previous: Option<(i32, i32)> = None;
        for dx in 0..dw {
            let (x, value) = point(dx);
            let y = y_of(value);
            if self.filled {
                canvas.fill_down(x, y, self.theme.fill);
            }
            if let Some(prev) = previous {
                canvas.line(prev, (x, y), self.theme.line);
            } else {
                canvas.set(x, y, self.theme.line);
            }
            previous = Some((x, y));
        }
        canvas.render(buf, area, self.theme.paint);
    }
}

/// Fill one row of `area` with a horizontal bar of `eighths` eighth-cells: full blocks, one
/// partial block, then the track. ASCII draws `#`, `=` and `-`. Shared by [`Gauge`] and
/// [`Meter`] so there is one bar.
fn paint_bar(buf: &mut Buffer, area: Rect, eighths: u64, style: Style, theme: ChartTheme) {
    for cell in 0..area.width {
        let rem = eighths.saturating_sub(u64::from(cell) * 8).min(8) as usize;
        let (symbol, st) = match (rem, theme.ascii()) {
            (0, true) => ("-", theme.track),
            (0, false) => ("░", theme.track),
            (8, true) => ("#", style),
            (_, true) => ("=", style),
            (r, false) => (ACROSS[r], style),
        };
        put(buf, area.x + cell, area.y, symbol, st);
    }
}

/// A bare one-row bar filled to `part` of `whole` across the whole area, with no label and no
/// figure: the caller draws those beside it (a poll option's votes, a mail count, the load).
/// `part` beyond `whole` is full and a `whole` of zero is empty. Unicode ends the fill in a
/// partial block; ASCII rounds to whole cells (`#` and `-`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Meter {
    part: u64,
    whole: u64,
    tone: Tone,
    theme: ChartTheme,
}

impl Meter {
    /// A meter at `part` of `whole`.
    pub(crate) const fn new(part: u64, whole: u64, theme: ChartTheme) -> Self {
        Self {
            part,
            whole,
            tone: Tone::Plain,
            theme,
        }
    }

    /// Draw in `tone`.
    #[must_use]
    pub(crate) const fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

impl Widget for Meter {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        let whole = u128::from(self.whole.max(1));
        let part = u128::from(self.part.min(self.whole));
        let mut eighths = ((part * u128::from(area.width) * 8 + whole / 2) / whole) as u64;
        if self.theme.ascii() {
            // ASCII has no partial cell here: `#` for a cell at least half full, else the track.
            eighths = (eighths + 4) / 8 * 8;
        }
        paint_bar(
            buf,
            Rect::new(area.x, area.y, area.width, 1),
            eighths,
            self.theme.tone(self.tone),
            self.theme,
        );
    }
}

/// A one-row gauge: a bar, then the percentage.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Gauge<'a> {
    ratio: f64,
    label: Option<&'a str>,
    tone: Tone,
    theme: ChartTheme,
}

impl<'a> Gauge<'a> {
    /// A gauge at `ratio` (0 to 1; anything else is clamped, and not-a-number is 0).
    pub(crate) fn new(ratio: f64, theme: ChartTheme) -> Self {
        Self {
            ratio: if ratio.is_finite() {
                ratio.clamp(0.0, 1.0)
            } else {
                0.0
            },
            label: None,
            tone: Tone::Plain,
            theme,
        }
    }

    /// A label before the bar.
    #[must_use]
    pub(crate) const fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    /// Draw in `tone`.
    #[must_use]
    pub(crate) const fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

impl Widget for Gauge<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.width < 6 || area.is_empty() {
            return;
        }
        let percent = (self.ratio * 100.0).round() as u16;
        let value = format!("{percent:>3}%");
        let mut x = area.x;
        if let Some(label) = self.label {
            let text = clean(label, usize::from(area.width / 3));
            let used = text.chars().count() as u16;
            put_text(buf, x, area.y, &text, used, self.theme.text);
            x += used + 1;
        }
        let bar_w = (area.x + area.width).saturating_sub(x + 5).max(1);
        let eighths = (self.ratio * f64::from(bar_w) * 8.0).round() as u64;
        paint_bar(
            buf,
            Rect::new(x, area.y, bar_w, 1),
            eighths,
            self.theme.tone(self.tone),
            self.theme,
        );
        put_text(buf, x + bar_w + 1, area.y, &value, 4, self.theme.text);
    }
}

#[cfg(test)]
mod tests;
