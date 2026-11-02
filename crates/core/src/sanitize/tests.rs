//! Unit and property tests for the sanitizer.

use proptest::prelude::*;

use super::scan::is_noncharacter;
use super::scan::is_private_use;
use super::{
    MAX_COMBINING_MARKS, MultiLine, RESERVED_CHROME, SingleLine, UsernameError, sanitize_username,
};

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
    assert_eq!(line("a\x00b\x01c\x07d\x08e\x0bf\x0cg\x7fh"), "abcde f gh");
    assert_eq!(body("a\x00b\x1fc"), "abc");
}

#[test]
fn c1_controls_are_removed() {
    assert_eq!(line("a\u{80}b\u{85}c\u{8d}d\u{9c}e"), "ab cde");
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
    assert_eq!(body("a\u{2028}b\u{2029}c"), "a\nb\nc");
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

fn skel(name: &str) -> String {
    super::username::skeleton_of(name)
}

#[test]
fn skeleton_blocks_ascii_lookalikes() {
    assert_eq!(skel("Admin"), skel("AdmIn"));
    assert_eq!(skel("Admin"), skel("admln"));
    assert_eq!(skel("Admin"), skel("Adm1n"));
    assert_eq!(skel("tom"), skel("t0m"));
    assert_eq!(skel("a-b"), skel("a_b"));
    assert_ne!(skel("alice"), skel("alicf"));
    assert_eq!(skel("R00t"), "root");
}

#[test]
fn skeleton_folds_digit_and_multi_letter_lookalikes() {
    // F8: pairs and the digit/letter shapes the QA run found distinct.
    for (a, b) in [
        ("vvally", "wally"),
        ("clan", "dan"),
        ("mark", "rnark"),
        ("mike", "nnike"),
        ("5am", "sam"),
        ("zed", "2ed"),
        ("bob", "8ob"),
        ("greg", "qreg"),
    ] {
        assert_eq!(skel(a), skel(b), "{a} vs {b}");
    }
    assert_ne!(skel("alice"), skel("bob"));
}

#[test]
fn reserved_usernames_are_rejected_on_the_skeleton() {
    let reserved = [
        "sysop",
        "cosysop",
        "admin",
        "root",
        "system",
        "staff",
        "everyone",
        "moderator",
        "waiting",
        "member",
        "trusted",
    ];
    for name in reserved {
        assert_eq!(
            sanitize_username(name).err(),
            Some(UsernameError::Reserved),
            "{name}"
        );
        let upper = name.to_uppercase();
        assert_eq!(
            sanitize_username(&upper).err(),
            Some(UsernameError::Reserved),
            "{upper}"
        );
    }
    // Lookalikes: digit and letter substitutions, and the full-width form.
    for name in [
        "Sy50p",
        "sys0p",
        "r00t",
        "Adm1n",
        "ADMLN",
        "5taff",
        "rnoderator",
        "Ｓysop",
        "cl-sysop",
    ] {
        let got = sanitize_username(name);
        if name == "cl-sysop" {
            assert!(
                got.is_ok(),
                "only exact skeleton matches are reserved: {got:?}"
            );
        } else {
            assert_eq!(got.err(), Some(UsernameError::Reserved), "{name}");
        }
    }
}

#[test]
fn security_marker_labels_are_reserved_usernames() {
    for name in [
        "E2EE",
        "e2ee",
        "SIGNED",
        "KEY-CHANGED",
        "key_changed",
        "KEY-CHANGED",
        "SERVER-READABLE",
        "Server-Readable",
    ] {
        assert_eq!(
            sanitize_username(name).err(),
            Some(UsernameError::Reserved),
            "{name}"
        );
    }
    // Neighbours that are not the label are fine.
    assert!(sanitize_username("signed2").is_ok());
    // N3: separators do not make a different name.
    assert_eq!(
        sanitize_username("keychanged").err(),
        Some(UsernameError::Reserved)
    );
}

#[test]
fn reserved_is_a_distinct_error_from_malformed() {
    assert_eq!(sanitize_username("a").err(), Some(UsernameError::TooShort));
    assert_eq!(
        sanitize_username("root!").err(),
        Some(UsernameError::DisallowedCharacter)
    );
    assert_eq!(
        sanitize_username("root").err(),
        Some(UsernameError::Reserved)
    );
}

// ---- F3: invisible filler and format characters ----

const F3_CLASS: &[char] = &[
    '\u{3164}',
    '\u{115F}',
    '\u{1160}',
    '\u{FFA0}',
    '\u{2800}',
    '\u{17B4}',
    '\u{17B5}',
    '\u{034F}',
    '\u{00AD}',
    '\u{180B}',
    '\u{180C}',
    '\u{180D}',
    '\u{180E}',
    '\u{180F}',
    '\u{206A}',
    '\u{206B}',
    '\u{206C}',
    '\u{206D}',
    '\u{206E}',
    '\u{206F}',
    '\u{FFF9}',
    '\u{FFFA}',
    '\u{FFFB}',
    '\u{FFFC}',
    '\u{0600}',
    '\u{0601}',
    '\u{0602}',
    '\u{0603}',
    '\u{0604}',
    '\u{0605}',
    '\u{06DD}',
    '\u{070F}',
    '\u{08E2}',
    '\u{110BD}',
    '\u{1D173}',
    '\u{1D17A}',
    '\u{1BCA0}',
    '\u{1BCA3}',
    '\u{2065}',
    '\u{FE00}',
    '\u{FE0E}',
    '\u{E0100}',
    '\u{E01EF}',
];

#[test]
fn invisible_filler_and_format_characters_are_removed() {
    for &c in F3_CLASS {
        assert_eq!(line(&format!("a{c}b")), "ab", "U+{:04X}", c as u32);
        assert_eq!(body(&format!("a{c}b")), "ab", "U+{:04X}", c as u32);
    }
    // A name made only of filler becomes empty, so "is empty" checks work.
    assert_eq!(line("\u{3164}\u{2800}\u{115F}"), "");
}

#[test]
fn unicode_spaces_become_ascii_space() {
    for c in [
        '\u{00A0}', '\u{1680}', '\u{2000}', '\u{2005}', '\u{200A}', '\u{202F}', '\u{205F}',
        '\u{3000}',
    ] {
        assert_eq!(line(&format!("a{c}b")), "a b", "U+{:04X}", c as u32);
        assert_eq!(body(&format!("a{c}b")), "a b", "U+{:04X}", c as u32);
    }
}

#[test]
fn fe0f_is_kept_only_after_an_emoji_base() {
    // Decision D40: emoji presentation is kept (a heart stays a red heart); a variation
    // selector anywhere else is invisible and removed.
    assert_eq!(line("\u{2764}\u{FE0F}"), "\u{2764}\u{FE0F}");
    assert_eq!(line("\u{1F600}\u{FE0F}"), "\u{1F600}\u{FE0F}");
    assert_eq!(line("a\u{FE0F}b"), "ab");
    assert_eq!(line("\u{FE0F}x"), "x");
    assert_eq!(line("\u{2764}\u{FE0F}\u{FE0F}"), "\u{2764}\u{FE0F}");
    assert_eq!(line("a\u{FE0E}b\u{E0100}"), "ab");
}

// ---- F4: combining marks ----

#[test]
fn combining_marks_are_capped_per_base() {
    let flood = format!("a{}b", "\u{0301}".repeat(500));
    let out = line(&flood);
    assert_eq!(out.chars().count(), 1 + MAX_COMBINING_MARKS + 1);
    assert_eq!(MAX_COMBINING_MARKS, 4);
    // The cap is per base, not per string.
    let two = format!("a{}b{}", "\u{0301}".repeat(4), "\u{0302}".repeat(4));
    assert_eq!(line(&two).chars().count(), 10);
    // Real text is untouched: Vietnamese with stacked marks, NFD form.
    assert_eq!(line("e\u{0302}\u{0301}"), "e\u{0302}\u{0301}");
    // Marks at the very start count against the same cap.
    assert_eq!(
        line(&"\u{0301}".repeat(9)).chars().count(),
        MAX_COMBINING_MARKS
    );
}

// ---- F9: separators ----

#[test]
fn vt_ff_nel_ls_ps_are_separators_not_deletions() {
    for c in ['\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}'] {
        assert_eq!(line(&format!("a{c}b")), "a b", "U+{:04X}", c as u32);
        assert_eq!(body(&format!("a{c}b")), "a\nb", "U+{:04X}", c as u32);
    }
}

#[test]
fn escape_residue_is_not_left_after_an_escape() {
    assert_eq!(line("a\u{1b}[31mb"), "ab");
    assert_eq!(line("a\u{1b}[31"), "a");
    assert!(!line("\u{1b}[31;1m\u{1b}[0m").contains("31"));
}

/// Inputs weighted toward the characters that matter: controls and escape fragments, every
/// class the sanitizer neutralizes, the F3 filler list, spaces, separators, combining marks,
/// and the reserved chrome set.
fn nasty_string() -> impl Strategy<Value = String> {
    let ranges = prop_oneof![
        (0xE000u32..=0xF8FF),
        (0xF0000u32..=0xFFFFD),
        (0x10_0000_u32..=0x10_FFFD),
        (0x202Au32..=0x202E),
        (0x2066u32..=0x2069),
        (0xFDD0u32..=0xFDEF),
        (0xE0000u32..=0xE007F),
        (0xE0100u32..=0xE01EF),
        (0x2000u32..=0x206F),
        (0x0300u32..=0x036F),
        (0xFE00u32..=0xFE0F),
        (0xFFF0u32..=0xFFFF),
    ]
    .prop_filter_map("scalar", char::from_u32);
    let fixed = prop_oneof![
        Just('\u{1b}'),
        Just('['),
        Just(']'),
        Just('\\'),
        Just('\u{07}'),
        Just('\u{9b}'),
        Just('\u{9d}'),
        Just('\u{9c}'),
        Just('P'),
        Just(';'),
        Just('3'),
        Just('1'),
        Just('m'),
        Just('\u{200B}'),
        Just('\u{2060}'),
        Just('\u{FEFF}'),
        Just('\u{FFFE}'),
        Just('\u{1FFFF}'),
        Just('\u{10FFFF}'),
        Just('\u{0B}'),
        Just('\u{0C}'),
        Just('\u{85}'),
        Just('\u{2028}'),
        Just('\u{2029}'),
        Just('\u{200D}'),
        Just('\u{200C}'),
        Just('\u{FE0F}'),
        Just('\u{2764}'),
        Just('\u{FFFD}'),
    ];
    let piece = prop_oneof![
        3 => any::<char>(),
        3 => (0u32..0xA0).prop_filter_map("scalar", char::from_u32),
        3 => fixed,
        2 => ranges,
        2 => prop::sample::select(F3_CLASS.to_vec()),
        1 => prop::sample::select(RESERVED_CHROME.to_vec()),
        1 => prop::sample::select(vec!['\u{00A0}', '\u{1680}', '\u{202F}', '\u{205F}', '\u{3000}']),
    ];
    // Some strings are built around an escape introducer so a stray `31m` can follow it.
    let escape_tail = prop::collection::vec(
        prop_oneof![
            Just("\u{1b}["),
            Just("31m"),
            Just("\u{1b}]0;x"),
            Just("1;7m")
        ],
        0..6,
    )
    .prop_map(|v| v.concat());
    (prop::collection::vec(piece, 0..64), escape_tail)
        .prop_map(|(v, tail)| v.into_iter().collect::<String>() + &tail)
}

fn assert_clean(out: &str, allow_nl_tab: bool) {
    let mut marks = 0usize;
    for c in out.chars() {
        let ok_ctrl = allow_nl_tab && (c == '\n' || c == '\t');
        assert!(ok_ctrl || !c.is_control(), "control U+{:04X}", c as u32);
        assert!(!is_private_use(c), "private use U+{:04X}", c as u32);
        assert!(!is_noncharacter(c), "noncharacter U+{:04X}", c as u32);
        assert!(
            !RESERVED_CHROME.contains(&c),
            "reserved chrome U+{:04X}",
            c as u32
        );
        assert!(
            !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}'),
            "bidi U+{:04X}",
            c as u32
        );
        assert!(
            !matches!(
                c,
                '\u{E0000}'
                    ..='\u{E007F}' | '\u{200B}' | '\u{2060}' | '\u{FEFF}' | '\u{2028}' | '\u{2029}'
            ),
            "tag or invisible U+{:04X}",
            c as u32
        );
        // Independent of the implementation: Unicode data (a different crate) and a
        // transcribed Default_Ignorable table, so a character the scanner misses fails here.
        assert!(
            !is_format_or_default_ignorable(c) || matches!(c, '\u{FE0F}' | '\u{200C}' | '\u{200D}'),
            "invisible format U+{:04X}",
            c as u32
        );
        assert!(
            !matches!(
                c,
                '\u{00A0}' | '\u{1680}' | '\u{2000}'
                    ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
            ),
            "unicode space U+{:04X}",
            c as u32
        );
        if matches!(c, '\u{FE0F}' | '\u{200C}' | '\u{200D}') {
            // Variation selector and joiners: transparent to the mark run.
        } else if unicode_normalization::char::is_combining_mark(c) {
            marks += 1;
            assert!(marks <= MAX_COMBINING_MARKS, "too many marks");
        } else {
            marks = 0;
        }
    }
    assert!(!out.contains('\u{1b}'));
    // Escape residue: a CSI/OSC remnant (`[31m`, `]0;`) cannot be told from real text
    // like "[31m", so assert only the strong form: no ESC and no C1 introducer survive.
    assert!(!out.chars().any(|c| ('\u{80}'..='\u{9f}').contains(&c)));
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

// ---- N2/N4: property-based stripping and the joiner-proof mark cap -----------------------

/// `Default_Ignorable_Code_Point`, transcribed from `DerivedCoreProperties.txt` (stable since
/// Unicode 6.x apart from newly assigned characters inside these ranges). Deliberately not
/// shared with the implementation, which reads the property from `icu_properties`.
const DEFAULT_IGNORABLE: &[(u32, u32)] = &[
    (0x00AD, 0x00AD),
    (0x034F, 0x034F),
    (0x061C, 0x061C),
    (0x115F, 0x1160),
    (0x17B4, 0x17B5),
    (0x180B, 0x180F),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x206F),
    (0x3164, 0x3164),
    (0xFE00, 0xFE0F),
    (0xFEFF, 0xFEFF),
    (0xFFA0, 0xFFA0),
    (0xFFF0, 0xFFF8),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0000, 0xE0FFF),
];

