#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::items_after_statements
)]

use std::collections::VecDeque;

use proptest::prelude::*;
use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
use russh_bbs_core::menu::main_tree;
use russh_bbs_core::store::AccountId;

use super::{BoardName, Group, Outcome, Palette, Sources, entries, remember, score};
use crate::ssh::terminal::Key;
use crate::ui::Ui;

fn snapshot(role: &Slug, rank: u8, permissions: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        role,
        rank,
        &Via::Account,
        permissions.iter().copied(),
    )
}

fn everything() -> Snapshot {
    snapshot(&Slug::SYSOP, 100, Permission::ALL)
}

fn build(access: &Snapshot, doors: &[&str], boards: &[&str]) -> Vec<super::Entry> {
    let ui = Ui::standard().unwrap();
    entries(&Sources {
        tree: main_tree(),
        access,
        strings: ui.strings(),
        doors: doors.iter().map(|d| (*d).to_owned()).collect(),
        boards: boards.iter().map(|b| BoardName((*b).to_owned())).collect(),
    })
}

fn labels(entries: &[super::Entry]) -> Vec<String> {
    entries.iter().map(|e| e.label().to_owned()).collect()
}

fn has(entries: &[super::Entry], needle: &str) -> bool {
    entries.iter().any(|e| e.label().contains(needle))
}

#[test]
fn a_full_member_sees_areas_settings_doors_and_boards() {
    let found = build(
        &everything(),
        &["Coin Toss", "Saltmarch"],
        &["General", "Tech"],
    );
    for want in [
        "Forums",
        "Mail",
        "Settings",
        "Coin Toss",
        "Saltmarch",
        "General",
        "Tech",
    ] {
        assert!(has(&found, want), "{want} in {:?}", labels(&found));
    }
    assert!(found.iter().any(|e| e.group() == Group::Settings));
    assert!(found.iter().any(|e| e.group() == Group::Door));
    assert!(found.iter().any(|e| e.group() == Group::Board));
}

/// The no-leak rule: take a permission away and what it guards is gone from the palette, even
/// when the caller hands in the names (boards and doors are only listed with their permission).
#[test]
fn the_palette_never_lists_what_the_member_cannot_reach() {
    let without = |gone: Permission| {
        let held: Vec<Permission> = Permission::ALL
            .iter()
            .copied()
            .filter(|p| *p != gone)
            .collect();
        build(
            &snapshot(&Slug::MEMBER, 10, &held),
            &["Coin Toss"],
            &["General"],
        )
    };
    let no_doors = without(Permission::DoorsRun);
    assert!(!has(&no_doors, "Coin Toss") && !no_doors.iter().any(|e| e.group() == Group::Door));
    let no_forums = without(Permission::ForumList);
    assert!(!has(&no_forums, "General") && !no_forums.iter().any(|e| e.group() == Group::Board));
    assert!(!has(&no_forums, "Forums"), "{:?}", labels(&no_forums));
    let no_mail = without(Permission::MailRead);
    assert!(!has(&no_mail, "Mail"));
    let no_settings = without(Permission::AccountEditSettings);
    assert!(!no_settings.iter().any(|e| e.group() == Group::Settings));
    assert!(!has(&no_settings, "Settings"));
    let no_members = without(Permission::MembersView);
    assert!(!has(&no_members, "Member"), "{:?}", labels(&no_members));
    // Sysop-only areas: a plain member's palette has none of them.
    let plain = build(
        &snapshot(
            &Slug::MEMBER,
            10,
            &[Permission::ForumList, Permission::AccountEditSettings],
        ),
        &[],
        &[],
    );
    assert!(
        !has(&plain, "queue") && !has(&plain, "Queue") && !has(&plain, "Sysop"),
        "{:?}",
        labels(&plain)
    );
}

#[test]
fn a_member_with_no_permissions_gets_log_off_and_nothing_else() {
    let none = build(
        &snapshot(&Slug::MEMBER, 10, &[]),
        &["Coin Toss"],
        &["General"],
    );
    assert_eq!(none.len(), 1, "{:?}", labels(&none));
    assert!(
        has(&none, "Log off") || has(&none, "log off") || has(&none, "Logoff"),
        "{:?}",
        labels(&none)
    );
}

#[test]
fn an_anonymous_snapshot_gets_no_entries_with_a_permission() {
    let ui = Ui::standard().unwrap();
    let found = entries(&Sources {
        tree: main_tree(),
        access: &Snapshot::empty(),
        strings: ui.strings(),
        doors: vec!["Coin Toss".into()],
        boards: vec![BoardName("General".into())],
    });
    assert!(
        !has(&found, "Coin Toss") && !has(&found, "General"),
        "{:?}",
        labels(&found)
    );
}

#[test]
fn names_from_the_board_are_sanitized_in_the_label() {
    let found = build(
        &everything(),
        &["Evil\u{1b}[31m Door\u{27e6}x\u{27e7}"],
        &["Bo\u{7}ard\n\u{202e}"],
    );
    for entry in &found {
        let label = entry.label();
        assert!(
            !label
                .chars()
                .any(|c| c.is_control() || c == '\u{27e6}' || c == '\u{27e7}' || c == '\u{202e}'),
            "{label:?}"
        );
    }
}

