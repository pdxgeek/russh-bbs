#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::strings::{self, TextSource, fill};
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

/// The shipped string table, read from the server's TOML so these tests assert on the real
/// text and cannot drift from it.
struct Words(BTreeMap<String, String>);

impl TextSource for Words {
    fn text<'a>(&'a self, key: &'a str) -> &'a str {
        self.0.get(key).map_or(key, String::as_str)
    }
}

fn flatten(table: &toml::Table, prefix: &str, out: &mut BTreeMap<String, String>) {
    for (name, value) in table {
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            toml::Value::String(text) => {
                out.insert(key, text.clone());
            }
            toml::Value::Table(inner) => flatten(inner, &key, out),
            other => unreachable!("string table key {key} is not text: {other:?}"),
        }
    }
}

fn words() -> &'static Words {
    static WORDS: OnceLock<Words> = OnceLock::new();
    WORDS.get_or_init(|| {
        let root: toml::Table = include_str!("../../../server/src/ui/strings.toml")
            .parse()
            .unwrap();
        let mut map = BTreeMap::new();
        flatten(&root, "", &mut map);
        Words(map)
    })
}

fn text(view: &crate::waiting_room::View) -> String {
    let words = words();
    let mut out = vec![words.text(view.heading).to_owned()];
    out.extend(view.body.iter().cloned());
    out.extend(
        view.menu
            .iter()
            .map(|m| format!("[{}] {}", m.key, words.text(m.label))),
    );
    out.extend(view.fields.iter().map(|f| words.text(f.label).to_owned()));
    out.extend(view.notice.map(|n| words.text(n.text).to_owned()));
    out.push(words.text(view.footer).to_owned());
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
    assert_eq!(s.size(132, 24), (100, 24));
    let s = s.with_height(Some(40)).unwrap();
    assert_eq!(s.size(132, 50), (100, 40));
    let s = s.with_width(None).unwrap();
    assert_eq!(s.size(132, 50), (132, 40));
}

#[test]
fn an_override_larger_than_the_terminal_is_cut_down_to_it() {
    let s = TerminalSettings::default()
        .with_width(Some(500))
        .unwrap()
        .with_height(Some(200))
        .unwrap();
    assert_eq!(s.size(200, 60), (200, 60), "never off screen");
    assert_eq!(s.overrides_clamped(200, 60), (true, true));
    assert_eq!(s.size(500, 200), (500, 200), "an exact fit is not cut");
    assert_eq!(s.overrides_clamped(500, 200), (false, false));
    let s = TerminalSettings::default().with_width(Some(60)).unwrap();
    assert_eq!(s.size(80, 24), (60, 24), "a smaller override still applies");
    assert_eq!(s.overrides_clamped(80, 24), (false, false));
    assert_eq!(s.width(), Some(60), "the stored value is not changed");
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
    let login = text(&f.view(words()));
    assert!(login.contains("Signed in as alice."), "{login}");
    assert!(login.contains("This is your first login."), "{login}");
    assert!(login.contains("icon style not chosen yet"), "{login}");

    assert_eq!(press(&mut f, "x"), vec![MemberEffect::Stay]);
    assert!(text(&f.view(words())).contains("Do these look like icons?"));

    let effects = press(&mut f, "y");
    let expected = TerminalSettings::default().with_tier(Tier::NerdFont);
    assert_eq!(effects, vec![MemberEffect::Save(expected)]);
    assert_eq!(f.settings(), &expected);
    let home = text(&f.view(words()));
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
    let login = text(&f.view(words()));
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
    assert!(text(&f.view(words())).contains("Terminal settings"));
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
    assert!(text(&f.view(words())).contains("plain ASCII icons and no color"));
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
    assert!(text(&f.view(words())).contains("Do these look like icons?"));
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
    let view = text(&f.view(words()));
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
    assert_eq!(f.settings().size(200, 60), (120, 40));
    assert!(text(&f.view(words())).contains("Width: 120"));
    press(&mut f, "w\n");
    assert_eq!(f.settings().width(), None);
    assert!(text(&f.view(words())).contains("Width: automatic"));
}

#[test]
fn settings_rejects_out_of_range_sizes_with_words() {
    let mut f = at_settings();
    let effects = press(&mut f, "w9999\n");
    assert_eq!(effects.last(), Some(&MemberEffect::Stay));
    assert_eq!(f.settings().width(), None);
    assert!(text(&f.view(words())).contains(words().text(strings::ERR_WIDTH)));
    press(&mut f, "h5\n");
    assert!(text(&f.view(words())).contains(words().text(strings::ERR_HEIGHT)));
}

