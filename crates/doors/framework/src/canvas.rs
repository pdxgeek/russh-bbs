//! What a door draws on: its own off-screen buffer, the size of the area the board gave it.
//!
//! A door never sees the screen. Two ways to draw, both into the door's own buffer:
//!
//! * [`Canvas::text`] and friends (the original path): every string goes through the shared
//!   sanitizer and is clipped to the canvas.
//! * [`Canvas::frame`], a [`DoorFrame`] that mirrors ratatui's `Frame`: any `Widget` or
//!   `StatefulWidget` (blocks, paragraphs, lists, tables, tabs, gauges, sparklines, bar charts,
//!   charts, the braille and half-block `Canvas`, scrollbars, popups, with the door's own
//!   layouts) is rendered in place. Widgets write what they like.
//!
//! What a widget writes is not trusted. The board composites the door's buffer onto the screen
//! with [`DoorSurface::composite`](crate::DoorSurface::composite), which cleans **every cell**
//! (sanitizer, no reserved marker style, colours degraded to the member's depth, clipped to the
//! door's area); see `surface.rs` for the rules. So drawing is as free as ratatui, and what
//! reaches the screen is safe whichever path drew it.
//!
//! Styles are the theme's roles ([`DoorStyle`], mapped to colours by the board's theme through a
//! [`DoorPalette`]) and, in a frame, any `Style` the door builds itself, truecolor included.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{StatefulWidget, Widget};
use russh_bbs_core::limits::Day;
use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::store::UnixSeconds;

/// The longest string one `text` call draws, in characters, whatever its width.
const MAX_TEXT_CHARS: usize = 400;

/// The looks a door may ask for. The board decides what each one is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DoorStyle {
    /// Ordinary text.
    Plain,
    /// Less important text.
    Dim,
    /// Emphasis.
    Strong,
    /// A heading or a key hint.
    Accent,
    /// Good news.
    Good,
    /// Bad news or an error.
    Bad,
    /// The row under the cursor.
    Selected,
}

/// The board's mapping from [`DoorStyle`] to a ratatui style, built from the member's theme.
/// The server builds it; a door only ever uses [`DoorStyle`].
///
/// **No style can be the reserved security-marker style** (bold plus reverse video, D39): the
/// constructor removes bold from any style that is also reversed, and a cell takes exactly one
/// style (it is replaced, never merged with another), so a door cannot combine two of its styles
/// into the reserved one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoorPalette {
    plain: Style,
    dim: Style,
    strong: Style,
    accent: Style,
    good: Style,
    bad: Style,
    selected: Style,
}

/// `style` with the reserved combination broken: reverse video wins, bold goes.
fn unreserved(style: Style) -> Style {
    if style
        .add_modifier
        .contains(Modifier::BOLD | Modifier::REVERSED)
    {
        style.remove_modifier(Modifier::BOLD)
    } else {
        style
    }
}

impl DoorPalette {
    /// A palette from the seven styles, in the order of [`DoorStyle`].
    #[must_use]
    pub fn new(styles: [Style; 7]) -> Self {
        let [plain, dim, strong, accent, good, bad, selected] = styles.map(unreserved);
        Self {
            plain,
            dim,
            strong,
            accent,
            good,
            bad,
            selected,
        }
    }

    /// No colors at all: weight and reverse video only. For a member with color off, and for
    /// tests.
    #[must_use]
    pub fn mono() -> Self {
        Self::new([
            Style::new(),
            Style::new().add_modifier(Modifier::DIM),
            Style::new().add_modifier(Modifier::BOLD),
            Style::new().add_modifier(Modifier::BOLD),
            Style::new(),
            Style::new().add_modifier(Modifier::BOLD),
            Style::new().add_modifier(Modifier::REVERSED),
        ])
    }

    pub(crate) fn style(&self, style: DoorStyle) -> Style {
        match style {
            DoorStyle::Plain => self.plain,
            DoorStyle::Dim => self.dim,
            DoorStyle::Strong => self.strong,
            DoorStyle::Accent => self.accent,
            DoorStyle::Good => self.good,
            DoorStyle::Bad => self.bad,
            DoorStyle::Selected => self.selected,
        }
    }
}

/// Give `cell` exactly `style`: colors and modifiers replaced, not merged.
fn restyle(cell: &mut ratatui::buffer::Cell, style: Style) {
    cell.reset();
    cell.set_style(style);
}

/// A door's drawing surface. Coordinates are relative to the canvas, which is the area the board
/// gave the door; anything outside it is dropped.
pub struct Canvas<'a> {
    buf: &'a mut Buffer,
    area: Rect,
    palette: &'a DoorPalette,
    pub(crate) now: UnixSeconds,
}

impl<'a> Canvas<'a> {
    /// A canvas over `area` of `buf`. [`DoorSurface`](crate::DoorSurface) makes these (the
    /// server and the test helpers go through it); a door is handed one. `area` is cut down to
    /// the part inside the buffer.
    pub(crate) fn new(buf: &'a mut Buffer, area: Rect, palette: &'a DoorPalette) -> Self {
        let area = area.intersection(buf.area);
        Self {
            buf,
            area,
            palette,
            now: UnixSeconds::from_raw(0),
        }
    }

