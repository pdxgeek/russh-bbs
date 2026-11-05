//! Interaction input (UI-9): the **mouse**, from the bounded report parser to the keys a click
//! or a wheel turn stands for.
//!
//! The rule that keeps this safe and honest: **a mouse does nothing a key does not**. A click
//! or a wheel turn is turned into the keys the same action takes on the keyboard (a menu item's
//! hotkey, `Up`/`Down` to the row, `Enter`, `PageUp`/`PageDown` in the reader), and those keys
//! go through the normal path, so permissions and limits are checked exactly as for a typed key.
//! Nothing here reaches the store, the menu tree or a door.
//!
//! How the screens say what is clickable: while a frame is drawn, the widgets that draw a list
//! or a page (the menu box, the tables, the reader) *record* a region with [`list`] or [`page`].
//! The screen collects them with [`capture`] around the draw and keeps the last frame's
//! [`HitMap`]; a click is looked up there. Recording is a thread-local sink, set and read inside
//! one synchronous draw, so nothing is shared between sessions and a widget drawn with no sink
//! (every test) records nothing.

use std::cell::RefCell;

use ratatui::layout::Rect;

use crate::ssh::terminal::Key;

pub(crate) mod mouse;

/// How a click on a list row becomes keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Click {
    /// Each row has a hotkey (a menu): the click presses it, in every layout.
    Hotkeys(Vec<char>),
    /// Rows have a cursor (a table): the click moves it to the row, and a click on the row
    /// already under the cursor opens it (`Enter`).
    Cursor,
    /// Rows have a cursor and one click is enough (the palette): move to the row, then `Enter`.
    Open,
    /// Rows are picked by the digits `1` to `9` counted from the first row drawn (a numbered
    /// table with no cursor): the click presses that row's digit.
    Digits,
}

/// One clickable, scrollable area of the last frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Region {
    /// Rows of a list, `step` terminal rows each, from `rect.y`.
    List {
        rect: Rect,
        /// Terminal rows per list row (more than one when a row has a detail line).
        step: u16,
        /// The index of the first row drawn.
        offset: usize,
        /// How many rows the list has in all.
        len: usize,
        /// The row under the cursor, if the list has one.
        selected: Option<usize>,
        click: Click,
    },
    /// A page of text (the reader): the wheel turns pages; a click does nothing.
    Page { rect: Rect },
}

/// The clickable regions of the last frame drawn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HitMap {
    regions: Vec<Region>,
}

/// Most keys one click or wheel turn expands to, so a report that lands far from the cursor
/// cannot queue a long run of keys.
const MAX_STEPS: usize = 64;
/// Most lines a burst of wheel events moves in one cycle.
pub(crate) const MAX_WHEEL: usize = 3;

impl HitMap {
    /// A map holding only `regions` (tests and modal overlays).
    #[cfg(test)]
    pub(crate) fn of(regions: Vec<Region>) -> Self {
        Self { regions }
    }

    /// The regions of the frame (tests).
    #[cfg(test)]
    pub(crate) fn regions(&self) -> &[Region] {
        &self.regions
    }

    /// The keys a left click at `(col, row)` stands for. Empty when it hit nothing.
    pub(crate) fn click(&self, col: u16, row: u16) -> Vec<Key> {
        for region in &self.regions {
            let Region::List {
                rect,
                step,
                offset,
                len,
                selected,
                click,
            } = region
            else {
                continue;
            };
            if !contains(*rect, col, row) {
                continue;
            }
            let index = offset + usize::from((row - rect.y) / (*step).max(1));
            if index >= *len {
                return Vec::new();
            }
            return match click {
                Click::Hotkeys(keys) => keys
                    .get(index)
                    .map(|hotkey| vec![Key::Char(*hotkey)])
                    .unwrap_or_default(),
                Click::Cursor => cursor_keys(*selected, index),
                Click::Digits => u32::try_from(index - offset + 1)
                    .ok()
                    .and_then(|n| char::from_digit(n, 10))
                    .filter(|digit| *digit != '0')
                    .map(|digit| vec![Key::Char(digit)])
                    .unwrap_or_default(),
                Click::Open => {
                    let mut keys = move_keys(*selected, index);
                    keys.push(Key::Enter);
                    keys
                }
            };
        }
        Vec::new()
    }

