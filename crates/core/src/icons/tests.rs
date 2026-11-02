use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::width::TABLE;
use super::*;
use crate::sanitize::{MultiLine, RESERVED_CHROME, SingleLine};
use proptest::prelude::*;

fn is_private_use(c: char) -> bool {
    matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}' | '\u{100000}'..='\u{10FFFD}')
}

#[test]
fn every_icon_has_a_non_empty_glyph_in_every_tier() {
    for icon in Icon::all() {
        for tier in Tier::ALL {
            assert!(!icon.glyph(tier).is_empty(), "{icon:?} {tier:?}");
        }
        assert!(!icon.text_label().is_empty(), "{icon:?} label");
    }
}

#[test]
fn the_required_logical_icons_exist() {
    assert_eq!(ContentIcon::ALL.len(), 6);
    assert_eq!(SecurityMarker::ALL.len(), 4);
    assert_eq!(Icon::all().count(), 10);
}

#[test]
fn ascii_tier_and_labels_are_printable_ascii() {
    for icon in Icon::all() {
        for text in [icon.glyph(Tier::Ascii), icon.text_label()] {
            assert!(
                text.chars().all(|c| (' '..='~').contains(&c)),
                "{icon:?}: {text:?}"
            );
        }
    }
}

#[test]
fn unicode_tier_has_no_private_use_and_is_not_ascii() {
    for icon in Icon::all() {
        let g = icon.glyph(Tier::Unicode);
        assert!(!g.chars().any(is_private_use), "{icon:?}");
        assert!(g.chars().all(|c| !c.is_ascii()), "{icon:?}");
    }
}

#[test]
fn nerd_font_tier_is_all_private_use() {
    for icon in Icon::all() {
        let g = icon.glyph(Tier::NerdFont);
        assert_eq!(g.chars().count(), 1, "{icon:?}");
        assert!(g.chars().all(is_private_use), "{icon:?}");
    }
}

#[test]
fn the_sanitizer_never_passes_a_registry_nerd_font_glyph() {
    for icon in Icon::all() {
        let g = icon.glyph(Tier::NerdFont);
        let single = SingleLine::new(64).sanitize(&format!("a{g}b"));
        let multi = MultiLine::new(64).sanitize(&format!("a{g}b"));
        for out in [single.as_str(), multi.as_str()] {
            assert!(!out.contains(g), "{icon:?} survived: {out:?}");
            assert!(!out.chars().any(is_private_use), "{icon:?}: {out:?}");
        }
    }
}

#[test]
fn glyphs_are_unique_within_each_tier() {
    for tier in Tier::ALL {
        let mut seen = HashSet::new();
        for icon in Icon::all() {
            assert!(
                seen.insert(icon.glyph(tier)),
                "{icon:?} duplicates in {tier:?}"
            );
        }
    }
}

#[test]
fn security_markers_have_the_honest_text_fallbacks() {
    let server = Icon::from(SecurityMarker::ServerReadable);
    let e2ee = Icon::from(SecurityMarker::E2eeLock);
    assert_eq!(server.text_label(), "SERVER-READABLE");
    assert_eq!(server.glyph(Tier::Ascii), "SERVER-READABLE");
    assert_eq!(e2ee.text_label(), "E2EE");
    assert_eq!(e2ee.glyph(Tier::Ascii), "E2EE");
    for marker in SecurityMarker::ALL {
        let icon = Icon::from(marker);
        assert_eq!(icon.glyph(Tier::Ascii), icon.text_label(), "{marker:?}");
    }
}

#[test]
fn the_restricted_icon_is_never_the_e2ee_lock() {
    let restricted = Icon::from(ContentIcon::Restricted);
    let e2ee = Icon::from(SecurityMarker::E2eeLock);
    for tier in Tier::ALL {
        assert_ne!(restricted.glyph(tier), e2ee.glyph(tier), "{tier:?}");
    }
}

#[test]
fn only_security_markers_are_security_chrome() {
    for c in ContentIcon::ALL {
        let icon = Icon::from(c);
        assert!(!icon.is_security_chrome());
        assert!(!icon.render(Tier::Ascii).is_security_chrome());
    }
    for m in SecurityMarker::ALL {
        let icon = Icon::from(m);
        assert!(icon.is_security_chrome());
        for tier in Tier::ALL {
            assert!(icon.render(tier).is_security_chrome());
        }
    }
}

#[test]
fn every_non_ascii_glyph_character_has_a_width_entry_of_one_or_two() {
    for icon in Icon::all() {
        for tier in Tier::ALL {
            for c in icon.glyph(tier).chars() {
                let w = char_width(c).map(GlyphWidth::cells);
                assert!(matches!(w, Some(1 | 2)), "{icon:?} {tier:?} {c:?}: {w:?}");
            }
        }
    }
}

