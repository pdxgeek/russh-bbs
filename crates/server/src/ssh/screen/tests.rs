#![allow(clippy::unwrap_used)]

use russh_bbs_core::waiting_room::{RoomConfig, WaitingRoom};

use super::{KeyEffects, Screen};
use crate::ssh::terminal::{Key, PtySize, RENDER_COLS, RENDER_ROWS};

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn screen(size: PtySize) -> Screen {
    Screen::new(
        size,
        super::Content::Room(WaitingRoom::new(&RoomConfig::default(), 5)),
    )
    .unwrap()
}

/// Apply keys and render once, as a session does for one batch.
fn keys(screen: &mut Screen, keys: &[Key]) -> (KeyEffects, String) {
    let effects = screen.apply_keys(keys);
    let frame = screen
        .render()
        .unwrap()
        .map(|b| text(&b))
        .unwrap_or_default();
    (effects, frame)
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
    screen.set_size(PtySize::from_request(120, 40));
    let out = text(&screen.render().unwrap().unwrap());
    assert!(out.contains("\x1b[2J"), "screen cleared");
    assert!(out.contains("Waiting"), "full redraw after resize");
}

#[test]
fn many_resizes_cost_one_redraw() {
    let mut screen = default_screen();
    screen.enter().unwrap();
    for width in 30..130 {
        screen.set_size(PtySize::from_request(width, 24));
    }
    let frame = text(&screen.render().unwrap().unwrap());
    assert!(frame.contains("Waiting"));
    assert!(screen.render().unwrap().is_none(), "nothing left to draw");
}

#[test]
fn a_batch_of_keys_costs_one_redraw() {
    let mut screen = default_screen();
    screen.enter().unwrap();
    let many = vec![Key::Char('z'); 10_000];
    let (effects, frame) = keys(&mut screen, &many);
    assert!(!effects.disconnect);
    assert!(
        frame.len() < 4096,
        "one small frame, not one per key: {}",
        frame.len()
    );
    assert!(screen.render().unwrap().is_none(), "nothing left to draw");
}

#[test]
fn tiny_and_huge_windows_do_not_panic() {
    for (cols, rows) in [(1, 1), (2, 2), (10, 3), (u32::MAX, u32::MAX)] {
        let mut screen = screen(PtySize::from_request(cols, rows));
        screen.enter().unwrap();
        screen.set_size(PtySize::from_request(rows, cols));
        screen.render().unwrap();
        keys(&mut screen, &[Key::Char('r'), Key::Char('x')]);
        screen.leave().unwrap();
    }
}

#[test]
fn keys_navigate_and_redraw() {
    let mut screen = default_screen();
    screen.enter().unwrap();
    let (effects, frame) = keys(&mut screen, &[Key::Char('r')]);
    assert!(!effects.disconnect);
    assert!(frame.contains("House"), "rules page drawn");
}

#[test]
fn q_on_the_home_page_and_ctrl_c_anywhere_disconnect() {
    let mut home = default_screen();
    home.enter().unwrap();
    assert!(keys(&mut home, &[Key::Char('q')]).0.disconnect);

    let mut form = default_screen();
    form.enter().unwrap();
    keys(&mut form, &[Key::Char('i')]);
    assert!(
        !keys(&mut form, &[Key::Char('q')]).0.disconnect,
        "q is text in a form"
    );
    assert!(keys(&mut form, &[Key::Ctrl('c')]).0.disconnect);
    assert!(keys(&mut default_screen(), &[Key::Ctrl('d')]).0.disconnect);
}

#[test]
fn keys_after_a_disconnecting_key_are_ignored() {
    let mut screen = default_screen();
    screen.enter().unwrap();
    // 'q' ends the session; the 'r' behind it must not run.
    let (effects, frame) = keys(&mut screen, &[Key::Char('q'), Key::Char('r')]);
    assert!(effects.disconnect);
    assert!(!frame.contains("House"));
}

fn buffer_cells(screen: &mut Screen) -> usize {
    let area = screen.terminal.current_buffer_mut().area;
    usize::from(area.width) * usize::from(area.height)
}

#[test]
fn render_buffers_do_not_grow_with_the_window() {
    let cap = usize::from(RENDER_COLS) * usize::from(RENDER_ROWS);
    for (cols, rows) in [(80, 24), (500, 200), (u32::MAX, u32::MAX)] {
        let mut screen = screen(PtySize::from_request(cols, rows));
        assert!(buffer_cells(&mut screen) <= cap, "{cols}x{rows}");
        screen.enter().unwrap();
        screen.set_size(PtySize::from_request(400, 150));
        screen.render().unwrap();
        assert!(buffer_cells(&mut screen) <= cap, "after resize");
    }
}

#[test]
fn a_large_window_is_drawn_centered_in_the_capped_viewport() {
    let mut screen = screen(PtySize::from_request(500, 200));
    assert_eq!(
        screen.viewport,
        ratatui::layout::Rect::new(200, 80, RENDER_COLS, RENDER_ROWS)
    );
    assert!(text(&screen.enter().unwrap()).contains("Waiting"));
}

#[test]
fn an_unchanged_drawn_rectangle_repaints_without_rebuilding() {
    let mut screen = default_screen();
    screen.enter().unwrap();
    screen.set_size(PtySize::from_request(80, 24));
    let out = text(&screen.render().unwrap().unwrap());
    assert!(out.contains("\x1b[2J"), "screen cleared");
    assert!(out.contains("Waiting"), "everything drawn again");
    assert_eq!(screen.builds, 1, "terminal was rebuilt");

    let mut big = self::screen(PtySize::from_request(500, 200));
    big.enter().unwrap();
    big.set_size(PtySize::from_request(500, 200));
    let out = text(&big.render().unwrap().unwrap());
    assert!(out.contains("Waiting"));
    assert_eq!(big.builds, 1, "terminal was rebuilt");
    big.set_size(PtySize::from_request(300, 100));
    big.render().unwrap();
    assert_eq!(big.builds, 2, "a different rectangle does rebuild");
}
