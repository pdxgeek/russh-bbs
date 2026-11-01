//! Unit and property tests for the sanitizer.

use proptest::prelude::*;

use super::scan::is_private_use;
use super::{MultiLine, SingleLine, UsernameError, sanitize_username, username_skeleton};

const BIG: usize = 10_000;

fn line(s: &str) -> String {
    SingleLine::new(BIG).sanitize(s).into_string()
}

fn body(s: &str) -> String {
    MultiLine::new(BIG).sanitize(s).into_string()
}

#[test]
fn plain_text_is_unchanged() {
    assert_eq!(line("hello, wörld 👋"), "hello, wörld 👋");
    assert_eq!(body("a\nb\tc"), "a\nb\tc");
}

#[test]
fn c0_controls_are_removed() {
    assert_eq!(line("a\x00b\x01c\x07d\x08e\x0bf\x0cg\x7fh"), "abcdefgh");
    assert_eq!(body("a\x00b\x1fc"), "abc");
}

#[test]
fn c1_controls_are_removed() {
    assert_eq!(line("a\u{80}b\u{85}c\u{8d}d\u{9c}e"), "abcde");
}

#[test]
fn csi_sequences_leave_no_residue() {
    assert_eq!(line("\x1b[31mred\x1b[0m"), "red");
    assert_eq!(line("a\x1b[2Jb"), "ab");
    assert_eq!(line("a\x1b[10;20Hb"), "ab");
    assert_eq!(line("a\x1b[?25lb"), "ab");
    assert_eq!(line("a\x1b[1;38;5;196mb"), "ab");
    assert_eq!(line("a\x1b[3 qb"), "ab");
}

#[test]
fn csi_aborted_by_out_of_grammar_char() {
    assert_eq!(line("\x1b[3\x1b[31m1m"), "1m");
    assert_eq!(line("\x1b[31\u{e9}x"), "éx");
}

#[test]
fn osc_title_bel_and_st_terminated() {
    assert_eq!(line("a\x1b]0;pwned\x07b"), "ab");
    assert_eq!(line("a\x1b]0;pwned\x1b\\b"), "ab");
    assert_eq!(line("a\x1b]2;x\u{9c}b"), "ab");
}

#[test]
fn osc8_hyperlinks_are_removed() {
    assert_eq!(
        line("\x1b]8;;http://evil.example\x07click\x1b]8;;\x07"),
        "click"
    );
    assert_eq!(
        line("\x1b]8;id=1;http://evil.example\x1b\\click\x1b]8;;\x1b\\"),
        "click"
    );
}

#[test]
fn dcs_sos_pm_apc_are_removed() {
    assert_eq!(line("a\x1bPq#0;2;0;0;0\x1b\\b"), "ab");
    assert_eq!(line("a\x1bXjunk\x1b\\b"), "ab");
    assert_eq!(line("a\x1b^junk\x1b\\b"), "ab");
    assert_eq!(line("a\x1b_junk\x1b\\b"), "ab");
    // BEL does not end a DCS: it swallows to the end.
    assert_eq!(line("a\x1bPjunk\x07still hidden"), "a");
}

#[test]
fn unterminated_string_swallows_rest() {
    assert_eq!(line("a\x1b]0;never ends"), "a");
}

#[test]
fn lone_and_truncated_escapes() {
    assert_eq!(line("abc\x1b"), "abc");
    assert_eq!(line("abc\x1b["), "abc");
    assert_eq!(line("abc\x1b[31"), "abc");
    assert_eq!(line("abc\x1b]"), "abc");
    assert_eq!(line("\x1b\x1b\x1b"), "");
    assert_eq!(line("\x1b\x1bx"), "");
    assert_eq!(line("a\x1b\nb"), "a b");
}

#[test]
fn two_byte_and_nf_escapes() {
    assert_eq!(line("a\x1bcb"), "ab");
    assert_eq!(line("a\x1b(Bb"), "ab");
    assert_eq!(line("a\x1b7b\x1b8"), "ab");
}

#[test]
fn eight_bit_c1_sequences() {
    assert_eq!(line("a\u{9b}31mb"), "ab");
    assert_eq!(line("a\u{9b}2Jb"), "ab");
    assert_eq!(line("a\u{9d}0;title\u{9c}b"), "ab");
    assert_eq!(line("a\u{90}q\u{9c}b"), "ab");
}