#[test]
fn the_width_table_has_no_duplicates_and_no_unused_entries() {
    let used: HashSet<char> = Icon::all()
        .flat_map(|i| Tier::ALL.into_iter().flat_map(move |t| i.glyph(t).chars()))
        .collect();
    let mut seen = HashSet::new();
    for (c, _) in TABLE {
        assert!(seen.insert(*c), "duplicate {c:?}");
        assert!(used.contains(c), "unused {c:?}");
    }
}

#[test]
fn nerd_font_glyphs_reserve_two_cells_and_other_tiers_are_one_per_char() {
    for icon in Icon::all() {
        assert_eq!(icon.render(Tier::NerdFont).width(), 2, "{icon:?}");
        for tier in [Tier::Unicode, Tier::Ascii] {
            let r = icon.render(tier);
            assert_eq!(r.width(), icon.glyph(tier).chars().count(), "{icon:?}");
        }
    }
}

#[test]
fn rendered_text_matches_its_width_and_pads_nerd_glyphs() {
    for icon in Icon::all() {
        for tier in Tier::ALL {
            let r = icon.render(tier);
            assert_eq!(r.tier(), tier);
            // Written text is one native cell per character; padding supplies the rest.
            let cells = r.plain().chars().count();
            assert_eq!(cells, r.width(), "{icon:?} {tier:?}");
        }
    }
    let r = Icon::from(ContentIcon::Mail).render(Tier::NerdFont);
    assert_eq!(r.plain(), "\u{f0e0} ");
}

#[test]
fn unicode_tier_characters_are_bmp_and_one_cell() {
    // Guard against swapping in an emoji (non-BMP) character, whose width varies by terminal.
    for icon in Icon::all() {
        for c in icon.glyph(Tier::Unicode).chars() {
            assert!((c as u32) < 0x1_0000, "{icon:?} is not BMP");
            assert_eq!(char_width(c), Some(GlyphWidth::One), "{icon:?}");
        }
    }
}

#[test]
fn unknown_characters_have_no_width() {
    assert_eq!(char_width('\u{e000}'), None);
    assert_eq!(char_width('\u{1b}'), None);
    assert_eq!(char_width('\u{4e00}'), None);
}

fn collect_rs(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_raw_private_use_codepoints_exist_in_source_outside_the_registry() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let icons_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/icons");
    let mut files = Vec::new();
    collect_rs(&crates, &mut files);
    assert!(!files.is_empty(), "found no source files to scan");
    for file in files {
        let Ok(canonical) = file.canonicalize() else {
            continue;
        };
        let inside = icons_dir
            .canonicalize()
            .is_ok_and(|d| canonical.starts_with(d));
        if inside {
            continue;
        }
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        assert!(
            !text.chars().any(is_private_use),
            "raw private-use character in {}",
            file.display()
        );
    }
}

#[test]
fn registry_source_uses_escapes_not_raw_private_use_characters() {
    let icons_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/icons");
    let mut files = Vec::new();
    collect_rs(&icons_dir, &mut files);
    for file in files {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        assert!(!text.chars().any(is_private_use), "{}", file.display());
    }
}

// ---- Security chrome (decision D39) ----

#[test]
fn restricted_says_restricted_and_is_not_a_padlock_or_a_privacy_claim() {
    let r = Icon::from(ContentIcon::Restricted);
    assert_eq!(r.text_label(), "restricted");
    assert_eq!(r.glyph(Tier::Ascii), "[restricted]");
    // fa-key, never fa-lock (U+F023), which is E2EE chrome.
    assert_eq!(r.glyph(Tier::NerdFont), "\u{f084}");
    assert_ne!(
        r.glyph(Tier::NerdFont),
        Icon::from(SecurityMarker::E2eeLock).glyph(Tier::NerdFont)
    );
}

#[test]
fn server_readable_unicode_glyph_does_not_read_as_prohibited() {
    // The old glyph U+2298 (circled division slash) reads as "none/forbidden", the opposite
    // of "the server can read this". U+2299 (circled dot) reads as an eye: honest.
    let g = Icon::from(SecurityMarker::ServerReadable).glyph(Tier::Unicode);
    assert!(g.contains('\u{2299}'));
    assert!(!g.contains('\u{2298}'));
}

#[test]
fn every_marker_carries_the_reserved_style_in_every_tier_and_content_icons_never_do() {
    for tier in Tier::ALL {
        for m in SecurityMarker::ALL {
            let r = Icon::from(m).render(tier);
            assert_eq!(r.chrome_style(), Some(ChromeStyle), "{m:?} {tier:?}");
            let t = r.to_terminal();
            assert!(t.starts_with(ChromeStyle::SGR_ON), "{m:?} {tier:?}");
            assert!(t.ends_with(ChromeStyle::SGR_OFF), "{m:?} {tier:?}");
            assert!(t.contains(r.plain()));
        }
        for c in ContentIcon::ALL {
            let r = Icon::from(c).render(tier);
            assert_eq!(r.chrome_style(), None);
            assert_eq!(r.to_terminal(), r.plain());
            assert!(!r.to_terminal().contains('\u{1b}'));
        }
    }
}

