//! The door's own off-screen buffer, and the compositing that puts it on the member's screen.
//!
//! A door draws any ratatui widget into a [`DoorSurface`] (through a [`Canvas`] and its
//! [`DoorFrame`](crate::DoorFrame)): a buffer exactly the size of the area the board gave it,
//! which the door never sees the screen behind. The board then calls
//! [`DoorSurface::composite`], which builds a **new** buffer, cell by cell, from only the parts
//! of each door cell that are safe, and returns it as a [`CleanBuffer`]. Nothing the door put in
//! a cell survives unless it is copied here, so the rules below hold for every widget and every
//! custom `Widget` a door writes, with no cooperation from the door:
//!
//! 1. **The symbol goes through the shared sanitizer, whole.** A cell's symbol may be several
//!    characters (a grapheme); it is checked as one string, so an escape sequence, a C1 control,
//!    an invisible format character, a Nerd Font private-use glyph or one of the reserved
//!    security-marker codepoints (`sanitize::RESERVED_CHROME`) cannot get through by being split
//!    across a cluster. A cell that ends up empty is a blank. A cell is at most two columns wide.
//! 2. **The reserved marker style is removed.** A cell that is bold *and* reversed (SGR `1;7`,
//!    D39) loses its bold. Hidden and blinking text is removed too: hidden text cannot be read
//!    by the member, and blinking imitates an alert.
//! 3. **Colours are brought down to the member's depth** ([`ColorDepth`]).
//! 4. **Output is the door's area and nothing else.** The result has exactly the width and height
//!    the surface was made with, however the door's buffer was changed after it was made. The
//!    area never contains the security-marker cell, the status row or the box around the door:
//!    the board draws those itself.
//! 5. **Only symbol, foreground, background and the ordinary modifiers are copied.** A cell's
//!    "skip" flag, its forced width and its underline colour are dropped, so a door cannot make
//!    the terminal skip over screen cells it does not own or mis-measure a row.
//!
//! What this does not stop: a door can still draw a *lookalike* of a marker (the lookalike
//! brackets and glyphs D45 lists, or black-on-white bold text that resembles reverse video), as
//! any player's text can. A real marker is told apart by the reserved style and a chrome-only
//! screen position, and a door's area is never chrome (D39, D45).
//!
//! Seams for later slices: effects (UI-8) run on the surface after the door has drawn and
//! *before* [`DoorSurface::composite`], so they are cleaned like everything else; mouse events
//! (UI-9) are a new default-bodied method on `DoorSession`, which breaks no door.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use russh_bbs_core::sanitize::SingleLine;
use tracing::warn;

use crate::ascii::ascii_glyph;
use crate::canvas::{Canvas, DoorPalette, DoorStyle};
use crate::color::ColorDepth;

/// The most characters one cell's symbol may have before the rest is dropped. Real clusters (an
/// emoji with a skin tone and a joiner, a letter with its marks) are well under this.
const MAX_SYMBOL_CHARS: usize = 12;

/// How many distinct non-ASCII symbols one composite remembers. A hostile door can make every
/// cell different; past this, symbols are cleaned afresh each time instead of remembered.
const MEMO_CAP: usize = 2048;

/// What the board knows about the member's screen when it composites a door.
///
/// Built with [`Composite::new`]; more fields may be added (effects, UI-8), so it cannot be
/// written as a struct literal outside this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Composite {
    /// The member's colour depth.
    pub depth: ColorDepth,
    /// Whether the member is in the ASCII icon tier: box drawing, blocks, braille and shapes are
    /// replaced by ASCII stand-ins.
    pub ascii_icons: bool,
}

impl Composite {
    /// Options for a member with `depth` colours, in the ASCII icon tier or not.
    #[must_use]
    pub const fn new(depth: ColorDepth, ascii_icons: bool) -> Self {
        Self { depth, ascii_icons }
    }
}

/// Whether [`DoorSurface::draw`] got to the end of the door's drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drew {
    /// The door drew its screen.
    Done,
    /// The door's drawing panicked (a widget given an area it cannot use, a bug in the door).
    /// The surface now shows a one-line notice instead.
    Failed,
}

/// The off-screen buffer a door draws into.
#[derive(Debug, Clone)]
pub struct DoorSurface {
    buf: Buffer,
    width: u16,
    height: u16,
    palette: DoorPalette,
}

impl DoorSurface {
    /// A blank surface of `width` by `height` cells.
    #[must_use]
    pub fn new(width: u16, height: u16, palette: DoorPalette) -> Self {
        Self {
            buf: Buffer::empty(Rect::new(0, 0, width, height)),
            width,
            height,
            palette,
        }
    }