#[test]
fn bidi_controls_are_removed() {
    for c in ('\u{202A}'..='\u{202E}').chain('\u{2066}'..='\u{2069}') {
        assert_eq!(line(&format!("a{c}b")), "ab", "U+{:04X}", c as u32);
    }
    assert_eq!(line("a\u{200E}b\u{200F}c\u{061C}d"), "abcd");
}

#[test]
fn invisible_characters_are_removed_but_joiners_kept() {
    assert_eq!(line("a\u{200B}b\u{FEFF}c\u{2060}d"), "abcd");
    assert_eq!(line("a\u{E0041}b"), "ab");
    assert_eq!(line("👨\u{200D}👩"), "👨\u{200D}👩");
}

#[test]
fn private_use_is_neutralized() {
    // Nerd Font lock (BMP), a plane-15 glyph, a plane-16 glyph, both range ends.
    assert_eq!(line("\u{F023}"), "\u{FFFD}");
    assert_eq!(line("\u{E000}\u{F8FF}"), "\u{FFFD}\u{FFFD}");
    assert_eq!(line("\u{F0001}\u{FFFFD}"), "\u{FFFD}\u{FFFD}");
    assert_eq!(line("\u{100000}\u{10FFFD}"), "\u{FFFD}\u{FFFD}");
    assert!(!line("signed \u{F023} by sysop").chars().any(is_private_use));
}

#[test]
fn noncharacters_are_replaced() {
    assert_eq!(
        line("\u{FFFE}\u{FFFF}\u{FDD0}\u{1FFFF}"),
        "\u{FFFD}".repeat(4)
    );
}

#[test]
fn newline_and_tab_per_context() {
    assert_eq!(line("a\nb\r\nc\rd\te"), "a b c d e");
    assert_eq!(body("a\nb\r\nc\rd\te"), "a\nb\nc\nd\te");
    assert_eq!(body("a\u{2028}b\u{2029}c"), "abc");
}

#[test]
fn limit_counts_chars_and_never_splits_utf8() {
    let s = SingleLine::new(3).sanitize("héllo wörld");
    assert_eq!(s.as_str(), "hél");
    let s = SingleLine::new(2).sanitize("🔒🔒🔒");
    assert_eq!(s.as_str(), "🔒🔒");
    assert_eq!(SingleLine::new(0).sanitize("abc").as_str(), "");
    // Removed characters do not count against the limit.
    assert_eq!(
        SingleLine::new(3).sanitize("\x1b[31mabcdef").as_str(),
        "abc"
    );
    assert_eq!(MultiLine::new(4).sanitize("ab\ncd").as_str(), "ab\nc");
}

#[test]
fn huge_limit_does_not_overflow() {
    assert_eq!(SingleLine::new(usize::MAX).sanitize("ab").as_str(), "ab");
}

#[test]
fn username_accepts_plain_names() {
    assert_eq!(
        sanitize_username("neo_42").map(|u| u.to_string()),
        Ok("neo_42".into())
    );
    assert!(sanitize_username("a-b").is_ok());
}

#[test]
fn username_nfkc_collapses_compatibility_forms() {
    assert_eq!(
        sanitize_username("ＡＢＣ１").map(|u| u.to_string()),
        Ok("ABC1".into())
    );
    // Kelvin sign normalizes to ASCII K.
    assert_eq!(
        sanitize_username("\u{212A}ay").map(|u| u.to_string()),
        Ok("Kay".into())
    );
}

#[test]
fn username_rejects_everything_else() {
    use UsernameError::{BadFirstCharacter, DisallowedCharacter, TooLong, TooShort};
    let err = |s: &str| sanitize_username(s).map(|u| u.to_string());
    assert_eq!(err("a"), Err(TooShort));
    assert_eq!(err(""), Err(TooShort));
    assert_eq!(err(&"a".repeat(25)), Err(TooLong));
    assert!(err(&"a".repeat(24)).is_ok());
    assert_eq!(err("_ab"), Err(BadFirstCharacter));
    assert_eq!(err("a b"), Err(DisallowedCharacter));
    assert_eq!(err("a\x1b[31mb"), Err(DisallowedCharacter));
    assert_eq!(err("a\u{202E}b"), Err(DisallowedCharacter));
    assert_eq!(err("a\u{F023}b"), Err(DisallowedCharacter));
    assert_eq!(err("a\nb"), Err(DisallowedCharacter));
    // Cyrillic 'а' looks like Latin 'a' but is not in the allowed set.
    assert_eq!(err("p\u{0430}ul"), Err(DisallowedCharacter));
    assert_eq!(err("josé"), Err(DisallowedCharacter));
    assert_eq!(err(&"a".repeat(10_000)), Err(TooLong));
}

