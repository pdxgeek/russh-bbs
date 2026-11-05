#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::items_after_statements
)]

use ratatui::layout::Rect;

use super::{Click, HitMap, Region, capture, list, modal, page};
use crate::ssh::terminal::Key;

fn menu(offset: usize, selected: Option<usize>) -> HitMap {
    HitMap::of(vec![Region::List {
        rect: Rect::new(2, 5, 30, 4),
        step: 1,
        offset,
        len: 6,
        selected,
        click: Click::Hotkeys(vec!['f', 'm', 'c', 'd', 'u', 's']),
    }])
}

#[test]
fn a_click_on_a_menu_row_presses_that_rows_hotkey() {
    let hits = menu(0, None);
    assert_eq!(hits.click(3, 5), vec![Key::Char('f')]);
    assert_eq!(hits.click(31, 8), vec![Key::Char('d')]);
    // The list is scrolled by two: the first drawn row is the third item.
    assert_eq!(menu(2, None).click(3, 5), vec![Key::Char('c')]);
}

#[test]
fn a_click_outside_every_list_or_past_its_last_row_does_nothing() {
    let hits = menu(0, None);
    for (col, row) in [
        (1, 5),
        (32, 5),
        (3, 4),
        (3, 9),
        (0, 0),
        (u16::MAX, u16::MAX),
    ] {
        assert!(hits.click(col, row).is_empty(), "{col},{row}");
    }
    // A 4-row window onto 6 items, scrolled to the end: row 4 would be index 6 = past the end.
    let short = HitMap::of(vec![Region::List {
        rect: Rect::new(0, 0, 10, 4),
        step: 1,
        offset: 4,
        len: 6,
        selected: None,
        click: Click::Cursor,
    }]);
    assert!(short.click(0, 3).is_empty());
}

#[test]
fn a_click_on_a_table_row_moves_the_cursor_and_a_second_click_opens_it() {
    let table = |selected| {
        HitMap::of(vec![Region::List {
            rect: Rect::new(0, 2, 40, 5),
            step: 1,
            offset: 3,
            len: 20,
            selected,
            click: Click::Cursor,
        }])
    };
    // Row 2 of the window is index 5; the cursor is on 3: two Down.
    assert_eq!(table(Some(3)).click(1, 4), vec![Key::Down, Key::Down]);
    assert_eq!(table(Some(6)).click(1, 4), vec![Key::Up]);
    assert_eq!(table(Some(5)).click(1, 4), vec![Key::Enter]);
}

#[test]
fn a_palette_click_moves_and_opens_in_one_go() {
    let hits = HitMap::of(vec![Region::List {
        rect: Rect::new(0, 0, 20, 8),
        step: 1,
        offset: 0,
        len: 8,
        selected: Some(0),
        click: Click::Open,
    }]);
    assert_eq!(hits.click(0, 2), vec![Key::Down, Key::Down, Key::Enter]);
    assert_eq!(hits.click(0, 0), vec![Key::Enter]);
}

#[test]
fn two_line_rows_map_a_click_on_either_line_to_the_same_row() {
    let hits = HitMap::of(vec![Region::List {
        rect: Rect::new(0, 0, 20, 6),
        step: 2,
        offset: 0,
        len: 3,
        selected: Some(0),
        click: Click::Cursor,
    }]);
    assert_eq!(hits.click(0, 2), hits.click(0, 3));
    assert_eq!(hits.click(0, 4), vec![Key::Down, Key::Down]);
}

#[test]
fn the_wheel_moves_a_list_by_lines_and_a_page_by_pages_and_both_are_capped() {
    let hits = menu(0, Some(0));
    assert_eq!(hits.wheel(3, 5, 1), vec![Key::Down]);
    assert_eq!(hits.wheel(3, 5, -2), vec![Key::Up, Key::Up]);
    assert_eq!(hits.wheel(3, 5, 500).len(), super::MAX_WHEEL);
    assert!(hits.wheel(3, 5, 0).is_empty());
    let reader = HitMap::of(vec![Region::Page {
        rect: Rect::new(0, 1, 80, 20),
    }]);
    assert_eq!(reader.wheel(5, 5, 9), vec![Key::PageDown]);
    assert_eq!(reader.wheel(5, 5, -9), vec![Key::PageUp]);
    // Nothing clickable on the frame: the wheel does nothing (a form must not be scrolled).
    assert!(HitMap::default().wheel(0, 0, 3).is_empty());
}

#[test]
fn recording_collects_what_a_draw_recorded_and_is_empty_outside_a_capture() {
    // Outside a capture nothing is kept (every widget test draws like this).
    list(Rect::new(0, 0, 5, 5), 1, 0, 3, None, Click::Cursor);
    let ((), hits) = capture(|| {
        list(Rect::new(0, 0, 5, 5), 1, 0, 3, None, Click::Cursor);
        page(Rect::new(0, 6, 5, 5));
        // Empty rectangles record nothing.
        list(Rect::new(0, 0, 0, 5), 1, 0, 3, None, Click::Cursor);
        page(Rect::new(0, 0, 5, 0));
    });
    assert_eq!(hits.regions.len(), 2);
    let ((), nested) = capture(|| {
        list(Rect::new(0, 0, 5, 5), 1, 0, 3, None, Click::Cursor);
        modal();
        page(Rect::new(0, 0, 5, 5));
    });
    assert_eq!(nested.regions.len(), 1, "a modal clears what is under it");
}

#[test]
fn a_click_on_a_numbered_row_without_a_cursor_presses_its_digit() {
    let hits = HitMap::of(vec![Region::List {
        rect: Rect::new(0, 4, 30, 12),
        step: 1,
        offset: 3,
        len: 40,
        selected: None,
        click: Click::Digits,
    }]);
    assert_eq!(hits.click(2, 4), vec![Key::Char('1')]);
    assert_eq!(hits.click(2, 12), vec![Key::Char('9')]);
    // The tenth drawn row has no digit: nothing happens, as nothing does on the keyboard.
    assert!(hits.click(2, 13).is_empty());
}
