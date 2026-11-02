#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::unused_async_trait_impl
)]

use std::collections::HashMap;
use std::sync::Mutex;

use super::screens::{entry_for, menu, pages};
use super::{
    NoticeKind, Page, RoomConfig, RoomEffect, RoomInput, View, WaitingRoom, WaitingRoomSource,
    strings,
};
use crate::access::NoMembers;
use crate::store::{
    RegistrationMode, SettingKey, SettingsStore, StoreError, set_registration_mode,
};

const ALL_MODES: [RegistrationMode; 3] = [
    RegistrationMode::Invite,
    RegistrationMode::Apply,
    RegistrationMode::Open,
];

fn room(mode: RegistrationMode) -> WaitingRoom {
    WaitingRoom::new(
        &RoomConfig {
            mode,
            ..RoomConfig::default()
        },
        5,
    )
}

fn type_text(room: &mut WaitingRoom, text: &str) {
    for c in text.chars() {
        room.input(RoomInput::Char(c));
    }
}

/// Every string a view can show, flattened.
fn all_text(view: &View) -> String {
    let mut out = vec![view.title.to_owned(), view.heading.to_owned()];
    out.extend(view.banner.iter().cloned());
    out.extend(view.body.iter().cloned());
    out.extend(view.note.map(str::to_owned));
    out.extend(view.menu.iter().map(|m| m.label.to_owned()));
    for field in &view.fields {
        out.push(field.label.to_owned());
        out.push(field.value.clone());
    }
    out.extend(view.notice.map(|n| n.text.to_owned()));
    out.push(view.footer.to_owned());
    out.join("\n")
}

// ---- screen sets per mode ------------------------------------------------------------------

#[test]
fn each_mode_has_its_own_screen_set() {
    assert_eq!(
        pages(RegistrationMode::Invite),
        [
            Page::Home,
            Page::Rules,
            Page::RequestInvite,
            Page::RedeemInvite
        ]
    );
    assert_eq!(
        pages(RegistrationMode::Apply),
        [Page::Home, Page::Rules, Page::Apply]
    );
    assert_eq!(
        pages(RegistrationMode::Open),
        [Page::Home, Page::Rules, Page::CreateAccount]
    );
}

#[test]
fn every_menu_key_is_unique_and_reaches_its_page() {
    for mode in ALL_MODES {
        let entries = menu(mode);
        for (i, entry) in entries.iter().enumerate() {
            assert_ne!(entry.key, crate::waiting_room::screens::QUIT_KEY);
            assert!(entries[i + 1..].iter().all(|other| other.key != entry.key));
            assert_eq!(entry_for(mode, entry.key.to_ascii_uppercase()), Some(entry));
        }
    }
}

#[test]
fn the_home_view_lists_exactly_the_modes_menu() {
    for mode in ALL_MODES {
        let view = room(mode).view();
        let labels: Vec<_> = view.menu.iter().map(|m| m.label).collect();
        let expected: Vec<_> = menu(mode).iter().map(|e| e.label).collect();
        assert_eq!(labels, expected, "{mode:?}");
        assert!(!view.banner.is_empty(), "{mode:?} shows a banner");
    }
}

#[test]
fn a_page_from_another_mode_cannot_be_reached() {
    // Keys for other modes' pages do nothing.
    for (mode, foreign) in [
        (RegistrationMode::Invite, ['a', 'n']),
        (RegistrationMode::Apply, ['i', 'c']),
        (RegistrationMode::Open, ['i', 'a']),
    ] {
        for key in foreign {
            let mut room = room(mode);
            assert_eq!(room.input(RoomInput::Char(key)), RoomEffect::Stay);
            assert_eq!(room.page(), Page::Home, "{mode:?} key {key}");
        }
    }
}

#[test]
fn navigation_reaches_every_page_and_returns_home() {
    for mode in ALL_MODES {
        for entry in menu(mode) {
            let mut room = room(mode);
            room.input(RoomInput::Char(entry.key));
            assert_eq!(room.page(), entry.page);
            assert_eq!(room.view().heading, entry.page.heading());
            room.input(RoomInput::Escape);
            assert_eq!(room.page(), Page::Home);
        }
    }
}

#[test]
fn q_disconnects_from_home_but_is_text_in_a_form() {
    let mut home = room(RegistrationMode::Invite);
    assert_eq!(home.input(RoomInput::Char('q')), RoomEffect::Disconnect);

    let mut form = room(RegistrationMode::Invite);
    form.input(RoomInput::Char('i'));
    assert_eq!(form.input(RoomInput::Char('q')), RoomEffect::Stay);
    assert_eq!(form.view().fields[0].value, "q");
    assert_eq!(form.input(RoomInput::Quit), RoomEffect::Disconnect);
}

#[test]
fn rules_page_shows_the_fixed_privacy_note() {
    let mut room = room(RegistrationMode::Invite);
    room.input(RoomInput::Char('r'));
    let view = room.view();
    assert_eq!(view.note, Some(strings::PRIVACY_NOTE));
    assert_eq!(view.body.len(), strings::DEFAULT_RULES.lines().count());
}

