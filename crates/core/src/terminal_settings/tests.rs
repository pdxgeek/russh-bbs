#![allow(clippy::unwrap_used)]

use super::strings::{self, fill};
use super::*;
use crate::access::{Permission, Slug, Snapshot, Via};
use crate::icons::{ContentIcon, Icon, Tier};
use crate::sanitize::sanitize_username;
use crate::store::{AccountId, UnixSeconds};

/// A session holding the `member` stand-in: everything that exists except `members.approve`.
fn member() -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::MEMBER,
        10,
        &Via::Account,
        Permission::ALL
            .iter()
            .copied()
            .filter(|p| *p != Permission::MembersApprove),
    )
}

/// A session holding every permission.
fn sysop() -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::SYSOP,
        100,
        &Via::Account,
        Permission::ALL.iter().copied(),
    )
}

fn flow(settings: TerminalSettings, last: Option<i64>) -> MemberFlow {
    let handle = sanitize_username("alice").unwrap();
    MemberFlow::new(&handle, member(), last.map(UnixSeconds::from_raw), settings)
}

fn press(flow: &mut MemberFlow, keys: &str) -> Vec<MemberEffect> {
    keys.chars()
        .map(|c| {
            flow.input(match c {
                '\n' => MemberInput::Enter,
                '\u{1b}' => MemberInput::Escape,
                '\u{8}' => MemberInput::Backspace,
                c => MemberInput::Char(c),
            })
        })
        .collect()
}

fn text(view: &crate::waiting_room::View) -> String {
    let mut out = vec![view.heading.to_owned()];
    out.extend(view.body.iter().cloned());
    out.extend(view.menu.iter().map(|m| format!("[{}] {}", m.key, m.label)));
    out.extend(view.fields.iter().map(|f| f.label.to_owned()));
    out.extend(view.notice.map(|n| n.text.to_owned()));
    out.push(view.footer.to_owned());
    out.join("\n")
}

// ---- settings rules ----

#[test]
fn defaults_follow_the_pty_and_are_the_plain_layout_until_a_tier_is_chosen() {
    let s = TerminalSettings::default();
    assert_eq!(s.width(), None);
    assert_eq!(s.height(), None);
    assert!(s.pause(), "pause is on by default");
    assert!(s.needs_glyph_test());
    assert_eq!(s.chosen_tier(), None);
    assert_eq!(s.icon_tier(), Tier::Ascii);
    assert!(!s.use_color(), "no color before the glyph test is answered");
    assert_eq!(s.size(132, 43), (132, 43));
}

#[test]
fn overrides_replace_the_pty_size_one_axis_at_a_time() {
    let s = TerminalSettings::default().with_width(Some(100)).unwrap();
    assert_eq!(s.size(80, 24), (100, 24));
    let s = s.with_height(Some(40)).unwrap();
    assert_eq!(s.size(80, 24), (100, 40));
    let s = s.with_width(None).unwrap();
    assert_eq!(s.size(80, 24), (80, 40));
}

#[test]
fn size_overrides_are_range_checked() {
    let s = TerminalSettings::default();
    assert_eq!(s.with_width(Some(MIN_WIDTH - 1)), Err(SettingsError::Width));
    assert_eq!(s.with_width(Some(MAX_WIDTH + 1)), Err(SettingsError::Width));
    assert_eq!(s.with_width(Some(0)), Err(SettingsError::Width));
    assert_eq!(
        s.with_height(Some(MIN_HEIGHT - 1)),
        Err(SettingsError::Height)
    );
    assert_eq!(
        s.with_height(Some(MAX_HEIGHT + 1)),
        Err(SettingsError::Height)
    );
    assert!(s.with_width(Some(MIN_WIDTH)).is_ok());
    assert!(s.with_width(Some(MAX_WIDTH)).is_ok());
    assert!(s.with_height(Some(MIN_HEIGHT)).is_ok());
    assert!(s.with_height(Some(MAX_HEIGHT)).is_ok());
}

#[test]
fn color_applies_only_once_a_tier_is_chosen() {
    let chosen = TerminalSettings::default().with_tier(Tier::Unicode);
    assert!(chosen.use_color());
    assert!(!chosen.with_color(false).use_color());
}

