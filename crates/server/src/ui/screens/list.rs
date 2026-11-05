//! What every list screen shares (UI-4): which rows the last draw showed, the keys that move
//! through them, and the sort a column header offers.
//!
//! **The window (FIX-M3-4).** Drawing is the only thing that knows how many rows fit, so it
//! records the window in a [`ViewCell`] and the keys read it back: a digit picks among the drawn
//! rows, and Page Up / Page Down move by one drawn page (the FIX-M3-1 keys: arrows, Page Up,
//! Page Down, Home, End, Enter, `1`-`9`).
//!
//! **Sorting.** `o` cycles the sort: each column ascending, then descending, then the next
//! column. A list the store pages (threads, mail, search) sorts **the page on screen**, not the
//! whole list: the keys, the cursor and the numbers all follow the order drawn, because the
//! rows themselves are reordered in the screen's state. The header marks the column with `^` or
//! `v` (an arrow in the Unicode tier), so a sort reads in words and without colour.

use std::sync::{Mutex, PoisonError};

use crate::ssh::terminal::Key;

/// Which rows of the current list the last draw showed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ListView {
    /// Index of the first drawn item.
    pub(crate) top: usize,
    /// How many items were drawn (0 before the first draw).
    pub(crate) shown: usize,
}

/// A [`ListView`] shared between drawing (`&self`) and keys. A mutex because the screen is held
/// across awaits and so must be `Sync`; the value is `Copy` and never held across a call.
#[derive(Debug, Default)]
pub(crate) struct ViewCell(Mutex<ListView>);

impl ViewCell {
    pub(crate) fn get(&self) -> ListView {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn set(&self, view: ListView) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = view;
    }
}

/// The row a list key lands on, if it moves or picks one.
pub(crate) enum Pick {
    Move(usize),
    Open(usize),
    None,
}

/// What a list key does. `view` is what the last draw showed: digits count the drawn rows, so
/// `Enter` and a digit can only ever open a row that is on screen.
pub(crate) fn pick(key: Key, cursor: usize, len: usize, view: ListView) -> Pick {
    if len == 0 {
        return Pick::None;
    }
    // Before the first draw nothing is windowed.
    let view = if view.shown == 0 {
        ListView { top: 0, shown: len }
    } else {
        view
    };
    let page = view.shown.max(1);
    let cursor = cursor.min(len - 1);
    match key {
        Key::Up => Pick::Move(cursor.saturating_sub(1)),
        Key::Down => Pick::Move((cursor + 1).min(len - 1)),
        Key::PageUp => Pick::Move(cursor.saturating_sub(page)),
        Key::PageDown => Pick::Move((cursor + page).min(len - 1)),
        Key::Home => Pick::Move(0),
        Key::End => Pick::Move(len - 1),
        Key::Enter => Pick::Open(cursor),
        Key::Char(c @ '1'..='9') => c
            .to_digit(10)
            .and_then(|d| usize::try_from(d - 1).ok())
            .filter(|n| *n < view.shown)
            .map(|n| view.top + n)
            .filter(|at| *at < len)
            .map_or(Pick::None, Pick::Open),
        _ => Pick::None,
    }
}

/// The key that cycles a list's sort.
pub(crate) const SORT_KEY: char = 'o';

/// How a list is sorted: the column and the direction, or the order the store gave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Sort {
    column: Option<usize>,
    descending: bool,
}

impl Sort {
    /// The next sort in the cycle for a list with `columns` sortable columns: the first column
    /// ascending, then descending, then the next column, and round again.
    #[must_use]
    pub(crate) fn next(self, columns: usize) -> Self {
        if columns == 0 {
            return self;
        }
        match (self.column, self.descending) {
            (None, _) => Self {
                column: Some(0),
                descending: false,
            },
            (Some(column), false) => Self {
                column: Some(column),
                descending: true,
            },
            (Some(column), true) => Self {
                column: Some((column + 1) % columns),
                descending: false,
            },
        }
    }

    /// `(column, descending)` for the table header's mark.
    pub(crate) const fn mark(self) -> Option<(usize, bool)> {
        match self.column {
            Some(column) => Some((column, self.descending)),
            None => None,
        }
    }

    /// Reorder `items` by `key`, stably: equal keys keep the order the store gave.
    pub(crate) fn apply<T, K: Ord>(self, items: &mut [T], key: impl Fn(&T, usize) -> K) {
        let Some(column) = self.column else {
            return;
        };
        if self.descending {
            items.sort_by(|a, b| key(b, column).cmp(&key(a, column)));
        } else {
            items.sort_by(|a, b| key(a, column).cmp(&key(b, column)));
        }
    }
}

/// A text key that sorts the way a person reads: case-blind.
pub(crate) fn text_key(text: &str) -> String {
    text.to_lowercase()
}
