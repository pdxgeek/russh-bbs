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

// ---- UI-9: the mouse, the command palette, toasts and the ticker on a real screen ----

mod interaction {
    #![allow(clippy::panic, clippy::assert_is_empty)]

    use std::time::{Duration, Instant};

    use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
    use russh_bbs_core::icons::Tier;
    use russh_bbs_core::sanitize::sanitize_username;
    use russh_bbs_core::store::AccountId;
    use russh_bbs_core::terminal_settings::{LayoutId, MemberFlow, TerminalSettings};

    use super::{Key, PtySize, RenderCap, Screen, text};
    use crate::config::BoardConfig;
    use crate::ssh::screen::{KeyEffects, PaletteAsk};
    use crate::ui::Ui;
    use crate::ui::board::Board;
    use crate::ui::input::mouse::{Button, Kind, Mouse};
    use crate::ui::input::{Click, Region};
    use crate::ui::toast::{Kind as ToastKind, Text};

    const ON: &str = "\x1b[?1000h\x1b[?1006h";
    const OFF: &str = "\x1b[?1006l\x1b[?1000l";

    fn settings(mouse: bool) -> TerminalSettings {
        TerminalSettings::default()
            .with_tier(Tier::Unicode)
            .with_mouse(mouse)
    }

    fn screen_with(ui: Ui, permissions: &[Permission], settings: TerminalSettings) -> Screen {
        let flow = MemberFlow::new(
            &sanitize_username("alice").unwrap(),
            Snapshot::from_role(
                Some(AccountId::from_raw(1)),
                &Slug::MEMBER,
                10,
                &Via::Account,
                permissions.iter().copied(),
            ),
            None,
            settings,
        );
        let mut screen = Screen::new(
            PtySize::from_request(100, 30),
            RenderCap::default(),
            super::super::Content::Member(Box::new(flow)),
            std::sync::Arc::new(ui),
        )
        .unwrap();
        screen.set_client_terminal(Some("xterm-256color"), Some("truecolor"));
        screen
    }

    fn member(permissions: &[Permission], settings: TerminalSettings) -> Screen {
        screen_with(Ui::standard().unwrap(), permissions, settings)
    }

    /// Enter, then press a key past the sign-in page so the main menu is up and drawn.
    fn at_menu(screen: &mut Screen) {
        screen.enter().unwrap();
        screen.apply_keys(&[Key::Char('x')]);
        screen.render().unwrap();
    }

    fn all() -> Vec<Permission> {
        Permission::ALL.to_vec()
    }

    fn click(col: u16, row: u16) -> Mouse {
        Mouse {
            kind: Kind::Down(Button::Left),
            col,
            row,
        }
    }

    /// Where the last frame drew the menu row whose hotkey is `hotkey`.
    fn row_of(screen: &Screen, hotkey: char) -> (u16, u16) {
        for region in screen.interact.hits().regions() {
            if let Region::List {
                rect,
                offset,
                click: Click::Hotkeys(keys),
                ..
            } = region
                && let Some(at) = keys.iter().position(|k| *k == hotkey)
            {
                let row = u16::try_from(at - offset).unwrap();
                return (rect.x + 2, rect.y + row);
            }
        }
        panic!("no menu row for {hotkey:?}");
    }

    fn redraw(screen: &mut Screen) -> String {
        let size = screen.pty;
        screen.set_size(size);
        text(&screen.render().unwrap().unwrap_or_default())
    }

    #[test]
    fn the_mouse_is_off_by_default_and_the_client_is_never_told_to_report_it() {
        let mut screen = member(&all(), settings(false));
        let first = text(&screen.enter().unwrap());
        assert!(
            !first.contains("1000h") && !first.contains("1006h"),
            "{first:?}"
        );
        let last = text(&screen.leave().unwrap());
        assert!(!last.contains("1000") && !last.contains("1006"), "{last:?}");
    }