fn is_format_or_default_ignorable(c: char) -> bool {
    use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};
    let v = c as u32;
    c.general_category() == GeneralCategory::Format
        || DEFAULT_IGNORABLE
            .iter()
            .any(|&(lo, hi)| (lo..=hi).contains(&v))
        || matches!(c, '\u{2800}' | '\u{FFFC}')
}

#[test]
fn every_scalar_value_is_checked_against_unicode_properties() {
    // Whole code space, both contexts: every Cf and Default_Ignorable character is gone
    // except ZWJ/ZWNJ (context) and U+FE0F (emoji), and nothing else non-ASCII vanishes
    // for this reason. N4: this catches U+0890, U+110CD, U+13430 and the unassigned
    // default-ignorable ranges without a list in the implementation to agree with.
    let mut checked = 0u32;
    for v in 0..=0x0010_FFFF_u32 {
        let Some(c) = char::from_u32(v) else { continue };
        if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
            continue;
        }
        let input = format!("\u{e9}{c}\u{e9}");
        for out in [line(&input), body(&input)] {
            if is_format_or_default_ignorable(c)
                && !matches!(c, '\u{200C}' | '\u{200D}' | '\u{FE0F}')
            {
                assert_eq!(out, "\u{e9}\u{e9}", "U+{v:04X} survived");
            }
        }
        checked += 1;
    }
    assert!(checked > 1_000_000);
}

