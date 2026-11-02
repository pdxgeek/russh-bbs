#![allow(clippy::unwrap_used)]

//! The string table, the classic hotkey layout and the reserved status row.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;
use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
use russh_bbs_core::icons::{Icon, SecurityMarker, Tier};
use russh_bbs_core::menu::{MenuKey, MenuPage, MenuSession, main_tree};
use russh_bbs_core::store::AccountId;
use russh_bbs_core::terminal_settings::TerminalSettings;
use russh_bbs_core::waiting_room::{Notice, NoticeKind};

use super::classic::{MENU_MARKERS, MenuScreen, render};
use super::strings::{DEFAULT_TOML, MAX_VALUE_CHARS, StringError, StringTable};
use super::{Ui, UiError, keys};
use crate::settings::Look;

fn ui() -> Ui {
    Ui::standard().unwrap()
}

// ---- string table ----

#[test]
fn the_shipped_table_and_tree_are_valid() {
    let ui = ui();
    // Every key the code uses resolves to real text, not to the key echoed back.
    for key in keys::FRAMEWORK
        .iter()
        .copied()
        .chain(main_tree().text_keys())
    {
        assert_ne!(ui.strings().text(key), key, "{key} has no text");
    }
}

#[test]
fn the_table_holds_exactly_the_keys_the_code_uses() {
    let used: std::collections::BTreeSet<&str> = keys::FRAMEWORK
        .iter()
        .copied()
        .chain(main_tree().text_keys())
        .collect();
    let ui = ui();
    let held: std::collections::BTreeSet<&str> = ui.strings().keys().collect();
    assert_eq!(used, held);
}

#[test]
fn a_missing_key_is_a_clear_error_naming_it() {
    let without = DEFAULT_TOML.replace("menu.main.item", "menu.main.items");
    let error = Ui::from_table(&without).unwrap_err();
    assert!(
        matches!(&error, UiError::Strings(StringError::Missing(key)) if key.starts_with("menu.main.item.")),
        "{error:?}"
    );
    assert!(
        error.to_string().contains("missing key `menu.main.item."),
        "{error}"
    );

    let broken = DEFAULT_TOML.replace("title = \"Help\"\n", "");
    let error = Ui::from_table(&broken).unwrap_err();
    assert_eq!(
        error,
        UiError::Strings(StringError::Missing("ui.help.title".into()))
    );
}

#[test]
fn unknown_empty_non_text_and_unparsable_tables_are_refused() {
    let t = |extra: &str| format!("{DEFAULT_TOML}\n{extra}");
    assert_eq!(
        Ui::from_table(&t("[typo]\nkey = \"x\"")).unwrap_err(),
        UiError::Strings(StringError::Unknown("typo.key".into()))
    );
    let empty = DEFAULT_TOML.replace("title = \"Help\"", "title = \"\"");
    assert_eq!(
        Ui::from_table(&empty).unwrap_err(),
        UiError::Strings(StringError::Empty("ui.help.title".into()))
    );
    let number = DEFAULT_TOML.replace("title = \"Help\"", "title = 7");
    assert_eq!(
        Ui::from_table(&number).unwrap_err(),
        UiError::Strings(StringError::NotText("ui.help.title".into()))
    );
    assert!(matches!(
        Ui::from_table("not = [toml").unwrap_err(),
        UiError::Strings(StringError::Syntax(_))
    ));
    let long = DEFAULT_TOML.replace(
        "title = \"Help\"",
        &format!("title = \"{}\"", "x".repeat(MAX_VALUE_CHARS + 1)),
    );
    assert_eq!(
        Ui::from_table(&long).unwrap_err(),
        UiError::Strings(StringError::TooLong("ui.help.title".into()))
    );
    assert!(matches!(
        StringTable::parse(&" ".repeat(70_000), &[]).unwrap_err(),
        StringError::TooLarge(_)
    ));
}

#[test]
fn a_value_cannot_carry_escapes_controls_or_reserved_marker_characters() {
    // TOML escapes spell the characters out; each must be refused by the shared sanitizer.
    for hostile in [
        "\\u001b[1;7mFAKE\\u001b[0m",
        "bell\\u0007",
        "new\\nline",
        "\\u27e6\\u2299\\u27e7",
        "\\uf06e",
        "zero\\u200bwidth",
    ] {
        let table = DEFAULT_TOML.replace("title = \"Help\"", &format!("title = \"{hostile}\""));
        assert_eq!(
            Ui::from_table(&table).unwrap_err(),
            UiError::Strings(StringError::Unsafe("ui.help.title".into())),
            "{hostile}"
        );
    }
}