    #[test]
    fn mouse_reporting_turns_on_with_the_first_frame_and_off_before_the_screen_is_left() {
        let mut screen = member(&all(), settings(true));
        let first = text(&screen.enter().unwrap());
        assert!(first.contains(ON), "{first:?}");
        let last = text(&screen.leave().unwrap());
        assert!(last.contains(OFF), "{last:?}");
        // Off while the alternate screen is still up, so the person's own terminal is left as
        // it was.
        assert!(
            last.find(OFF).unwrap() < last.find("\x1b[?1049l").unwrap(),
            "{last:?}"
        );
        // And never twice.
        assert!(!text(&screen.leave().unwrap()).contains(OFF));
    }

    #[test]
    fn turning_the_setting_off_mid_session_turns_reporting_off_and_says_so() {
        let mut screen = member(&all(), settings(true));
        at_menu(&mut screen);
        // The settings screen: `s` opens it, `o` flips the mouse and saves.
        let effects = screen.apply_keys(&[Key::Char('s'), Key::Char('o')]);
        assert!(effects.save_settings.is_some_and(|s| !s.mouse()));
        let frame = text(&screen.render().unwrap().unwrap());
        assert!(frame.contains(OFF), "{frame:?}");
        // The toast is the next frame.
        let toast = redraw(&mut screen);
        assert!(toast.contains("Mouse") && toast.contains("off"), "{toast}");
    }

    #[test]
    fn reports_do_nothing_while_the_mouse_is_off() {
        let mut screen = member(&all(), settings(false));
        at_menu(&mut screen);
        // No regions are acted on: a click where Forums is draws nothing and presses nothing.
        let (col, row) = row_of(&screen, 'f');
        assert!(screen.mouse_keys(&[click(col, row)], false).is_empty());
    }

    /// Keyboard parity: a click on a row presses exactly the key the row names, so it can do
    /// nothing a typed key cannot, and every row is also reachable without the mouse.
    #[test]
    fn a_click_on_a_menu_row_does_exactly_what_its_hotkey_does() {
        for layout in [
            LayoutId::Classic,
            LayoutId::Lightbar,
            LayoutId::Dashboard,
            LayoutId::Wide,
        ] {
            let make = |_mouse| member(&all(), settings(true).with_layout(layout));
            for hotkey in ['f', 'm', 'c', 'd', 'u', 'y', 'i', 's', 'o', 'l'] {
                let mut clicked = make(true);
                at_menu(&mut clicked);
                let (col, row) = row_of(&clicked, hotkey);
                let keys = clicked.mouse_keys(&[click(col, row)], false);
                assert_eq!(keys, vec![Key::Char(hotkey)], "{layout:?} {hotkey}");
                let by_mouse = clicked.apply_keys(&keys);
                let mut typed = make(false);
                at_menu(&mut typed);
                let by_key = typed.apply_keys(&[Key::Char(hotkey)]);
                assert_eq!(
                    format!("{by_mouse:?}"),
                    format!("{by_key:?}"),
                    "{layout:?} {hotkey}"
                );
                assert_eq!(
                    redraw(&mut clicked),
                    redraw(&mut typed),
                    "{layout:?} {hotkey}"
                );
            }
        }
    }

    /// The compact layout is for tiny terminals and screen readers: it has no mouse targets, so a
    /// click or the wheel does nothing there and the keys do everything.
    #[test]
    fn the_compact_layout_has_no_mouse_targets_and_stays_keyboard_only() {
        let mut screen = member(&all(), settings(true).with_layout(LayoutId::Compact));
        at_menu(&mut screen);
        assert!(screen.interact.hits().regions().is_empty());
        let wheel = Mouse {
            kind: Kind::ScrollDown,
            col: 5,
            row: 5,
        };
        assert!(screen.mouse_keys(&[click(5, 5), wheel], false).is_empty());
        let effects = screen.apply_keys(&[Key::Char('f')]);
        assert!(effects.forum_request.is_some());
    }

