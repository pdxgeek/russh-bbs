#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! ART-1: the area and logoff art for every style, and the example board kit in
//! `examples/board/`, loaded through the same loaders a real board uses. Nothing here is a
//! second parser: a file that is wrong fails in the loader and so fails here.

use std::fs;
use std::path::{Path, PathBuf};

use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::menu::{OutlineKind, outline};
use russh_bbs_core::sanitize::RESERVED_CHROME;
use russh_bbs_core::store::AccountId;
use russh_bbs_core::terminal_settings::ThemeName;

use crate::ui::Ui;
use crate::ui::art::draw::{ArtCtx, Filling};
use crate::ui::art::{ArtPiece, ArtSet, Slot};
use crate::ui::sysop_menus::{self, Block};
use crate::ui::theme::{ThemeSet, pack};

const STYLES: [&str; 6] = ["phosphor", "neon", "paper", "amber", "deepspace", "ansi"];
const SLOTS: [Slot; 4] = [Slot::Login, Slot::Menu, Slot::Area, Slot::Logoff];
/// The pages the kit's `menus.toml` links.
const PAGES: [&str; 5] = ["about", "faq", "history", "rules", "whoswho"];
/// Most bytes in one kit file, and in the whole kit: "keep the files small".
const FILE_BYTES: u64 = 8 * 1024;
const KIT_BYTES: u64 = 40 * 1024;

fn kit() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/board")
}

fn name(style: &str) -> ThemeName {
    ThemeName::new(style).unwrap()
}

fn files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, found);
        } else {
            found.push(path);
        }
    }
}

/// What no art cell may hold: a control, the replacement character the sanitizer leaves where it
/// removed something, the reserved marker codepoints, or a Nerd Font private-use character.
fn check_grid(piece: &ArtPiece) {
    let grid = piece.grid();
    for cell in grid.cells() {
        for c in [cell.ch, cell.ascii] {
            assert!(!c.is_control(), "{}: control character", piece.set());
            assert_ne!(
                c,
                '\u{FFFD}',
                "{}: the sanitizer replaced something",
                piece.set()
            );
            assert!(
                !RESERVED_CHROME.contains(&c),
                "{}: reserved marker character {c:?}",
                piece.set()
            );
            assert!(
                !('\u{E000}'..='\u{F8FF}').contains(&c),
                "{}: private-use character",
                piece.set()
            );
        }
        assert!(
            cell.ascii == ' ' || cell.ascii.is_ascii_graphic(),
            "{}: the ASCII stand-in must be printable ASCII",
            piece.set()
        );
        assert!(cell.fg.is_none_or(|c| c < 16) && cell.bg.is_none_or(|c| c < 16));
    }
}

/// Every piece: inside its slot's caps, with a credit that names this project, a fallback line,
/// and an ASCII-tier form that is its own (not the large-letter stand-in).
fn check_piece(piece: &ArtPiece, style: &str) {
    let (cols, rows) = piece.slot().cap();
    let grid = piece.grid();
    assert!(
        grid.width() <= cols && grid.height() <= rows,
        "{style} {}",
        piece.slot().name()
    );
    assert_eq!(piece.style(), Some(name(style)));
    assert!(
        piece.credit().contains("russh-bbs"),
        "{style}: credit names the project"
    );
    assert!(
        piece.fallback().is_some(),
        "{style} {}: a fallback line",
        piece.slot().name()
    );
    assert!(
        piece.ascii_safe(),
        "{style} {}: an ASCII form",
        piece.slot().name()
    );
    check_grid(piece);
}

#[test]
fn every_style_ships_area_and_logoff_art_within_the_caps() {
    let set = ArtSet::bundled().unwrap();
    for style in STYLES {
        for slot in [Slot::Area, Slot::Logoff] {
            let found: Vec<_> = set
                .pieces()
                .iter()
                .filter(|p| p.slot() == slot && p.style() == Some(name(style)))
                .collect();
            assert_eq!(found.len(), 1, "{style} {}", slot.name());
            let piece = found[0];
            assert!(!piece.is_sysop());
            check_piece(piece, style);
            // An area header must leave a forum list its room at 80x24; a logoff screen is
            // a small picture, not a full-screen one.
            if slot == Slot::Area {
                assert!(piece.grid().height() <= 3 && piece.grid().width() <= 76);
            } else {
                assert!(piece.grid().height() <= 8 && piece.grid().width() <= 60);
            }
        }
    }
}

#[test]
fn area_art_fills_the_slot_in_every_tier_and_gives_way_to_its_fallback() {
    let set = ArtSet::bundled().unwrap();
    for style in STYLES.iter().copied().chain(["unknown-style"]) {
        let ctx = ArtCtx {
            set: &set,
            style: name(style),
            board: "Tidepool BBS",
            enabled: true,
        };
        for slot in [Slot::Area, Slot::Logoff] {
            for tier in [Tier::Unicode, Tier::Ascii] {
                let wide = ctx.fill(slot, tier, 100, 12);
                if style == "unknown-style" {
                    // No style of its own: the board's name in large letters (or nothing), and
                    // never a piece made for another style.
                    assert!(!matches!(wide, Some(Filling::Piece(_))), "{style}");
                    continue;
                }
                let Some(Filling::Piece(piece)) = wide else {
                    panic!("{style} {} {tier:?}: expected the piece", slot.name());
                };
                assert_eq!(piece.style(), Some(name(style)));
            }
            if style == "unknown-style" {
                continue;
            }
            // Too narrow for the picture: the piece's own one-line fallback, not nothing.
            // (39 columns is narrower than every logoff picture and every area header.)
            let narrow = ctx.fill(slot, Tier::Unicode, 39, 12);
            assert!(
                matches!(narrow, Some(Filling::Text(_))),
                "{style} {}: no one-line fallback",
                slot.name()
            );
        }
    }
}