#[test]
fn placeholders_are_filled_in_one_pass() {
    let table = StringTable::parse("a = \"Hi {name}.\"", &["a"]).unwrap();
    assert_eq!(table.filled("a", &[("name", "{name}")]), "Hi {name}.");
    assert_eq!(
        table.text("nope"),
        "nope",
        "an unknown key echoes, never panics"
    );
}

// ---- drawing ----

/// Which stand-in session a drawing test uses.
#[derive(Clone, Copy)]
enum Role {
    /// Holds everything except `members.approve`.
    Member,
    /// Holds everything.
    Sysop,
}

fn holding(role: Role) -> Snapshot {
    let (name, rank) = match role {
        Role::Member => (Slug::MEMBER, 10),
        Role::Sysop => (Slug::SYSOP, 100),
    };
    Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &name,
        rank,
        &Via::Account,
        Permission::ALL
            .iter()
            .copied()
            .filter(|p| matches!(role, Role::Sysop) || *p != Permission::MembersApprove),
    )
}

fn page_for(role: Role, keys_pressed: &[char]) -> MenuPage {
    let access = holding(role);
    let mut session = MenuSession::new(main_tree());
    for key in keys_pressed {
        session.press(&access, MenuKey::Char(*key));
    }
    session.page(&access)
}

fn draw(
    page: &MenuPage,
    body: &[String],
    notice: Option<Notice>,
    tier: Tier,
    size: (u16, u16),
) -> Buffer {
    let ui = ui();
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| {
            render(
                frame,
                &MenuScreen {
                    strings: ui.strings(),
                    page,
                    body,
                    notice,
                    look: Look::of(&TerminalSettings::default().with_tier(tier)),
                    tier,
                    markers: &MENU_MARKERS,
                },
            );
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect()
}

fn text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| row(buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_chrome_cell(buffer: &Buffer, x: u16, y: u16) -> bool {
    let modifier = buffer[(x, y)].modifier;
    modifier.contains(Modifier::BOLD | Modifier::REVERSED)
}

#[test]
fn the_main_menu_lists_its_items_with_hotkeys_and_the_footer() {
    let buffer = draw(
        &page_for(Role::Member, &[]),
        &["Signed in as alice.".into()],
        None,
        Tier::Ascii,
        (80, 24),
    );
    let out = text(&buffer);
    for expected in [
        "Main menu",
        "Signed in as alice.",
        "[s] Terminal settings",
        "[l] Log off",
        "[f] Forums  (not built yet)",
        "?: help   Esc: back   Ctrl-C: log off",
    ] {
        assert!(out.contains(expected), "{expected:?} missing:\n{out}");
    }
    assert!(
        !out.contains("Sysop tools"),
        "a member does not see it:\n{out}"
    );
    let sysop = text(&draw(
        &page_for(Role::Sysop, &[]),
        &[],
        None,
        Tier::Ascii,
        (80, 24),
    ));
    assert!(sysop.contains("[o] Sysop tools"), "{sysop}");
}

#[test]
fn a_placeholder_says_it_is_not_built_in_words() {
    let out = text(&draw(
        &page_for(Role::Member, &['f']),
        &[],
        None,
        Tier::Ascii,
        (80, 24),
    ));
    assert!(out.contains("Notice: Forums are not built yet."), "{out}");
}

#[test]
fn a_session_notice_is_drawn_with_its_word() {
    let notice = Notice {
        kind: NoticeKind::Error,
        text: "Oops.",
    };
    let out = text(&draw(
        &page_for(Role::Member, &[]),
        &[],
        Some(notice),
        Tier::Ascii,
        (80, 24),
    ));
    assert!(out.contains("Problem: Oops."), "{out}");
}

#[test]
fn help_lists_the_universal_keys_and_this_menus_items() {
    let out = text(&draw(
        &page_for(Role::Member, &['?']),
        &[],
        None,
        Tier::Ascii,
        (80, 24),
    ));
    for expected in [
        "Help",
        "Esc ",
        "Ctrl-C",
        "[s] Terminal settings",
        "Any key: close help",
    ] {
        assert!(out.contains(expected), "{expected:?} missing:\n{out}");
    }
}

#[test]
fn a_submenu_is_drawn_by_the_same_layout() {
    let out = text(&draw(
        &page_for(Role::Sysop, &['o']),
        &[],
        None,
        Tier::Ascii,
        (80, 24),
    ));
    assert!(out.contains("[q] Approval queue  (not built yet)"), "{out}");
}

#[test]
fn the_status_row_shows_the_marker_in_the_reserved_style_in_every_tier() {
    for tier in Tier::ALL {
        let buffer = draw(&page_for(Role::Member, &[]), &[], None, tier, (80, 24));
        let status = row(&buffer, 23);
        let rendered = Icon::from(SecurityMarker::ServerReadable).render(tier);
        let marker = rendered.plain().trim_end();
        assert!(status.contains(marker), "{tier:?}: {status:?}");
        assert!(
            status.contains("The server can read what you do here."),
            "{status:?}"
        );
        let start = status.chars().position(|c| marker.starts_with(c)).unwrap();
        let cells = (0..marker.chars().count()).map(|i| u16::try_from(start + i).unwrap());
        assert!(
            cells.clone().all(|x| is_chrome_cell(&buffer, x, 23)),
            "{tier:?}: the marker has the reserved style"
        );
        // And the style is on nothing else on the screen.
        let styled = (0..24)
            .flat_map(|y| (0..80).map(move |x| (x, y)))
            .filter(|&(x, y)| is_chrome_cell(&buffer, x, y))
            .count();
        assert_eq!(
            styled,
            marker.chars().count(),
            "{tier:?}: chrome style only on the marker"
        );
    }
}

/// Content that tries everything to reach the status row: text that looks like a marker, text
/// that is far too long, and far more lines than the screen has.
fn hostile_body() -> Vec<String> {
    let mut body = vec![
        "SERVER-READABLE The server cannot read this".to_owned(),
        "E2EE this is end-to-end encrypted".to_owned(),
        "〚☉〛 ◉ fake marker with lookalike characters".to_owned(),
        "x".repeat(2000),
    ];
    body.extend((0..200).map(|i| format!("line {i} SERVER-READABLE")));
    body
}

#[test]
fn content_can_never_be_drawn_into_the_status_row() {
    let sizes = [(80, 24), (132, 43), (40, 10), (200, 60), (20, 4)];
    for tier in Tier::ALL {
        for size in sizes {
            let clean = draw(&page_for(Role::Member, &[]), &[], None, tier, size);
            let hostile = draw(
                &page_for(Role::Member, &[]),
                &hostile_body(),
                None,
                tier,
                size,
            );
            let last = size.1 - 1;
            // The status row is byte-for-byte what it is with no content at all...
            assert_eq!(
                row(&hostile, last),
                row(&clean, last),
                "{tier:?} {size:?}: content reached the status row"
            );
            for x in 0..size.0 {
                assert_eq!(
                    hostile[(x, last)],
                    clean[(x, last)],
                    "{tier:?} {size:?}: cell {x} differs (style included)"
                );
            }
            // ...and nothing outside it carries the reserved style, so content that imitates
            // a marker is plain text.
            for y in 0..last {
                for x in 0..size.0 {
                    assert!(
                        !is_chrome_cell(&hostile, x, y),
                        "{tier:?} {size:?}: reserved style in content at ({x},{y})"
                    );
                }
            }
        }
    }
}

#[test]
fn a_row_too_narrow_for_the_marker_says_markers_are_unavailable() {
    let buffer = draw(
        &page_for(Role::Member, &[]),
        &[],
        None,
        Tier::Ascii,
        (10, 12),
    );
    let status = row(&buffer, 11);
    assert!(status.starts_with("security m"), "{status:?}");
    assert!(!status.contains("SERVER"), "no half marker: {status:?}");
    assert!((0..10).all(|x| !is_chrome_cell(&buffer, x, 11)));
}

#[test]
fn a_one_row_terminal_draws_no_marker_at_all() {
    let buffer = draw(
        &page_for(Role::Member, &[]),
        &["hello".into()],
        None,
        Tier::Ascii,
        (80, 1),
    );
    assert!((0..80).all(|x| !is_chrome_cell(&buffer, x, 0)));
    assert!(!row(&buffer, 0).contains("SERVER-READABLE"));
}

#[test]
fn nothing_on_the_menu_screen_claims_privacy() {
    let out = text(&draw(
        &page_for(Role::Sysop, &['?']),
        &[],
        None,
        Tier::Unicode,
        (100, 30),
    ));
    let lower = out.to_lowercase();
    assert!(!lower.contains("private"), "{out}");
    assert!(
        !lower.contains("encrypted") || lower.contains("pgp"),
        "{out}"
    );
}