#[test]
fn settings_number_fields_take_digits_only_and_cap_their_length() {
    let mut f = at_settings();
    press(&mut f, "wa1b2\u{8}3");
    let view = f.view(words());
    assert_eq!(view.fields.first().map(|x| x.value.as_str()), Some("13"));
    press(&mut f, "99999");
    let view = f.view(words());
    assert_eq!(view.fields.first().map(|x| x.value.len()), Some(4));
    press(&mut f, "\u{1b}");
    assert!(
        text(&f.view(words())).contains("Width:"),
        "escape cancels the edit"
    );
    assert_eq!(f.settings().width(), None);
}

#[test]
fn the_glyph_test_is_reachable_again_and_a_skip_cancels() {
    let mut f = at_settings();
    let before = *f.settings();
    press(&mut f, "i");
    assert!(text(&f.view(words())).contains("Esc: cancel"));
    assert_eq!(press(&mut f, "\u{1b}"), vec![MemberEffect::Stay]);
    assert_eq!(f.settings(), &before);
    press(&mut f, "iy");
    assert_eq!(f.settings().chosen_tier(), Some(Tier::NerdFont));
}

#[test]
fn logging_off_shows_the_goodbye_and_has_a_farewell() {
    let mut f = flow(TerminalSettings::default().with_tier(Tier::Ascii), None);
    assert_eq!(f.farewell(words()), None);
    press(&mut f, "x");
    assert_eq!(press(&mut f, "l"), vec![MemberEffect::Disconnect]);
    let view = text(&f.view(words()));
    assert!(view.contains("Logged off"), "{view}");
    assert!(view.contains("Goodbye, alice."), "{view}");
    assert_eq!(
        f.farewell(words())
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
    assert!(text(&f.view(words())).contains("Signed in as bob_2."));
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
        (words().text(strings::ERR_WIDTH), MIN_WIDTH, MAX_WIDTH),
        (words().text(strings::ERR_HEIGHT), MIN_HEIGHT, MAX_HEIGHT),
    ] {
        assert!(
            msg.contains(&lo.to_string()) && msg.contains(&hi.to_string()),
            "{msg}"
        );
    }
    assert!(words().text(strings::FIELD_WIDTH).contains("20 to 500"));
    assert!(words().text(strings::FIELD_HEIGHT).contains("6 to 200"));
}

#[test]
fn the_settings_screen_says_when_an_override_is_cut_down_to_the_terminal() {
    let stored = TerminalSettings::default()
        .with_tier(Tier::Ascii)
        .with_width(Some(500))
        .unwrap()
        .with_height(Some(200))
        .unwrap();
    let mut f = flow(stored, None);
    f.set_terminal(200, 60);
    press(&mut f, "xs");
    let shown = text(&f.view(words()));
    assert!(
        shown.contains("Width: 500 (your terminal is 200 wide, so 200 is used)"),
        "{shown}"
    );
    assert!(
        shown.contains("Height: 200 (your terminal is 60 tall, so 60 is used)"),
        "{shown}"
    );
    // A window that fits the override says nothing; one that shrinks again says so again.
    f.set_terminal(500, 200);
    let shown = text(&f.view(words()));
    assert!(
        shown.contains("Width: 500\n") && !shown.contains("so 500 is used"),
        "{shown}"
    );
    f.set_terminal(80, 24);
    assert!(text(&f.view(words())).contains("so 80 is used"));
}

#[test]
fn blind_recovery_clears_a_bad_override_with_s_w_enter_h_enter() {
    let stored = TerminalSettings::default()
        .with_tier(Tier::Ascii)
        .with_width(Some(500))
        .unwrap()
        .with_height(Some(200))
        .unwrap();
    let mut f = flow(stored, None);
    f.set_terminal(80, 24);
    // Past the login screen, then the documented keys: s, w, Enter, h, Enter.
    press(&mut f, "x");
    let effects = press(&mut f, "sw\nh\n");
    assert_eq!(f.settings().width(), None);
    assert_eq!(f.settings().height(), None);
    assert!(
        effects.iter().any(|e| matches!(e, MemberEffect::Save(_))),
        "the cleared overrides are saved"
    );
    let shown = text(&f.view(words()));
    assert!(shown.contains("Width: automatic") && shown.contains("Height: automatic"));
    assert!(
        !shown.contains("so "),
        "nothing is cut once cleared: {shown}"
    );
}

#[test]
fn the_pause_line_does_not_overstate_what_it_does() {
    let f = at_settings();
    let shown = text(&f.view(words()));
    assert!(
        shown.contains("long text stops at the end of each screen"),
        "{shown}"
    );
}

