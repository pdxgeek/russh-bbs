//! H-3 property tests: arbitrary Unicode and arbitrary bytes through every sanitizer mode,
//! well-formed escape sequences, the username rules, and the security-marker cues.
//!
//! Complements `tests.rs` (its generator is private there). Everything runs in `cargo test`
//! in a few seconds; the long run is `#[ignore]`d (`cargo test -p russh-bbs-core --
//! --ignored sanitize::properties`).

use proptest::prelude::*;

use super::{MultiLine, SingleLine, UsernameError, sanitize_username};
use crate::icons::{ChromeStyle, Icon, SecurityMarker, Tier};

fn cases() -> ProptestConfig {
    ProptestConfig::with_cases(1500)
}

/// The nerd-font and Unicode tier texts of every marker (the ASCII tier is the label).
fn marker_texts() -> Vec<String> {
    let mut v = Vec::new();
    for m in SecurityMarker::ALL {
        for tier in [Tier::NerdFont, Tier::Unicode] {
            v.push(Icon::from(m).render(tier).plain().trim_end().to_owned());
        }
    }
    v
}

/// Check one output against every "never present" rule. `multi` allows `\n` and `\t`.
fn check(out: &str, multi: bool) -> Result<(), TestCaseError> {
    for c in out.chars() {
        let ok = multi && matches!(c, '\n' | '\t');
        prop_assert!(ok || !c.is_control(), "control U+{:04X}", c as u32);
        prop_assert!(
            !matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{10FFFF}'),
            "private use U+{:04X}",
            c as u32
        );
        prop_assert!(
            !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}'),
            "bidi U+{:04X}",
            c as u32
        );
        prop_assert!(
            !matches!(
                c,
                '\u{200B}'
                    | '\u{2060}'
                    | '\u{FEFF}'
                    | '\u{00AD}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{E0000}'..='\u{E007F}'
            ),
            "invisible U+{:04X}",
            c as u32
        );
        prop_assert!(
            !super::RESERVED_CHROME.contains(&c),
            "reserved U+{:04X}",
            c as u32
        );
    }
    // The reserved marker style and every non-ASCII marker rendering are unreachable.
    prop_assert!(!out.contains(ChromeStyle::SGR_ON));
    prop_assert!(!out.contains(ChromeStyle::SGR_OFF));
    for t in marker_texts() {
        if !t.is_ascii() {
            prop_assert!(!out.contains(&t), "marker text {t:?} in output");
        }
    }
    Ok(())
}

fn all_modes(s: &str, max: usize) -> Result<(), TestCaseError> {
    let one = SingleLine::new(max).sanitize(s);
    check(one.as_str(), false)?;
    prop_assert!(one.as_str().chars().count() <= max);
    prop_assert!(!one.as_str().contains(['\n', '\r', '\t']));
    prop_assert_eq!(&SingleLine::new(max).sanitize(one.as_str()), &one);

    let many = MultiLine::new(max).sanitize(s);
    check(many.as_str(), true)?;
    prop_assert!(many.as_str().chars().count() <= max);
    prop_assert!(!many.as_str().contains('\r'));
    prop_assert_eq!(&MultiLine::new(max).sanitize(many.as_str()), &many);
    Ok(())
}

/// Any scalar value, weighted to the interesting low and special ranges.
fn any_text() -> impl Strategy<Value = String> {
    let ch = prop_oneof![
        4 => any::<char>(),
        3 => (0u32..0x100).prop_filter_map("scalar", char::from_u32),
        2 => (0x2000u32..0x2070).prop_filter_map("scalar", char::from_u32),
        2 => (0xE000u32..0xF900).prop_filter_map("scalar", char::from_u32),
        1 => prop::sample::select(super::RESERVED_CHROME.to_vec()),
    ];
    prop::collection::vec(ch, 0..96).prop_map(|v| v.into_iter().collect())
}

