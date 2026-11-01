use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::width::TABLE;
use super::*;
use crate::sanitize::{MultiLine, SingleLine};

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
fn the_generic_lock_is_never_the_e2ee_lock() {
    let lock = Icon::from(ContentIcon::Lock);
    let e2ee = Icon::from(SecurityMarker::E2eeLock);
    for tier in Tier::ALL {
        assert_ne!(lock.glyph(tier), e2ee.glyph(tier), "{tier:?}");
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
            let cells = r.as_str().chars().count();
            assert_eq!(cells, r.width(), "{icon:?} {tier:?}");
        }
    }
    let r = Icon::from(ContentIcon::Mail).render(Tier::NerdFont);
    assert_eq!(r.as_str(), "\u{f0e0} ");
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