/// D93: the welcome carries no sentence about mail at rest over ssh or telnet; where mail is
/// withheld (telnet) it says so.
#[test]
fn the_welcome_has_no_d78_sentence_over_ssh_or_telnet() {
    let ssh = text(&flow(TerminalSettings::default(), None).view(words()));
    assert!(
        !ssh.contains("can read your mail") && !ssh.contains("encrypted at rest"),
        "{ssh}"
    );
    assert!(!ssh.contains("Mail is available over ssh."), "{ssh}");
    let telnet = text(
        &flow(TerminalSettings::default(), None)
            .without_mail_note()
            .view(words()),
    );
    assert!(telnet.contains("Mail is available over ssh."), "{telnet}");
    assert!(
        !telnet.contains("can read your mail") && !telnet.contains("encrypted at rest"),
        "{telnet}"
    );
}

// ---- pager (L8) ----

fn numbered(n: usize) -> Vec<String> {
    (1..=n).map(|i| format!("line {i}")).collect()
}

fn pager(n: usize, pause: bool) -> Pager {
    Pager::new(numbered(n), &TerminalSettings::default().with_pause(pause))
}

#[test]
fn with_pause_on_a_long_text_pages_with_a_more_prompt_and_every_line_is_reachable() {
    let mut p = pager(25, true);
    let mut seen = Vec::new();
    let mut prompts = Vec::new();
    loop {
        let view = p.view(10);
        seen.extend(view.lines.iter().cloned());
        prompts.push(view.prompt);
        if p.input(PagerKey::Next, 10) == PagerEffect::Done {
            break;
        }
    }
    assert_eq!(seen, numbered(25), "nothing dropped, nothing repeated");
    assert_eq!(
        prompts,
        [
            PagerPrompt::More { page: 1, pages: 3 },
            PagerPrompt::More { page: 2, pages: 3 },
            PagerPrompt::End { page: 3, pages: 3 },
        ]
    );
}

#[test]
fn with_pause_off_there_is_no_prompt_and_the_cut_is_stated() {
    let mut p = pager(25, false);
    let view = p.view(10);
    assert_eq!(view.lines, &numbered(10)[..]);
    assert_eq!(view.prompt, PagerPrompt::Clipped { hidden: 15 });
    assert_eq!(p.input(PagerKey::Next, 10), PagerEffect::Done, "no paging");
    assert_eq!(pager(8, false).view(10).prompt, PagerPrompt::None);
}

#[test]
fn a_text_that_fits_has_no_prompt_either_way() {
    for pause in [true, false] {
        let p = pager(10, pause);
        assert_eq!(p.view(10).prompt, PagerPrompt::None);
        assert_eq!(p.view(10).lines.len(), 10);
    }
    assert_eq!(pager(0, true).view(10).prompt, PagerPrompt::None);
}

#[test]
fn previous_and_stop_work_and_a_resize_keeps_the_reader_on_a_valid_page() {
    let mut p = pager(25, true);
    p.input(PagerKey::Next, 10);
    assert_eq!(p.view(10).lines[0], "line 11");
    assert_eq!(p.input(PagerKey::Previous, 10), PagerEffect::Stay);
    assert_eq!(p.view(10).lines[0], "line 1");
    assert_eq!(
        p.input(PagerKey::Previous, 10),
        PagerEffect::Stay,
        "stops at the top"
    );
    p.input(PagerKey::Next, 10);
    // The window shrinks to 4 rows: the page that holds line 11 is shown.
    assert!(p.view(4).lines.contains(&"line 11".to_owned()));
    assert_eq!(p.view(0).lines.len(), 1, "zero rows is treated as one");
    assert_eq!(p.input(PagerKey::Stop, 10), PagerEffect::Done);
}

