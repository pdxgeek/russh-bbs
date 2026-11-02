#![allow(clippy::unwrap_used)]

use russh_bbs_core::waiting_room::{RoomConfig, WaitingRoom};

use super::Screen;
use crate::ssh::terminal::{Key, PtySize};

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn screen(size: PtySize) -> Screen {
    Screen::new(size, WaitingRoom::new(&RoomConfig::default(), 5)).unwrap()
}

fn default_screen() -> Screen {
    screen(PtySize::from_request(80, 24))
}

#[test]
fn entering_draws_the_waiting_room_on_the_alternate_screen() {
    let mut screen = default_screen();
    let out = text(&screen.enter().unwrap());
    assert!(out.contains("\x1b[?1049h"), "alternate screen: {out:?}");
    assert!(out.contains("\x1b[?25l"), "cursor hidden");
    // ratatui skips blank cells, so words stay whole but phrases are split by cursor moves.
    assert!(out.contains("Waiting"));
    assert!(out.contains("rules"));
}

#[test]
fn leaving_restores_the_cursor_and_main_screen() {
    let mut screen = default_screen();
    screen.enter().unwrap();
    let out = text(&screen.leave().unwrap());
    assert!(out.contains("\x1b[?25h"));
    assert!(out.contains("\x1b[?1049l"));
}

#[test]
fn resize_redraws_for_the_new_size() {
    let mut screen = default_screen();
    screen.enter().unwrap();
    let out = text(&screen.resize(PtySize::from_request(120, 40)).unwrap());
    assert!(out.contains("Waiting"), "full redraw after resize");
}

#[test]
fn tiny_and_huge_windows_do_not_panic() {
    for (cols, rows) in [(1, 1), (2, 2), (10, 3), (u32::MAX, u32::MAX)] {
        let mut screen = screen(PtySize::from_request(cols, rows));
        screen.enter().unwrap();
        screen.resize(PtySize::from_request(rows, cols)).unwrap();
        screen.keys(&[Key::Char('r'), Key::Char('x')]).unwrap();
        screen.leave().unwrap();
    }
}

#[test]
fn keys_navigate_and_redraw() {
    let mut screen = default_screen();
    screen.enter().unwrap();
    let update = screen.keys(&[Key::Char('r')]).unwrap();
    assert!(!update.disconnect);
    assert!(text(&update.bytes).contains("House"), "rules page drawn");
}

#[test]
fn q_on_the_home_page_and_ctrl_c_anywhere_disconnect() {
    let mut home = default_screen();
    home.enter().unwrap();
    assert!(home.keys(&[Key::Char('q')]).unwrap().disconnect);

    let mut form = default_screen();
    form.enter().unwrap();
    form.keys(&[Key::Char('i')]).unwrap();
    assert!(
        !form.keys(&[Key::Char('q')]).unwrap().disconnect,
        "q is text in a form"
    );
    assert!(form.keys(&[Key::Ctrl('c')]).unwrap().disconnect);
    assert!(default_screen().keys(&[Key::Ctrl('d')]).unwrap().disconnect);
}

#[test]
fn keys_after_a_disconnecting_key_are_ignored() {
    let mut screen = default_screen();
    screen.enter().unwrap();
    // 'q' ends the session; the 'r' behind it must not run.
    let update = screen.keys(&[Key::Char('q'), Key::Char('r')]).unwrap();
    assert!(update.disconnect);
    assert!(!text(&update.bytes).contains("House"));
}