    #[test]
    fn a_click_that_hits_nothing_changes_nothing() {
        let mut screen = member(&all(), settings(true));
        at_menu(&mut screen);
        for (col, row) in [(0, 0), (99, 29), (50, 0), (50, 29), (200, 200)] {
            assert!(
                screen.mouse_keys(&[click(col, row)], false).is_empty(),
                "{col},{row}"
            );
        }
    }

    #[test]
    fn the_wheel_moves_the_lightbar_like_the_arrow_keys() {
        let make = || member(&all(), settings(true).with_layout(LayoutId::Lightbar));
        let mut wheeled = make();
        at_menu(&mut wheeled);
        let (col, row) = row_of(&wheeled, 'f');
        let wheel = Mouse {
            kind: Kind::ScrollDown,
            col,
            row,
        };
        let keys = wheeled.mouse_keys(&[wheel, wheel], false);
        assert_eq!(keys, vec![Key::Down, Key::Down]);
        wheeled.apply_keys(&keys);
        let mut typed = make();
        at_menu(&mut typed);
        typed.apply_keys(&[Key::Down, Key::Down]);
        assert_eq!(redraw(&mut wheeled), redraw(&mut typed));
    }

    #[test]
    fn a_click_in_the_same_batch_as_typing_is_dropped() {
        let mut screen = member(&all(), settings(true));
        at_menu(&mut screen);
        let (col, row) = row_of(&screen, 'f');
        assert!(screen.mouse_keys(&[click(col, row)], true).is_empty());
    }

    // ---- the palette ----

    fn open_palette(screen: &mut Screen) -> (KeyEffects, String) {
        let effects = screen.apply_keys(&[Key::Ctrl('k')]);
        let frame = redraw(screen);
        (effects, frame)
    }

    #[test]
    fn ctrl_k_opens_the_palette_and_a_chosen_area_runs_through_the_normal_path() {
        let mut screen = member(&all(), settings(false));
        at_menu(&mut screen);
        let (effects, frame) = open_palette(&mut screen);
        assert_eq!(
            effects.palette,
            PaletteAsk::Boards,
            "the session is asked for the board names"
        );
        assert!(frame.contains("Go to"), "{frame}");
        // Type, and see only the match; Enter runs it as the Forums hotkey would.
        let effects =
            screen.apply_keys(&[Key::Char('f'), Key::Char('o'), Key::Char('r'), Key::Enter]);
        assert!(effects.forum_request.is_some(), "{effects:?}");
        let after = redraw(&mut screen);
        assert!(!after.contains("Go to"), "the palette closed");
    }

    #[test]
    fn the_palette_runs_a_setting_page_and_a_door_by_their_key_paths() {
        let mut screen = member(&all(), settings(true));
        at_menu(&mut screen);
        screen.apply_keys(&[Key::Ctrl('k')]);
        let effects = screen.apply_keys(&[
            Key::Char('m'),
            Key::Char('o'),
            Key::Char('u'),
            Key::Char('s'),
            Key::Char('e'),
            Key::Enter,
        ]);
        // "mouse" matches "Settings: mouse on or off" best: Settings, then `o`.
        assert!(
            effects.save_settings.is_some_and(|s| !s.mouse()),
            "{effects:?}"
        );
    }

    #[test]
    fn a_member_without_a_permission_never_sees_what_it_guards_in_the_palette() {
        let without_forums: Vec<Permission> = Permission::ALL
            .iter()
            .copied()
            .filter(|p| *p != Permission::ForumList && *p != Permission::DoorsRun)
            .collect();
        let mut screen = member(&without_forums, settings(false));
        at_menu(&mut screen);
        open_palette(&mut screen);
        screen.set_palette_boards(vec!["Secret Board".to_owned()]);
        let frame = redraw(&mut screen);
        assert!(
            !frame.contains("Forums") && !frame.contains("Secret"),
            "{frame}"
        );
        // Typing the hidden name finds nothing, and Enter on nothing runs nothing.
        let effects = screen.apply_keys(&[
            Key::Char('f'),
            Key::Char('o'),
            Key::Char('r'),
            Key::Char('u'),
            Key::Char('m'),
            Key::Enter,
        ]);
        assert!(effects.forum_request.is_none(), "{effects:?}");
        let frame = redraw(&mut screen);
        assert!(frame.contains("Nothing matches"), "{frame}");
    }

