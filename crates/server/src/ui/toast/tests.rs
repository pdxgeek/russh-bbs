#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::items_after_statements
)]

use std::time::{Duration, Instant};

use proptest::prelude::*;
use ratatui::layout::Rect;

use super::{Kind, LIFETIME, MAX_PER_MINUTE, MAX_QUEUED, MAX_SHOWN, Text, Toasts};
use crate::ssh::interact::overlay_area;
use crate::ui::Ui;

fn strings() -> crate::ui::StringTable {
    Ui::standard().unwrap().strings().clone()
}

#[test]
fn a_toast_shows_then_expires_by_itself() {
    let strings = strings();
    let now = Instant::now();
    let mut toasts = Toasts::default();
    assert!(toasts.push(Kind::Mail, Text::Count("toast.new_mail", 2), &strings, now));
    assert_eq!(
        toasts.announcements(),
        vec!["Mail: You have 2 unread mail.".to_owned()]
    );
    assert_eq!(toasts.next_wake(), Some(now + LIFETIME));
    assert!(!toasts.tick(now + Duration::from_secs(1)) || !toasts.is_empty());
    assert!(toasts.tick(now + LIFETIME));
    assert!(toasts.is_empty());
    assert!(
        toasts.next_wake().is_none(),
        "nothing pending means no timer"
    );
}

#[test]
fn the_text_form_names_the_kind_first_so_it_reads_without_colour() {
    let strings = strings();
    let mut toasts = Toasts::default();
    let now = Instant::now();
    for (kind, word) in [
        (Kind::Notice, "Notice: "),
        (Kind::Mail, "Mail: "),
        (Kind::Page, "Page: "),
        (Kind::Door, "Door: "),
    ] {
        let mut toasts_for = Toasts::default();
        toasts_for.push(kind, Text::Member("hello"), &strings, now);
        let line = &toasts_for.announcements()[0];
        assert!(line.starts_with(word), "{line}");
    }
    toasts.push(Kind::Notice, Text::Key("toast.mouse_on"), &strings, now);
    assert!(toasts.announcements()[0].starts_with("Notice: Mouse is on"));
}

#[test]
fn member_text_is_sanitized_to_one_line_without_controls_or_marker_glyphs() {
    let strings = strings();
    let mut toasts = Toasts::default();
    toasts.push(
        Kind::Page,
        Text::Member("hi\u{1b}[31m red\nsecond\u{7} \u{27e6}UNENCRYPTED\u{27e7} \u{202e}rtl"),
        &strings,
        Instant::now(),
    );
    let line = &toasts.announcements()[0];
    assert!(
        !line
            .chars()
            .any(|c| c.is_control() || c == '\u{27e6}' || c == '\u{27e7}' || c == '\u{202e}'),
        "{line:?}"
    );
    assert!(!line.contains('\n'));
    // An empty or all-control message is no toast.
    assert!(!toasts.push(
        Kind::Page,
        Text::Member("\u{7}\u{1b}"),
        &strings,
        Instant::now()
    ));
}

#[test]
fn long_text_is_capped() {
    let strings = strings();
    let mut toasts = Toasts::default();
    toasts.push(
        Kind::Door,
        Text::Member(&"w".repeat(5000)),
        &strings,
        Instant::now(),
    );
    assert!(toasts.announcements()[0].chars().count() < 200);
}

#[test]
fn a_repeat_is_folded_and_the_rate_is_limited() {
    let strings = strings();
    let now = Instant::now();
    let mut toasts = Toasts::default();
    assert!(toasts.push(Kind::Page, Text::Member("same"), &strings, now));
    assert!(
        !toasts.push(Kind::Page, Text::Member("same"), &strings, now),
        "a repeat adds nothing"
    );
    let mut accepted = 1;
    for n in 0..50 {
        if toasts.push(
            Kind::Door,
            Text::Member(&format!("event {n}")),
            &strings,
            now,
        ) {
            accepted += 1;
        }
    }
    assert_eq!(
        accepted, MAX_PER_MINUTE,
        "no more than the per-minute limit"
    );
    // A minute later there is room again.
    assert!(toasts.push(
        Kind::Door,
        Text::Member("later"),
        &strings,
        now + Duration::from_secs(61)
    ));
}

#[test]
fn at_most_three_show_and_the_rest_wait_in_a_bounded_queue() {
    let strings = strings();
    let now = Instant::now();
    let mut toasts = Toasts::default();
    for n in 0..MAX_PER_MINUTE {
        toasts.push(
            Kind::Door,
            Text::Member(&format!("event {n}")),
            &strings,
            now,
        );
    }
    assert_eq!(toasts.announcements().len(), MAX_SHOWN);
    assert!(toasts.queued.len() <= MAX_QUEUED);
    // When the first three expire the waiting ones come in, and a redraw is asked for.
    assert!(toasts.tick(now + LIFETIME));
    assert_eq!(toasts.announcements().len(), MAX_PER_MINUTE - MAX_SHOWN);
}

proptest! {
    /// Placement: every toast rectangle lies inside the overlay area, so none can reach the
    /// header row, the status row (where the marker cell is) or anything outside the screen.
    #[test]
    fn toasts_never_leave_the_overlay_area(
        cols in 1_u16..200, rows in 1_u16..80, shown in 0_usize..=MAX_SHOWN,
    ) {
        let strings = strings();
        let now = Instant::now();
        let mut toasts = Toasts::default();
        for n in 0..shown {
            toasts.push(Kind::Door, Text::Member(&format!("event {n}")), &strings, now);
        }
        let screen = Rect::new(0, 0, cols, rows);
        let area = overlay_area(screen);
        for rect in toasts.place(area) {
            prop_assert!(rect.width > 0 && rect.height > 0);
            prop_assert!(area.contains(ratatui::layout::Position::new(rect.x, rect.y)));
            prop_assert!(rect.x + rect.width <= area.x + area.width);
            prop_assert!(rect.y + rect.height <= area.y + area.height);
            // The status row is the last row of the screen: the marker cell is at its left end.
            prop_assert!(rect.y > screen.y && rect.y + rect.height < screen.y + screen.height);
            prop_assert!(rect.y != screen.y + screen.height - 1);
        }
    }
}

#[test]
fn the_overlay_area_leaves_the_first_and_last_rows_alone() {
    let screen = Rect::new(0, 0, 80, 24);
    let area = overlay_area(screen);
    assert_eq!((area.y, area.height), (1, 22));
    assert_eq!(
        overlay_area(Rect::new(0, 0, 80, 3)).height,
        0,
        "too short for anything"
    );
}