#[test]
fn the_new_art_differs_per_style_and_is_not_a_copy_of_another_piece() {
    let set = ArtSet::bundled().unwrap();
    let mut seen = std::collections::HashSet::new();
    for piece in set.pieces() {
        let rows: Vec<String> = (0..piece.grid().height())
            .map(|y| piece.grid().row_text(y, false))
            .collect();
        assert!(
            seen.insert(rows),
            "{} {} duplicates",
            piece.set(),
            piece.slot().name()
        );
    }
    assert_eq!(seen.len(), STYLES.len() * SLOTS.len());
}

fn everything() -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::MEMBER,
        10,
        &Via::Account,
        Permission::ALL.iter().copied(),
    )
}

#[test]
fn the_kit_menu_and_pages_load_through_the_real_loader() {
    let strings = Ui::standard().unwrap().strings().clone();
    let setup = sysop_menus::load(&kit(), &strings);
    assert_eq!(setup.problem(), None, "menus.toml and pages must load");
    assert!(setup.custom());
    let ids: Vec<&str> = setup.pages().map(sysop_menus::Page::id).collect();
    assert_eq!(ids, PAGES);
    for page in setup.pages() {
        assert!(!page.title().is_empty() && page.title().chars().count() <= 32);
        let text = page
            .blocks()
            .iter()
            .filter(|b| !matches!(b, Block::Rule | Block::Blank))
            .count();
        assert!(text >= 5, "{} has some text", page.id());
        if let Some(art) = page.art() {
            let (cols, rows) = Slot::Area.cap();
            assert!(art.grid().width() <= cols && art.grid().height() <= rows);
            assert!(art.credit().contains("russh-bbs"));
            check_grid(art);
        }
    }
    assert!(setup.page("about").unwrap().art().is_some());
    assert!(setup.page("history").unwrap().art().is_some());

    // Reordering, nesting and pages are all visible in the effective tree.
    let lines = outline(setup.tree(), &everything());
    let first = &lines[0];
    assert_eq!(
        (first.depth, first.hotkey, first.kind),
        (0, 'n', OutlineKind::Menu)
    );
    assert!(
        lines
            .iter()
            .any(|l| l.depth == 2 && l.kind == OutlineKind::Page),
        "three levels"
    );
    assert_eq!(
        lines.iter().filter(|l| l.kind == OutlineKind::Page).count(),
        PAGES.len()
    );
    assert!(
        lines.iter().any(|l| l.depth == 0 && l.hotkey == 'p'),
        "mail is re-keyed to p"
    );
}

#[test]
fn the_kit_pack_imports_and_the_seventh_style_loads_with_its_art() {
    let strings = Ui::standard().unwrap().strings().clone();
    let text = pack::read_file(&kit().join("packs/tidepool.pack.toml")).unwrap();
    let parsed = pack::parse(&text, &strings).unwrap();
    assert_eq!(parsed.theme().name().as_str(), "tidepool");
    assert_eq!(parsed.theme().title(), "Tidepool");
    assert_eq!(parsed.art_files(), 1);

    // Install it the way the sysop screen does, then load the directories the way boot does.
    let dir = tempfile::tempdir().unwrap();
    let (themes_dir, art_dir) = (dir.path().join("themes"), dir.path().join("art"));
    let mut themes = ThemeSet::shipped(&strings).unwrap();
    assert!(
        !themes.contains(name("tidepool")),
        "a seventh style, not a replacement"
    );
    pack::install(&parsed, &themes, &themes_dir, &art_dir).unwrap();
    assert_eq!(themes.add_dir(&themes_dir, &strings), 1);
    assert!(themes.contains(name("tidepool")));

    let mut art = ArtSet::bundled().unwrap();
    let bundled = art.pieces().len();
    assert_eq!(art.add_dir(&art_dir), SLOTS.len());
    assert!(art.problems().is_empty(), "{:?}", art.problems());
    assert_eq!(art.pieces().len(), bundled + SLOTS.len());
    for slot in SLOTS {
        let piece = art
            .pieces()
            .iter()
            .find(|p| p.slot() == slot && p.style() == Some(name("tidepool")))
            .unwrap_or_else(|| panic!("tidepool {}", slot.name()));
        assert!(piece.is_sysop());
        check_piece(piece, "tidepool");
    }

    // It differs from the style it was exported from.
    let paper = themes.get(name("paper"));
    assert_ne!(
        themes.get(name("tidepool")).export_text(),
        paper.export_text()
    );
}

#[test]
fn the_kit_is_small_and_holds_no_reserved_or_private_use_characters() {
    let mut found = Vec::new();
    files(&kit(), &mut found);
    assert!(found.len() >= 10, "menus, pages, art, pack and README");
    let mut total = 0;
    for path in &found {
        let size = fs::metadata(path).unwrap().len();
        total += size;
        assert!(size <= FILE_BYTES, "{} is {size} bytes", path.display());
        let text = fs::read_to_string(path).unwrap();
        for c in text.chars() {
            assert!(
                !RESERVED_CHROME.contains(&c) && !('\u{E000}'..='\u{F8FF}').contains(&c),
                "{}: {c:?} is reserved for the interface",
                path.display()
            );
            assert!(
                c == '\n' || c == '\t' || !c.is_control(),
                "{}: control character {c:?}",
                path.display()
            );
        }
        // Never a credential in an example.
        assert!(
            !text.to_ascii_lowercase().contains("-----begin"),
            "{}",
            path.display()
        );
    }
    assert!(total <= KIT_BYTES, "the kit is {total} bytes");
}
