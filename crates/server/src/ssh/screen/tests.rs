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

/// FIX-M3-1: a pasted line break is not Enter. Each kind of input point drops it, so a paste
/// cannot submit a form early or run a second command.
#[test]
fn a_multi_line_paste_into_a_waiting_room_form_does_not_submit_it() {
    let mut screen = open_screen();
    let paste = crate::ssh::terminal::parse_keys(b"n\x1b[200~vis\nitor\r\nmore\r\x1b[201~");
    assert!(!paste.contains(&Key::Enter), "{paste:?}");
    let (effects, _) = keys(&mut screen, &paste);
    assert!(effects.redeem.is_none(), "the paste submitted the form");
    // The breaks were flattened away, so one typed Enter submits one handle.
    let (effects, _) = keys(&mut screen, &[Key::Enter]);
    assert!(effects.redeem.is_some());
}

#[test]
fn editing_keys_and_pasted_breaks_do_nothing_at_the_room_claim_and_member_input_points() {
    use super::{claim_input, member_input, room_input};
    for key in [
        Key::Left,
        Key::Right,
        Key::Home,
        Key::End,
        Key::Delete,
        Key::PageUp,
        Key::PageDown,
        Key::PasteBreak,
    ] {
        assert!(room_input(key).is_none(), "{key:?}");
        assert!(claim_input(key).is_none(), "{key:?}");
        assert!(member_input(key).is_none(), "{key:?}");
    }
}

#[test]
fn a_paste_with_line_breaks_cannot_press_enter_or_quit_a_member_screen() {
    use super::member_input;
    let paste = crate::ssh::terminal::parse_keys(b"\x1b[200~q\nq\r\nx\x03\x1b[201~");
    let inputs: Vec<_> = paste.into_iter().filter_map(member_input).collect();
    assert!(
        inputs
            .iter()
            .all(|input| matches!(input, super::MemberInput::Char(_))),
        "{inputs:?}"
    );
}

/// FIX-M3-1: the chat input line never sends on a pasted line break, and the editing keys it
/// has no use for do nothing.
#[test]
fn a_multi_line_paste_into_the_chat_line_cannot_send_it() {
    use super::chat_key;
    use russh_bbs_core::chat::ChatKey;
    let paste = crate::ssh::terminal::parse_keys(b"\x1b[200~hi\nthere\r\n/quit\x1b[201~");
    let mapped: Vec<_> = paste.into_iter().filter_map(chat_key).collect();
    assert!(
        mapped.iter().all(|key| matches!(key, ChatKey::Char(_))),
        "{mapped:?}"
    );
    for key in [
        Key::Left,
        Key::Right,
        Key::Home,
        Key::End,
        Key::Delete,
        Key::PageUp,
        Key::PageDown,
        Key::PasteBreak,
    ] {
        assert!(chat_key(key).is_none(), "{key:?}");
    }
    // A typed Enter still sends.
    assert_eq!(chat_key(Key::Enter), Some(ChatKey::Enter));
}

// ---- UI-1: the frame's status bar and the client's colour depth ----

mod frame {
    use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
    use russh_bbs_core::icons::Tier;
    use russh_bbs_core::sanitize::sanitize_username;
    use russh_bbs_core::store::AccountId;
    use russh_bbs_core::terminal_settings::{MemberFlow, TerminalSettings};

    use super::{Key, PtySize, RenderCap, Screen, text};

    fn member_screen(role: &Slug, rank: u8, settings: TerminalSettings, size: PtySize) -> Screen {
        let flow = MemberFlow::new(
            &sanitize_username("alice").unwrap(),
            Snapshot::from_role(
                Some(AccountId::from_raw(1)),
                role,
                rank,
                &Via::Account,
                Permission::ALL.iter().copied(),
            ),
            None,
            settings,
        );
        Screen::new(
            size,
            RenderCap::default(),
            super::super::Content::Member(Box::new(flow)),
            std::sync::Arc::new(crate::ui::Ui::standard().unwrap()),
        )
        .unwrap()
    }

    /// The words of every full redraw after each key in `presses`, as one string per press.
    fn walk(screen: &mut Screen, presses: &[Key]) -> Vec<String> {
        let mut frames = vec![text(&screen.enter().unwrap())];
        for key in presses {
            screen.apply_keys(&[*key]);
            // A window change of the same size repaints everything, so the words are whole.
            let size = screen.pty;
            screen.set_size(size);
            frames.push(text(&screen.render().unwrap().unwrap_or_default()));
        }
        frames
    }

    fn ascii() -> TerminalSettings {
        TerminalSettings::default().with_tier(Tier::Ascii)
    }

    /// UI-4: a change of screen starts a short sweep and a typed input starts a blinking cursor,
    /// but only with motion on. Motion off, and accessibility mode with motion on, wake nothing:
    /// no timer-driven redraws at all.
    #[test]
    fn motion_wakes_the_shell_only_when_it_is_allowed() {
        let size = PtySize::from_request(80, 24);
        let settings = |motion| {
            TerminalSettings::default()
                .with_tier(Tier::Unicode)
                .with_color(true)
                .with_motion(motion)
        };
        // On: opening the Community menu is a change of screen.
        let mut on = member_screen(&Slug::SYSOP, 100, settings(true), size);
        on.enter().unwrap();
        on.apply_keys(&[Key::Char('x'), Key::Char('n'), Key::Char('y')]);
        on.render().unwrap();
        on.apply_keys(&[Key::Char('y')]);
        on.render().unwrap();
        assert!(on.motion_deadline().is_some(), "a screen change sweeps");
        // Off: the same keys wake nothing.
        let mut off = member_screen(&Slug::SYSOP, 100, settings(false), size);
        off.enter().unwrap();
        off.apply_keys(&[Key::Char('x'), Key::Char('n'), Key::Char('y')]);
        off.render().unwrap();
        off.apply_keys(&[Key::Char('y')]);
        off.render().unwrap();
        assert!(off.motion_deadline().is_none(), "motion off: no timer");
        // Accessibility mode: always plain, even with motion on.
        let mut plain = member_screen(&Slug::SYSOP, 100, settings(true), size);
        plain.set_accessible(true);
        plain.enter().unwrap();
        plain.apply_keys(&[Key::Char('x'), Key::Char('n'), Key::Char('y')]);
        plain.render().unwrap();
        plain.apply_keys(&[Key::Char('y')]);
        plain.render().unwrap();
        assert!(plain.motion_deadline().is_none(), "accessibility: no timer");
        // And the frames it draws carry no sweep: a second render has nothing to draw.
        assert!(plain.render().unwrap().is_none());
    }

