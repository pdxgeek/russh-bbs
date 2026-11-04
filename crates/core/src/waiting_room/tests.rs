#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unused_async_trait_impl
)]

use std::collections::HashMap;
use std::sync::Mutex;

use super::screens::{entry_for, menu, pages};
use super::{
    JoinKind, NoticeKind, Page, RedeemRequest, RoomConfig, RoomEffect, RoomInput, View,
    WaitingRoom, WaitingRoomSource, strings,
};
use crate::access::NoMembers;
use crate::queue::RequestKind;
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

/// The application form (`apply` mode, M2-G) is the request form's twin: a valid submission
/// is handed over as an application and answered at once with one fixed text.
#[test]
fn a_valid_application_is_handed_over_and_answered_with_the_fixed_text() {
    let mut room = room(RegistrationMode::Apply);
    room.input(RoomInput::Char('a'));
    assert_eq!(room.view().fields.len(), 2);
    type_text(&mut room, "applicant");
    assert_eq!(room.input(RoomInput::Enter), RoomEffect::Stay);
    type_text(&mut room, "  I run a small board \u{1b}[31m too ");
    let RoomEffect::RequestInvite(ask) = room.input(RoomInput::Enter) else {
        panic!("a valid application is handed over");
    };
    assert_eq!(ask.kind, RequestKind::Application);
    assert_eq!(ask.handle.as_str(), "applicant");
    assert!(!ask.message.contains('\u{1b}'), "{:?}", ask.message);
    let view = room.view();
    let notice = view.notice.expect("an answer");
    assert_eq!(notice.kind, NoticeKind::Info);
    assert_eq!(notice.text, strings::APPLY_ANSWER);
    assert!(
        view.fields.iter().all(|f| f.value.is_empty()),
        "typed text does not stay in the room"
    );
    // True either way: who can read it, that the key stays outside, that nobody is told.
    for claim in ["approved", "created", "success", "you are in"] {
        assert!(!notice.text.to_lowercase().contains(claim), "{claim}");
    }
    assert!(notice.text.contains("fingerprint"));
    assert!(notice.text.contains("outside the board"));
    assert!(notice.text.contains("Nobody is notified"));
}

/// An invalid application never reaches the front end, and its problem depends only on what
/// was typed.
#[test]
fn an_invalid_application_is_not_handed_over() {
    for (handle, message, problem) in [
        ("x", "hello", strings::ERR_HANDLE_TOO_SHORT),
        ("sysop", "hello", strings::ERR_HANDLE_RESERVED),
        ("applicant", "", strings::ERR_EMPTY),
    ] {
        let mut room = room(RegistrationMode::Apply);
        room.input(RoomInput::Char('a'));
        type_text(&mut room, handle);
        room.input(RoomInput::Enter);
        type_text(&mut room, message);
        assert_eq!(room.input(RoomInput::Enter), RoomEffect::Stay);
        let notice = room.view().notice.expect("a problem");
        assert_eq!(notice.kind, NoticeKind::Error);
        assert_eq!(notice.text, problem);
    }
}

/// The create-account form (`open` mode, M2-G) is the redeem form's twin: a valid handle
/// becomes an effect with nothing claimed, and a refusal reads as one fixed sentence.
#[test]
fn a_valid_create_account_form_hands_over_an_open_request() {
    let mut room = room(RegistrationMode::Open);
    room.input(RoomInput::Char('n'));
    assert_eq!(room.view().fields.len(), 1);
    type_text(&mut room, "newcomer");
    let RoomEffect::Redeem(request) = room.input(RoomInput::Enter) else {
        panic!("a valid handle is handed over");
    };
    assert_eq!(request.join, JoinKind::Open);
    assert_eq!(request.handle.as_str(), "newcomer");
    let view = room.view();
    assert!(view.fields.iter().all(|f| f.value.is_empty()));
    assert!(
        view.notice.is_none(),
        "nothing is claimed before the store answers"
    );
    assert!(
        !format!("{request:?}").contains("redacted"),
        "no code, so nothing to hide"
    );

    room.redeem_refused();
    let notice = room.view().notice.expect("a refusal");
    assert_eq!(notice.kind, NoticeKind::Error);
    assert_eq!(notice.text, strings::CREATE_ACCOUNT_FAILED);
}