#[test]
fn unicode_data_canaries() {
    // Pin that the property data is recent enough for the characters the QA re-check
    // named (Cf U+0890, U+110CD, U+13430; unassigned default-ignorable U+FFF0, U+E0080,
    // U+E01F0). If a dependency bump loses these, this fails.
    for c in [
        '\u{0890}',
        '\u{0891}',
        '\u{110CD}',
        '\u{13430}',
        '\u{1343F}',
        '\u{FFF0}',
        '\u{FFF8}',
        '\u{E0080}',
        '\u{E01F0}',
        '\u{E0FFF}',
    ] {
        assert_eq!(line(&format!("a{c}b")), "ab", "U+{:04X}", c as u32);
    }
}

#[test]
fn combining_cap_survives_interleaved_joiners() {
    for joiner in ['\u{200D}', '\u{200C}'] {
        let mut input = String::from("\u{e9}");
        for _ in 0..50 {
            input.push_str(&"\u{0301}".repeat(4));
            input.push(joiner);
        }
        for out in [line(&input), body(&input)] {
            let marks = out.chars().filter(|c| *c == '\u{0301}').count();
            assert_eq!(marks, MAX_COMBINING_MARKS, "joiner U+{:04X}", joiner as u32);
            assert!(
                out.chars().count() <= 1 + MAX_COMBINING_MARKS + 50,
                "joiners are not unbounded"
            );
        }
    }
}

