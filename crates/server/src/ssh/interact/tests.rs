#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::items_after_statements
)]

use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::terminal_settings::TerminalSettings;

use super::{Interact, MOUSE_OFF, MOUSE_ON, overlay_area};
use crate::config::BoardConfig;
use crate::ui::Ui;
use crate::ui::board::Board;
use crate::ui::depth::ColorDepth;
use crate::ui::frame::FrameSpec;
use crate::ui::input::mouse::{Button, Kind, Mouse};
use crate::ui::input::{Click, HitMap, Region};
use crate::ui::palette::{self, BoardName};
use crate::ui::toast::{Kind as ToastKind, Text};

fn interact() -> Interact {
    Interact::new(&Board::default(), Instant::now())
}

fn click(col: u16, row: u16) -> Mouse {
    Mouse {
        kind: Kind::Down(Button::Left),
        col,
        row,
    }
}

fn menu_hits() -> HitMap {
    HitMap::of(vec![Region::List {
        rect: Rect::new(0, 3, 20, 3),
        step: 1,
        offset: 0,
        len: 3,
        selected: Some(0),
        click: Click::Hotkeys(vec!['f', 'm', 'c']),
    }])
}

#[test]
fn mouse_mode_goes_on_and_off_only_when_the_setting_changes() {
    let strings = Ui::standard().unwrap().strings().clone();
    let mut state = interact();
    let now = Instant::now();
    assert_eq!(state.sync_mouse(false, &strings, now), b"");
    assert_eq!(state.sync_mouse(true, &strings, now), MOUSE_ON);
    assert_eq!(
        state.sync_mouse(true, &strings, now),
        b"",
        "once, not every frame"
    );
    assert_eq!(state.sync_mouse(false, &strings, now), MOUSE_OFF);
    assert_eq!(state.restore(), b"", "already off");
}

#[test]
fn leaving_turns_the_mouse_off_if_it_was_on_and_says_nothing_if_it_was_not() {
    let strings = Ui::standard().unwrap().strings().clone();
    let mut state = interact();
    state.sync_mouse(true, &strings, Instant::now());
    assert_eq!(state.restore(), MOUSE_OFF);
    assert_eq!(state.restore(), b"", "only once");
    assert_eq!(interact().restore(), b"");
}

#[test]
fn a_member_who_logs_in_with_the_mouse_on_is_not_toasted_but_a_change_is() {
    let strings = Ui::standard().unwrap().strings().clone();
    let mut state = interact();
    let now = Instant::now();
    state.sync_mouse(true, &strings, now);
    assert!(
        state.toasts.is_empty(),
        "the first frame after login is not a change"
    );
    state.sync_mouse(false, &strings, now);
    assert_eq!(
        state.toasts.announcements(),
        vec!["Notice: Mouse is off.".to_owned()]
    );
}

#[test]
fn reports_from_a_client_that_was_never_asked_do_nothing() {
    let mut state = interact();
    state.set_hits(menu_hits());
    assert!(state.mouse_keys(&[click(1, 3)], false).is_empty());
}

#[test]
fn clicks_and_the_wheel_become_keys_and_typing_in_the_same_batch_drops_them() {
    let strings = Ui::standard().unwrap().strings().clone();
    let mut state = interact();
    state.sync_mouse(true, &strings, Instant::now());
    state.set_hits(menu_hits());
    assert_eq!(
        state.mouse_keys(&[click(1, 4)], false),
        vec![crate::ssh::terminal::Key::Char('m')]
    );
    assert!(
        state.mouse_keys(&[click(1, 4)], true).is_empty(),
        "typing wins"
    );
    let wheel = Mouse {
        kind: Kind::ScrollDown,
        col: 1,
        row: 3,
    };
    assert_eq!(
        state.mouse_keys(&[wheel, wheel], false),
        vec![crate::ssh::terminal::Key::Down; 2]
    );
    // Right and middle clicks and releases do nothing.
    for kind in [
        Kind::Down(Button::Right),
        Kind::Down(Button::Middle),
        Kind::Up(Button::Left),
    ] {
        assert!(
            state
                .mouse_keys(
                    &[Mouse {
                        kind,
                        col: 1,
                        row: 3
                    }],
                    false
                )
                .is_empty()
        );
    }
    // Two clicks in one batch: only the first counts.
    assert_eq!(
        state.mouse_keys(&[click(1, 3), click(1, 5)], false).len(),
        1
    );
}

#[test]
fn nothing_is_pending_when_nothing_moves() {
    let state = interact();
    let now = Instant::now();
    assert!(state.next_wake(now, true).is_none());
    assert!(state.next_wake(now, false).is_none());
}