    /// The board's clock at the moment of drawing (injected, so a replay or a test sets it).
    /// A screen draws "today" from this, not from the day its session opened, so a session left
    /// open across midnight shows the new day. A canvas a test makes by hand (not through a
    /// [`Driver`](crate::host::Driver) or [`replay::draw_to_text`](crate::replay::draw_to_text))
    /// reports the epoch.
    #[must_use]
    pub const fn now(&self) -> UnixSeconds {
        self.now
    }

    /// The current UTC day when drawing (see [`Canvas::now`]).
    #[must_use]
    pub const fn today(&self) -> Day {
        Day::of(self.now)
    }

    /// Columns available.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.area.width
    }

    /// Rows available.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.area.height
    }

    /// The style the theme gives `style`, for building a widget (`Block::style(...)`,
    /// `Gauge::gauge_style(...)`).
    #[must_use]
    pub fn style(&self, style: DoorStyle) -> Style {
        self.palette.style(style)
    }

    /// A [`DoorFrame`] over this canvas, to render ratatui widgets into it.
    pub fn frame(&mut self) -> DoorFrame<'_> {
        DoorFrame {
            buf: &mut *self.buf,
            area: self.area,
            palette: self.palette,
        }
    }

    /// Blank the whole canvas.
    pub fn clear(&mut self) {
        for y in 0..self.area.height {
            for x in 0..self.area.width {
                if let Some(cell) = self.buf.cell_mut((self.area.x + x, self.area.y + y)) {
                    cell.reset();
                }
            }
        }
    }

    /// Draw `text` with its first character at column `x` of row `y`, sanitized and clipped to
    /// the canvas. Returns how many columns were drawn.
    pub fn text(&mut self, x: u16, y: u16, text: &str, style: DoorStyle) -> u16 {
        if y >= self.area.height || x >= self.area.width {
            return 0;
        }
        let clean = SingleLine::new(MAX_TEXT_CHARS).sanitize(text);
        let room = self.area.width - x;
        let (end, _) = self.buf.set_stringn(
            self.area.x + x,
            self.area.y + y,
            clean.as_str(),
            usize::from(room),
            self.palette.style(style),
        );
        end.saturating_sub(self.area.x + x)
    }

    /// Draw `text` centered on row `y`.
    pub fn center(&mut self, y: u16, text: &str, style: DoorStyle) {
        let clean = SingleLine::new(MAX_TEXT_CHARS).sanitize(text);
        let width = u16::try_from(ratatui::text::Line::raw(clean.as_str()).width())
            .unwrap_or(u16::MAX)
            .min(self.area.width);
        let x = (self.area.width - width) / 2;
        self.text(x, y, clean.as_str(), style);
    }

    /// Give every cell in row `y` the style, so a row can be highlighted.
    pub fn style_row(&mut self, y: u16, style: DoorStyle) {
        if y >= self.area.height {
            return;
        }
        let style = self.palette.style(style);
        for x in 0..self.area.width {
            if let Some(cell) = self.buf.cell_mut((self.area.x + x, self.area.y + y)) {
                // Keep the character, replace the style.
                let symbol = cell.symbol().to_owned();
                restyle(cell, style);
                cell.set_symbol(&symbol);
            }
        }
    }
}

/// Renders ratatui widgets into a door's canvas. It mirrors ratatui's `Frame`: coordinates are
/// relative to the door's area (`area()` starts at 0, 0), and a widget's area is cut down to the
/// part inside the door's area, so a widget given too large a rectangle draws what fits.
///
/// The widget writes into the door's own buffer; what it writes is cleaned when the board
/// composites the buffer (see the module documentation), so a custom `Widget` that puts escape
/// sequences, private-use glyphs or the reserved marker style in a cell shows none of them.
pub struct DoorFrame<'a> {
    buf: &'a mut Buffer,
    area: Rect,
    palette: &'a DoorPalette,
}