    #[test]
    fn boards_the_service_returns_become_entries_and_hostile_names_are_cleaned() {
        let mut screen = member(&all(), settings(false));
        at_menu(&mut screen);
        open_palette(&mut screen);
        screen.set_palette_boards(vec![
            "General".to_owned(),
            "Evil\u{1b}[31m\u{27e6}board".to_owned(),
        ]);
        // Only eight rows show at once; typing narrows to the boards.
        screen.apply_keys(&[Key::Char('g'), Key::Char('e'), Key::Char('n')]);
        let frame = redraw(&mut screen);
        assert!(frame.contains("General"), "{frame}");
        let at = frame.find('\u{27e6}');
        assert!(
            at.is_none(),
            "reserved delimiter drawn: {:?}",
            at.map(|i| &frame[i.saturating_sub(80)..(i + 40).min(frame.len())])
        );
        // Pick the second board: Forums, down once, Enter.
        screen.apply_keys(&[Key::Backspace, Key::Backspace, Key::Backspace]);
        let effects =
            screen.apply_keys(&[Key::Char('e'), Key::Char('v'), Key::Char('i'), Key::Enter]);
        assert!(effects.forum_request.is_some(), "{effects:?}");
    }

    #[test]
    fn the_palette_is_offered_on_the_menu_only_and_the_waiting_room_has_none() {
        // The waiting room.
        let mut room = super::screen(PtySize::from_request(80, 24));
        room.enter().unwrap();
        let effects = room.apply_keys(&[Key::Ctrl('k')]);
        assert_eq!(effects.palette, PaletteAsk::Nothing);
        let frame = redraw(&mut room);
        assert!(!frame.contains("Go to"), "{frame}");
        // Ctrl-K inside the settings screen does nothing either.
        let mut screen = member(&all(), settings(false));
        at_menu(&mut screen);
        screen.apply_keys(&[Key::Char('s')]);
        let effects = screen.apply_keys(&[Key::Ctrl('k')]);
        assert_eq!(effects.palette, PaletteAsk::Nothing);
        assert!(!redraw(&mut screen).contains("Go to"));
    }

    /// Where there is no palette, `Ctrl-K` is the key it always was: the screen gets it, so a
    /// screen that gives it a meaning keeps it.
    #[test]
    fn ctrl_k_outside_the_menu_reaches_the_screen_as_it_always_did() {
        let mut with = member(&all(), settings(false));
        let mut without = member(&all(), settings(false));
        for screen in [&mut with, &mut without] {
            at_menu(screen);
            screen.apply_keys(&[Key::Char('s')]);
        }
        let by_keys = with.apply_keys(&[Key::Ctrl('k')]);
        let plain = without.apply_keys_plain(&[Key::Ctrl('k')]);
        assert_eq!(format!("{by_keys:?}"), format!("{plain:?}"));
        assert_eq!(redraw(&mut with), redraw(&mut without));
    }

    #[test]
    fn escape_closes_the_palette_and_ctrl_c_still_logs_off() {
        let mut screen = member(&all(), settings(false));
        at_menu(&mut screen);
        open_palette(&mut screen);
        screen.apply_keys(&[Key::Escape]);
        assert!(!redraw(&mut screen).contains("Go to"));
        open_palette(&mut screen);
        let effects = screen.apply_keys(&[Key::Ctrl('c')]);
        assert!(effects.disconnect, "{effects:?}");
    }