/// A bad handle never reaches the front end in `open` mode either.
#[test]
fn an_invalid_create_account_form_is_not_handed_over() {
    for (handle, problem) in [
        ("x", strings::ERR_HANDLE_TOO_SHORT),
        ("bad name!", strings::ERR_HANDLE_CHARS),
        ("admin", strings::ERR_HANDLE_RESERVED),
        ("   ", strings::ERR_EMPTY),
    ] {
        let mut room = room(RegistrationMode::Open);
        room.input(RoomInput::Char('n'));
        type_text(&mut room, handle);
        assert_eq!(room.input(RoomInput::Enter), RoomEffect::Stay);
        assert_eq!(room.view().notice.expect("a problem").text, problem);
    }
}

/// A full day turns `open` into `apply`: same screens as an `apply` board plus an honest notice
/// on the home and application pages (D2). Nothing else changes.
#[test]
fn a_degraded_open_room_offers_the_application_form_with_a_notice() {
    let mut degraded = room(RegistrationMode::Open);
    degraded.degrade_open(true);
    assert_eq!(degraded.mode(), RegistrationMode::Apply);
    let home = degraded.view();
    assert_eq!(home.note, Some(strings::OPEN_CAPPED_NOTICE));
    let labels: Vec<_> = home.menu.iter().map(|m| (m.key, m.label)).collect();
    let apply_room = room(RegistrationMode::Apply).view();
    let apply_labels: Vec<_> = apply_room.menu.iter().map(|m| (m.key, m.label)).collect();
    assert_eq!(labels, apply_labels);
    assert!(
        !labels
            .iter()
            .any(|(_, l)| *l == strings::MENU_CREATE_ACCOUNT)
    );
    // The create-account key does nothing.
    degraded.input(RoomInput::Char('n'));
    assert_eq!(degraded.page(), Page::Home);
    // The application page carries the notice too, and the form works.
    degraded.input(RoomInput::Char('a'));
    assert_eq!(degraded.page(), Page::Apply);
    assert_eq!(degraded.view().note, Some(strings::OPEN_CAPPED_NOTICE));
    type_text(&mut degraded, "applicant");
    degraded.input(RoomInput::Enter);
    type_text(&mut degraded, "hello");
    assert!(matches!(
        degraded.input(RoomInput::Enter),
        RoomEffect::RequestInvite(_)
    ));
    // The notice is about the cap, so it is honest about what it says and nothing more.
    assert!(!strings::OPEN_CAPPED_NOTICE.contains(char::is_numeric));
}

/// Degrading without a notice (the cap could not be read) is just the stricter path.
#[test]
fn a_degraded_room_without_a_notice_shows_none() {
    let mut degraded = room(RegistrationMode::Open);
    degraded.degrade_open(false);
    assert_eq!(degraded.mode(), RegistrationMode::Apply);
    assert_eq!(degraded.view().note, None);
}

/// `degrade_open` can only make a room stricter: it does nothing to `invite` or `apply`.
#[test]
fn degrading_never_loosens_a_room() {
    for mode in [RegistrationMode::Invite, RegistrationMode::Apply] {
        let mut room = room(mode);
        let before = room.view();
        room.degrade_open(true);
        assert_eq!(room.mode(), mode);
        assert_eq!(room.view(), before);
    }
}

/// The request-an-invite form (M2-E) hands a valid submission to the front end and answers at
/// once with one fixed text, whatever the queue does with it.
#[test]
fn a_valid_invite_request_is_handed_over_and_answered_with_the_fixed_text() {
    let mut room = room(RegistrationMode::Invite);
    room.input(RoomInput::Char('i'));
    type_text(&mut room, "newcomer");
    assert_eq!(room.input(RoomInput::Enter), RoomEffect::Stay);
    type_text(&mut room, "  a friend of alice\u{1b}[31m  ");
    let RoomEffect::RequestInvite(ask) = room.input(RoomInput::Enter) else {
        panic!("a valid submission is handed over");
    };
    assert_eq!(ask.handle.as_str(), "newcomer");
    assert!(!ask.message.contains('\u{1b}'), "{:?}", ask.message);
    assert!(ask.message.starts_with("a friend of alice"));
    let view = room.view();
    let notice = view.notice.expect("an answer");
    assert_eq!(notice.kind, NoticeKind::Info);
    assert_eq!(notice.text, strings::REQUEST_INVITE_ANSWER);
    assert!(
        view.fields.iter().all(|f| f.value.is_empty()),
        "typed text does not stay in the room"
    );
    // The answer says what is true either way: who reads it, and that nobody is notified.
    assert!(notice.text.contains("trusted members"));
    assert!(notice.text.contains("Nobody is notified"));
    for claim in ["sent to", "approved", "accepted", "success", "stored"] {
        assert!(!notice.text.to_lowercase().contains(claim), "{claim}");
    }
    // The effect's Debug never carries what was typed.
    assert!(!format!("{ask:?}").contains("newcomer"));
}