#[test]
fn toasts_and_the_palette_stay_clear_of_the_header_row_and_the_marker_cell() {
    let ui = Ui::standard().unwrap();
    let strings = ui.strings().clone();
    for (cols, rows) in [(80, 24), (100, 30), (132, 43), (60, 14)] {
        let area = Rect::new(0, 0, cols, rows);
        for tier in [Tier::Unicode, Tier::Ascii] {
            let settings = TerminalSettings::default().with_tier(tier);
            let look = ui.look(&settings, area, ColorDepth::TrueColor);
            let draw = |with_overlay: bool| {
                let mut state = interact();
                if with_overlay {
                    for n in 0..4 {
                        state.toasts.push(
                            ToastKind::Page,
                            Text::Member(&format!("a long message number {n} from somebody")),
                            &strings,
                            Instant::now(),
                        );
                    }
                    let found = palette::entries(&palette::Sources {
                        tree: russh_bbs_core::menu::main_tree(),
                        access: &russh_bbs_core::access::Snapshot::from_role(
                            Some(russh_bbs_core::store::AccountId::from_raw(1)),
                            &russh_bbs_core::access::Slug::SYSOP,
                            100,
                            &russh_bbs_core::access::Via::Account,
                            russh_bbs_core::access::Permission::ALL.iter().copied(),
                        ),
                        strings: &strings,
                        doors: vec!["Coin Toss".into()],
                        boards: vec![BoardName("General".into())],
                    });
                    state.open_palette(found, 0);
                }
                let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
                terminal
                    .draw(|frame| {
                        FrameSpec::new(&strings, look, tier)
                            .markers(&[SecurityMarker::Unencrypted])
                            .clock(Some("UTC 12:00"))
                            .draw(frame);
                        if with_overlay {
                            state.overlay(frame, look, &strings);
                        }
                    })
                    .unwrap();
                terminal.backend().buffer().clone()
            };
            let (bare, covered) = (draw(false), draw(true));
            // The status row (last) and the header row (first) are exactly as they were.
            for row in [0, rows - 1] {
                for col in 0..cols {
                    assert_eq!(
                        bare[(col, row)],
                        covered[(col, row)],
                        "{cols}x{rows} {tier:?} cell {col},{row} was covered"
                    );
                }
            }
            // And the overlay did draw something in between (it is not vacuous).
            if rows >= 14 {
                let changed = (1..rows - 1)
                    .any(|row| (0..cols).any(|col| bare[(col, row)] != covered[(col, row)]));
                assert!(changed, "{cols}x{rows}: overlay drew nothing");
            }
        }
    }
}

#[test]
fn the_overlay_area_is_inside_the_screen_for_every_size() {
    for rows in 0..60 {
        let area = overlay_area(Rect::new(0, 0, 80, rows));
        assert!(
            area.y + area.height <= rows.saturating_sub(1) || area.height == 0,
            "{rows}"
        );
        assert!(area.height == 0 || area.y >= 1);
    }
}

#[test]
fn running_a_palette_choice_steps_out_of_submenus_first() {
    let mut state = interact();
    let ui = Ui::standard().unwrap();
    let access = russh_bbs_core::access::Snapshot::from_role(
        Some(russh_bbs_core::store::AccountId::from_raw(1)),
        &russh_bbs_core::access::Slug::SYSOP,
        100,
        &russh_bbs_core::access::Via::Account,
        russh_bbs_core::access::Permission::ALL.iter().copied(),
    );
    let found = palette::entries(&palette::Sources {
        tree: russh_bbs_core::menu::main_tree(),
        access: &access,
        strings: ui.strings(),
        doors: Vec::new(),
        boards: Vec::new(),
    });
    state.open_palette(found, 2);
    let mut last = super::PaletteStep::Open;
    for c in "mail".chars() {
        last = state.palette_key(crate::ssh::terminal::Key::Char(c));
    }
    assert_eq!(last, super::PaletteStep::Open);
    let step = state.palette_key(crate::ssh::terminal::Key::Enter);
    let super::PaletteStep::Run(keys) = step else {
        panic!("{step:?}");
    };
    use crate::ssh::terminal::Key;
    assert_eq!(keys, vec![Key::Escape, Key::Escape, Key::Char('m')]);
    assert!(!state.palette_open());
    // Ctrl-C from the palette closes it and still logs off.
    state.open_palette(Vec::new(), 0);
    assert_eq!(
        state.palette_key(Key::Ctrl('c')),
        super::PaletteStep::PassThrough(Key::Ctrl('c'))
    );
    // A `q` typed in the palette is text, not a quit.
    state.open_palette(Vec::new(), 0);
    assert_eq!(state.palette_key(Key::Char('q')), super::PaletteStep::Open);
}

#[test]
fn a_ticker_asks_for_one_frame_per_second_at_most_and_none_when_static() {
    let board = Board::from_config(&BoardConfig {
        ticker: "Hello there, this is a long enough line to scroll across the bar".into(),
        ..BoardConfig::default()
    })
    .unwrap();
    let start = Instant::now();
    let mut state = Interact::new(&board, start);
    let mut frames = 0;
    // Ten seconds of 50 ms ticks: one frame per second, not 200.
    for ms in (0..10_000).step_by(50) {
        if state.tick(start + Duration::from_millis(ms), true) {
            frames += 1;
        }
    }
    assert!((9..=10).contains(&frames), "{frames} frames in 10 s");
    // Static: motion off and nothing else pending means no wake-up at all.
    state.ticker_mut().set_moving(false);
    assert!(state.next_wake(start, true).is_none());
}