#[test]
fn no_screen_claims_privacy() {
    let mut f = at_settings();
    let mut all = text(&f.view(words()));
    for keys in ["\u{1b}", "l"] {
        press(&mut f, keys);
        all.push_str(&text(&f.view(words())));
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

// ---- layout and theme (M3-A) ----

fn at_settings_with_themes() -> MemberFlow {
    let mut f = at_settings();
    f.set_themes(vec![
        ThemeName::new("default").unwrap(),
        ThemeName::new("amber").unwrap(),
        ThemeName::new("mono").unwrap(),
    ]);
    f
}

#[test]
fn layout_and_theme_default_to_classic_and_default() {
    let s = TerminalSettings::default();
    assert_eq!(s.layout(), LayoutId::Classic);
    assert_eq!(s.theme().as_str(), DEFAULT_THEME);
    assert_eq!(
        ThemeName::default_theme(),
        ThemeName::new(DEFAULT_THEME).unwrap()
    );
}

#[test]
fn size_changes_keep_the_layout_and_theme() {
    let s = TerminalSettings::default()
        .with_layout(LayoutId::Compact)
        .with_theme(ThemeName::new("amber").unwrap())
        .with_width(Some(100))
        .unwrap()
        .with_height(Some(30))
        .unwrap();
    assert_eq!(s.layout(), LayoutId::Compact);
    assert_eq!(s.theme().as_str(), "amber");
}

#[test]
fn stored_layout_text_round_trips_and_unknown_text_is_refused() {
    for layout in LayoutId::ALL {
        assert_eq!(layout_from_stored(layout_to_stored(layout)), Some(layout));
    }
    assert_eq!(layout_from_stored("two-pane"), None);
    assert_eq!(layout_from_stored(""), None);
}

#[test]
fn the_next_layout_cycles_through_every_layout_and_wraps() {
    let mut seen = vec![LayoutId::Classic];
    let mut at = LayoutId::Classic.next();
    while at != LayoutId::Classic {
        seen.push(at);
        at = at.next();
    }
    assert_eq!(seen, LayoutId::ALL);
}

#[test]
fn theme_names_are_bounded_ascii() {
    for good in ["default", "a", "amber-2", "high_contrast", "0day"] {
        assert_eq!(ThemeName::new(good).unwrap().as_str(), good);
    }
    let longest = "a".repeat(THEME_NAME_MAX);
    assert!(ThemeName::new(&longest).is_ok());
    for bad in [
        "",
        "-lead",
        "_lead",
        "Upper",
        "has space",
        "dot.ted",
        "slash/es",
        "caf\u{e9}",
        "esc\u{1b}[31m",
        "nul\0",
        "\u{e000}",
        "\u{27e6}x",
        &"a".repeat(THEME_NAME_MAX + 1),
    ] {
        assert!(ThemeName::new(bad).is_err(), "{bad:?} must be refused");
    }
}

#[test]
fn the_settings_screen_switches_layout_live_and_saves_it() {
    let mut f = at_settings_with_themes();
    assert!(text(&f.view(words())).contains("Layout: classic"));
    let effects = press(&mut f, "l");
    assert_eq!(f.settings().layout(), LayoutId::Lightbar);
    assert!(
        matches!(effects.last(), Some(MemberEffect::Save(s)) if s.layout() == LayoutId::Lightbar)
    );
    assert!(text(&f.view(words())).contains("Layout: lightbar"));
    press(&mut f, "l");
    assert_eq!(f.settings().layout(), LayoutId::Compact);
    press(&mut f, "llll");
    assert_eq!(f.settings().layout(), LayoutId::Split);
    press(&mut f, "l");
    assert_eq!(
        f.settings().layout(),
        LayoutId::Classic,
        "wraps after seven"
    );
}

#[test]
fn the_settings_screen_cycles_themes_and_wraps() {
    let mut f = at_settings_with_themes();
    press(&mut f, "t");
    assert_eq!(f.settings().theme().as_str(), "amber");
    assert!(text(&f.view(words())).contains("Theme: amber"));
    press(&mut f, "tt");
    assert_eq!(f.settings().theme().as_str(), "default");
}

#[test]
fn a_stored_theme_that_is_not_loaded_moves_to_the_first_one() {
    let gone = ThemeName::new("removed-by-sysop").unwrap();
    let mut f = flow(
        TerminalSettings::default()
            .with_tier(Tier::Ascii)
            .with_theme(gone),
        None,
    );
    press(&mut f, "xs");
    f.set_themes(vec![
        ThemeName::new("default").unwrap(),
        ThemeName::new("amber").unwrap(),
    ]);
    press(&mut f, "t");
    assert_eq!(f.settings().theme().as_str(), "default");
}

#[test]
fn the_theme_key_does_nothing_when_no_themes_are_loaded() {
    let mut f = at_settings();
    let effects = press(&mut f, "t");
    assert_eq!(effects, [MemberEffect::Stay]);
    assert_eq!(f.settings().theme().as_str(), DEFAULT_THEME);
}

#[test]
fn layout_and_theme_changes_need_the_settings_permission() {
    // The same gate as every settings change: the page closes when the permission is gone.
    let mut f = at_settings_with_themes();
    f.set_access(Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::MEMBER,
        10,
        &Via::Account,
        std::iter::empty(),
    ));
    let effects = press(&mut f, "l");
    assert_eq!(effects, [MemberEffect::Stay]);
    assert_eq!(f.settings().layout(), LayoutId::Classic);
}

#[test]
fn cursor_keys_move_the_menu_only_in_a_layout_with_a_cursor() {
    let mut f = flow(
        TerminalSettings::default()
            .with_tier(Tier::Ascii)
            .with_layout(LayoutId::Classic),
        None,
    );
    press(&mut f, "x");
    // Classic draws no cursor: Down and Enter must not act on an unseen selection.
    f.input(MemberInput::Down);
    assert_eq!(f.input(MemberInput::Enter), MemberEffect::Stay);
    assert_eq!(f.menu_page().unwrap().cursor, 0);
    assert!(!f.is_logged_off());

    let mut f = flow(
        TerminalSettings::default()
            .with_tier(Tier::Ascii)
            .with_layout(LayoutId::Lightbar),
        None,
    );
    press(&mut f, "x");
    f.input(MemberInput::Down);
    assert_eq!(f.menu_page().unwrap().cursor, 1);
    f.input(MemberInput::Up);
    f.input(MemberInput::Up);
    assert_eq!(
        f.menu_page().unwrap().cursor,
        0,
        "the cursor stops at the first item"
    );
}

#[test]
fn the_seven_layouts_cycle_and_round_trip_their_stored_text() {
    assert_eq!(LayoutId::ALL.len(), 7);
    let mut layout = LayoutId::Classic;
    for expected in LayoutId::ALL {
        assert_eq!(layout, expected);
        assert_eq!(layout_from_stored(layout_to_stored(layout)), Some(layout));
        assert_ne!(words().text(layout.label_key()), layout.label_key());
        layout = layout.next();
    }
    assert_eq!(layout, LayoutId::Classic);
    for text in ["dashboard", "wide", "focus", "split"] {
        assert!(layout_from_stored(text).is_some(), "{text}");
    }
    assert_eq!(layout_from_stored("grid"), None);
    for layout in LayoutId::ALL {
        assert_eq!(
            layout.has_cursor(),
            !matches!(layout, LayoutId::Classic | LayoutId::Compact)
        );
    }
}

#[test]
fn depth_cap_and_borders_cycle_and_round_trip_their_stored_text() {
    let mut cap = DepthCap::Auto;
    for expected in DepthCap::ALL {
        assert_eq!(cap, expected);
        assert_eq!(depth_cap_from_stored(depth_cap_to_stored(cap)), Some(cap));
        assert_ne!(words().text(cap.label_key()), cap.label_key());
        cap = cap.next();
    }
    assert_eq!(cap, DepthCap::Auto);
    assert_eq!(depth_cap_from_stored("24"), None);
    for borders in BorderChoice::ALL {
        assert_eq!(
            borders_from_stored(borders_to_stored(borders)),
            Some(borders)
        );
        assert_eq!(borders.next().next(), borders);
        assert_ne!(words().text(borders.label_key()), borders.label_key());
    }
    assert_eq!(borders_from_stored("heavy"), None);
}

#[test]
fn new_settings_default_to_following_the_board_with_motion_on() {
    let s = TerminalSettings::default();
    assert_eq!(s.layout_choice(), None);
    assert_eq!(s.theme_choice(), None);
    assert!(s.motion());
    assert_eq!(s.depth_cap(), DepthCap::Auto);
    assert_eq!(s.borders(), BorderChoice::ThemeDefault);
    let chosen = s
        .with_layout(LayoutId::Wide)
        .with_theme(ThemeName::new("amber").unwrap());
    assert_eq!(chosen.layout_choice(), Some(LayoutId::Wide));
    assert_eq!(chosen.with_default_layout().layout_choice(), None);
    assert_eq!(chosen.with_default_theme().theme_choice(), None);
    // A size change keeps every other setting.
    let sized = chosen.with_motion(false).with_width(Some(100)).unwrap();
    assert_eq!(sized.layout_choice(), Some(LayoutId::Wide));
    assert!(!sized.motion());
}

#[test]
fn motion_depth_and_borders_keys_save_at_once_and_show_in_words() {
    let mut f = at_settings();
    let out = text(&f.view(words()));
    assert!(
        out.contains("Motion: on  Depth: auto  Borders: style's own  Mouse: off"),
        "{out}"
    );
    let effects = press(&mut f, "m");
    assert!(matches!(effects.last(), Some(MemberEffect::Save(s)) if !s.motion()));
    let effects = press(&mut f, "d");
    assert!(
        matches!(effects.last(), Some(MemberEffect::Save(s)) if s.depth_cap() == DepthCap::Ansi16)
    );
    let effects = press(&mut f, "r");
    assert!(
        matches!(effects.last(), Some(MemberEffect::Save(s)) if s.borders() == BorderChoice::Ascii)
    );
    let out = text(&f.view(words()));
    assert!(
        out.contains("Motion: off  Depth: 16 colors  Borders: ascii  Mouse: off"),
        "{out}"
    );
    press(&mut f, "ddd");
    assert_eq!(f.settings().depth_cap(), DepthCap::Auto);
}

/// UI-9: mouse reporting is the member's choice. It starts off, `o` toggles it and saves at
/// once, and the settings screen says which it is in words.
#[test]
fn mouse_key_toggles_the_choice_and_shows_it_in_words() {
    let mut f = at_settings();
    assert!(!f.settings().mouse());
    assert!(text(&f.view(words())).contains("Mouse: off"));
    let effects = press(&mut f, "o");
    assert!(matches!(effects.last(), Some(MemberEffect::Save(s)) if s.mouse()));
    assert!(text(&f.view(words())).contains("Mouse: on"));
    let effects = press(&mut f, "o");
    assert!(matches!(effects.last(), Some(MemberEffect::Save(s)) if !s.mouse()));
}

#[test]
fn the_new_settings_need_the_settings_permission() {
    let mut f = at_settings();
    f.set_access(Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::MEMBER,
        10,
        &Via::Account,
        std::iter::empty(),
    ));
    for key in ["m", "d", "r"] {
        assert_eq!(press(&mut f, key), [MemberEffect::Stay], "{key}");
    }
    assert!(f.settings().motion());
}