#[test]
fn an_invalid_invite_request_is_not_handed_over() {
    for (handle, message) in [("a", "hi"), ("fine-name", ""), ("", "hi"), ("-bad", "hi")] {
        let mut room = room(RegistrationMode::Invite);
        room.input(RoomInput::Char('i'));
        type_text(&mut room, handle);
        room.input(RoomInput::Enter);
        type_text(&mut room, message);
        assert_eq!(
            room.input(RoomInput::Enter),
            RoomEffect::Stay,
            "{handle:?} {message:?}"
        );
        assert_eq!(room.view().notice.unwrap().kind, NoticeKind::Error);
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
fn banner_and_rules_are_length_capped_and_the_cut_is_marked() {
    let config = RoomConfig {
        banner: Some("b".repeat(100_000)),
        rules: Some("r".repeat(100_000)),
        ..RoomConfig::default()
    };
    let mut room = WaitingRoom::new(&config, 5);
    let banner: String = room.view().banner.concat();
    assert_eq!(banner.chars().count(), super::BANNER_MAX_CHARS + 1);
    assert!(banner.ends_with('\u{2026}'), "a cut is visible: {banner:?}");
    room.input(RoomInput::Char('r'));
    let rules: String = room.view().body.concat();
    assert_eq!(rules.chars().count(), super::RULES_MAX_CHARS + 1);
    assert!(rules.ends_with('\u{2026}'));
}

#[test]
fn text_at_the_limits_is_shown_whole_and_extra_lines_are_cut() {
    let exact = "x\n".repeat(super::BANNER_MAX_LINES - 1) + "x";
    let config = RoomConfig {
        banner: Some(exact.clone()),
        rules: Some("many\n".repeat(super::RULES_MAX_LINES + 5)),
        ..RoomConfig::default()
    };
    let mut room = WaitingRoom::new(&config, 5);
    assert_eq!(room.view().banner.join("\n"), exact, "no mark at the limit");
    room.input(RoomInput::Char('r'));
    let body = room.view().body;
    assert_eq!(body.len(), super::RULES_MAX_LINES);
    assert!(body.last().unwrap().ends_with('\u{2026}'));
}

#[test]
fn setting_text_over_the_limit_is_refused_with_the_numbers() {
    use super::{RoomText, TextLimitError, check_room_text};
    assert_eq!(check_room_text(RoomText::Banner, "fine"), Ok(()));
    let long = "a".repeat(super::BANNER_MAX_CHARS + 1);
    let error = check_room_text(RoomText::Banner, &long).unwrap_err();
    assert_eq!(
        error,
        TextLimitError::TooLong {
            text: RoomText::Banner,
            actual: super::BANNER_MAX_CHARS + 1,
            max: super::BANNER_MAX_CHARS
        }
    );
    assert_eq!(
        error.to_string(),
        "The banner is 301 characters; the most the waiting room shows is 300. Shorten it."
    );
    let tall = "\n".repeat(super::RULES_MAX_LINES);
    let error = check_room_text(RoomText::Rules, &(tall + "x")).unwrap_err();
    assert!(matches!(
        error,
        TextLimitError::TooManyLines {
            actual: 11,
            max: 10,
            ..
        }
    ));
    assert!(error.to_string().starts_with("The rules is 11 lines"));
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

// ---- the redeem form (M2-D) --------------------------------------------------------------

fn redeem_room() -> WaitingRoom {
    let mut room = room(RegistrationMode::Invite);
    room.input(RoomInput::Char('c'));
    assert_eq!(room.page(), Page::RedeemInvite);
    room
}

fn fill_redeem(room: &mut WaitingRoom, code: &str, handle: &str) -> RoomEffect {
    type_text(room, code);
    room.input(RoomInput::Enter);
    type_text(room, handle);
    room.input(RoomInput::Enter)
}

#[test]
fn a_filled_redeem_form_hands_the_front_end_a_request_and_forgets_what_was_typed() {
    let mut room = redeem_room();
    let effect = fill_redeem(&mut room, "  ABCD-EFGH  ", "newcomer");
    let RoomEffect::Redeem(RedeemRequest { join, handle, .. }) = effect else {
        unreachable!("expected a redeem request, got {effect:?}");
    };
    assert_eq!(
        join,
        JoinKind::Invite {
            code: "ABCD-EFGH".to_owned()
        }
    );
    assert_eq!(handle.as_str(), "newcomer");
    let view = room.view();
    assert!(view.fields.iter().all(|f| f.value.is_empty()));
    assert!(
        view.notice.is_none(),
        "nothing is claimed before the store answers"
    );
}

#[test]
fn a_bad_handle_or_an_empty_field_never_reaches_the_front_end() {
    for (code, handle, message) in [
        ("ABCD", "x", strings::ERR_HANDLE_TOO_SHORT),
        ("ABCD", "bad name!", strings::ERR_HANDLE_CHARS),
        ("ABCD", "sysop", strings::ERR_HANDLE_RESERVED),
        ("", "newcomer", strings::ERR_EMPTY),
        ("   ", "newcomer", strings::ERR_EMPTY),
    ] {
        let mut room = redeem_room();
        let effect = fill_redeem(&mut room, code, handle);
        assert_eq!(effect, RoomEffect::Stay, "{code:?} {handle:?}");
        let notice = room.view().notice.expect("a problem is shown");
        assert_eq!((notice.kind, notice.text), (NoticeKind::Error, message));
    }
}

#[test]
fn a_refused_redemption_shows_one_fixed_message_and_the_form_is_ready_again() {
    let mut room = redeem_room();
    assert!(matches!(
        fill_redeem(&mut room, "ABCD", "newcomer"),
        RoomEffect::Redeem(_)
    ));
    room.redeem_refused();
    let view = room.view();
    let notice = view.notice.expect("a result");
    assert_eq!(
        (notice.kind, notice.text),
        (NoticeKind::Error, strings::REDEEM_FAILED)
    );
    assert_eq!(view.fields.len(), 2);
    assert!(view.fields[0].active, "the cursor is back on the code");
    // The message names no reason: it would read the same for every kind of refusal.
    let lower = strings::REDEEM_FAILED.to_lowercase();
    for reason in [
        "expired",
        "already used",
        "unknown",
        "taken",
        "no such",
        "member",
        "not found",
    ] {
        assert!(!lower.contains(reason), "{reason}");
    }
}

#[test]
fn redemptions_count_against_the_session_cap() {
    let mut room = WaitingRoom::new(
        &RoomConfig {
            mode: RegistrationMode::Invite,
            ..RoomConfig::default()
        },
        2,
    );
    room.input(RoomInput::Char('c'));
    for _ in 0..2 {
        assert!(matches!(
            fill_redeem(&mut room, "ABCD", "newcomer"),
            RoomEffect::Redeem(_)
        ));
        room.redeem_refused();
    }
    assert_eq!(
        fill_redeem(&mut room, "ABCD", "newcomer"),
        RoomEffect::Disconnect
    );
}

#[test]
fn the_request_does_not_print_the_code() {
    let mut room = redeem_room();
    let effect = fill_redeem(&mut room, "SECRET-CODE", "newcomer");
    assert!(!format!("{effect:?}").contains("SECRET"));
}

#[test]
fn the_redeem_form_exists_only_in_invite_mode() {
    for mode in [RegistrationMode::Apply, RegistrationMode::Open] {
        assert!(!pages(mode).contains(&Page::RedeemInvite), "{mode:?}");
    }
}

// ---- password sign-up (telnet, D94) ---------------------------------------------------------

fn password_room(mode: RegistrationMode, key: char) -> WaitingRoom {
    let mut room = WaitingRoom::new(
        &RoomConfig {
            mode,
            ..RoomConfig::default()
        },
        5,
    )
    .with_password_signup();
    room.input(RoomInput::Char(key));
    room
}

const PASSWORD: &str = "correct horse battery";

fn fill(room: &mut WaitingRoom, values: &[&str]) -> RoomEffect {
    let mut effect = RoomEffect::Stay;
    for value in values {
        type_text(room, value);
        effect = room.input(RoomInput::Enter);
    }
    effect
}

#[test]
fn the_password_room_asks_for_a_password_and_its_repeat_on_every_account_form() {
    for (mode, key, labels) in [
        (RegistrationMode::Invite, 'c', 4),
        (RegistrationMode::Invite, 'i', 4),
        (RegistrationMode::Apply, 'a', 4),
        (RegistrationMode::Open, 'n', 3),
    ] {
        let room = password_room(mode, key);
        let fields = room.view().fields;
        assert_eq!(fields.len(), labels, "{mode:?} {key}");
        assert_eq!(
            fields[fields.len() - 2].label,
            strings::LABEL_PASSWORD,
            "{mode:?} {key}"
        );
        assert_eq!(
            fields[fields.len() - 1].label,
            strings::LABEL_PASSWORD_AGAIN,
            "{mode:?} {key}"
        );
    }
}

#[test]
fn the_key_room_is_unchanged_by_the_password_room_existing() {
    let mut room = room(RegistrationMode::Invite);
    room.input(RoomInput::Char('c'));
    assert_eq!(room.view().fields.len(), 2);
    let mut room = self::room(RegistrationMode::Open);
    room.input(RoomInput::Char('n'));
    assert_eq!(room.view().fields.len(), 1);
}

#[test]
fn a_password_never_appears_in_a_view_and_the_effect_carries_it_as_a_secret() {
    let mut room = password_room(RegistrationMode::Open, 'n');
    type_text(&mut room, "walkin");
    room.input(RoomInput::Enter);
    type_text(&mut room, PASSWORD);
    let typing = room.view();
    assert!(!all_text(&typing).contains(PASSWORD));
    assert_eq!(typing.fields[1].value, "*".repeat(PASSWORD.len()));
    room.input(RoomInput::Enter);
    type_text(&mut room, PASSWORD);
    let RoomEffect::Redeem(request) = room.input(RoomInput::Enter) else {
        unreachable!("expected a registration");
    };
    assert_eq!(request.join, JoinKind::Open);
    let secret = request.password.clone().unwrap();
    assert_eq!(secret, crate::passwords::Secret::new(PASSWORD.to_owned()));
    assert!(!format!("{request:?}").contains(PASSWORD));
    // And the room forgot it.
    assert!(room.view().fields.iter().all(|f| f.value.is_empty()));
}

#[test]
fn a_mismatched_or_weak_password_never_reaches_the_front_end_and_is_not_kept() {
    for (first, second, message) in [
        (
            PASSWORD,
            "another horse battery",
            strings::ERR_PASSWORD_MISMATCH,
        ),
        ("short", "short", crate::passwords::strings::ERR_SHORT),
        ("", "", strings::ERR_EMPTY),
    ] {
        let mut room = password_room(RegistrationMode::Open, 'n');
        let effect = fill(&mut room, &["walkin", first, second]);
        assert_eq!(effect, RoomEffect::Stay, "{first}/{second}");
        let view = room.view();
        assert_eq!(view.notice.unwrap().text, message);
        assert_eq!(view.notice.unwrap().kind, NoticeKind::Error);
        // The handle stays, the secrets are wiped and the cursor is on the first of them.
        assert_eq!(view.fields[0].value, "walkin");
        assert!(view.fields[1].value.is_empty() && view.fields[2].value.is_empty());
        assert!(view.fields[1].active);
    }
}

#[test]
fn only_printable_ascii_is_taken_into_a_password_field_and_it_is_not_trimmed() {
    let mut room = password_room(RegistrationMode::Open, 'n');
    type_text(&mut room, "walkin");
    room.input(RoomInput::Enter);
    type_text(&mut room, " caf\u{e9} \u{1b}[31m\u{7}\u{200b}pass phrase ");
    // The non-ASCII letter, the escape, the bell and the zero-width space were not taken.
    assert_eq!(
        room.view().fields[1].value.len(),
        " caf [31mpass phrase ".len()
    );
    room.input(RoomInput::Enter);
    type_text(&mut room, " caf [31mpass phrase ");
    let RoomEffect::Redeem(request) = room.input(RoomInput::Enter) else {
        unreachable!("expected a registration");
    };
    assert_eq!(
        request.password.unwrap(),
        crate::passwords::Secret::new(" caf [31mpass phrase ".to_owned())
    );
}

#[test]
fn a_password_stops_at_128_characters() {
    let mut room = password_room(RegistrationMode::Open, 'n');
    type_text(&mut room, "walkin");
    room.input(RoomInput::Enter);
    type_text(&mut room, &"x".repeat(300));
    assert_eq!(room.view().fields[1].value.len(), 128);
}

#[test]
fn a_request_form_hands_over_the_password_and_shows_the_telnet_answer() {
    for (mode, key, answer, kind) in [
        (
            RegistrationMode::Invite,
            'i',
            strings::REQUEST_INVITE_ANSWER_PASSWORD,
            RequestKind::Invite,
        ),
        (
            RegistrationMode::Apply,
            'a',
            strings::APPLY_ANSWER_PASSWORD,
            RequestKind::Application,
        ),
    ] {
        let mut room = password_room(mode, key);
        let effect = fill(&mut room, &["hopeful", "a friend", PASSWORD, PASSWORD]);
        let RoomEffect::RequestInvite(ask) = effect else {
            unreachable!("expected a request, got {effect:?}");
        };
        assert_eq!(ask.kind, kind);
        assert!(ask.password.is_some());
        let view = room.view();
        assert_eq!(view.notice.unwrap().text, answer);
        assert!(!answer.contains("fingerprint") && !answer.contains("same key"));
    }
}