// ---- forms ---------------------------------------------------------------------------------

#[test]
fn a_valid_submission_says_it_is_not_open_and_keeps_nothing() {
    let cases = [
        (
            RegistrationMode::Invite,
            'i',
            strings::REQUEST_INVITE_UNAVAILABLE,
            2,
        ),
        (
            RegistrationMode::Invite,
            'c',
            strings::REDEEM_INVITE_UNAVAILABLE,
            2,
        ),
        (RegistrationMode::Apply, 'a', strings::APPLY_UNAVAILABLE, 2),
        (
            RegistrationMode::Open,
            'n',
            strings::CREATE_ACCOUNT_UNAVAILABLE,
            1,
        ),
    ];
    for (mode, key, message, field_count) in cases {
        let mut room = room(mode);
        room.input(RoomInput::Char(key));
        assert_eq!(room.view().fields.len(), field_count);
        for _ in 0..field_count {
            type_text(&mut room, "validname");
            room.input(RoomInput::Enter);
        }
        let notice = room.view().notice.expect("a result");
        assert_eq!(notice.kind, NoticeKind::Info, "{mode:?}");
        assert_eq!(notice.text, message);
        assert!(
            room.view().fields.iter().all(|f| f.value.is_empty()),
            "typed text must not be retained after a submission that is dropped"
        );
        // The message never claims success.
        for claim in ["sent to", "submitted", "approved", "created!", "success"] {
            assert!(!notice.text.to_lowercase().contains(claim), "{claim}");
        }
        assert!(notice.text.contains("not"), "{}", notice.text);
    }
}

#[test]
fn validation_errors_depend_only_on_the_text() {
    let mut room = room(RegistrationMode::Open);
    room.input(RoomInput::Char('n'));

    room.input(RoomInput::Enter);
    assert_eq!(room.view().notice.unwrap().text, strings::ERR_EMPTY);

    type_text(&mut room, "a");
    room.input(RoomInput::Enter);
    assert_eq!(
        room.view().notice.unwrap().text,
        strings::ERR_HANDLE_TOO_SHORT
    );

    // Backspace edits the active field.
    room.input(RoomInput::Backspace);
    type_text(&mut room, "-bad");
    room.input(RoomInput::Enter);
    assert_eq!(room.view().notice.unwrap().text, strings::ERR_HANDLE_CHARS);
    assert_eq!(room.view().notice.unwrap().kind, NoticeKind::Error);
}

#[test]
fn enter_moves_through_fields_before_submitting() {
    let mut room = room(RegistrationMode::Invite);
    room.input(RoomInput::Char('i'));
    assert!(room.view().fields[0].active);
    type_text(&mut room, "alice");
    room.input(RoomInput::Enter);
    let view = room.view();
    assert!(view.fields[1].active);
    assert_eq!(view.fields[0].value, "alice");
    assert!(view.notice.is_none(), "not submitted yet");
}

#[test]
fn typed_text_is_length_capped_and_sanitized() {
    let mut room = room(RegistrationMode::Invite);
    room.input(RoomInput::Char('i'));
    type_text(&mut room, &"x".repeat(100));
    assert_eq!(room.view().fields[0].value.chars().count(), 24);

    room.input(RoomInput::Enter);
    type_text(&mut room, &"y".repeat(1_000));
    assert_eq!(room.view().fields[1].value.chars().count(), 280);

    // Control characters and private-use glyphs never reach the buffer intact.
    let mut room = room_in_form();
    for c in ['\u{1b}', '\u{7}', '\u{f023}', '\u{202e}', 'o', 'k'] {
        room.input(RoomInput::Char(c));
    }
    let value = room.view().fields[0].value.clone();
    assert!(!value.contains('\u{1b}') && !value.contains('\u{7}'));
    assert!(!value.contains('\u{f023}') && !value.contains('\u{202e}'));
    assert!(value.ends_with("ok"));
}

fn room_in_form() -> WaitingRoom {
    let mut room = room(RegistrationMode::Invite);
    room.input(RoomInput::Char('i'));
    room
}

#[test]
fn a_session_is_disconnected_after_too_many_submissions() {
    let config = RoomConfig {
        mode: RegistrationMode::Open,
        ..RoomConfig::default()
    };
    let mut room = WaitingRoom::new(&config, 3);
    room.input(RoomInput::Char('n'));
    for attempt in 1..=3 {
        assert_eq!(room.input(RoomInput::Enter), RoomEffect::Stay, "{attempt}");
    }
    // Empty submissions count too: they cost the server as much as valid ones.
    assert_eq!(room.input(RoomInput::Enter), RoomEffect::Disconnect);
    assert_eq!(room.view().notice.unwrap().text, strings::ERR_TOO_MANY);
}

#[test]
fn leaving_a_form_clears_it() {
    let mut room = room_in_form();
    type_text(&mut room, "secret");
    room.input(RoomInput::Escape);
    room.input(RoomInput::Char('i'));
    assert_eq!(room.view().fields[0].value, "");
}