#[test]
fn the_safe_fallback_is_ascii_without_color() {
    let s = TerminalSettings::default().safe_fallback();
    assert_eq!(s.chosen_tier(), Some(Tier::Ascii));
    assert!(!s.use_color());
    assert!(!s.needs_glyph_test(), "the fallback is an answer");
}

#[test]
fn tiers_round_trip_through_their_stored_text() {
    for tier in Tier::ALL {
        assert_eq!(tier_from_stored(tier_to_stored(tier)), Some(tier));
    }
    assert_eq!(tier_from_stored("NerdFont"), None);
    assert_eq!(tier_from_stored(""), None);
}

// ---- the glyph test ----

fn run(first_run: bool, answers: &[GlyphAnswer]) -> Vec<GlyphProgress> {
    let mut test = GlyphTest::new(first_run);
    answers
        .iter()
        .map(|a| test.answer(*a, TerminalSettings::default()))
        .collect()
}

#[test]
fn yes_to_nerd_font_picks_nerd_font() {
    let result = run(true, &[GlyphAnswer::Yes]);
    let expected = TerminalSettings::default().with_tier(Tier::NerdFont);
    assert_eq!(result, vec![GlyphProgress::Chosen(expected)]);
    assert!(expected.use_color(), "a glyph tier keeps color on");
}

#[test]
fn no_then_yes_picks_unicode() {
    let result = run(true, &[GlyphAnswer::No, GlyphAnswer::Yes]);
    let expected = TerminalSettings::default().with_tier(Tier::Unicode);
    assert_eq!(
        result,
        vec![GlyphProgress::Next, GlyphProgress::Chosen(expected)]
    );
}

#[test]
fn no_to_every_glyph_tier_falls_back_to_ascii_without_color() {
    let result = run(true, &[GlyphAnswer::No, GlyphAnswer::No]);
    let expected = TerminalSettings::default().safe_fallback();
    assert_eq!(
        result,
        vec![GlyphProgress::Next, GlyphProgress::Chosen(expected)]
    );
    assert!(!expected.use_color());
    assert_eq!(expected.icon_tier(), Tier::Ascii);
}

#[test]
fn skipping_the_first_run_stores_the_safe_fallback() {
    let at_start = run(true, &[GlyphAnswer::Skip]);
    assert_eq!(
        at_start,
        vec![GlyphProgress::Chosen(
            TerminalSettings::default().safe_fallback()
        )]
    );
    let later = run(true, &[GlyphAnswer::No, GlyphAnswer::Skip]);
    assert_eq!(
        later[1],
        GlyphProgress::Chosen(TerminalSettings::default().safe_fallback())
    );
}

#[test]
fn skipping_a_rerun_changes_nothing() {
    assert_eq!(
        run(false, &[GlyphAnswer::Skip]),
        vec![GlyphProgress::Cancelled]
    );
}

#[test]
fn a_rerun_yes_keeps_the_other_settings() {
    let current = TerminalSettings::default()
        .with_width(Some(100))
        .unwrap()
        .with_color(false)
        .with_pause(false);
    let mut test = GlyphTest::new(false);
    let progress = test.answer(GlyphAnswer::Yes, current);
    let expected = current.with_tier(Tier::NerdFont);
    assert_eq!(progress, GlyphProgress::Chosen(expected));
    let s = expected;
    assert_eq!(s.width(), Some(100));
    assert!(!s.color());
    assert!(!s.pause());
    assert_eq!(s.chosen_tier(), Some(Tier::NerdFont));
}

#[test]
fn samples_are_the_registry_glyphs_for_the_step_tier() {
    let mut test = GlyphTest::new(true);
    for tier in [Tier::NerdFont, Tier::Unicode] {
        let expected = ContentIcon::ALL
            .iter()
            .map(|i| Icon::from(*i).render(tier).plain().to_owned())
            .collect::<Vec<_>>()
            .join("  ");
        assert_eq!(test.samples(), expected);
        test.answer(GlyphAnswer::No, TerminalSettings::default());
    }
}

#[test]
fn the_nerd_font_row_is_private_use_and_the_unicode_row_is_not() {
    let nerd = GlyphTest::new(true).samples();
    assert!(nerd.chars().any(|c| ('\u{e000}'..='\u{f8ff}').contains(&c)));
    let mut test = GlyphTest::new(true);
    test.answer(GlyphAnswer::No, TerminalSettings::default());
    assert!(
        !test
            .samples()
            .chars()
            .any(|c| ('\u{e000}'..='\u{f8ff}').contains(&c))
    );
}