#[test]
fn joiners_are_kept_only_in_a_joining_context() {
    // Emoji sequence, Persian ZWNJ, Indic virama + ZWJ keep their joiner.
    assert_eq!(
        line("\u{1F468}\u{200D}\u{1F469}"),
        "\u{1F468}\u{200D}\u{1F469}"
    );
    assert_eq!(line("\u{0645}\u{200C}\u{06CC}"), "\u{0645}\u{200C}\u{06CC}");
    assert_eq!(
        line("\u{0915}\u{094D}\u{200D}\u{0937}"),
        "\u{0915}\u{094D}\u{200D}\u{0937}"
    );
    // After ASCII, at the start, after another joiner, after a space: dropped.
    assert_eq!(line("ad\u{200D}min"), "admin");
    assert_eq!(line("\u{200C}x"), "x");
    assert_eq!(
        line("\u{1F468}\u{200D}\u{200D}\u{200C}\u{1F469}"),
        "\u{1F468}\u{200D}\u{1F469}"
    );
    assert_eq!(line("a \u{200D}\u{3000}\u{200D}b"), "a  b");
}

// ---- N3: reserved names ------------------------------------------------------------------

#[test]
fn reserved_names_ignore_separators_and_fold_more_leet() {
    for name in [
        "sys_op",
        "sys-op",
        "ad-min",
        "ad_min",
        "co-sysop",
        "s-y-s-o-p",
        "syst3m",
        "4dmin",
        "3veryone",
        "st4ff",
        "mod3rator",
        "serverreadable",
        "KEYCHANGED",
        "e2e3",
        "key_changed",
        "SERVER-READABLE",
        "adm1n1strator",
        "sysadmin",
        "support",
        "0wner",
        "operator",
        "webm4ster",
        "mod",
        "gu3st",
        "9uest",
        "tru5ted",
    ] {
        assert_eq!(
            sanitize_username(name).err(),
            Some(UsernameError::Reserved),
            "{name}"
        );
    }
    // A suffix is not stripped (policy), and ordinary names stay free.
    for name in [
        "admin1",
        "sysop2",
        "modest",
        "alice",
        "ownership",
        "guesthouse",
    ] {
        assert!(sanitize_username(name).is_ok(), "{name}");
    }
}

#[test]
fn lookalike_brackets_and_glyphs_are_not_blocked_and_the_docs_say_so() {
    // N1/D44: only the exact reserved codepoints are neutralized. These pass through, so
    // the Unicode tier depends on style and position like the ASCII tier. If the reserved
    // set is ever widened, change this test and D44 together.
    for fake in [
        "\u{301A}\u{2609}\u{301B}",
        "\u{2E28}\u{25C9}\u{2E29}",
        "\u{FF3B}\u{2299}".replace('\u{2299}', "\u{25C9}").as_str(),
        "[|\u{25C9}|]",
    ] {
        assert_eq!(line(fake), fake);
    }
    assert_eq!(line("\u{27E6}\u{2299}\u{27E7}"), "\u{FFFD}\u{FFFD}\u{FFFD}");
}
