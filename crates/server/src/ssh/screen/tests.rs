#![allow(clippy::unwrap_used)]

use russh_bbs_core::waiting_room::{RoomConfig, WaitingRoom};

use super::{KeyEffects, Screen};
use crate::ssh::terminal::{Key, PtySize, RenderCap};

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn screen(size: PtySize) -> Screen {
    capped_screen(size, RenderCap::default())
}

fn capped_screen(size: PtySize, cap: RenderCap) -> Screen {
    Screen::new(
        size,
        cap,
        super::Content::Room(WaitingRoom::new(&RoomConfig::default(), 5)),
        std::sync::Arc::new(crate::ui::Ui::standard().unwrap()),
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
fn render_buffers_do_not_grow_past_the_cap() {
    let cap = RenderCap::new(100, 40);
    for (cols, rows) in [(80, 24), (500, 200), (u32::MAX, u32::MAX)] {
        let mut screen = capped_screen(PtySize::from_request(cols, rows), cap);
        assert!(buffer_cells(&mut screen) <= cap.cells(), "{cols}x{rows}");
        screen.enter().unwrap();
        screen.set_size(PtySize::from_request(400, 150));
        screen.render().unwrap();
        assert!(buffer_cells(&mut screen) <= cap.cells(), "after resize");
    }
}

#[test]
fn the_default_cap_draws_the_whole_clamped_window() {
    let mut screen = screen(PtySize::from_request(500, 200));
    assert_eq!(screen.viewport, ratatui::layout::Rect::new(0, 0, 500, 200));
    assert_eq!(buffer_cells(&mut screen), 100_000);
    let mut hostile = screen_from(u32::MAX, u32::MAX);
    assert_eq!(buffer_cells(&mut hostile), 100_000, "clamped, not grown");
}

fn screen_from(cols: u32, rows: u32) -> Screen {
    screen(PtySize::from_request(cols, rows))
}

#[test]
fn a_large_window_is_drawn_centered_in_the_capped_viewport() {
    let mut screen = capped_screen(PtySize::from_request(500, 200), RenderCap::new(100, 40));
    assert_eq!(
        screen.viewport,
        ratatui::layout::Rect::new(200, 80, 100, 40)
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
    assert_eq!(screen.resizes, 0, "viewport was resized");

    let mut big = self::screen(PtySize::from_request(500, 200));
    big.enter().unwrap();
    big.set_size(PtySize::from_request(500, 200));
    let out = text(&big.render().unwrap().unwrap());
    assert!(out.contains("Waiting"));
    assert_eq!(big.resizes, 0, "viewport was resized");
}

#[test]
fn a_changed_drawn_rectangle_resizes_the_buffers_in_place() {
    let mut big = screen_from(500, 200);
    big.enter().unwrap();
    let before = big.terminal.current_buffer_mut().content.capacity();
    big.set_size(PtySize::from_request(300, 100));
    let out = text(&big.render().unwrap().unwrap());
    assert!(out.contains("\x1b[2J"), "screen cleared");
    assert!(out.contains("Waiting"), "everything drawn again");
    assert_eq!(big.resizes, 1);
    assert_eq!(big.viewport, ratatui::layout::Rect::new(0, 0, 300, 100));
    assert_eq!(buffer_cells(&mut big), 30_000);
    // A shrink keeps the allocation: no new terminal, no new buffers.
    assert_eq!(big.terminal.current_buffer_mut().content.capacity(), before);
}

#[test]
fn a_stored_override_larger_than_the_window_is_drawn_inside_it() {
    use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
    use russh_bbs_core::icons::Tier;
    use russh_bbs_core::sanitize::sanitize_username;
    use russh_bbs_core::store::AccountId;
    use russh_bbs_core::terminal_settings::{MemberFlow, TerminalSettings};

    // The QA M1 re-check case (L7): 500x200 stored, a 200x60 window.
    let settings = TerminalSettings::default()
        .with_tier(Tier::Ascii)
        .with_width(Some(500))
        .unwrap()
        .with_height(Some(200))
        .unwrap();
    let flow = MemberFlow::new(
        &sanitize_username("alice").unwrap(),
        Snapshot::from_role(
            Some(AccountId::from_raw(1)),
            &Slug::MEMBER,
            10,
            &Via::Account,
            Permission::ALL.iter().copied(),
        ),
        None,
        settings,
    );
    let mut screen = Screen::new(
        PtySize::from_request(200, 60),
        RenderCap::default(),
        super::Content::Member(Box::new(flow)),
        std::sync::Arc::new(crate::ui::Ui::standard().unwrap()),
    )
    .unwrap();
    let v = screen.viewport;
    assert!(
        v.x + v.width <= 200 && v.y + v.height <= 60,
        "{v:?} is outside 200x60"
    );
    screen.set_size(PtySize::from_request(120, 40));
    screen.render().unwrap();
    let v = screen.viewport;
    assert!(
        v.x + v.width <= 120 && v.y + v.height <= 40,
        "{v:?} is outside 120x40"
    );
}

// D75: nothing in the render path may ask the server process's own terminal anything.

#[test]
fn the_backend_answers_size_and_cursor_from_the_session() {
    use ratatui::backend::Backend;
    use ratatui::layout::{Position, Size};

    let mut screen = default_screen();
    screen.enter().unwrap();
    let backend = screen.terminal.backend_mut();
    assert_eq!(backend.size().unwrap(), Size::new(80, 24));
    backend.set_cursor_position(Position::new(7, 3)).unwrap();
    assert_eq!(backend.get_cursor_position().unwrap(), Position::new(7, 3));
    assert!(backend.window_size().is_err(), "no pixel size to report");

    // A window change moves the answer with it, whatever the host's own terminal is.
    screen.set_size(PtySize::from_request(132, 43));
    screen.render().unwrap();
    assert_eq!(
        screen.terminal.backend().size().unwrap(),
        Size::new(132, 43)
    );
}

#[test]
fn resizes_work_in_a_process_with_no_terminal() {
    // The tests that resize, run in a child with stdin, stdout and stderr not a tty, an empty
    // environment (no TERM, no PATH, so crossterm's `tput` fallback cannot answer) and, where
    // `setsid` exists, no controlling terminal. Before D75 these failed with `NotFound` or
    // `WouldBlock` on a CI runner.
    use std::process::{Command, Stdio};

    const NAMES: [&str; 4] = [
        "ssh::screen::tests::resize_redraws_for_the_new_size",
        "ssh::screen::tests::many_resizes_cost_one_redraw",
        "ssh::screen::tests::a_changed_drawn_rectangle_resizes_the_buffers_in_place",
        "ssh::screen::tests::tiny_and_huge_windows_do_not_panic",
    ];
    let exe = std::env::current_exe().unwrap();
    let mut command = match which_setsid() {
        Some(setsid) => {
            let mut command = Command::new(setsid);
            command.arg("-w").arg(&exe);
            command
        }
        None => Command::new(&exe),
    };
    command.args(["--test-threads=1", "--exact"]).args(NAMES);
    let output = command
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("4 passed"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn which_setsid() -> Option<std::path::PathBuf> {
    ["/usr/bin/setsid", "/bin/setsid"]
        .into_iter()
        .map(std::path::PathBuf::from)
        .find(|path| path.exists())
}

fn open_screen() -> Screen {
    let config = RoomConfig {
        mode: russh_bbs_core::store::RegistrationMode::Open,
        ..RoomConfig::default()
    };
    Screen::new(
        PtySize::from_request(80, 24),
        RenderCap::default(),
        super::Content::Room(WaitingRoom::new(&config, 5)),
        std::sync::Arc::new(crate::ui::Ui::standard().unwrap()),
    )
    .unwrap()
}

fn room_of(screen: &Screen) -> &WaitingRoom {
    let super::Content::Room(room) = &screen.content else {
        unreachable!("these tests only build room screens");
    };
    room
}

/// Open the create-account form and submit a handle, as a visitor does.
fn submit_a_handle(screen: &mut Screen) {
    let mut typed = vec![Key::Char('n')];
    typed.extend("visitor".chars().map(Key::Char));
    typed.push(Key::Enter);
    let (effects, _) = keys(screen, &typed);
    assert!(effects.redeem.is_some());
}

/// FIX-M2-4 (QA M2 L3): the form shows the one refusal sentence when the day's last place went;
/// leaving the form shows the `apply` room and its notice, without a reconnect.
#[test]
fn a_create_account_form_open_across_the_cap_degrades_when_the_visitor_leaves_it() {
    use russh_bbs_core::store::RegistrationMode;
    use russh_bbs_core::waiting_room::{Page, strings};

    let mut screen = open_screen();
    submit_a_handle(&mut screen);
    screen.open_cap_reached();
    // Still on the form, with exactly the sentence every other refusal shows, and nothing about
    // the cap yet.
    let room = room_of(&screen);
    assert_eq!(room.page(), Page::CreateAccount);
    assert_eq!(room.mode(), RegistrationMode::Open);
    let view = room.view();
    assert_eq!(view.notice.unwrap().text, strings::CREATE_ACCOUNT_FAILED);
    assert_eq!(view.note, None);

    keys(&mut screen, &[Key::Escape]);
    let room = room_of(&screen);
    assert_eq!(room.page(), Page::Home);
    assert_eq!(room.mode(), RegistrationMode::Apply);
    let view = room.view();
    assert_eq!(view.note, Some(strings::OPEN_CAPPED_NOTICE));
    assert!(
        view.menu
            .iter()
            .all(|m| m.label != strings::MENU_CREATE_ACCOUNT)
    );
    assert!(view.menu.iter().any(|m| m.label == strings::MENU_APPLY));
}

/// Any other refusal leaves the room as it was: only a full day changes it.
#[test]
fn an_ordinary_refusal_does_not_degrade_the_open_room() {
    use russh_bbs_core::store::RegistrationMode;
    use russh_bbs_core::waiting_room::Page;

    let mut screen = open_screen();
    submit_a_handle(&mut screen);
    screen.redeem_refused();
    keys(&mut screen, &[Key::Escape]);
    let room = room_of(&screen);
    assert_eq!(room.page(), Page::Home);
    assert_eq!(room.mode(), RegistrationMode::Open);
    assert_eq!(room.view().note, None);
}