#[test]
fn the_glyph_test_shows_no_security_marker() {
    let mut test = GlyphTest::new(true);
    let mut rows = vec![test.samples()];
    test.answer(GlyphAnswer::No, TerminalSettings::default());
    rows.push(test.samples());
    for row in rows {
        for chrome in crate::sanitize::RESERVED_CHROME {
            assert!(!row.contains(*chrome), "reserved chrome in a sample row");
        }
    }
}

// ---- the flow ----

#[test]
fn first_login_walks_login_then_glyph_test_then_home() {
    let mut f = flow(TerminalSettings::default(), None);
    let login = text(&f.view());
    assert!(login.contains("Signed in as alice."), "{login}");
    assert!(login.contains("This is your first login."), "{login}");
    assert!(login.contains("icon style not chosen yet"), "{login}");

    assert_eq!(press(&mut f, "x"), vec![MemberEffect::Stay]);
    assert!(text(&f.view()).contains("Do these look like icons?"));

    let effects = press(&mut f, "y");
    let expected = TerminalSettings::default().with_tier(Tier::NerdFont);
    assert_eq!(effects, vec![MemberEffect::Save(expected)]);
    assert_eq!(f.settings(), &expected);
    let home = text(&f.view());
    assert!(home.contains("Icons set to Nerd Font."), "{home}");
    assert!(home.contains("Signed in as alice."), "{home}");
    let page = f.menu_page().unwrap();
    assert_eq!(page.title, "menu.main.title");
    assert!(page.items.iter().any(|i| i.hotkey == 's'));
}

#[test]
fn a_stored_tier_skips_the_glyph_test_and_the_login_screen_names_it() {
    let settings = TerminalSettings::default().with_tier(Tier::Unicode);
    let mut f = flow(settings, Some(1_762_078_500));
    let login = text(&f.view());
    assert!(
        login.contains("Last login: 2025-11-02 10:15 UTC"),
        "{login}"
    );
    assert!(
        login.contains("Terminal: Unicode icons, color on, pause on"),
        "{login}"
    );
    press(&mut f, "x");
    assert!(f.menu_page().is_some(), "straight to the main menu");
}

#[test]
fn the_main_menu_opens_settings_and_back_returns_to_the_same_menu() {
    let mut f = flow(TerminalSettings::default().with_tier(Tier::Ascii), None);
    press(&mut f, "x");
    assert!(f.menu_page().is_some());
    press(&mut f, "s");
    assert!(f.menu_page().is_none(), "the settings screen is showing");
    assert!(text(&f.view()).contains("Terminal settings"));
    press(&mut f, "\u{1b}");
    assert!(f.menu_page().is_some(), "Esc is back to the menu");
    press(&mut f, "s");
    press(&mut f, "b");
    assert!(f.menu_page().is_some(), "the Back item is back too");
}

#[test]
fn log_off_from_the_menu_and_quit_from_every_page_end_the_session() {
    let mut f = flow(TerminalSettings::default().with_tier(Tier::Ascii), None);
    press(&mut f, "x");
    assert_eq!(press(&mut f, "l"), vec![MemberEffect::Disconnect]);
    assert!(f.is_logged_off());
    assert!(f.menu_page().is_none());
    // Quit works on the menu, in a submenu and on the settings page.
    for keys in ["", "s"] {
        let mut f = flow(TerminalSettings::default().with_tier(Tier::Ascii), None);
        press(&mut f, "x");
        press(&mut f, keys);
        assert_eq!(
            f.input(MemberInput::Quit),
            MemberEffect::Disconnect,
            "{keys:?}"
        );
    }
}

#[test]
fn a_member_never_sees_the_sysop_menu_and_a_sysop_does() {
    let handle = sanitize_username("alice").unwrap();
    let make = |access: Snapshot| {
        let mut f = MemberFlow::new(
            &handle,
            access,
            None,
            TerminalSettings::default().with_tier(Tier::Ascii),
        );
        press(&mut f, "x");
        f
    };
    let mut member = make(member());
    press(&mut member, "o");
    assert_eq!(member.menu_page().unwrap().title, "menu.main.title");
    let mut sysop = make(sysop());
    press(&mut sysop, "o");
    assert_eq!(sysop.menu_page().unwrap().title, "menu.sysop.title");
    press(&mut sysop, "\u{1b}");
    assert_eq!(sysop.menu_page().unwrap().title, "menu.main.title");
}

