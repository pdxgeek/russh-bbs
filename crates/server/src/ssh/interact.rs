//! What a session's screen keeps for **interaction** (UI-9): the mouse, the command palette,
//! toasts and the header ticker. The screen owns one [`Interact`] and calls these few methods;
//! the rules live in `ui::input`, `ui::palette`, `ui::toast` and `ui::ticker`.
//!
//! **Mouse mode** is on only when the member asked for it (the `mouse` setting) and is told to
//! the client in the same frame that changes it: `ESC [ ? 1000 h` (press and release, and the
//! wheel) with `ESC [ ? 1006 h` (SGR coordinates). It is turned off again when the setting goes
//! off, and [`Interact::restore`] turns it off when the session leaves, so the person's terminal
//! is never left reporting the mouse. There is no accessibility mode yet (M5); when there is,
//! [`mouse_allowed`] is the one place that says no.
//!
//! **A mouse does nothing a key does not.** Events become keys ([`Interact::mouse_keys`]) that
//! the screen applies like typed ones. Reports are dropped when mouse mode is not on (a client
//! that was never asked cannot drive the screen with them), and a click is dropped when keys
//! arrive in the same batch (the person is typing; the click's target may be out of date).

use std::collections::VecDeque;
use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::Rect;
use russh_bbs_core::terminal_settings::TerminalSettings;

use super::terminal::Key;
use crate::settings::Look;
use crate::ui::board::Board;
use crate::ui::input::mouse::{Button, Kind, Mouse};
use crate::ui::input::{self, HitMap};
use crate::ui::palette::{self, Outcome, Palette};
use crate::ui::strings::StringTable;
use crate::ui::ticker::{self, Ticker};
use crate::ui::toast::{Kind as ToastKind, Text, Toasts};

/// Turn mouse reporting on: button press and release with the wheel (1000), SGR coordinates
/// (1006). No motion reporting yet; pane dragging (UI-4) adds 1002.
pub(crate) const MOUSE_ON: &[u8] = b"\x1b[?1000h\x1b[?1006h";
/// Turn it off again, in the opposite order.
pub(crate) const MOUSE_OFF: &[u8] = b"\x1b[?1006l\x1b[?1000l";

/// Whether the member may have mouse reporting at all. Today that is their setting; accessibility
/// mode (M5) will say no here.
pub(crate) fn mouse_allowed(settings: &TerminalSettings) -> bool {
    settings.mouse()
}

/// A session's interaction state.
pub(crate) struct Interact {
    hits: HitMap,
    /// The client's terminal is reporting the mouse.
    mouse_on: bool,
    /// The mouse state has been decided once, so the first sync after login is not announced.
    mouse_known: bool,
    palette: Option<Palette>,
    recent: VecDeque<String>,
    /// Boards the viewer may list, as the forum service last said (for the palette).
    boards: Vec<palette::BoardName>,
    /// How many menus deep the palette was opened, so running an entry can step back to the root.
    palette_depth: usize,
    pub(crate) toasts: Toasts,
    ticker: Ticker,
    last_step: usize,
}

impl Interact {
    pub(crate) fn new(board: &Board, now: Instant) -> Self {
        Self {
            hits: HitMap::default(),
            mouse_on: false,
            mouse_known: false,
            palette: None,
            recent: VecDeque::new(),
            boards: Vec::new(),
            palette_depth: 0,
            toasts: Toasts::default(),
            ticker: Ticker::new(board, true, now),
            last_step: 0,
        }
    }

    // ---- mouse ------------------------------------------------------------------------