#[test]
fn unicode_markers_are_drawn_inside_the_reserved_delimiters() {
    for m in SecurityMarker::ALL {
        let g = Icon::from(m).glyph(Tier::Unicode);
        assert!(
            g.starts_with('\u{27e6}') && g.ends_with('\u{27e7}'),
            "{m:?}: {g:?}"
        );
        assert_eq!(g.chars().count(), 3, "{m:?}");
    }
}

#[test]
fn the_sanitizer_reserved_set_is_exactly_what_unicode_markers_use() {
    let used: HashSet<char> = SecurityMarker::ALL
        .into_iter()
        .flat_map(|m| Icon::from(m).glyph(Tier::Unicode).chars())
        .collect();
    let reserved: HashSet<char> = RESERVED_CHROME.iter().copied().collect();
    assert_eq!(used, reserved);
    // No content icon may use a reserved character, or content icons would be neutralized.
    for c in ContentIcon::ALL {
        assert!(
            !Icon::from(c)
                .glyph(Tier::Unicode)
                .chars()
                .any(|ch| reserved.contains(&ch))
        );
    }
}

/// Everything a sanitized string must never contain, per tier and marker: the reserved
/// delimiters and glyphs (Unicode), the private-use glyph (Nerd Font), and the reserved
/// style bytes (all tiers).
fn reserved_forms() -> Vec<String> {
    let mut forms = vec![
        ChromeStyle::SGR_ON.to_string(),
        ChromeStyle::SGR_OFF.to_string(),
    ];
    for m in SecurityMarker::ALL {
        for tier in [Tier::NerdFont, Tier::Unicode] {
            let r = Icon::from(m).render(tier);
            forms.push(r.to_terminal());
            for ch in r.plain().chars().filter(|c| *c != ' ') {
                forms.push(ch.to_string());
            }
        }
    }
    forms
}

fn weighted_forgery() -> impl Strategy<Value = String> {
    let forms = reserved_forms();
    let label_pieces: Vec<String> = SecurityMarker::ALL
        .iter()
        .flat_map(|m| {
            [
                m.label().to_string(),
                Icon::from(*m).render(Tier::Unicode).plain().to_string(),
            ]
        })
        .collect();
    let piece = prop_oneof![
        4 => prop::sample::select(forms),
        2 => prop::sample::select(label_pieces),
        2 => prop::sample::select(vec![
            "\u{1b}[".to_string(), "1;7m".to_string(), "\u{1b}[0m".to_string(), "\u{9b}1;7m".to_string(),
            "\u{27e6}".to_string(), "\u{27e7}".to_string(), " ".to_string(), "\n".to_string()]),
        1 => any::<char>().prop_map(|c| c.to_string()),
    ];
    prop::collection::vec(piece, 0..24).prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(3000))]

    /// For all four markers and the Nerd Font and Unicode tiers, no sanitized string can
    /// contain the marker's reserved form (delimiters, glyphs, private-use glyph), and for
    /// all three tiers none can contain the reserved style bytes or any ESC.
    #[test]
    fn no_sanitized_string_reproduces_a_reserved_marker_form(s in weighted_forgery(), max in 0usize..200) {
        let forms = reserved_forms();
        for out in [SingleLine::new(max).sanitize(&s), MultiLine::new(max).sanitize(&s)] {
            let out = out.as_str();
            prop_assert!(!out.contains('\u{1b}'), "ESC survived: {out:?}");
            for form in &forms {
                prop_assert!(!out.contains(form.as_str()), "{form:?} survived in {out:?}");
            }
            for m in SecurityMarker::ALL {
                for tier in [Tier::NerdFont, Tier::Unicode] {
                    let g = Icon::from(m).glyph(tier);
                    prop_assert!(!out.contains(g), "{m:?} {tier:?} glyph survived in {out:?}");
                }
            }
        }
    }
}

#[test]
fn ascii_tier_text_is_not_protected_by_the_sanitizer_and_the_docs_say_so() {
    // Honest limit (D39): content can type any ASCII, so the ASCII-tier label is not
    // unforgeable text. Only the reserved style and a chrome-only position distinguish the
    // real marker. This test pins the limit so nobody "fixes" the docs without a mechanism.
    for m in SecurityMarker::ALL {
        let label = Icon::from(m).glyph(Tier::Ascii);
        let out = SingleLine::new(64).sanitize(&format!("x {label} y"));
        assert!(out.as_str().contains(label), "{m:?}");
        let styled = Icon::from(m).render(Tier::Ascii).to_terminal();
        assert!(!out.as_str().contains(&styled), "{m:?}");
    }
}