#[test]
fn the_layout_key_cycles_only_the_layouts_the_sysop_allows() {
    let mut f = at_settings_with_themes();
    f.set_layouts(vec![LayoutId::Dashboard, LayoutId::Focus]);
    // No choice yet and the board default (classic) is not allowed: the first allowed.
    assert_eq!(f.effective_layout(), LayoutId::Dashboard);
    press(&mut f, "l");
    assert_eq!(f.settings().layout_choice(), Some(LayoutId::Focus));
    press(&mut f, "l");
    assert_eq!(f.settings().layout(), LayoutId::Dashboard);
    // A stored choice that is no longer allowed moves to the first allowed.
    let mut g = flow(
        TerminalSettings::default()
            .with_tier(Tier::Ascii)
            .with_layout(LayoutId::Split),
        None,
    );
    press(&mut g, "xs");
    g.set_layouts(vec![LayoutId::Wide, LayoutId::Focus]);
    assert_eq!(g.effective_layout(), LayoutId::Wide);
    press(&mut g, "l");
    assert_eq!(g.settings().layout(), LayoutId::Wide);
}

#[test]
fn a_single_allowed_layout_or_theme_is_locked_and_the_screen_says_so() {
    let mut f = at_settings_with_themes();
    f.set_layouts(vec![LayoutId::Wide]);
    f.set_themes(vec![ThemeName::new("amber").unwrap()]);
    assert_eq!(press(&mut f, "l"), [MemberEffect::Stay]);
    assert_eq!(press(&mut f, "t"), [MemberEffect::Stay]);
    assert_eq!(f.settings().layout_choice(), None, "nothing was saved");
    assert_eq!(f.effective_layout(), LayoutId::Wide);
    assert_eq!(f.effective_theme().as_str(), "amber");
    let out = text(&f.view(words()));
    assert!(
        out.contains("Layout: wide (set by the sysop)   Theme: amber (set by the sysop)"),
        "{out}"
    );
}

