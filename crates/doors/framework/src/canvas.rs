//! What a door draws on: a rectangle of the board's own screen buffer, reached only through
//! methods that sanitize.
//!
//! A door never gets the ratatui `Buffer` itself. [`Canvas::text`] runs every string through the
//! shared sanitizer (control characters, ANSI escapes, invisible format characters, the Nerd
//! Font private-use range and the reserved security-marker codepoints are neutralized) and clips
//! it to the canvas, so door output cannot move the cursor, change colors by escape, draw outside
//! its area or forge a security cue. Styles are the closed set [`DoorStyle`], mapped to colors by
//! the board's theme through a [`DoorPalette`]; none of them is the reserved marker style, and a
//! door cannot name a color of its own.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use russh_bbs_core::sanitize::SingleLine;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoorPalette {
    /// [`DoorStyle::Plain`].
    pub plain: Style,
    /// [`DoorStyle::Dim`].
    pub dim: Style,
    /// [`DoorStyle::Strong`].
    pub strong: Style,
    /// [`DoorStyle::Accent`].
    pub accent: Style,
    /// [`DoorStyle::Good`].
    pub good: Style,
    /// [`DoorStyle::Bad`].
    pub bad: Style,
    /// [`DoorStyle::Selected`].
    pub selected: Style,
}

impl DoorPalette {
    /// No colors at all: weight and reverse video only. For a member with color off, and for
    /// tests.
    #[must_use]
    pub fn mono() -> Self {
        Self {
            plain: Style::new(),
            dim: Style::new().add_modifier(Modifier::DIM),
            strong: Style::new().add_modifier(Modifier::BOLD),
            accent: Style::new().add_modifier(Modifier::BOLD),
            good: Style::new(),
            bad: Style::new().add_modifier(Modifier::BOLD),
            selected: Style::new().add_modifier(Modifier::REVERSED),
        }
    }

    fn style(&self, style: DoorStyle) -> Style {
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

/// A door's drawing surface. Coordinates are relative to the canvas, which is the area the board
/// gave the door; anything outside it is dropped.
pub struct Canvas<'a> {
    buf: &'a mut Buffer,
    area: Rect,
    palette: &'a DoorPalette,
}

impl<'a> Canvas<'a> {
    /// A canvas over `area` of `buf`. The server (and the test helpers) make these; a door is
    /// handed one. `area` is cut down to the part inside the buffer.
    pub fn new(buf: &'a mut Buffer, area: Rect, palette: &'a DoorPalette) -> Self {
        let area = area.intersection(buf.area);
        Self { buf, area, palette }
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

    /// Blank the whole canvas.
    pub fn clear(&mut self) {
        for y in 0..self.area.height {
            for x in 0..self.area.width {
                if let Some(cell) = self
                    .buf
                    .cell_mut((self.area.x + x, self.area.y + y))
                {
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
        let width = u16::try_from(
            ratatui::text::Line::raw(clean.as_str()).width(),
        )
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
                cell.set_style(style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert!(!buf[(1, 1)].modifier.contains(Modifier::BOLD), "outside the canvas");
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