    /// Columns.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.width
    }

    /// Rows.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.height
    }

    /// A canvas over the whole surface, for a door's `render`.
    pub fn canvas(&mut self) -> Canvas<'_> {
        Canvas::new(
            &mut self.buf,
            Rect::new(0, 0, self.width, self.height),
            &self.palette,
        )
    }

    /// Let `draw` paint the surface. A panic inside it is caught: the surface is blanked and
    /// shows a notice, and [`Drew::Failed`] says so, so one door's bug in a widget cannot take
    /// the member's session down with it.
    pub fn draw(&mut self, draw: impl FnOnce(&mut Canvas<'_>)) -> Drew {
        let outcome = catch_unwind(AssertUnwindSafe(|| draw(&mut self.canvas())));
        if outcome.is_ok() {
            return Drew::Done;
        }
        warn!("a door panicked while drawing; its screen is replaced by a notice");
        self.buf = Buffer::empty(Rect::new(0, 0, self.width, self.height));
        let mut canvas = self.canvas();
        canvas.text(0, 0, "This door had a problem drawing.", DoorStyle::Bad);
        Drew::Failed
    }

    /// Build the screen's view of the door: a new buffer, clean in every cell (see the module
    /// documentation for the rules).
    #[must_use]
    pub fn composite(&self, options: Composite) -> CleanBuffer {
        let area = Rect::new(0, 0, self.width, self.height);
        let mut out = Buffer::empty(area);
        let mut cleaner = Cleaner::new(options.ascii_icons);
        for y in 0..self.height {
            let mut x = 0;
            while x < self.width {
                // The door's buffer may have been resized or moved by a custom widget; a cell it
                // no longer has is a blank.
                let Some(cell) = self.buf.cell((x, y)) else {
                    x += 1;
                    continue;
                };
                let Some(target) = out.cell_mut((x, y)) else {
                    x += 1;
                    continue;
                };
                let mut width = cleaner.put(cell.symbol(), target);
                // A wide symbol in the last column has no room: it becomes a blank.
                if width == 2 && x + 1 >= self.width {
                    target.set_symbol(" ");
                    width = 1;
                }
                target.set_style(clean_style(cell, options.depth));
                // A wide symbol covers the next cell; whatever the door put there is gone.
                x += u16::try_from(width.max(1)).unwrap_or(1);
            }
        }
        CleanBuffer { buf: out }
    }
}

/// The style of `cell` with the reserved combination, hidden text and blinking removed and the
/// colours brought down to `depth`. The underline colour and the skip flags are not copied.
fn clean_style(cell: &Cell, depth: ColorDepth) -> Style {
    let mut modifier = cell.modifier;
    modifier.remove(Modifier::HIDDEN | Modifier::SLOW_BLINK | Modifier::RAPID_BLINK);
    // Reverse video wins and bold goes: bold plus reverse is the security-marker style (D39).
    if modifier.contains(Modifier::BOLD | Modifier::REVERSED) {
        modifier.remove(Modifier::BOLD);
    }
    Style::new()
        .fg(depth.degrade(cell.fg))
        .bg(depth.degrade(cell.bg))
        .add_modifier(modifier)
}

/// A cleaned symbol and the columns it takes (1 or 2).
struct Cleaned {
    text: Box<str>,
    width: usize,
}

/// Cleans symbols, remembering the answers: a screen is mostly a few dozen distinct symbols.
struct Cleaner {
    ascii_icons: bool,
    memo: HashMap<Box<str>, Cleaned>,
}

impl Cleaner {
    fn new(ascii_icons: bool) -> Self {
        Self {
            ascii_icons,
            memo: HashMap::new(),
        }
    }

    /// Set `target`'s symbol to the clean form of `raw`; the columns it takes.
    fn put(&mut self, raw: &str, target: &mut Cell) -> usize {
        // A single printable ASCII character needs no cleaning: the common case, no allocation.
        if let [byte] = raw.as_bytes()
            && (0x20..0x7f).contains(byte)
        {
            target.set_symbol(raw);
            return 1;
        }
        if let Some(known) = self.memo.get(raw) {
            target.set_symbol(&known.text);
            return known.width;
        }
        let cleaned = self.clean(raw);
        target.set_symbol(&cleaned.text);
        let width = cleaned.width;
        if self.memo.len() < MEMO_CAP {
            self.memo.insert(raw.into(), cleaned);
        }
        width
    }