// ---- sysop text ----------------------------------------------------------------------------

#[test]
fn banner_and_rules_are_sanitized() {
    let config = RoomConfig {
        banner: Some("\u{1b}[31mEVIL\u{1b}[0m \u{f023} \u{202e}done".into()),
        rules: Some("rule\u{7} one\r\n\u{9b}31mrule two \u{e0b0}".into()),
        ..RoomConfig::default()
    };
    let mut room = WaitingRoom::new(&config, 5);
    let mut seen = all_text(&room.view());
    room.input(RoomInput::Char('r'));
    seen.push_str(&all_text(&room.view()));

    assert!(seen.contains("EVIL"), "text survives: {seen:?}");
    assert!(seen.contains("rule one"));
    assert!(seen.contains("rule two"));
    for bad in [
        '\u{1b}', '\u{7}', '\u{9b}', '\u{f023}', '\u{e0b0}', '\u{202e}',
    ] {
        assert!(!seen.contains(bad), "leaked {bad:?} in {seen:?}");
    }
    assert!(!seen.contains("[31m"), "escape sequence removed whole");
    assert!(
        seen.contains('\u{fffd}'),
        "private-use becomes visible U+FFFD"
    );
}

#[test]
fn banner_and_rules_are_length_capped() {
    let config = RoomConfig {
        banner: Some("b".repeat(100_000)),
        rules: Some("r".repeat(100_000)),
        ..RoomConfig::default()
    };
    let mut room = WaitingRoom::new(&config, 5);
    let banner: usize = room.view().banner.iter().map(|l| l.chars().count()).sum();
    assert_eq!(banner, super::BANNER_MAX_CHARS);
    room.input(RoomInput::Char('r'));
    let rules: usize = room.view().body.iter().map(|l| l.chars().count()).sum();
    assert_eq!(rules, super::RULES_MAX_CHARS);
}

#[test]
fn defaults_apply_when_text_is_unset() {
    let view = WaitingRoom::new(&RoomConfig::default(), 5).view();
    assert_eq!(view.banner, [strings::DEFAULT_BANNER]);
}

// ---- no leak -------------------------------------------------------------------------------

#[test]
fn views_carry_no_member_board_or_stat_words_in_any_mode() {
    let forbidden = [
        "members online",
        "who's online",
        "last caller",
        "callers",
        "boards",
        "posts",
        "users",
        "member list",
        "total",
    ];
    for mode in ALL_MODES {
        for page_key in std::iter::once(None).chain(menu(mode).iter().map(|e| Some(e.key))) {
            let mut room = room(mode);
            if let Some(key) = page_key {
                room.input(RoomInput::Char(key));
            }
            // Walk through a submission too, so notices are covered.
            for _ in 0..3 {
                room.input(RoomInput::Enter);
            }
            let text = all_text(&room.view()).to_lowercase();
            for word in forbidden {
                assert!(!text.contains(word), "{mode:?} {page_key:?} leaks {word:?}");
            }
        }
    }
}

// ---- source --------------------------------------------------------------------------------

#[derive(Default)]
struct MemorySettings(Mutex<HashMap<&'static str, String>>);

impl SettingsStore for MemorySettings {
    async fn get_setting(&self, key: SettingKey) -> Result<Option<String>, StoreError> {
        Ok(self.0.lock().unwrap().get(key.as_str()).cloned())
    }

    async fn set_setting(&self, key: SettingKey, value: &str) -> Result<(), StoreError> {
        self.0
            .lock()
            .unwrap()
            .insert(key.as_str(), value.to_owned());
        Ok(())
    }
}

#[tokio::test]
async fn a_settings_store_is_a_waiting_room_source() {
    let settings = MemorySettings::default();
    let loaded = settings.load_room_config().await.unwrap();
    assert_eq!(
        loaded,
        RoomConfig::default(),
        "nothing set: strictest defaults"
    );

    set_registration_mode(&settings, RegistrationMode::Apply)
        .await
        .unwrap();
    settings
        .set_setting(SettingKey::WaitingRoomBanner, "Hello")
        .await
        .unwrap();
    settings
        .set_setting(SettingKey::WaitingRoomRules, "   ")
        .await
        .unwrap();
    let loaded = settings.load_room_config().await.unwrap();
    assert_eq!(loaded.mode, RegistrationMode::Apply);
    assert_eq!(loaded.banner.as_deref(), Some("Hello"));
    assert_eq!(loaded.rules, None, "blank text means unset");
}

#[tokio::test]
async fn a_corrupt_mode_is_an_error_not_a_looser_mode() {
    let settings = MemorySettings::default();
    settings
        .set_setting(SettingKey::RegistrationMode, "wide-open")
        .await
        .unwrap();
    assert!(settings.load_room_config().await.is_err());
}

#[tokio::test]
async fn the_empty_directory_gives_defaults() {
    assert_eq!(
        NoMembers.load_room_config().await.unwrap(),
        RoomConfig::default()
    );
}