#[test]
fn a_member_who_has_not_chosen_follows_the_board_default_and_the_screen_says_so() {
    let mut f = at_settings_with_themes();
    f.set_board_defaults(LayoutId::Dashboard, ThemeName::new("amber").unwrap());
    assert_eq!(f.effective_layout(), LayoutId::Dashboard);
    assert_eq!(f.effective_theme().as_str(), "amber");
    let out = text(&f.view(words()));
    assert!(
        out.contains("Layout: dashboard (board default)   Theme: amber (board default)"),
        "{out}"
    );
    // The key moves on from what they see, not from classic.
    press(&mut f, "l");
    assert_eq!(f.settings().layout(), LayoutId::Wide);
    press(&mut f, "t");
    assert_eq!(f.settings().theme().as_str(), "mono");
    // A cursor layout in force by default gives the cursor keys meaning.
    let mut g = at_settings();
    g.set_board_defaults(LayoutId::Lightbar, ThemeName::default_theme());
    press(&mut g, "b");
    let before = g.menu_page().unwrap().cursor;
    g.input(MemberInput::Down);
    assert_eq!(g.menu_page().unwrap().cursor, before + 1);
}

// ---- style gallery and theme packs (UI-7) --------------------------------------------------

fn style(name: &str) -> ThemeName {
    ThemeName::new(name).unwrap()
}

fn gallery_flow(allowed: &[&str]) -> MemberFlow {
    let mut f = flow(
        TerminalSettings::default()
            .with_tier(Tier::Ascii)
            .with_theme(style("phosphor")),
        None,
    );
    f.set_themes(allowed.iter().map(|n| style(n)).collect());
    press(&mut f, "x"); // past the login page
    f
}

/// Open the gallery from the menu: settings, then `y`.
fn open_gallery(f: &mut MemberFlow) {
    press(f, "sy");
}