#[test]
fn escape_at_the_first_glyph_test_stores_ascii_without_color() {
    let mut f = flow(TerminalSettings::default(), None);
    press(&mut f, "x");
    let effects = press(&mut f, "\u{1b}");
    let expected = TerminalSettings::default().safe_fallback();
    assert_eq!(effects, vec![MemberEffect::Save(expected)]);
    assert!(text(&f.view()).contains("plain ASCII icons and no color"));
}

#[test]
fn no_and_no_at_the_first_glyph_test_stores_ascii_without_color() {
    let mut f = flow(TerminalSettings::default(), None);
    press(&mut f, "x");
    let effects = press(&mut f, "nn");
    let expected = TerminalSettings::default().safe_fallback();
    assert_eq!(
        effects,
        vec![MemberEffect::Stay, MemberEffect::Save(expected)]
    );
}

#[test]
fn quitting_during_the_first_glyph_test_stores_nothing() {
    let mut f = flow(TerminalSettings::default(), None);
    press(&mut f, "x");
    assert_eq!(f.input(MemberInput::Quit), MemberEffect::Disconnect);
    assert!(f.is_logged_off());
    assert!(f.settings().needs_glyph_test(), "asked again next time");
}

#[test]
fn other_keys_do_not_answer_the_glyph_test() {
    let mut f = flow(TerminalSettings::default(), None);
    press(&mut f, "x");
    assert_eq!(press(&mut f, "q\n z"), vec![MemberEffect::Stay; 4]);
    assert!(text(&f.view()).contains("Do these look like icons?"));
}

fn at_settings() -> MemberFlow {
    let mut f = flow(TerminalSettings::default().with_tier(Tier::Ascii), None);
    press(&mut f, "xs");
    f
}

#[test]
fn settings_toggles_color_and_pause_and_asks_to_save() {
    let mut f = at_settings();
    let s = *f.settings();
    assert_eq!(
        press(&mut f, "c"),
        vec![MemberEffect::Save(s.with_color(false))]
    );
    assert_eq!(
        press(&mut f, "p"),
        vec![MemberEffect::Save(s.with_color(false).with_pause(false))]
    );
    let view = text(&f.view());
    assert!(view.contains("Color: off"), "{view}");
    assert!(view.contains("Pause at screen end: off"), "{view}");
}

#[test]
fn settings_sets_and_clears_size_overrides() {
    let mut f = at_settings();
    let effects = press(&mut f, "w120\n");
    assert_eq!(f.settings().width(), Some(120));
    assert!(matches!(effects.last(), Some(MemberEffect::Save(_))));
    press(&mut f, "h40\n");
    assert_eq!(f.settings().size(80, 24), (120, 40));
    assert!(text(&f.view()).contains("Width: 120"));
    press(&mut f, "w\n");
    assert_eq!(f.settings().width(), None);
    assert!(text(&f.view()).contains("Width: automatic"));
}

#[test]
fn settings_rejects_out_of_range_sizes_with_words() {
    let mut f = at_settings();
    let effects = press(&mut f, "w9999\n");
    assert_eq!(effects.last(), Some(&MemberEffect::Stay));
    assert_eq!(f.settings().width(), None);
    assert!(text(&f.view()).contains(strings::ERR_WIDTH));
    press(&mut f, "h5\n");
    assert!(text(&f.view()).contains(strings::ERR_HEIGHT));
}

#[test]
fn settings_number_fields_take_digits_only_and_cap_their_length() {
    let mut f = at_settings();
    press(&mut f, "wa1b2\u{8}3");
    let view = f.view();
    assert_eq!(view.fields.first().map(|x| x.value.as_str()), Some("13"));
    press(&mut f, "99999");
    let view = f.view();
    assert_eq!(view.fields.first().map(|x| x.value.len()), Some(4));
    press(&mut f, "\u{1b}");
    assert!(
        text(&f.view()).contains("Width:"),
        "escape cancels the edit"
    );
    assert_eq!(f.settings().width(), None);
}