proptest! {
    #![proptest_config(cases())]

    #[test]
    fn unicode_input_every_mode(s in any_text(), max in 0usize..120) {
        all_modes(&s, max)?;
    }

    /// Arbitrary bytes, as a front end would see them: decode lossily, then sanitize.
    #[test]
    fn byte_input_every_mode(b in prop::collection::vec(any::<u8>(), 0..200), max in 0usize..120) {
        let s = String::from_utf8_lossy(&b).into_owned();
        all_modes(&s, max)?;
    }

    /// Bytes weighted to escape introducers, C1 bytes and UTF-8 lead/continuation bytes.
    #[test]
    fn escape_heavy_bytes(b in prop::collection::vec(
        prop_oneof![
            Just(0x1bu8), Just(b'['), Just(b']'), Just(b'P'), Just(b'\\'), Just(0x07), Just(0x9b),
            Just(0x9d), Just(0x90), Just(0x9c), Just(b';'), Just(b'm'), Just(0xc2), Just(0xe2),
            Just(0x80), Just(0xf3), any::<u8>()
        ], 0..200), max in 0usize..120) {
        all_modes(&String::from_utf8_lossy(&b), max)?;
    }

    /// Well-formed CSI, OSC and DCS sequences (7-bit and 8-bit introducers) between plain
    /// letters are removed whole: nothing of their parameters survives.
    #[test]
    fn well_formed_sequences_leave_only_the_text(
        parts in prop::collection::vec(("[a-z]{0,4}", seq()), 0..8),
        tail in "[a-z]{0,4}",
    ) {
        let mut input = String::new();
        let mut want = String::new();
        for (text, s) in &parts {
            input.push_str(text);
            input.push_str(s);
            want.push_str(text);
        }
        input.push_str(&tail);
        want.push_str(&tail);
        let one = SingleLine::new(10_000).sanitize(&input);
        let many = MultiLine::new(10_000).sanitize(&input);
        prop_assert_eq!(one.as_str(), want.as_str());
        prop_assert_eq!(many.as_str(), want.as_str());
    }

    /// Length caps hold in chars, multi-byte input is never split, and a larger cap never
    /// yields a shorter result than a smaller one (the cap is a prefix cut).
    #[test]
    fn caps_hold_and_are_prefix_cuts(s in any_text(), a in 0usize..60, extra in 0usize..60) {
        let small = SingleLine::new(a).sanitize(&s);
        let large = SingleLine::new(a + extra).sanitize(&s);
        prop_assert!(small.as_str().chars().count() <= a);
        prop_assert!(large.as_str().starts_with(small.as_str()));
        let small = MultiLine::new(a).sanitize(&s);
        let large = MultiLine::new(a + extra).sanitize(&s);
        prop_assert!(small.as_str().chars().count() <= a);
        prop_assert!(large.as_str().starts_with(small.as_str()));
    }

    /// No input produces the reserved style or a non-ASCII marker rendering, even when the
    /// input is built from the pieces of one.
    #[test]
    fn markers_cannot_be_assembled(
        picks in prop::collection::vec(0usize..16, 0..10),
        noise in any_text(),
    ) {
        let pieces: Vec<String> = marker_texts()
            .into_iter()
            .chain([ChromeStyle::SGR_ON.to_owned(), ChromeStyle::SGR_OFF.to_owned()])
            .chain(super::RESERVED_CHROME.iter().map(char::to_string))
            .collect();
        let mut s = noise;
        for p in picks {
            s.push_str(&pieces[p % pieces.len()]);
        }
        all_modes(&s, 400)?;
    }

    /// Usernames: whatever is accepted is ASCII, in range, stable, not a reserved word or
    /// marker label on the reserved key, and its skeleton is ASCII lowercase-ish.
    #[test]
    fn username_accepts_only_safe_unreserved_names(b in prop::collection::vec(any::<u8>(), 0..60)) {
        check_username(&String::from_utf8_lossy(&b))?;
    }

    /// Mostly-valid names, so the accept path (and the reserved check on it) is exercised.
    #[test]
    fn username_from_name_alphabet(s in "[A-Za-z0-9_-]{0,30}") {
        check_username(&s)?;
    }

    #[test]
    fn username_from_unicode(s in any_text()) {
        check_username(&s)?;
    }

    /// Mutating a reserved name by case, separators, leet and fullwidth forms never gets it
    /// accepted.
    #[test]
    fn reserved_names_resist_mutation(
        idx in 0usize..64,
        flips in prop::collection::vec((any::<bool>(), 0u8..4), 0..24),
        sep in prop::sample::select(vec!["", "_", "-"]),
    ) {
        let names = reserved_samples();
        let base = &names[idx % names.len()];
        let mut out = String::new();
        for (i, c) in base.chars().enumerate() {
            let (up, kind) = flips.get(i).copied().unwrap_or((false, 0));
            let c = match (kind, c.to_ascii_lowercase()) {
                (1, 'o') => '0',
                (1, 'i') => '1',
                (1, 's') => '5',
                (1, 'a') => '4',
                (1, 'e') => '3',
                (2, ch) if ch.is_ascii_lowercase() => fullwidth(ch),
                (_, ch) => ch,
            };
            out.push(if up { c.to_ascii_uppercase() } else { c });
            if i + 1 < base.chars().count() && kind == 3 {
                out.push_str(sep);
            }
        }
        match sanitize_username(&out) {
            Err(_) => {}
            Ok(u) => prop_assert!(false, "reserved {base:?} accepted as {out:?} -> {u}"),
        }
    }
}

fn fullwidth(c: char) -> char {
    char::from_u32(0xFF41 + (c as u32 - 'a' as u32)).unwrap_or(c)
}

fn reserved_samples() -> Vec<String> {
    let mut v: Vec<String> = [
        "sysop",
        "cosysop",
        "admin",
        "root",
        "system",
        "staff",
        "everyone",
        "moderator",
        "waiting",
        "new",
        "claim",
        "member",
        "trusted",
        "administrator",
        "sysadmin",
        "support",
        "owner",
        "operator",
        "webmaster",
        "mod",
        "guest",
    ]
    .map(str::to_owned)
    .to_vec();
    v.extend(SecurityMarker::ALL.map(|m| m.label().to_owned()));
    v
}