#[test]
fn skeleton_blocks_ascii_lookalikes() {
    assert_eq!(username_skeleton("Admin"), username_skeleton("AdmIn"));
    assert_eq!(username_skeleton("Admin"), username_skeleton("admln"));
    assert_eq!(username_skeleton("r00t"), username_skeleton("root"));
    assert_eq!(username_skeleton("a-b"), username_skeleton("a_b"));
    assert_ne!(username_skeleton("alice"), username_skeleton("alicf"));
    let u = sanitize_username("R00t");
    assert_eq!(u.map(|u| u.skeleton()), Ok("root".into()));
}

/// Inputs weighted toward the characters that matter.
fn nasty_string() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        4 => any::<char>(),
        3 => (0u32..0xA0).prop_filter_map("scalar", char::from_u32),
        2 => prop_oneof![Just('\u{1b}'), Just('['), Just(']'), Just('\\'), Just('\u{07}'),
                         Just('\u{9b}'), Just('\u{9d}'), Just('\u{9c}'), Just('P'), Just(';')],
        2 => prop_oneof![
            (0xE000u32..=0xF8FF), (0xF0000u32..=0xFFFFD), (0x10_0000_u32..=0x10_FFFD),
            (0x202Au32..=0x202E), (0x2066u32..=0x2069)
        ].prop_filter_map("scalar", char::from_u32),
    ];
    prop::collection::vec(piece, 0..64).prop_map(|v| v.into_iter().collect())
}

fn assert_clean(out: &str, allow_nl_tab: bool) {
    for c in out.chars() {
        let ok_ctrl = allow_nl_tab && (c == '\n' || c == '\t');
        assert!(ok_ctrl || !c.is_control(), "control U+{:04X}", c as u32);
        assert!(!is_private_use(c), "private use U+{:04X}", c as u32);
        assert!(
            !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}'),
            "bidi U+{:04X}",
            c as u32
        );
    }
    assert!(!out.contains('\u{1b}'));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn single_line_output_is_clean(s in nasty_string(), max in 0usize..80) {
        let out = SingleLine::new(max).sanitize(&s);
        assert_clean(out.as_str(), false);
        prop_assert!(out.as_str().chars().count() <= max);
        prop_assert!(!out.as_str().contains(['\n', '\t', '\r']));
    }

    #[test]
    fn multi_line_output_is_clean(s in nasty_string(), max in 0usize..80) {
        let out = MultiLine::new(max).sanitize(&s);
        assert_clean(out.as_str(), true);
        prop_assert!(out.as_str().chars().count() <= max);
        prop_assert!(!out.as_str().contains('\r'));
    }

    #[test]
    fn sanitizing_is_idempotent(s in nasty_string(), max in 0usize..80) {
        let once = SingleLine::new(max).sanitize(&s);
        prop_assert_eq!(&SingleLine::new(max).sanitize(once.as_str()), &once);
        let once = MultiLine::new(max).sanitize(&s);
        prop_assert_eq!(&MultiLine::new(max).sanitize(once.as_str()), &once);
    }

    #[test]
    fn arbitrary_unicode_never_panics(s in any::<String>()) {
        let _ = SingleLine::new(50).sanitize(&s);
        let _ = MultiLine::new(50).sanitize(&s);
        let _ = sanitize_username(&s);
    }

    #[test]
    fn username_ok_is_idempotent_and_ascii(s in nasty_string()) {
        if let Ok(u) = sanitize_username(&s) {
            prop_assert!(u.as_str().chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
            prop_assert!((2..=24).contains(&u.as_str().len()));
            prop_assert_eq!(sanitize_username(u.as_str()), Ok(u));
        }
    }
}