impl DoorFrame<'_> {
    /// The whole drawing area: `0, 0` and the door's width and height. Lay out from this.
    #[must_use]
    pub const fn area(&self) -> Rect {
        Rect::new(0, 0, self.area.width, self.area.height)
    }

    /// The style the theme gives `style`.
    #[must_use]
    pub fn style(&self, style: DoorStyle) -> Style {
        self.palette.style(style)
    }

    /// `rect` (relative to the door's area) as a rectangle of the buffer, cut down to the door's
    /// area; `None` when nothing of it is inside.
    fn place(&self, rect: Rect) -> Option<Rect> {
        let x = self.area.x.checked_add(rect.x)?;
        let y = self.area.y.checked_add(rect.y)?;
        let moved = Rect::new(x, y, rect.width, rect.height);
        let inside = moved.intersection(self.area);
        (inside.width > 0 && inside.height > 0).then_some(inside)
    }

    /// Render `widget` in `rect`.
    pub fn render_widget<W: Widget>(&mut self, widget: W, rect: Rect) {
        if let Some(rect) = self.place(rect) {
            widget.render(rect, self.buf);
        }
    }

    /// Render `widget` in `rect` with its `state` (a list's selection, a table's scroll
    /// position, a scrollbar's offset).
    pub fn render_stateful_widget<W: StatefulWidget>(
        &mut self,
        widget: W,
        rect: Rect,
        state: &mut W::State,
    ) {
        if let Some(rect) = self.place(rect) {
            widget.render(rect, self.buf, state);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;

    fn draw(f: impl FnOnce(&mut Canvas<'_>)) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        let palette = DoorPalette::mono();
        // The canvas is the middle of the buffer, so drawing outside it is visible.
        let mut canvas = Canvas::new(&mut buf, Rect::new(2, 1, 10, 2), &palette);
        f(&mut canvas);
        buf
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_owned())
            .collect()
    }

    #[test]
    fn text_is_clipped_to_the_canvas_and_never_leaves_it() {
        let buf = draw(|c| {
            c.text(0, 0, "abcdefghijklmnopqrstuvwxyz", DoorStyle::Plain);
            c.text(8, 1, "xyz123", DoorStyle::Plain);
            c.text(0, 2, "below", DoorStyle::Plain);
            c.text(10, 0, "right", DoorStyle::Plain);
        });
        assert_eq!(row(&buf, 0).trim_end(), "");
        assert_eq!(row(&buf, 1).trim_end(), "  abcdefghij");
        assert_eq!(row(&buf, 2).trim_end(), "          xy");
        assert_eq!(row(&buf, 3).trim_end(), "");
    }

    #[test]
    fn hostile_text_is_neutralized_before_it_reaches_the_buffer() {
        let marker = russh_bbs_core::sanitize::RESERVED_CHROME[0];
        let hostile = format!("\u{1b}[31mred\u{1b}[0m\u{7}\u{202e}x\u{e000}{marker}\nZ");
        let buf = draw(|c| {
            c.text(0, 0, &hostile, DoorStyle::Plain);
        });
        let line = row(&buf, 1);
        for cell in buf.content() {
            let symbol = cell.symbol();
            assert!(
                symbol.chars().all(|ch| !ch.is_control()
                    && ch != '\u{202e}'
                    && ch != '\u{e000}'
                    && ch != marker),
                "{symbol:?} in {line:?}"
            );
        }
        assert!(line.contains("red"), "{line:?}");
        assert!(!line.contains('\u{1b}'));
    }

    #[test]
    fn styles_come_from_the_palette_and_a_door_has_no_other_way_to_color() {
        let buf = draw(|c| {
            c.text(0, 0, "ab", DoorStyle::Strong);
            c.style_row(1, DoorStyle::Selected);
        });
        assert!(buf[(2, 1)].modifier.contains(Modifier::BOLD));
        assert!(buf[(2, 2)].modifier.contains(Modifier::REVERSED));
        assert!(
            !buf[(1, 1)].modifier.contains(Modifier::BOLD),
            "outside the canvas"
        );
    }

    #[test]
    fn no_style_and_no_combination_of_styles_is_the_reserved_marker_style() {
        let reserved = Modifier::BOLD | Modifier::REVERSED;
        // A theme that tried to make a style the reserved one gets it broken.
        let palette = DoorPalette::new([Style::new().add_modifier(reserved); 7]);
        for style in [
            DoorStyle::Plain,
            DoorStyle::Dim,
            DoorStyle::Strong,
            DoorStyle::Accent,
            DoorStyle::Good,
            DoorStyle::Bad,
            DoorStyle::Selected,
        ] {
            assert!(
                !palette.style(style).add_modifier.contains(reserved),
                "{style:?}"
            );
        }
        // Drawing bold text and then highlighting its row replaces the style, it does not add
        // reverse video to bold.
        let buf = draw(|c| {
            c.text(0, 0, "ab", DoorStyle::Strong);
            c.style_row(0, DoorStyle::Selected);
        });
        let cell = &buf[(2, 1)];
        assert_eq!(cell.symbol(), "a");
        assert!(
            cell.modifier.contains(Modifier::REVERSED) && !cell.modifier.contains(Modifier::BOLD)
        );
    }

    #[test]
    fn center_and_clear_stay_inside() {
        let buf = draw(|c| {
            c.center(0, "mid", DoorStyle::Plain);
            c.center(1, "this line is far too long", DoorStyle::Plain);
        });
        assert!(row(&buf, 1).contains("mid"));
        assert_eq!(row(&buf, 0).trim_end(), "");
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
        let palette = DoorPalette::mono();
        let mut canvas = Canvas::new(&mut buf, Rect::new(0, 0, 6, 2), &palette);
        canvas.text(0, 0, "abc", DoorStyle::Plain);
        canvas.clear();
        assert_eq!(row(&buf, 0).trim_end(), "");
    }
}
