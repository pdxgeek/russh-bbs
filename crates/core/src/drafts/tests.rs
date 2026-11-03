//! Context keys, body bounds and quoting. The save/load rules need a store, so they are
//! checked in the shared store suite (`russh-bbs-store`, `suite::drafts`) on every backend.
#![allow(clippy::unwrap_used)]

use crate::sanitize::{MultiLine, RESERVED_CHROME, SingleLine};

use super::*;

fn clean(text: &str) -> crate::sanitize::Sanitized {
    MultiLine::new(10_000).sanitize(text)
}

#[test]
fn context_keys_are_short_ascii_and_nothing_else() {
    for good in ["forum:reply:1234", "mail:new", "poll/3.option-2", "A_b"] {
        assert_eq!(ContextKey::parse(good).unwrap().as_str(), good);
    }
    assert_eq!(ContextKey::parse(""), Err(ContextKeyError::Empty));
    assert_eq!(
        ContextKey::parse(&"a".repeat(MAX_CONTEXT_CHARS + 1)),
        Err(ContextKeyError::TooLong)
    );
    assert!(ContextKey::parse(&"a".repeat(MAX_CONTEXT_CHARS)).is_ok());
    for bad in [
        "has space",
        "new\nline",
        "esc\u{1b}[2J",
        "tab\t",
        "caf\u{e9}",
        "a;b",
        "quote'",
        "\u{27E6}x",
        "nul\0",
    ] {
        assert_eq!(
            ContextKey::parse(bad),
            Err(ContextKeyError::BadCharacter),
            "{bad:?}"
        );
    }
}

#[test]
fn a_body_is_sanitized_by_the_shared_sanitizer() {
    let hostile = "hi\u{1b}[31m red\u{7}\r\nline\u{27E6}\u{2299}\u{27E7} \u{e000}end";
    let body = Body::prepare(BodyLimits::MAX, hostile).unwrap();
    let text = body.as_str();
    assert!(!text.contains('\u{1b}') && !text.contains('\u{7}') && !text.contains('\r'));
    assert!(!text.chars().any(|c| RESERVED_CHROME.contains(&c)));
    assert!(!text.contains('\u{e000}'));
    assert_eq!(
        text,
        MultiLine::new(MAX_BODY_CHARS + 1)
            .sanitize(hostile)
            .as_str()
    );
    assert!(text.contains('\n'));
}

#[test]
fn a_body_at_the_limit_is_kept_and_one_over_is_refused_not_cut() {
    let limits = BodyLimits::new(10, 3);
    assert!(Body::prepare(limits, "0123456789").is_ok());
    assert_eq!(
        Body::prepare(limits, "01234567890"),
        Err(BodyError::TooLong { max: 10 })
    );
    assert!(Body::prepare(limits, "a\nb\nc").is_ok());
    assert_eq!(
        Body::prepare(limits, "a\nb\nc\nd"),
        Err(BodyError::TooManyLines { max: 3 })
    );
    // Characters are counted after sanitizing: stripped escapes do not count against the cap.
    assert!(Body::prepare(limits, "\u{1b}[1m012345\u{1b}[0m6789").is_ok());
}

#[test]
fn limits_can_be_narrowed_never_widened() {
    let wide = BodyLimits::new(usize::MAX, usize::MAX);
    assert_eq!(wide, BodyLimits::MAX);
    assert_eq!(BodyLimits::new(0, 0).max_chars(), 1);
    assert_eq!(BodyLimits::new(0, 0).max_lines(), 1);
    let huge = "x".repeat(MAX_BODY_CHARS + 1);
    assert!(Body::prepare(wide, &huge).is_err());
    let lines = "x\n".repeat(MAX_BODY_LINES);
    assert!(Body::prepare(wide, &lines).is_err());
}

#[test]
fn blank_and_trimmed_bodies() {
    let blank = Body::prepare(BodyLimits::MAX, " \n\t \n").unwrap();
    assert!(blank.is_blank());
    let body = Body::prepare(BodyLimits::MAX, "hello  \n\n").unwrap();
    assert!(!body.is_blank());
    assert_eq!(body.trimmed(), "hello");
    assert_eq!(body.lines(), 3);
    assert_eq!(body.chars(), 9);
}

#[test]
fn quoting_prefixes_every_line_and_leaves_room_to_reply() {
    let who = SingleLine::new(80).sanitize("alice wrote:");
    let quoted = quote(Some(&who), &clean("one\n\ntwo\n"), QuoteLimits::default());
    assert_eq!(quoted.as_str(), "alice wrote:\n> one\n>\n> two\n\n");
    let plain = quote(None, &clean("solo"), QuoteLimits::default());
    assert_eq!(plain.as_str(), "> solo\n\n");
    assert_eq!(
        quote(None, &clean(" \n\n"), QuoteLimits::default()).as_str(),
        ""
    );
}

#[test]
fn quoting_a_quote_nests_once_and_stops() {
    let text = clean("> said\n> > earlier\n> > > way back\nnew");
    let quoted = quote(None, &text, QuoteLimits::default());
    assert_eq!(quoted.as_str(), "> > said\n> new\n\n");
    assert_eq!(quote_depth("> > > x"), 3);
    assert_eq!(quote_depth(">> x"), 2);
    assert_eq!(quote_depth("x > y"), 0);
    assert!(is_quote_line("  > indented"));
    assert!(!is_quote_line("plain"));
}

#[test]
fn a_long_quote_is_cut_at_a_line_with_a_marker_line() {
    let lines: Vec<String> = (1..=100).map(|n| format!("line {n}")).collect();
    let text = clean(&lines.join("\n"));
    let limits = QuoteLimits {
        max_lines: 5,
        max_chars: 4_000,
    };
    let quoted = quote(None, &text, limits);
    assert_eq!(
        quoted.as_str(),
        "> line 1\n> line 2\n> line 3\n> line 4\n> line 5\n> [...]\n\n"
    );
    let tight = QuoteLimits {
        max_lines: 100,
        max_chars: 30,
    };
    let quoted = quote(None, &text, tight);
    assert!(quoted.as_str().chars().count() <= 30 + "> [...]\n\n".len());
    assert!(quoted.as_str().contains("> [...]"));
    assert!(
        quoted
            .as_str()
            .lines()
            .all(|l| l.is_empty() || l.starts_with('>'))
    );
}

#[test]
fn quoting_cannot_smuggle_chrome_or_control_text() {
    // The inputs are already sanitized; the output must be too, whatever they hold.
    let text = clean("\u{1b}[7mevil\u{27E6}\u{2299}\u{27E7}\u{e001}\r\nnext");
    let who = SingleLine::new(80).sanitize("bob\u{1b}[1m wrote:\u{27E6}");
    let quoted = quote(Some(&who), &text, QuoteLimits::default());
    let out = quoted.as_str();
    assert!(!out.contains('\u{1b}') && !out.contains('\r'));
    assert!(!out.chars().any(|c| RESERVED_CHROME.contains(&c)));
    assert!(!out.contains('\u{e001}'));
    assert_eq!(out, MultiLine::new(10_000).sanitize(out).as_str());
    // A multi-line attribution contributes only its first line.
    let sneaky = clean("first\nsecond");
    let quoted = quote(Some(&sneaky), &clean("x"), QuoteLimits::default());
    assert_eq!(quoted.as_str(), "first\n> x\n\n");
}
