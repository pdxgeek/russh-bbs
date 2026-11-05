#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::items_after_statements
)]

use std::time::{Duration, Instant};

use super::{REFRESH, STEP, Ticker, current, scope};
use crate::config::BoardConfig;
use crate::ui::board::Board;

fn board(ticker: &str, feeds: bool) -> Board {
    Board::from_config(&BoardConfig {
        ticker: ticker.to_owned(),
        ticker_feeds: feeds,
        ..BoardConfig::default()
    })
    .unwrap()
}

#[test]
fn a_board_without_ticker_lines_has_no_ticker_text() {
    let now = Instant::now();
    let ticker = Ticker::new(&board("", false), true, now);
    assert!(!ticker.has_text());
    assert!(ticker.view(now, false).is_none());
    assert!(ticker.next_wake(now).is_none());
}

#[test]
fn with_motion_off_the_ticker_is_static_and_asks_for_no_redraws() {
    let now = Instant::now();
    let ticker = Ticker::new(&board("Welcome aboard | Second line", false), false, now);
    let view = ticker.view(now, false).unwrap();
    let later = ticker.view(now + Duration::from_secs(600), false).unwrap();
    assert_eq!(view, later, "static: ten minutes later it is the same");
    assert_eq!(view.window(40).as_str(), "Welcome aboard");
    assert!(
        ticker.next_wake(now).is_none(),
        "motion off: no timer-driven redraws"
    );
    // Too narrow for the line: cut with an ellipsis.
    assert_eq!(view.window(8).as_str(), "Welcome\u{2026}");
    let ascii = ticker.view(now, true).unwrap();
    assert_eq!(ascii.window(8).as_str(), "Welcome~");
}

#[test]
fn with_motion_on_it_moves_one_cell_a_second_and_wakes_once_per_step() {
    let now = Instant::now();
    let ticker = Ticker::new(
        &board("Alpha beta gamma delta epsilon zeta eta theta", false),
        true,
        now,
    );
    let at = |secs: u64| {
        ticker
            .view(now + Duration::from_secs(secs), false)
            .unwrap()
            .window(12)
    };
    let first = at(0);
    let second = at(1);
    assert_ne!(first, second);
    assert_eq!(
        first.as_str().chars().skip(1).collect::<String>(),
        second.as_str().chars().take(11).collect::<String>(),
        "it scrolls by exactly one cell"
    );
    // Same second, same frame: no extra redraws inside a step.
    assert_eq!(
        at(1),
        ticker
            .view(now + Duration::from_millis(1900), false)
            .unwrap()
            .window(12)
    );
    let wake = ticker.next_wake(now).unwrap();
    assert_eq!(wake, now + STEP);
    let wake = ticker.next_wake(now + Duration::from_millis(2500)).unwrap();
    assert_eq!(wake, now + 3 * STEP, "the next step, not a timer per frame");
}

#[test]
fn a_line_that_fits_does_not_scroll() {
    let now = Instant::now();
    let ticker = Ticker::new(&board("Hi", false), true, now);
    let a = ticker.view(now, false).unwrap().window(30);
    let b = ticker
        .view(now + Duration::from_secs(5), false)
        .unwrap()
        .window(30);
    assert_eq!(a, b);
    assert_eq!(a.as_str(), "Hi");
}

#[test]
fn headlines_are_sanitized_capped_and_refreshed_on_a_timer() {
    let now = Instant::now();
    let mut ticker = Ticker::new(&board("", true), false, now);
    assert!(ticker.wants_headlines() && ticker.headlines_due(now));
    ticker.set_headlines(
        [
            "Breaking\u{1b}[31m news\nline two",
            "\u{27e6}UNENCRYPTED\u{27e7} alert",
            "   ",
            "ok",
        ],
        now,
    );
    assert!(!ticker.headlines_due(now + Duration::from_secs(10)));
    assert!(ticker.headlines_due(now + REFRESH));
    let text = ticker.view(now, false).unwrap().window(100);
    let shown = text.as_str();
    assert!(
        !shown
            .chars()
            .any(|c| c.is_control() || c == '\u{27e6}' || c == '\u{27e7}'),
        "{shown:?}"
    );
    let many: Vec<String> = (0..100).map(|n| format!("headline {n}")).collect();
    ticker.set_headlines(many.iter().map(String::as_str), now);
    assert!(ticker.headlines.len() <= super::MAX_HEADLINES);
    // A board that did not turn feeds on never asks.
    let off = Ticker::new(&board("x", false), true, now);
    assert!(!off.headlines_due(now));
}

#[test]
fn the_sysop_lines_are_checked_like_the_tagline() {
    for bad in [
        "\u{1b}[31mred",
        "\u{27e6}UNENCRYPTED\u{27e7}",
        "a very secure vault",
    ] {
        let config = BoardConfig {
            ticker: bad.to_owned(),
            ..BoardConfig::default()
        };
        assert!(Board::from_config(&config).is_err(), "{bad:?}");
    }
    let too_many = (0..9)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        Board::from_config(&BoardConfig {
            ticker: too_many,
            ..BoardConfig::default()
        })
        .is_err()
    );
    let long = "x".repeat(81);
    assert!(
        Board::from_config(&BoardConfig {
            ticker: long,
            ..BoardConfig::default()
        })
        .is_err()
    );
}

#[test]
fn the_header_reads_the_ticker_only_inside_a_scope() {
    assert!(current().is_none());
    let now = Instant::now();
    let ticker = Ticker::new(&board("Hello", false), false, now);
    scope(ticker.view(now, false), || {
        assert!(current().is_some());
    });
    assert!(current().is_none(), "cleared after the draw");
}