fn check_username(s: &str) -> Result<(), TestCaseError> {
    match sanitize_username(s) {
        Ok(u) => {
            let t = u.as_str();
            prop_assert!(
                t.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            );
            prop_assert!((2..=24).contains(&t.len()));
            prop_assert!(!t.starts_with(['_', '-']));
            prop_assert_eq!(sanitize_username(t), Ok(u.clone()));
            prop_assert!(u.skeleton().is_ascii());
            // Equal skeletons are the uniqueness key: a name's skeleton never equals a
            // reserved word's skeleton.
            for r in reserved_samples() {
                let rk = sanitize_username_skeleton(&r);
                prop_assert_ne!(u.skeleton(), rk, "{} collides with reserved {}", t, r);
            }
        }
        Err(e) => {
            // Rejection is always one of the typed reasons; nothing panics.
            let _ = matches!(
                e,
                UsernameError::TooShort
                    | UsernameError::TooLong
                    | UsernameError::DisallowedCharacter
                    | UsernameError::BadFirstCharacter
                    | UsernameError::Reserved
            );
        }
    }
    Ok(())
}

/// The skeleton of a reserved word, computed the way a member's would be.
fn sanitize_username_skeleton(word: &str) -> String {
    super::username::skeleton_of(word)
}

/// A well-formed escape sequence in 7-bit or 8-bit form.
fn seq() -> impl Strategy<Value = String> {
    let params = "[0-9;:?<=>]{0,8}";
    let inter = "[ -/]{0,2}";
    let fin = "[@-~]";
    let csi = (prop::bool::ANY, params, inter, fin).prop_map(|(eight, p, i, f)| {
        let intro = if eight { "\u{9b}" } else { "\u{1b}[" };
        format!("{intro}{p}{i}{f}")
    });
    let body = "[ -~&&[^\\x1b\\\\]]{0,12}";
    // OSC may end with BEL; DCS, SOS, PM and APC end only with ST (a BEL inside one is
    // payload and an unterminated one swallows the rest, by design).
    let osc = (
        prop::sample::select(vec!["\u{1b}]", "\u{9d}"]),
        body,
        prop::sample::select(vec!["\u{07}", "\u{1b}\\", "\u{9c}"]),
    )
        .prop_map(|(intro, b, term)| format!("{intro}{b}{term}"));
    let string = (
        prop::sample::select(vec!["\u{1b}P", "\u{90}", "\u{1b}X", "\u{1b}^", "\u{1b}_"]),
        body,
        prop::sample::select(vec!["\u{1b}\\", "\u{9c}"]),
    )
        .prop_map(|(intro, b, term)| format!("{intro}{b}{term}"));
    prop_oneof![3 => csi, 1 => osc, 1 => string]
}

#[test]
fn ascii_marker_labels_are_typeable_so_style_and_position_are_the_cue() {
    // D39/D45: the ASCII tier of a marker is its plain label, and content may contain that
    // text. This documents that the sanitizer does NOT (and cannot) block it: what tells a
    // real marker apart is the reserved style and the chrome-only cell, never the text.
    for m in SecurityMarker::ALL {
        let label = m.label();
        let out = SingleLine::new(100).sanitize(label);
        assert_eq!(out.as_str(), label, "label text passes through as content");
        // ...but the reserved style cannot come with it, in any form of the input.
        let styled = format!("{}{}{}", ChromeStyle::SGR_ON, label, ChromeStyle::SGR_OFF);
        let out = SingleLine::new(100).sanitize(&styled);
        assert_eq!(out.as_str(), label);
        assert!(!out.as_str().contains('\u{1b}'));
        // The rendered marker is styled; the same text as content is not.
        let r = Icon::from(m).render(Tier::Ascii);
        assert!(r.to_terminal().starts_with(ChromeStyle::SGR_ON));
    }
}

#[test]
fn unicode_and_nerd_tier_markers_cannot_be_typed() {
    for t in marker_texts() {
        for s in [&t, &format!("x{t}y")] {
            let out = SingleLine::new(100).sanitize(s);
            assert!(!out.as_str().contains(t.as_str()), "{t:?}");
            assert!(
                out.as_str().contains('\u{FFFD}'),
                "neutralized, not dropped: {t:?}"
            );
        }
    }
}

/// Long run for release checks: `cargo test -p russh-bbs-core --release -- --ignored
/// sanitize::properties`. Reported in the PR.
#[test]
#[ignore = "long fuzz run"]
fn long_run_every_mode() {
    use proptest::test_runner::{Config, TestRunner};
    let mut runner = TestRunner::new(Config::with_cases(300_000));
    let bytes = runner.run(
        &(prop::collection::vec(any::<u8>(), 0..400), 0usize..200),
        |(b, max)| all_modes(&String::from_utf8_lossy(&b), max),
    );
    assert!(bytes.is_ok(), "{bytes:?}");
    let text = runner.run(&(any_text(), 0usize..200), |(s, max)| all_modes(&s, max));
    assert!(text.is_ok(), "{text:?}");
}
