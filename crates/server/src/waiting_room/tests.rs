#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Drawing every page of every mode, checking the text on screen and that nothing about the
//! board's members or boards can appear.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use russh_bbs_core::access::Slug;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::RegistrationMode;
use russh_bbs_core::store::{AccountState, AccountStore, NewAccount, SettingKey, SettingsStore};
use russh_bbs_core::waiting_room::{
    RoomConfig, RoomInput, WaitingRoom, WaitingRoomSource, screens, strings,
};
use russh_bbs_store::SqliteStore;

use super::render;

const MODES: [RegistrationMode; 3] = [
    RegistrationMode::Invite,
    RegistrationMode::Apply,
    RegistrationMode::Open,
];

/// Names that exist only in the seeded store. None may ever be drawn.
const MEMBER_NAMES: [&str; 3] = ["zorgon_member", "qwerty_sysop", "hidden-hal"];
/// Placeholder board names. There are no boards yet (M3); the test pins the rule early.
const BOARD_NAMES: [&str; 3] = ["Secret Lounge", "Sysop Backchannel", "Trading Floor"];

fn draw(room: &WaitingRoom, cols: u16, rows: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
    let view = room.view();
    terminal.draw(|frame| render(frame, &view)).unwrap();
    let buffer = terminal.backend().buffer();
    let width = usize::from(buffer.area.width);
    buffer
        .content()
        .chunks(width)
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn seeded_store(dir: &std::path::Path, mode: RegistrationMode) -> SqliteStore {
    let store = SqliteStore::open(&dir.join("bbs.db")).await.unwrap();
    for (name, role) in [
        (MEMBER_NAMES[0], Slug::MEMBER),
        (MEMBER_NAMES[1], Slug::SYSOP),
        (MEMBER_NAMES[2], Slug::MEMBER),
    ] {
        store
            .create_account(NewAccount {
                username: sanitize_username(name).unwrap(),
                role,
                state: AccountState::Active,
            })
            .await
            .unwrap();
    }
    russh_bbs_core::store::set_registration_mode(&store, mode)
        .await
        .unwrap();
    store
        .set_setting(SettingKey::FounderClaimed, "true")
        .await
        .unwrap();
    store
}

/// Every page reachable in `mode`, including each form after a failed and a valid submission.
fn walk(mode: RegistrationMode, config: &RoomConfig) -> Vec<String> {
    let mut frames = Vec::new();
    for entry in screens::menu(mode) {
        let mut room = WaitingRoom::new(config, 5);
        frames.push(draw(&room, 80, 24));
        room.input(RoomInput::Char(entry.key));
        frames.push(draw(&room, 80, 24));
        if entry.page.is_form() {
            room.input(RoomInput::Enter);
            frames.push(draw(&room, 80, 24));
            for _ in 0..3 {
                for c in "someone".chars() {
                    room.input(RoomInput::Char(c));
                }
                room.input(RoomInput::Enter);
            }
            frames.push(draw(&room, 80, 24));
        }
    }
    frames
}

#[tokio::test]
async fn an_unknown_key_learns_nothing_in_any_mode() {
    for mode in MODES {
        let dir = tempfile::tempdir().unwrap();
        let store = seeded_store(dir.path(), mode).await;
        // The room is built exactly as the server builds it: from the store's settings.
        let config = store.load_room_config().await.unwrap();
        assert_eq!(config.mode, mode);

        let frames = walk(mode, &config);
        assert!(frames.len() >= 4, "{mode:?} walked {} frames", frames.len());
        for (i, frame) in frames.iter().enumerate() {
            let lower = frame.to_lowercase();
            for name in MEMBER_NAMES.iter().chain(&BOARD_NAMES) {
                assert!(
                    !lower.contains(&name.to_lowercase()),
                    "{mode:?} frame {i} leaks {name}"
                );
            }
            for word in [
                "founder",
                "registration_mode",
                "sysop",
                "online",
                "caller",
                "members:",
                "boards:",
                "posts:",
                "statistics",
            ] {
                assert!(
                    !lower.contains(word),
                    "{mode:?} frame {i} leaks {word:?}:\n{frame}"
                );
            }
        }
    }
}

#[tokio::test]
async fn a_member_name_typed_into_a_form_is_not_confirmed_or_denied() {
    // The visitor can type any handle. The reply is the same whether or not it is taken,
    // because the room never consults the store.
    let dir = tempfile::tempdir().unwrap();
    let store = seeded_store(dir.path(), RegistrationMode::Open).await;
    let config = store.load_room_config().await.unwrap();

    let reply = |name: &str| {
        let mut room = WaitingRoom::new(&config, 5);
        room.input(RoomInput::Char('n'));
        for c in name.chars() {
            room.input(RoomInput::Char(c));
        }
        room.input(RoomInput::Enter);
        room.view().notice.unwrap().text
    };
    assert_eq!(reply(MEMBER_NAMES[0]), reply("not_a_member"));
    assert_eq!(reply(MEMBER_NAMES[0]), strings::CREATE_ACCOUNT_UNAVAILABLE);
}

#[test]
fn each_mode_draws_its_own_menu() {
    let cases: [(RegistrationMode, &[&str], &[&str]); 3] = [
        (
            RegistrationMode::Invite,
            &["Request an invite", "Redeem an invite code"],
            &["Apply for membership", "Create an account"],
        ),
        (
            RegistrationMode::Apply,
            &["Apply for membership"],
            &[
                "Request an invite",
                "Redeem an invite code",
                "Create an account",
            ],
        ),
        (
            RegistrationMode::Open,
            &["Create an account"],
            &[
                "Request an invite",
                "Redeem an invite code",
                "Apply for membership",
            ],
        ),
    ];
    for (mode, present, absent) in cases {
        let room = WaitingRoom::new(
            &RoomConfig {
                mode,
                ..RoomConfig::default()
            },
            5,
        );
        let screen = draw(&room, 80, 24);
        for text in present.iter().chain(&["Read the rules", "Disconnect"]) {
            assert!(screen.contains(text), "{mode:?} shows {text:?}:\n{screen}");
        }
        for text in absent {
            assert!(!screen.contains(text), "{mode:?} hides {text:?}:\n{screen}");
        }
    }
}

#[test]
fn the_banner_is_drawn_neutralized() {
    let config = RoomConfig {
        banner: Some("\u{1b}[31mEVIL\u{1b}[0m \u{f023}\u{e0b0} welcome".into()),
        ..RoomConfig::default()
    };
    let room = WaitingRoom::new(&config, 5);
    let screen = draw(&room, 80, 24);
    assert!(screen.contains("EVIL"), "{screen}");
    assert!(screen.contains("welcome"));
    for bad in ['\u{1b}', '\u{f023}', '\u{e0b0}'] {
        assert!(!screen.contains(bad), "drew {bad:?}");
    }
    assert!(
        screen.contains('\u{fffd}'),
        "private-use shown as the replacement mark"
    );
}

#[test]
fn the_rules_page_keeps_the_honest_privacy_note() {
    let mut room = WaitingRoom::new(&RoomConfig::default(), 5);
    room.input(RoomInput::Char('r'));
    let screen = draw(&room, 100, 24);
    // The note wraps; check a stable fragment.
    assert!(screen.contains("server can read everything"), "{screen}");
    assert!(
        !screen.contains('\u{1f512}'),
        "no lock glyph on a server-readable screen"
    );
}

#[test]
fn the_notice_uses_words_not_only_color() {
    let mut room = WaitingRoom::new(&RoomConfig::default(), 5);
    room.input(RoomInput::Char('i'));
    room.input(RoomInput::Enter);
    room.input(RoomInput::Enter);
    let screen = draw(&room, 80, 24);
    assert!(screen.contains("Problem:"), "{screen}");
}

#[test]
fn small_terminals_do_not_panic_on_any_page() {
    for mode in MODES {
        for entry in screens::menu(mode) {
            for (cols, rows) in [(1, 1), (5, 2), (20, 6), (300, 100)] {
                let mut room = WaitingRoom::new(&RoomConfig::default(), 5);
                room.input(RoomInput::Char(entry.key));
                let _ = draw(&room, cols, rows);
            }
        }
    }
}