#[test]
fn the_gallery_lists_exactly_the_styles_the_board_allows() {
    let mut f = gallery_flow(&["phosphor", "neon", "paper"]);
    open_gallery(&mut f);
    let gallery = f.gallery().unwrap();
    assert_eq!(
        gallery.choices(),
        &[style("phosphor"), style("neon"), style("paper")]
    );
    assert_eq!(gallery.selected(), style("phosphor"));
    assert!(!gallery.locked());
}

#[test]
fn picking_in_the_gallery_saves_the_style() {
    let mut f = gallery_flow(&["phosphor", "neon", "paper"]);
    open_gallery(&mut f);
    // Browsing previews without saving.
    assert_eq!(f.input(MemberInput::Down), MemberEffect::Stay);
    assert_eq!(f.gallery().unwrap().selected(), style("neon"));
    assert_eq!(f.settings().theme_choice(), Some(style("phosphor")));
    // Enter uses it, through the same save path as the settings key.
    let effect = f.input(MemberInput::Enter);
    let MemberEffect::Save(saved) = effect else {
        unreachable!("expected a save, got {effect:?}");
    };
    assert_eq!(saved.theme_choice(), Some(style("neon")));
    assert_eq!(f.settings().theme_choice(), Some(style("neon")));
    // Picking the style already in use saves nothing.
    assert_eq!(f.input(MemberInput::Enter), MemberEffect::Stay);
    // Up wraps around; Esc goes back to the settings page.
    f.input(MemberInput::Up);
    f.input(MemberInput::Up);
    assert_eq!(f.gallery().unwrap().selected(), style("paper"));
    f.input(MemberInput::Escape);
    assert!(f.gallery().is_none());
}

#[test]
fn a_locked_board_lets_the_member_look_and_not_pick() {
    let mut f = gallery_flow(&["neon"]);
    open_gallery(&mut f);
    let gallery = f.gallery().unwrap();
    assert!(gallery.locked());
    assert_eq!(gallery.choices(), &[style("neon")]);
    f.input(MemberInput::Down);
    assert_eq!(f.input(MemberInput::Enter), MemberEffect::Stay);
    assert_ne!(f.settings().theme_choice(), Some(style("paper")));
}

#[test]
fn the_gallery_needs_the_settings_permission() {
    let mut f = gallery_flow(&["phosphor", "neon"]);
    open_gallery(&mut f);
    assert!(f.gallery().is_some());
    f.set_access(Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::MEMBER,
        10,
        &Via::Account,
        Permission::ALL
            .iter()
            .copied()
            .filter(|p| *p != Permission::AccountEditSettings),
    ));
    f.input(MemberInput::Down);
    assert!(f.gallery().is_none());
}

fn sysop_flow() -> MemberFlow {
    let handle = sanitize_username("ravenna").unwrap();
    let mut f = MemberFlow::new(
        &handle,
        sysop(),
        None,
        TerminalSettings::default().with_tier(Tier::Ascii),
    );
    press(&mut f, "x");
    f
}

#[test]
fn theme_packs_are_a_sysop_action_and_only_for_themes_manage() {
    // A member without the permission never sees or reaches the item.
    let mut member_flow = flow(TerminalSettings::default().with_tier(Tier::Ascii), None);
    press(&mut member_flow, "x");
    press(&mut member_flow, "op");
    assert!(member_flow.packs().is_none());

    // The sysop gets the screen and a request to list.
    let mut f = sysop_flow();
    let effects = press(&mut f, "op");
    assert_eq!(
        effects.last(),
        Some(&MemberEffect::Packs(PackRequest::List))
    );
    assert!(f.packs().is_some());

    // Taking the permission away closes the screen on the next key.
    let mut without = Permission::ALL.to_vec();
    without.retain(|p| *p != Permission::ThemesManage);
    f.set_access(Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::SYSOP,
        100,
        &Via::Account,
        without,
    ));
    assert_eq!(f.input(MemberInput::Char('e')), MemberEffect::Stay);
    assert!(f.packs().is_none());
}