    #[test]
    fn a_click_on_a_palette_row_runs_it_and_the_screen_under_it_does_not_answer() {
        let mut screen = member(&all(), settings(true));
        at_menu(&mut screen);
        open_palette(&mut screen);
        let rows: Vec<_> = screen
            .interact
            .hits()
            .regions()
            .iter()
            .filter_map(|r| match r {
                Region::List {
                    rect,
                    click: Click::Open,
                    ..
                } => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(rows.len(), 1, "only the palette answers the mouse");
        assert_eq!(screen.interact.hits().regions().len(), 1);
        // The first row is the best match for an empty query: the first area, Forums. A click
        // there is Enter.
        let keys = screen.mouse_keys(&[click(rows[0].x + 1, rows[0].y)], false);
        let effects = screen.apply_keys(&keys);
        assert!(effects.forum_request.is_some(), "{keys:?} {effects:?}");
    }

    // ---- toasts ----

    #[test]
    fn a_toast_appears_over_the_body_and_goes_away_by_itself() {
        let mut screen = member(&all(), settings(false));
        at_menu(&mut screen);
        screen.toast(
            ToastKind::Page,
            Text::Member("hello \u{1b}[31mthere\nfriend"),
        );
        let frame = redraw(&mut screen);
        assert!(
            frame.contains("Page:") && frame.contains("hello"),
            "{frame}"
        );
        assert!(
            !frame.contains("\u{1b}[31mthere"),
            "controls are stripped: {frame:?}"
        );
        let wake = screen.next_wake(Instant::now()).unwrap();
        screen.tick(wake + Duration::from_secs(10));
        let gone = text(&screen.render().unwrap().unwrap());
        assert!(!gone.contains("hello"), "{gone}");
        assert!(
            screen.next_wake(Instant::now()).is_none(),
            "no timer once nothing is pending"
        );
    }

    // ---- the ticker and the redraw budget ----

    fn ticker_ui(text: &str) -> Ui {
        Ui::standard().unwrap().with_board(
            Board::from_config(&BoardConfig {
                ticker: text.to_owned(),
                ..BoardConfig::default()
            })
            .unwrap(),
        )
    }

    #[test]
    fn the_header_shows_the_sysop_ticker_and_motion_off_makes_it_static() {
        let line = "Tonight: the Saltmarch tournament starts at nine";
        let mut moving = screen_with(ticker_ui(line), &all(), settings(false).with_motion(true));
        at_menu(&mut moving);
        let first = redraw(&mut moving);
        assert!(first.contains("Tonight"), "{first}");
        // Static: with motion off nothing is ever pending, so no timer-driven frame is asked for.
        let mut still = screen_with(ticker_ui(line), &all(), settings(false).with_motion(false));
        at_menu(&mut still);
        let frame = redraw(&mut still);
        assert!(frame.contains("Tonight"), "{frame}");
        assert!(still.next_wake(Instant::now()).is_none());
        let now = Instant::now();
        for secs in 0..30 {
            still.tick(now + Duration::from_secs(secs));
        }
        assert!(still.render().unwrap().is_none(), "static means no redraws");
    }

    /// The redraw budget: with motion on, the ticker costs at most one frame a second, and each
    /// frame sends only the header row, not the screen.
    #[test]
    fn the_moving_ticker_stays_inside_the_redraw_budget() {
        let mut screen = screen_with(
            ticker_ui("A fairly long ticker line that has to scroll to be read in full by anyone"),
            &all(),
            settings(false).with_motion(true),
        );
        let full = screen.enter().unwrap().len();
        let start = Instant::now();
        let mut frames = 0;
        let mut biggest = 0;
        for ms in (0..10_000).step_by(50) {
            screen.tick(start + Duration::from_millis(ms));
            if let Some(bytes) = screen.render().unwrap() {
                frames += 1;
                biggest = biggest.max(bytes.len());
            }
        }
        // One step a second; the clock reads the real time so allow the boundary either side.
        assert!((1..=12).contains(&frames), "{frames} frames in ten seconds");
        assert!(
            biggest < full / 2,
            "a step sends the header, not the screen: {biggest} of {full}"
        );
    }
}