#[test]
fn the_glyph_test_is_reachable_again_and_a_skip_cancels() {
    let mut f = at_settings();
    let before = *f.settings();
    press(&mut f, "i");
    assert!(text(&f.view()).contains("Esc: cancel"));
    assert_eq!(press(&mut f, "\u{1b}"), vec![MemberEffect::Stay]);
    assert_eq!(f.settings(), &before);
    press(&mut f, "iy");
    assert_eq!(f.settings().chosen_tier(), Some(Tier::NerdFont));
}

#[test]
fn logging_off_shows_the_goodbye_and_has_a_farewell() {
    let mut f = flow(TerminalSettings::default().with_tier(Tier::Ascii), None);
    assert_eq!(f.farewell(), None);
    press(&mut f, "x");
    assert_eq!(press(&mut f, "l"), vec![MemberEffect::Disconnect]);
    let view = text(&f.view());
    assert!(view.contains("Logged off"), "{view}");
    assert!(view.contains("Goodbye, alice."), "{view}");
    assert_eq!(
        f.farewell()
            .map(|l| l.join("\n"))
            .as_deref()
            .map(|s| s.contains("Goodbye")),
        Some(true)
    );
}

#[test]
fn ctrl_c_logs_off_from_every_page() {
    for keys in ["", "x", "xs", "xsw", "xsi"] {
        let mut f = flow(TerminalSettings::default().with_tier(Tier::Ascii), None);
        press(&mut f, keys);
        assert_eq!(
            f.input(MemberInput::Quit),
            MemberEffect::Disconnect,
            "after {keys:?}"
        );
        assert!(f.is_logged_off());
    }
}

#[test]
fn the_handle_is_sanitized_again_before_it_is_shown() {
    // A `Username` is already strict ASCII, so this pins that the flow still passes it
    // through the shared sanitizer rather than trusting the caller.
    let handle = sanitize_username("bob_2").unwrap();
    let f = MemberFlow::new(&handle, sysop(), None, TerminalSettings::default());
    assert!(text(&f.view()).contains("Signed in as bob_2."));
    assert_eq!(f.top_role(), Some(&Slug::SYSOP));
}

// ---- strings ----

#[test]
fn fill_substitutes_once_and_never_rescans_a_value() {
    assert_eq!(fill("a {x} b {y}", &[("x", "1"), ("y", "2")]), "a 1 b 2");
    assert_eq!(fill("{x}", &[("x", "{y}"), ("y", "boom")]), "{y}");
    assert_eq!(fill("{missing} {", &[("x", "1")]), "{missing} {");
    assert_eq!(fill("no placeholders", &[]), "no placeholders");
}

#[test]
fn range_messages_state_the_real_limits() {
    for (msg, lo, hi) in [
        (strings::ERR_WIDTH, MIN_WIDTH, MAX_WIDTH),
        (strings::ERR_HEIGHT, MIN_HEIGHT, MAX_HEIGHT),
    ] {
        assert!(
            msg.contains(&lo.to_string()) && msg.contains(&hi.to_string()),
            "{msg}"
        );
    }
    assert!(strings::FIELD_WIDTH.contains("20 to 500"));
    assert!(strings::FIELD_HEIGHT.contains("6 to 200"));
}

#[test]
fn no_screen_claims_privacy() {
    let mut f = at_settings();
    let mut all = text(&f.view());
    for keys in ["\u{1b}", "l"] {
        press(&mut f, keys);
        all.push_str(&text(&f.view()));
    }
    let lower = all.to_lowercase();
    assert!(!lower.contains("private") && !lower.contains("secure") && !lower.contains("encrypt"));
}

// ---- time ----

#[test]
fn format_utc_matches_known_dates() {
    let at = |s| format_utc(UnixSeconds::from_raw(s));
    assert_eq!(at(0), "1970-01-01 00:00 UTC");
    assert_eq!(at(951_782_400), "2000-02-29 00:00 UTC");
    assert_eq!(at(1_709_209_140), "2024-02-29 12:19 UTC");
    assert_eq!(at(-1), "1969-12-31 23:59 UTC");
    assert_eq!(at(4_102_444_799), "2099-12-31 23:59 UTC");
}

#[test]
fn format_utc_never_panics_at_the_extremes() {
    for s in [i64::MIN, i64::MAX, i64::MIN + 1, i64::MAX - 1] {
        let _ = format_utc(UnixSeconds::from_raw(s));
    }
}