#[test]
fn an_entry_is_a_key_path_through_the_menu_not_a_shortcut_around_it() {
    let found = build(
        &everything(),
        &["Coin Toss", "Saltmarch"],
        &["General", "Tech"],
    );
    let keys = |needle: &str| {
        found
            .iter()
            .find(|e| e.label().contains(needle))
            .unwrap()
            .keys
            .clone()
    };
    assert_eq!(keys("Forums"), vec![Key::Char('f')]);
    // The second door: the Doors key, down once, Enter.
    assert_eq!(
        keys("Saltmarch"),
        vec![Key::Char('d'), Key::Down, Key::Enter]
    );
    assert_eq!(keys("Tech"), vec![Key::Char('f'), Key::Down, Key::Enter]);
    // A setting is the Settings key and the settings key.
    assert_eq!(
        keys("Settings: mouse"),
        vec![Key::Char('s'), Key::Char('o')]
    );
}

#[test]
fn the_score_is_a_subsequence_match_that_prefers_runs_and_word_starts() {
    assert!(score("frm", "Forums").is_some());
    assert!(score("xyz", "Forums").is_none());
    assert!(score("", "anything").is_some());
    assert!(score("FOR", "forums").is_some(), "case does not matter");
    assert!(score("fo", "Forums") > score("fs", "Forums"));
    assert!(score("mail", "Mail") > score("mil", "Mail"));
    assert!(
        score("sm", "Salt Marsh") > score("sm", "Saltmarsh"),
        "word starts count"
    );
    assert!(score("zzz", "Zzz").is_some());
}

fn palette_over(found: Vec<super::Entry>) -> Palette {
    Palette::new(found, &VecDeque::new())
}

#[test]
fn typing_narrows_and_enter_runs_the_best_match() {
    let mut palette = palette_over(build(&everything(), &["Coin Toss"], &["General"]));
    for c in "mail".chars() {
        assert_eq!(palette.input(Key::Char(c)), Outcome::Stay);
    }
    assert_eq!(palette.shown_labels().first(), Some(&"Mail"));
    assert_eq!(
        palette.input(Key::Enter),
        Outcome::Run(vec![Key::Char('m')])
    );
}

#[test]
fn nothing_matching_runs_nothing() {
    let mut palette = palette_over(build(&everything(), &[], &[]));
    for c in "qqqqzzzz".chars() {
        palette.input(Key::Char(c));
    }
    assert!(palette.shown_labels().is_empty());
    assert_eq!(palette.input(Key::Enter), Outcome::Stay);
}

#[test]
fn escape_closes_and_backspace_widens() {
    let mut palette = palette_over(build(&everything(), &[], &[]));
    palette.input(Key::Char('z'));
    palette.input(Key::Char('z'));
    assert!(palette.shown_labels().is_empty());
    palette.input(Key::Backspace);
    palette.input(Key::Backspace);
    assert!(!palette.shown_labels().is_empty());
    assert_eq!(palette.input(Key::Escape), Outcome::Close);
    assert_eq!(palette.input(Key::Ctrl('k')), Outcome::Close);
}

#[test]
fn recent_choices_come_first_when_nothing_is_typed_and_the_list_is_bounded() {
    let found = build(&everything(), &["Coin Toss"], &["General"]);
    let mut recent = VecDeque::new();
    remember(&mut recent, "board:0");
    let palette = Palette::new(found.clone(), &recent);
    assert_eq!(palette.shown_labels().first(), Some(&"General"));
    for n in 0..40 {
        remember(&mut recent, &format!("x{n}"));
    }
    assert!(recent.len() <= super::MAX_RECENT);
    // Remembering again moves it to the front without a duplicate.
    remember(&mut recent, "x39");
    assert_eq!(recent.iter().filter(|r| *r == "x39").count(), 1);
}

#[test]
fn the_query_is_capped() {
    let mut palette = palette_over(build(&everything(), &[], &[]));
    for _ in 0..500 {
        palette.input(Key::Char('a'));
    }
    assert_eq!(palette.typed().chars().count(), super::MAX_QUERY);
}

proptest! {
    /// Any key sequence leaves the palette in a state that can be drawn and chosen from without
    /// a panic, and what it runs is always one of its own entries' key paths.
    #[test]
    fn any_keys_are_safe_and_only_run_known_paths(
        presses in proptest::collection::vec(
            prop_oneof![
                any::<char>().prop_map(|c| Key::Char(if c.is_ascii_graphic() || c == ' ' { c } else { 'a' })),
                Just(Key::Up), Just(Key::Down), Just(Key::PageUp), Just(Key::PageDown),
                Just(Key::Home), Just(Key::End), Just(Key::Backspace), Just(Key::Enter),
                Just(Key::Left), Just(Key::Other),
            ],
            0..80,
        ),
    ) {
        let found = build(&everything(), &["Coin Toss"], &["General"]);
        let known: Vec<Vec<Key>> = found.iter().map(|e| e.keys.clone()).collect();
        let mut palette = palette_over(found);
        for key in presses {
            if let Outcome::Run(path) = palette.input(key) {
                prop_assert!(known.contains(&path));
                break;
            }
        }
    }
}