    /// The keys a wheel burst of `net` notches (negative is up) stands for at `(col, row)`.
    /// A list moves its cursor by lines; a page turns by pages, at most one per cycle.
    pub(crate) fn wheel(&self, col: u16, row: u16, net: i32) -> Vec<Key> {
        if net == 0 {
            return Vec::new();
        }
        let region = self
            .regions
            .iter()
            .find(|region| contains(rect_of(region), col, row))
            .or_else(|| self.regions.first());
        let steps = usize::try_from(net.unsigned_abs()).unwrap_or(MAX_WHEEL);
        match region {
            Some(Region::List { .. }) => {
                let key = if net < 0 { Key::Up } else { Key::Down };
                vec![key; steps.min(MAX_WHEEL)]
            }
            Some(Region::Page { .. }) => {
                vec![if net < 0 { Key::PageUp } else { Key::PageDown }]
            }
            None => Vec::new(),
        }
    }
}

fn rect_of(region: &Region) -> Rect {
    match region {
        Region::List { rect, .. } | Region::Page { rect } => *rect,
    }
}

fn contains(rect: Rect, col: u16, row: u16) -> bool {
    col >= rect.x
        && row >= rect.y
        && u32::from(col) < u32::from(rect.x) + u32::from(rect.width)
        && u32::from(row) < u32::from(rect.y) + u32::from(rect.height)
}

/// Move the cursor from `selected` to `index`; a click on the row already selected opens it.
fn cursor_keys(selected: Option<usize>, index: usize) -> Vec<Key> {
    match selected {
        Some(at) if at == index => vec![Key::Enter],
        _ => move_keys(selected, index),
    }
}

/// The arrow keys that take the cursor from `selected` to `index`.
fn move_keys(selected: Option<usize>, index: usize) -> Vec<Key> {
    match selected {
        Some(at) if at < index => vec![Key::Down; (index - at).min(MAX_STEPS)],
        Some(at) => vec![Key::Up; (at - index).min(MAX_STEPS)],
        // No cursor yet: the first click only puts it on the row.
        None => vec![Key::Down; (index + 1).min(MAX_STEPS)],
    }
}

thread_local! {
    static SINK: RefCell<Option<HitMap>> = const { RefCell::new(None) };
}

/// Run `draw` and return what it recorded with [`list`] and [`page`].
pub(crate) fn capture<R>(draw: impl FnOnce() -> R) -> (R, HitMap) {
    let previous = SINK.with(|sink| sink.replace(Some(HitMap::default())));
    let result = draw();
    let hits = SINK.with(|sink| sink.replace(previous)).unwrap_or_default();
    (result, hits)
}

fn record(region: Region) {
    SINK.with(|sink| {
        if let Some(map) = sink.borrow_mut().as_mut() {
            map.regions.push(region);
        }
    });
}

/// A modal overlay (the palette) was drawn over the screen: what is under it no longer answers
/// the mouse.
pub(crate) fn modal() {
    SINK.with(|sink| {
        if let Some(map) = sink.borrow_mut().as_mut() {
            map.regions.clear();
        }
    });
}

/// Record a list of rows drawn in `rect`, `step` terminal rows each.
pub(crate) fn list(
    rect: Rect,
    step: u16,
    offset: usize,
    len: usize,
    selected: Option<usize>,
    click: Click,
) {
    if rect.width > 0 && rect.height > 0 {
        record(Region::List {
            rect,
            step,
            offset,
            len,
            selected,
            click,
        });
    }
}

/// Record a page of text drawn in `rect`.
pub(crate) fn page(rect: Rect) {
    if rect.width > 0 && rect.height > 0 {
        record(Region::Page { rect });
    }
}

#[cfg(test)]
mod tests;
