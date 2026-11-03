//! Unit tests for what is not the wire: the staff filter and the window clamp. The listener,
//! the login and the screens are exercised over real TCP in `tests/telnet.rs`.
#![allow(clippy::expect_used, clippy::assert_is_empty)]

use russh_bbs_core::access::{Permission, Slug, Snapshot, Source, Via};
use russh_bbs_core::store::AccountId;

use super::connection::clamp_window;
use super::{STAFF_RANK, without_staff};

fn role(name: &str, rank: u8, permissions: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(7)),
        &Slug::parse(name).expect("slug"),
        rank,
        &Via::Account,
        permissions.iter().copied(),
    )
}

fn merged(parts: &[Snapshot]) -> Snapshot {
    let held = parts.iter().flat_map(|part| part.held().to_vec()).collect();
    Snapshot::new(Some(AccountId::from_raw(7)), held)
}

#[test]
fn roles_of_rank_forty_and_above_are_dropped_and_lower_ones_kept() {
    let snapshot = merged(&[
        role("member", 10, &[Permission::ForumRead]),
        role("moderator", 30, &[Permission::MembersView]),
        role("cosysop", 40, &[Permission::MembersApprove]),
        role(
            "sysop",
            100,
            &[Permission::RolesEdit, Permission::SettingsEdit],
        ),
    ]);
    let kept = without_staff(&snapshot);
    let permissions: Vec<_> = kept.held().iter().map(|held| held.permission).collect();
    assert_eq!(
        permissions,
        [Permission::ForumRead, Permission::MembersView]
    );
    assert_eq!(kept.account(), Some(AccountId::from_raw(7)));
    assert!(kept.top_rank().is_some_and(|rank| rank < STAFF_RANK));
}

#[test]
fn a_custom_role_at_or_above_the_line_is_staff_too() {
    let kept = without_staff(&merged(&[
        role("member", 10, &[Permission::ForumRead]),
        role("door-admin", STAFF_RANK, &[Permission::MembersApprove]),
        role("deputy", STAFF_RANK + 1, &[Permission::RolesEdit]),
    ]));
    assert_eq!(kept.held().len(), 1);
}

#[test]
fn a_staff_only_account_is_left_holding_nothing() {
    let kept = without_staff(&role("sysop", 100, Permission::ALL));
    assert!(kept.held().is_empty());
}

#[test]
fn what_is_kept_is_unchanged_in_every_field() {
    let member = role(
        "member",
        10,
        &[Permission::ForumRead, Permission::MembersView],
    );
    let kept = without_staff(&merged(&[
        member.clone(),
        role("sysop", 100, &[Permission::RolesEdit]),
    ]));
    assert_eq!(kept, member);
    assert!(matches!(
        &kept.held()[0].source,
        Source::Role { rank: 10, .. }
    ));
}

#[test]
fn the_window_is_clamped_like_ssh_and_never_below_what_the_screens_need() {
    // Unknown (zero) is 80x24; tiny is raised to the minimum; huge is cut to the SSH maximum.
    assert_eq!(
        (clamp_window(0, 0).cols(), clamp_window(0, 0).rows()),
        (80, 24)
    );
    assert_eq!(
        (clamp_window(1, 1).cols(), clamp_window(1, 1).rows()),
        (40, 10)
    );
    let huge = clamp_window(u16::MAX, u16::MAX);
    assert_eq!((huge.cols(), huge.rows()), (500, 200));
    let normal = clamp_window(132, 43);
    assert_eq!((normal.cols(), normal.rows()), (132, 43));
}

mod footer {
    use std::sync::Arc;

    use russh_bbs_core::icons::SecurityMarker;

    use crate::ssh::screen::{Content, Screen};
    use crate::ssh::terminal::{PtySize, RenderCap};
    use crate::ui::Ui;

    fn frame(cols: u32, rows: u32, lines: Vec<String>) -> String {
        let mut screen = Screen::with_footer(
            PtySize::from_request(cols, rows),
            RenderCap::default(),
            Content::Limited(lines),
            Arc::new(Ui::standard().expect("ui")),
            Some(SecurityMarker::Unencrypted),
        )
        .expect("screen");
        String::from_utf8_lossy(&screen.enter().expect("frame")).into_owned()
    }

    #[test]
    fn content_that_fills_the_window_never_reaches_the_marker_row() {
        // More and longer lines than the window holds: the layout is given one row less, so
        // the only write to the last row is the marker's own.
        let lines = (0..80)
            .map(|n| format!("line {n} {}", "x".repeat(300)))
            .collect();
        let frame = frame(80, 24, lines);
        assert_eq!(frame.matches("\u{1b}[24;").count(), 1, "{frame:?}");
        assert!(
            frame.ends_with("\u{1b}[24;1H\u{1b}[2K\u{1b}[1;7mUNENCRYPTED\u{1b}[0m anyone on the network can read this session"),
            "{frame:?}"
        );
    }

    #[test]
    fn a_narrow_window_keeps_the_whole_marker_and_cuts_only_the_note() {
        let frame = frame(40, 12, vec!["x".to_owned()]);
        assert!(
            frame.contains("\u{1b}[1;7mUNENCRYPTED\u{1b}[0m"),
            "{frame:?}"
        );
        let last = frame.rsplit("\u{1b}[12;1H").next().expect("last row");
        // 11 cells of marker, a space, then at most 28 of the note.
        let visible: String = last
            .replace("\u{1b}[2K", "")
            .replace("\u{1b}[1;7m", "")
            .replace("\u{1b}[0m", "");
        assert!(visible.chars().count() <= 40, "{visible:?}");
    }

    #[test]
    fn a_resize_moves_the_marker_and_the_layouts_follow() {
        let mut screen = Screen::with_footer(
            PtySize::from_request(80, 24),
            RenderCap::default(),
            Content::Limited(vec!["hello".to_owned()]),
            Arc::new(Ui::standard().expect("ui")),
            Some(SecurityMarker::Unencrypted),
        )
        .expect("screen");
        screen.enter().expect("first frame");
        screen.set_size(PtySize::from_request(100, 40));
        let frame =
            String::from_utf8_lossy(&screen.render().expect("frame").expect("redraw")).into_owned();
        assert!(
            frame.contains("\u{1b}[40;1H\u{1b}[2K\u{1b}[1;7mUNENCRYPTED"),
            "{frame:?}"
        );
        assert_eq!(frame.matches("\u{1b}[40;").count(), 1, "{frame:?}");
    }

    #[test]
    fn without_a_footer_nothing_extra_is_drawn() {
        let mut screen = Screen::new(
            PtySize::from_request(80, 24),
            RenderCap::default(),
            Content::Limited(vec!["hello".to_owned()]),
            Arc::new(Ui::standard().expect("ui")),
        )
        .expect("screen");
        let frame = String::from_utf8_lossy(&screen.enter().expect("frame")).into_owned();
        assert!(!frame.contains("UNENCRYPTED"), "{frame:?}");
    }
}