    /// The bytes that bring the client's mouse mode in line with `wanted`, and a toast saying
    /// so when the member just changed it.
    pub(crate) fn sync_mouse(
        &mut self,
        wanted: bool,
        strings: &StringTable,
        now: Instant,
    ) -> &'static [u8] {
        let announce = self.mouse_known;
        self.mouse_known = true;
        if wanted == self.mouse_on {
            return b"";
        }
        self.mouse_on = wanted;
        if announce {
            let key = if wanted {
                "toast.mouse_on"
            } else {
                "toast.mouse_off"
            };
            self.toasts
                .push(ToastKind::Notice, Text::Key(key), strings, now);
        }
        if wanted { MOUSE_ON } else { MOUSE_OFF }
    }

    /// The bytes that leave the client's terminal as it was.
    pub(crate) fn restore(&mut self) -> &'static [u8] {
        if std::mem::take(&mut self.mouse_on) {
            MOUSE_OFF
        } else {
            b""
        }
    }

    /// The last frame's hit map.
    #[cfg(test)]
    pub(crate) fn hits(&self) -> &HitMap {
        &self.hits
    }

    /// Replace the hit map with the last frame's.
    pub(crate) fn set_hits(&mut self, hits: HitMap) {
        self.hits = hits;
    }

    /// The keys the mouse reports stand for. Empty when mouse mode is off, and when `typing`
    /// (real keys came in the same batch). A burst of wheel notches counts once, capped.
    pub(crate) fn mouse_keys(&self, events: &[Mouse], typing: bool) -> Vec<Key> {
        if !self.mouse_on || typing || events.is_empty() {
            return Vec::new();
        }
        let mut keys = Vec::new();
        let mut wheel = 0_i32;
        let mut wheel_at = (0_u16, 0_u16);
        for event in events {
            match event.kind {
                Kind::Down(Button::Left) => {
                    // Only the first click of a batch: two clicks cannot both be right.
                    if keys.is_empty() {
                        keys = self.hits.click(event.col, event.row);
                    }
                }
                Kind::ScrollUp => {
                    wheel -= 1;
                    wheel_at = (event.col, event.row);
                }
                Kind::ScrollDown => {
                    wheel += 1;
                    wheel_at = (event.col, event.row);
                }
                Kind::Down(_) | Kind::Up(_) | Kind::Drag(_) => {}
            }
        }
        if keys.is_empty() {
            keys = self.hits.wheel(wheel_at.0, wheel_at.1, wheel);
        }
        keys
    }

    // ---- palette ----------------------------------------------------------------------

    pub(crate) fn palette_open(&self) -> bool {
        self.palette.is_some()
    }

    pub(crate) fn boards(&self) -> &[palette::BoardName] {
        &self.boards
    }

    /// Open the palette over `entries`, remembering how deep the menu is.
    pub(crate) fn open_palette(&mut self, entries: Vec<palette::Entry>, depth: usize) {
        self.palette_depth = depth;
        self.palette = Some(Palette::new(entries, &self.recent));
    }

    /// New board names arrived; if the palette is open it is rebuilt around them.
    pub(crate) fn set_boards(&mut self, boards: Vec<palette::BoardName>) {
        self.boards = boards;
    }

    /// Rebuild the open palette's entries (boards arrived), keeping what was typed.
    pub(crate) fn refresh_palette(&mut self, entries: Vec<palette::Entry>) {
        if let Some(old) = &self.palette {
            let query = old.typed().to_owned();
            let mut fresh = Palette::new(entries, &self.recent);
            fresh.retype(&query);
            self.palette = Some(fresh);
        }
    }

    /// Give a key to the open palette. `Some(keys)` is what to press from the main menu's root
    /// (the choice, after stepping back out of any submenu); the palette has closed.
    pub(crate) fn palette_key(&mut self, key: Key) -> PaletteStep {
        let Some(palette) = self.palette.as_mut() else {
            return PaletteStep::Closed;
        };
        // Logging off from the palette still logs off.
        if key.is_quit() && !matches!(key, Key::Char(_)) {
            self.palette = None;
            return PaletteStep::PassThrough(key);
        }
        let chosen = palette.chosen_id().map(str::to_owned);
        match palette.input(key) {
            Outcome::Stay => PaletteStep::Open,
            Outcome::Close => {
                self.palette = None;
                PaletteStep::Closed
            }
            Outcome::Run(path) => {
                self.palette = None;
                if let Some(id) = chosen {
                    palette::remember(&mut self.recent, &id);
                }
                let mut keys = vec![Key::Escape; self.palette_depth];
                keys.extend(path);
                PaletteStep::Run(keys)
            }
        }
    }

    pub(crate) fn close_palette(&mut self) {
        self.palette = None;
    }

    // ---- ticker -----------------------------------------------------------------------

    pub(crate) fn ticker_mut(&mut self) -> &mut Ticker {
        &mut self.ticker
    }

    pub(crate) fn ticker(&self) -> &Ticker {
        &self.ticker
    }

    /// Advance time. `true` when a frame is due: a toast came or went, or the marquee moved.
    pub(crate) fn tick(&mut self, now: Instant, ticker_visible: bool) -> bool {
        let mut redraw = self.toasts.tick(now);
        if ticker_visible {
            let step = self.ticker.step(now);
            if step != self.last_step {
                self.last_step = step;
                redraw = true;
            }
        }
        redraw
    }

    /// When the screen next needs a frame with nobody pressing anything.
    pub(crate) fn next_wake(&self, now: Instant, ticker_visible: bool) -> Option<Instant> {
        let toast = self.toasts.next_wake();
        let ticker = if ticker_visible {
            self.ticker.next_wake(now)
        } else {
            None
        };
        [toast, ticker].into_iter().flatten().min()
    }

    // ---- drawing ----------------------------------------------------------------------

    /// The overlays: the palette (if open) and the toasts, in the area between the header row
    /// and the status row of `frame`. Both are clamped to it, so neither can reach a bar or the
    /// marker cell.
    pub(crate) fn overlay(&self, frame: &mut Frame<'_>, look: Look, strings: &StringTable) {
        let area = overlay_area(frame.area());
        if let Some(palette) = &self.palette {
            palette.render(frame, area, look, strings);
        }
        self.toasts.render(frame, area, look);
    }
}

/// What the palette did with a key.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PaletteStep {
    /// There is no palette.
    Closed,
    /// Still open.
    Open,
    /// Closed, and this key is for the screen to handle (a log-off).
    PassThrough(Key),
    /// Closed; press these keys from the main menu.
    Run(Vec<Key>),
}

/// The part of a screen that overlays may use: everything but the first and the last row, which
/// belong to the header bar and the status bar (where the marker cell is).
pub(crate) fn overlay_area(area: Rect) -> Rect {
    if area.height < 4 {
        return Rect { height: 0, ..area };
    }
    Rect {
        y: area.y + 1,
        height: area.height - 2,
        ..area
    }
}

/// Use a ticker view for one draw; kept here so the screen has one import for interaction.
pub(crate) fn with_ticker<R>(view: Option<ticker::TickerView>, draw: impl FnOnce() -> R) -> R {
    ticker::scope(view, draw)
}

/// Record the hits of one draw; kept here for the same reason.
pub(crate) fn capture_hits<R>(draw: impl FnOnce() -> R) -> (R, HitMap) {
    input::capture(draw)
}

#[cfg(test)]
mod tests;