#[test]
fn keys_on_the_packs_screen_make_requests_for_the_row_under_the_cursor() {
    let mut f = sysop_flow();
    press(&mut f, "op");
    let listing = PackListing {
        styles: vec![
            StyleRow {
                name: ThemeName::default_theme(),
                title: "default".into(),
                exportable: false,
            },
            StyleRow {
                name: style("neon"),
                title: "Neon Night".into(),
                exportable: true,
            },
        ],
        files: vec![
            FileRow {
                file: "good.pack.toml".into(),
                verdict: Ok(style("good")),
            },
            FileRow {
                file: "bad.pack.toml".into(),
                verdict: Err("nope \u{1b}[31m".into()),
            },
        ],
        unavailable: false,
    };
    f.packs_done(listing, Some("hello \u{1b}[31m there"));
    // The message is sanitized on the way in.
    assert!(!f.packs().unwrap().message().unwrap().contains('\u{1b}'));
    // Row 0 is the built-in style: nothing to export.
    assert_eq!(f.input(MemberInput::Char('e')), MemberEffect::Stay);
    f.input(MemberInput::Down);
    assert_eq!(
        f.input(MemberInput::Char('e')),
        MemberEffect::Packs(PackRequest::Export(style("neon")))
    );
    // A style row cannot be imported.
    assert_eq!(f.input(MemberInput::Char('i')), MemberEffect::Stay);
    f.input(MemberInput::Down);
    assert_eq!(
        f.input(MemberInput::Char('i')),
        MemberEffect::Packs(PackRequest::Import("good.pack.toml".into()))
    );
    // A refused file cannot be imported.
    f.input(MemberInput::Down);
    assert_eq!(f.input(MemberInput::Char('i')), MemberEffect::Stay);
}

#[test]
fn the_menus_and_pages_screen_is_a_sysop_action_and_only_for_themes_manage() {
    use crate::menu::{MenuToolsListing, MenuToolsRequest, PageRow};
    // A member never sees or reaches the item.
    let mut member_flow = flow(TerminalSettings::default().with_tier(Tier::Ascii), None);
    press(&mut member_flow, "x");
    press(&mut member_flow, "om");
    assert!(member_flow.menu_tools().is_none());

    // The sysop gets the screen and a request to list.
    let mut f = sysop_flow();
    let effects = press(&mut f, "om");
    assert_eq!(
        effects.last(),
        Some(&MemberEffect::MenuTools(MenuToolsRequest::List))
    );
    assert!(f.menu_tools().is_some());
    f.menu_tools_done(
        MenuToolsListing {
            pages: vec![PageRow {
                id: "rules".into(),
                title: "Rules".into(),
            }],
            ..MenuToolsListing::default()
        },
        Some("ok\u{1b}[2J"),
    );
    // The message is one sanitized line.
    assert!(
        !f.menu_tools()
            .unwrap()
            .message()
            .unwrap()
            .contains('\u{1b}')
    );
    // `l` asks for a reload; Enter views the page; Esc leaves the page, then the screen.
    assert_eq!(
        f.input(MemberInput::Char('l')),
        MemberEffect::MenuTools(MenuToolsRequest::Reload)
    );
    f.input(MemberInput::Enter);
    assert_eq!(f.menu_tools().unwrap().viewing(), Some("rules"));
    f.input(MemberInput::Escape);
    assert!(f.menu_tools().unwrap().viewing().is_none());
    f.input(MemberInput::Escape);
    assert!(f.menu_tools().is_none());

    // Taking the permission away closes the screen on the next key.
    press(&mut f, "om");
    let mut without = Permission::ALL.to_vec();
    without.retain(|p| *p != Permission::ThemesManage);
    f.set_access(Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::SYSOP,
        100,
        &Via::Account,
        without,
    ));
    assert_eq!(f.input(MemberInput::Char('l')), MemberEffect::Stay);
    assert!(f.menu_tools().is_none());
}

#[test]
fn a_sysop_page_opens_scrolls_to_its_end_and_returns_to_the_same_menu() {
    use crate::menu::{ItemSpec, MenuSpec, Spec, build};
    let spec = Spec {
        menus: vec![MenuSpec {
            id: "main".into(),
            title: None,
            items: vec![ItemSpec {
                target: "page:rules".into(),
                key: Some('r'),
                label: None,
                hidden: false,
            }],
        }],
    };
    let tree = build(&spec, &|id| (id == "rules").then(|| "Rules".to_owned()))
        .unwrap()
        .tree;
    let handle = sanitize_username("alice").unwrap();
    let mut f = MemberFlow::new(
        &handle,
        member(),
        None,
        TerminalSettings::default().with_tier(Tier::Ascii),
    )
    .with_menu_tree(tree);
    press(&mut f, "x");
    assert_eq!(f.menu_tree() as *const _, tree as *const _);
    press(&mut f, "r");
    assert_eq!(f.bulletin().unwrap().id(), "rules");
    // The draw reports how far the page can scroll; the keys stop there.
    f.bulletin_extent(2);
    press(&mut f, "jjjjj");
    assert_eq!(f.bulletin().unwrap().scroll(), 2);
    press(&mut f, "kkkkk");
    assert_eq!(f.bulletin().unwrap().scroll(), 0);
    // Esc goes back to the menu, which is still the board's menu.
    f.input(MemberInput::Escape);
    assert!(f.bulletin().is_none());
    assert_eq!(f.menu_page().unwrap().items[0].hotkey, 'r');
}