    /// FIX-M3-6: while the board is in low-security mode every screen of a staff session shows
    /// `LOW-SECURITY` in the status bar's marker cell: login, menu, settings, members,
    /// community and the sysop menu. A plain member never does, and nobody does outside the mode.
    #[test]
    fn low_security_shows_on_every_screen_of_a_staff_session_and_only_there() {
        let size = PtySize::from_request(80, 24);
        let presses = [
            Key::Char('x'),
            Key::Char('s'),
            Key::Escape,
            Key::Char('u'),
            Key::Escape,
            Key::Char('y'),
            Key::Escape,
            Key::Char('o'),
        ];
        for (role, rank, low, expect) in [
            (&Slug::SYSOP, 100, true, true),
            (&Slug::SYSOP, 100, false, false),
            (&Slug::MEMBER, 10, true, false),
        ] {
            let mut screen = member_screen(role, rank, ascii(), size);
            screen.set_low_security(low);
            for (at, frame) in walk(&mut screen, &presses).iter().enumerate() {
                assert_eq!(
                    frame.contains("LOW-SECURITY"),
                    expect,
                    "{role:?} low={low} after {at} keys:\n{frame}"
                );
                // The caption is split by cursor moves in a diff redraw; its words are whole.
                assert_eq!(frame.contains("telnet"), expect, "{frame}");
            }
        }
    }

    fn all_sgr(bytes: &str) -> Vec<String> {
        bytes
            .split("\u{1b}[")
            .skip(1)
            .filter_map(|part| part.split_once('m').map(|(sgr, _)| sgr.to_owned()))
            .collect()
    }

    fn first_frame(term: Option<&str>, colorterm: Option<&str>, color: bool) -> String {
        let mut screen = member_screen(
            &Slug::MEMBER,
            10,
            TerminalSettings::default()
                .with_tier(Tier::Unicode)
                .with_color(color),
            PtySize::from_request(100, 30),
        );
        screen.set_client_terminal(term, colorterm);
        screen.apply_keys(&[Key::Char('x')]);
        text(&screen.enter().unwrap())
    }

    /// The client's `TERM` and `COLORTERM` pick the depth the bars are painted in, and the
    /// member's colour setting only lowers it.
    #[test]
    fn the_clients_terminal_picks_the_colour_depth_and_the_member_can_only_lower_it() {
        let has = |frame: &str, part: &str| all_sgr(frame).iter().any(|s| s.contains(part));
        // Truecolor: 24-bit backgrounds on the bars.
        let truecolor = first_frame(Some("xterm-256color"), Some("truecolor"), true);
        assert!(has(&truecolor, "48;2;"), "{:?}", all_sgr(&truecolor));
        // 256 colours: palette backgrounds, no 24-bit ones.
        let c256 = first_frame(Some("xterm-256color"), None, true);
        assert!(
            has(&c256, "48;5;") && !has(&c256, "48;2;"),
            "{:?}",
            all_sgr(&c256)
        );
        // Sixteen colours (TERM=xterm, or nothing said): palette cells 0 to 15 only, never a
        // 24-bit colour or a cell of the 256-colour cube.
        for frame in [
            first_frame(Some("xterm"), None, true),
            first_frame(None, None, true),
        ] {
            let sgr = all_sgr(&frame);
            assert!(!sgr.iter().any(|s| s.contains(";2;")), "{sgr:?}");
            for part in &sgr {
                let numbers: Vec<&str> = part.split(';').collect();
                for window in numbers.windows(3) {
                    if matches!(window[0], "38" | "48") && window[1] == "5" {
                        assert!(window[2].parse::<u8>().unwrap() <= 15, "{part}");
                    }
                }
            }
            assert!(
                sgr.iter().any(|s| s.contains("48;5;")),
                "bars are painted: {sgr:?}"
            );
        }
        // A terminal that says it has no colour gets none.
        let mono = first_frame(Some("vt100"), None, true);
        let colour_sgr: Vec<_> = all_sgr(&mono)
            .into_iter()
            .filter(|s| {
                s.split(';')
                    .any(|p| matches!(p.parse::<u8>(), Ok(30..=38 | 40..=48 | 90..=107)))
            })
            .collect();
        assert!(colour_sgr.is_empty(), "{colour_sgr:?}");
        // Colour off wins over a truecolor client; it never raises a lower depth either.
        let off = first_frame(Some("xterm-256color"), Some("truecolor"), false);
        let colour_sgr: Vec<_> = all_sgr(&off)
            .into_iter()
            .filter(|s| {
                s.split(';')
                    .any(|p| matches!(p.parse::<u8>(), Ok(30..=38 | 40..=48 | 90..=107)))
            })
            .collect();
        assert!(colour_sgr.is_empty(), "{colour_sgr:?}");
    }
}