    fn clean(&self, raw: &str) -> Cleaned {
        let sanitized = SingleLine::new(MAX_SYMBOL_CHARS)
            .sanitize(raw)
            .into_string();
        let text: String = if self.ascii_icons {
            // The stand-ins are ASCII, so a joiner that followed a replaced character no longer
            // follows a non-ASCII one: sanitize again so the result is stable.
            let mapped: String = sanitized.chars().map(ascii_glyph).collect();
            SingleLine::new(MAX_SYMBOL_CHARS)
                .sanitize(&mapped)
                .into_string()
        } else {
            sanitized
        };
        let width = Span::raw(text.as_str()).width();
        match width {
            1 | 2 => Cleaned {
                text: text.into(),
                width,
            },
            // Nothing visible (empty, or only marks with no base): a blank.
            0 => blank(),
            // More than two columns is not one cell's worth: keep the first character if it
            // fits.
            _ => text
                .chars()
                .next()
                .map(|first| first.to_string())
                .filter(|first| matches!(Span::raw(first.as_str()).width(), 1 | 2))
                .map_or_else(blank, |first| {
                    let width = Span::raw(first.as_str()).width();
                    Cleaned {
                        text: first.into(),
                        width,
                    }
                }),
        }
    }
}

fn blank() -> Cleaned {
    Cleaned {
        text: " ".into(),
        width: 1,
    }
}

/// A door's screen after compositing: every cell clean, exactly the door's area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanBuffer {
    buf: Buffer,
}

impl CleanBuffer {
    /// The cleaned buffer.
    #[must_use]
    pub const fn buffer(&self) -> &Buffer {
        &self.buf
    }

    /// The cleaned buffer, by value.
    #[must_use]
    pub fn into_buffer(self) -> Buffer {
        self.buf
    }

    /// Columns.
    #[must_use]
    pub const fn width(&self) -> u16 {
        self.buf.area.width
    }

    /// Rows.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.buf.area.height
    }

    /// The rows as plain text (symbols only), with trailing blanks removed. For tests and for
    /// screen-reader views.
    #[must_use]
    pub fn rows_text(&self) -> Vec<String> {
        self.rows()
            .map(|cells| {
                let mut row = String::new();
                for cell in cells {
                    row.push_str(cell.symbol());
                }
                row.trim_end().to_owned()
            })
            .collect()
    }

    /// The rows as styled lines for the board's own box: runs of cells with one style become one
    /// span, the cell a wide symbol covers is skipped, and trailing cells that are blank with no
    /// style are dropped so a row never wraps. A blank cell that has a background colour or
    /// reverse video is kept, because it is drawn (a gauge's fill).
    #[must_use]
    pub fn to_lines(&self) -> Vec<Line<'static>> {
        self.rows()
            .map(|cells| {
                let end = cells
                    .iter()
                    .rposition(|cell| !is_plain_blank(cell))
                    .map_or(0, |at| at + 1);
                let mut spans: Vec<Span<'static>> = Vec::new();
                let mut run = String::new();
                let mut run_style = Style::new();
                for cell in &cells[..end] {
                    let style = cell.style();
                    if style != run_style && !run.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut run), run_style));
                    }
                    run_style = style;
                    run.push_str(cell.symbol());
                }
                if !run.is_empty() {
                    spans.push(Span::styled(run, run_style));
                }
                Line::from(spans)
            })
            .collect()
    }

    /// The cells of each row that are drawn: a cell covered by the wide symbol before it is not.
    fn rows(&self) -> impl Iterator<Item = Vec<&Cell>> {
        let area = self.buf.area;
        (0..area.height).map(move |y| {
            let mut cells = Vec::with_capacity(usize::from(area.width));
            let mut x = 0;
            while x < area.width {
                if let Some(cell) = self.buf.cell((x, y)) {
                    let width = if cell.symbol().len() == 1 {
                        1
                    } else {
                        Span::raw(cell.symbol()).width().clamp(1, 2)
                    };
                    cells.push(cell);
                    x += u16::try_from(width).unwrap_or(1);
                } else {
                    x += 1;
                }
            }
            cells
        })
    }
}

/// A blank cell with no colours and no modifiers: nothing is drawn there.
fn is_plain_blank(cell: &Cell) -> bool {
    cell.symbol() == " "
        && cell.fg == Color::Reset
        && cell.bg == Color::Reset
        && cell.modifier.is_empty()
}

#[cfg(test)]
mod tests;
