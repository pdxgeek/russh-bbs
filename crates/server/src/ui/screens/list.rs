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
            items.sort_by_key(|item| std::cmp::Reverse(key(item, column)));
        } else {
            items.sort_by_key(|item| key(item, column));
        }
    }
}

/// A text key that sorts the way a person reads: case-blind.
pub(crate) fn text_key(text: &str) -> String {
    text.to_lowercase()
}

/// What a list the front end holds the cursor for (the members list, whose rows core numbers)
/// does with a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavKey {
    /// The key moved the cursor or the sort; nothing goes to core.
    Consumed,
    /// Enter on the row at this index of the order drawn.
    Open(usize),
    /// Not a list-navigation key: hand it to core unchanged.
    Pass,
}

/// A cursor, a sort and a window for a list whose rows core owns. Core numbers the rows and
/// opens one when its digit is pressed; this only moves a highlight, reorders the **display**
/// and turns Enter into the digit of the highlighted row, so core's rules are untouched.
#[derive(Debug, Default)]
pub(crate) struct Nav {
    pub(crate) cursor: usize,
    pub(crate) sort: Sort,
    pub(crate) view: ViewCell,
    sig: u64,
}

impl Nav {
    /// Start again at the top when the rows are different ones (another page, a new search).
    /// The sort stays: it is a choice about the list, not about a page.
    pub(crate) fn rows_are(&mut self, sig: u64) {
        if self.sig != sig {
            self.sig = sig;
            self.cursor = 0;
        }
    }

    /// Handle `key` for a list of `len` rows with `columns` sortable columns. Digits are never
    /// taken: they are core's.
    pub(crate) fn key(&mut self, key: Key, len: usize, columns: usize) -> NavKey {
        if matches!(key, Key::Char('1'..='9')) {
            return NavKey::Pass;
        }
        if key == Key::Char(SORT_KEY) {
            self.sort = self.sort.next(columns);
            self.cursor = 0;
            return NavKey::Consumed;
        }
        match pick(key, self.cursor, len, self.view.get()) {
            Pick::Move(to) => {
                self.cursor = to;
                NavKey::Consumed
            }
            Pick::Open(at) => NavKey::Open(at),
            Pick::None => NavKey::Pass,
        }
    }
}
