#![allow(clippy::unwrap_used)]

use super::Screen;
use crate::ssh::terminal::PtySize;

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn entering_draws_the_placeholder_on_the_alternate_screen() {
    let mut screen = Screen::new(PtySize::from_request(80, 24)).unwrap();
    let out = text(&screen.enter().unwrap());
    assert!(out.contains("\x1b[?1049h"), "alternate screen: {out:?}");
    assert!(out.contains("\x1b[?25l"), "cursor hidden");
    // ratatui skips blank cells, so words stay whole but phrases are split by cursor moves.
    assert!(out.contains("disconnect."));
    assert!(out.contains("Waiting"));
}

#[test]
fn leaving_restores_the_cursor_and_main_screen() {
    let mut screen = Screen::new(PtySize::from_request(80, 24)).unwrap();
    screen.enter().unwrap();
    let out = text(&screen.leave().unwrap());
    assert!(out.contains("\x1b[?25h"));
    assert!(out.contains("\x1b[?1049l"));
}

#[test]
fn resize_redraws_for_the_new_size() {
    let mut screen = Screen::new(PtySize::from_request(80, 24)).unwrap();
    screen.enter().unwrap();
    let out = text(&screen.resize(PtySize::from_request(120, 40)).unwrap());
    assert!(out.contains("disconnect."), "full redraw after resize");
}

#[test]
fn tiny_and_huge_windows_do_not_panic() {
    for (cols, rows) in [(1, 1), (2, 2), (10, 3), (u32::MAX, u32::MAX)] {
        let mut screen = Screen::new(PtySize::from_request(cols, rows)).unwrap();
        screen.enter().unwrap();
        screen.resize(PtySize::from_request(rows, cols)).unwrap();
        screen.leave().unwrap();
    }
}

#[test]
fn the_placeholder_reveals_nothing_about_the_board() {
    let mut screen = Screen::new(PtySize::from_request(80, 24)).unwrap();
    let out = text(&screen.enter().unwrap()).to_lowercase();
    for word in ["member list", "users online", "boards", "posts", "callers"] {
        assert!(!out.contains(word), "leaks {word:?}");
    }
}
