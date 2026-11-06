//! H-4: property tests for every loader that reads a sysop- or file-supplied document.
//!
//! Each loader is fed a valid document with random mutations, random bytes, structured garbage
//! (deep nesting, huge strings, duplicate keys, wrong types, NaN and huge numbers, unknown
//! fields) and valid documents with reserved characters spliced in. The properties:
//!
//! - nothing panics (a panic fails the property);
//! - a refusal carries one short, loggable reason (no control characters, bounded length);
//! - what loads contains no control character, no Nerd Font private-use glyph and none of the
//!   reserved marker codepoints, and no cell carries an attribute;
//! - a refused file leaves the safe fallback in place (the built-in menu, the default theme);
//! - a theme pack is imported all or nothing.
//!
//! The tests run in `cargo test` on stable and stay fast in a debug build. The long run is
//! `loader_fuzz_long`, behind `#[ignore]`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use proptest::prelude::*;

use super::art::{ArtSet, Grid, MAX_COLS, MAX_ROWS, parse_ans, parse_art_toml};
use super::strings::StringTable;
use super::sysop_menus::{Block, MenuSetup, load as load_menus};
use super::theme::pack;
use super::theme::{MAX_THEME_BYTES, Theme, ThemeSet};
use crate::fuzz::{forbidden_in_content, garbage, mutated, random_text, reason_is_loggable};
use crate::ui::Ui;

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 96,
        // The failing input is the report; do not write regression files into the tree.
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn base() -> &'static StringTable {
    static BASE: OnceLock<StringTable> = OnceLock::new();
    BASE.get_or_init(|| Ui::standard().unwrap().strings().clone())
}

// ---- valid documents the mutations start from -------------------------------------------------

const THEME: &str = include_str!("themes/neon.toml");

const ART_TOML: &str = r#"format = 1
name = "mark"
style = "neon"
credit = "tests"

[[piece]]
slot = "menu"
rows = ["+-----+", "| BBS |", "+-----+"]
fg = ["rrrrrrr", "rgggggr", "rrrrrrr"]
bg = ["kkkkkkk", "kkkkkkk", "kkkkkkk"]
ascii = ["+-----+", "| BBS |", "+-----+"]

[[piece]]
slot = "login"
rows = ["hello", "world"]

[[piece]]
slot = "area"
rows = ["~ rules ~"]
"#;

const MENUS: &str = r#"version = 1

[[menu]]
id = "main"
title = "Main Hall"

  [[menu.item]]
  target = "page:rules"
  key = "r"

  [[menu.item]]
  target = "main.mail"
  key = "x"
  label = "Messages"

  [[menu.item]]
  target = "group:talk"
  key = "t"
  label = "Talk"

  [[menu.item]]
  target = "main.doors"
  hidden = true

[[menu]]
id = "talk"
title = "Talk Hall"

  [[menu.item]]
  target = "main.chat"
"#;

const PAGE: &str = r#"version = 1
title = "House rules"
art = "mark.art.toml"
text = """
# Be kind
Plain paragraphs wrap to the room there is.
- no spam
> a quotation
---
The end.
"""
"#;

fn pack_text() -> &'static str {
    static PACK: OnceLock<String> = OnceLock::new();
    PACK.get_or_init(|| {
        let themes = ThemeSet::shipped(base()).unwrap();
        let art = ArtSet::bundled().unwrap();
        let name = themes
            .names()
            .into_iter()
            .find(|n| n.as_str() == "neon")
            .unwrap();
        pack::export(&themes, &art, name).unwrap()
    })
}

// ---- helpers ---------------------------------------------------------------------------------

fn check_grid(grid: &Grid) {
    assert!(grid.width() <= MAX_COLS, "width {}", grid.width());
    assert!(grid.height() <= MAX_ROWS, "height {}", grid.height());
    for cell in grid.cells() {
        for c in [cell.ch, cell.ascii] {
            assert!(!forbidden_in_content(c), "forbidden {c:?}");
        }
        assert!(cell.ascii.is_ascii_graphic() || cell.ascii == ' ');
        assert!(cell.fg.is_none_or(|i| i < 16) && cell.bg.is_none_or(|i| i < 16));
    }
}

fn check_strings(strings: &StringTable) {
    for key in strings.keys() {
        let value = strings.text(key);
        assert!(
            !value.chars().any(forbidden_in_content),
            "string {key}: {value:?}"
        );
    }
}

fn is_refused(setup: &MenuSetup) -> bool {
    setup.problem().is_some()
}

fn assert_loggable(reason: &str) {
    if let Err(why) = reason_is_loggable(reason) {
        panic!("{why}");
    }
}

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A generous bound for one parse: the caps keep every loader far below it, even in a debug
/// build, and a quadratic path would blow through it.
const PARSE_BUDGET: Duration = Duration::from_secs(3);

fn timed<T>(what: &str, work: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let out = work();
    assert!(
        started.elapsed() < PARSE_BUDGET,
        "{what} took {:?}",
        started.elapsed()
    );
    out
}

// ---- theme files -----------------------------------------------------------------------------

fn check_theme(text: &str) {
    match timed("theme", || Theme::parse(text, base())) {
        Ok(theme) => {
            assert!(text.len() <= MAX_THEME_BYTES);
            check_strings(theme.strings());
            assert!(!theme.title().chars().any(forbidden_in_content));
            assert!(!theme.credit().chars().any(forbidden_in_content));
        }
        Err(error) => assert_loggable(&error.to_string()),
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn theme_files_mutated(text in mutated(THEME)) {
        check_theme(&text);
    }

    #[test]
    fn theme_files_garbage(text in garbage()) {
        check_theme(&text);
    }

    #[test]
    fn theme_files_random_bytes(text in random_text()) {
        check_theme(&text);
    }
}

/// A theme directory full of hostile files loads what is valid and nothing else; the default
/// theme and the shipped ones are still there, and the loader never fails.
#[test]
fn a_directory_of_hostile_themes_leaves_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let big = format!("name = \"big\"\n# {}\n", "x".repeat(MAX_THEME_BYTES));
    let deep = format!(
        "name = \"deep\"\na = {}{}\n",
        "[".repeat(5000),
        "]".repeat(5000)
    );
    write(dir.path(), "big.toml", &big);
    write(dir.path(), "deep.toml", &deep);
    write(dir.path(), "empty.toml", "");
    write(dir.path(), "wrong.toml", "name = \"other\"\n");
    write(dir.path(), "default.toml", "name = \"default\"\n");
    fs::write(
        dir.path().join("binary.toml"),
        [0xff, 0xfe, 0x00, 0x9f, 0x92],
    )
    .unwrap();
    fs::create_dir(dir.path().join("dir.toml")).unwrap();
    for n in 0..40 {
        write(dir.path(), &format!("many{n}.toml"), "name = \"many\"\n");
    }
    let mut set = ThemeSet::shipped(base()).unwrap();
    let before = set.names();
    let added = set.add_dir(dir.path(), base());
    // `many*.toml` all fail the name check (the file stem differs), so nothing is added.
    assert_eq!(added, 0);
    assert_eq!(set.names(), before);
    let default = set.default_theme().name();
    assert_eq!(default.as_str(), "default");
    assert!(set.contains(set.default_theme().name()));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, ..config() })]

    #[test]
    fn a_hostile_theme_file_on_disk_never_replaces_a_shipped_theme(text in mutated(THEME)) {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "neon.toml", &text);
        let mut set = ThemeSet::shipped(base()).unwrap();
        let before = set.get(set.names().into_iter().find(|n| n.as_str() == "neon").unwrap()).clone();
        let _ = set.add_dir(dir.path(), base());
        let name = set.names().into_iter().find(|n| n.as_str() == "neon").unwrap();
        // A sysop file cannot shadow a shipped style: it is refused as a duplicate.
        prop_assert_eq!(set.get(name), &before);
        let default = set.default_theme().name();
        prop_assert_eq!(default.as_str(), "default");
    }
}

// ---- theme packs -----------------------------------------------------------------------------

fn check_pack(text: &str) {
    match timed("pack", || pack::parse(text, base())) {
        Ok(parsed) => {
            check_strings(parsed.theme().strings());
            assert!(parsed.art_files() <= pack::MAX_ART_ENTRIES);
        }
        Err(error) => assert_loggable(&error.to_string()),
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn packs_mutated(text in mutated(pack_text())) {
        check_pack(&text);
    }

    #[test]
    fn packs_garbage(text in garbage()) {
        check_pack(&text);
    }
}

// ---- board art -------------------------------------------------------------------------------

fn check_art_toml(text: &str) {
    match timed("art.toml", || parse_art_toml(text, true)) {
        Ok(file) => {
            assert!(text.len() <= super::art::ART_TOML_MAX_BYTES);
            assert_ne!(file.pieces.len(), 0);
            for piece in &file.pieces {
                check_grid(piece.grid());
                if let Some(fallback) = piece.fallback() {
                    assert!(!fallback.chars().any(forbidden_in_content), "{fallback:?}");
                }
            }
        }
        Err(error) => assert_loggable(&error.to_string()),
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn art_toml_mutated(text in mutated(ART_TOML)) {
        check_art_toml(&text);
    }

    #[test]
    fn art_toml_garbage(text in garbage()) {
        check_art_toml(&text);
    }

    /// A picture file of any bytes, SAUCE record or none: caps hold and cells are clean.
    #[test]
    fn ans_bytes_with_sauce_and_without(
        body in prop::collection::vec(any::<u8>(), 0..2048),
        columns in any::<u16>(),
        comments in 0_u8..=255,
        with_sauce in any::<bool>(),
        junk in prop::collection::vec(any::<u8>(), 0..160),
    ) {
        let mut bytes = body;
        if with_sauce {
            bytes.push(0x1a);
            // Comment block header and lines the SAUCE record may claim.
            if comments > 0 {
                bytes.extend(b"COMNT");
                bytes.extend(junk.iter().cycle().take(usize::from(comments) * 64));
            }
            let mut record = vec![0_u8; 128];
            record[..5].copy_from_slice(b"SAUCE");
            record[90] = comments;
            record[96..98].copy_from_slice(&columns.to_le_bytes());
            for (i, b) in junk.iter().take(64).enumerate() {
                record[7 + i] = *b;
            }
            bytes.extend(record);
        }
        match timed("ans", || parse_ans(&bytes)) {
            Ok(parsed) => check_grid(&parsed.grid),
            Err(error) => assert_loggable(&error.to_string()),
        }
    }
}

/// Worst-case pictures at the byte cap parse inside the time budget and the grid caps.
#[test]
fn ans_worst_cases_at_the_byte_cap_are_fast_and_capped() {
    let cap = super::art::ANS_MAX_BYTES;
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("newlines", vec![b'\n'; cap]),
        ("home and clear", b"\x1b[1;1H\x1b[2J".repeat(cap / 10)),
        ("far moves", b"\x1b[9999C\x1b[9999B".repeat(cap / 14)),
        (
            "params",
            format!("\x1b[{}m", "1;".repeat(120))
                .repeat(cap / 250)
                .into_bytes(),
        ),
        ("unterminated", {
            let mut v = vec![0x1b, b'['];
            v.extend(std::iter::repeat_n(b'9', cap - 2));
            v
        }),
        ("tabs and backspaces", b"\t\x08\t\x08x".repeat(cap / 5)),
        (
            "every byte",
            (0..cap).map(|i| u8::try_from(i % 256).unwrap()).collect(),
        ),
        ("eof marks", vec![0x1a; cap]),
    ];
    for (what, bytes) in cases {
        assert!(bytes.len() <= cap);
        match timed(what, || parse_ans(&bytes)) {
            Ok(parsed) => check_grid(&parsed.grid),
            Err(error) => assert_loggable(&error.to_string()),
        }
    }
}

/// An art directory of hostile files keeps the bundled art, loads only clean pieces, and
/// records one loggable reason for each refusal.
fn check_art_dir(files: &[(&str, Vec<u8>)]) {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in files {
        fs::write(dir.path().join(name), bytes).unwrap();
    }
    let bundled = ArtSet::bundled().unwrap();
    let mut set = ArtSet::bundled().unwrap();
    timed("art dir", || set.add_dir(dir.path()));
    assert!(set.pieces().len() >= bundled.pieces().len());
    assert_eq!(
        &set.pieces()[..bundled.pieces().len()],
        bundled.pieces(),
        "the bundled art is unchanged"
    );
    for piece in set.pieces() {
        check_grid(piece.grid());
    }
    for problem in set.problems() {
        assert_loggable(&problem.reason);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, ..config() })]

    #[test]
    fn an_art_directory_of_hostile_files_keeps_the_bundled_art(
        toml_a in mutated(ART_TOML),
        toml_b in garbage(),
        ans in prop::collection::vec(any::<u8>(), 0..1500),
    ) {
        check_art_dir(&[
            ("a.art.toml", toml_a.into_bytes()),
            ("b.art.toml", toml_b.into_bytes()),
            ("menu.ans", ans.clone()),
            ("neon.login.ans", ans),
            ("bad name.ans", b"x".to_vec()),
            ("c.art.toml", vec![0xff, 0xfe, 0x00]),
        ]);
    }
}

/// More files than the cap: the rest are ignored with a reason, not loaded.
#[test]
fn an_art_directory_over_the_file_cap_stops_at_the_cap() {
    let files: Vec<(String, Vec<u8>)> = (0..super::art::MAX_FILES + 20)
        .map(|n| (format!("f{n:03}.art.toml"), ART_TOML.as_bytes().to_vec()))
        .collect();
    let borrowed: Vec<(&str, Vec<u8>)> =
        files.iter().map(|(n, b)| (n.as_str(), b.clone())).collect();
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in &borrowed {
        fs::write(dir.path().join(name), bytes).unwrap();
    }
    let mut set = ArtSet::default();
    set.add_dir(dir.path());
    assert!(
        set.pieces().len() <= super::art::MAX_FILES * 3,
        "{}",
        set.pieces().len()
    );
    assert_ne!(set.problems().len(), 0);
}

// ---- menus.toml and pages --------------------------------------------------------------------

fn board_dir(menus: &str, page: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "menus.toml", menus);
    write(dir.path(), "pages/rules.toml", page);
    write(dir.path(), "pages/history.toml", page);
    write(dir.path(), "pages/mark.art.toml", ART_TOML);
    dir
}

fn check_menu_setup(setup: &MenuSetup) {
    if is_refused(setup) {
        // A refusal is the built-in menu, and says why in one line.
        assert!(!setup.custom());
        assert!(std::ptr::eq(
            setup.tree(),
            russh_bbs_core::menu::main_tree()
        ));
        assert_loggable(setup.problem().unwrap());
        return;
    }
    for text in setup.tree().text_keys() {
        assert!(
            !text.chars().any(forbidden_in_content),
            "menu text {text:?}"
        );
    }
    for page in setup.pages() {
        assert!(!page.title().chars().any(forbidden_in_content));
        for block in page.blocks() {
            match block {
                Block::Heading(t) | Block::Text(t) | Block::Bullet(t) | Block::Quote(t) => {
                    assert!(!t.chars().any(forbidden_in_content), "page text {t:?}");
                }
                Block::Rule | Block::Blank => {}
            }
        }
        if let Some(art) = page.art() {
            check_grid(art.grid());
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..config() })]

    #[test]
    fn menus_mutated(text in mutated(MENUS)) {
        let dir = board_dir(&text, PAGE);
        check_menu_setup(&timed("menus", || load_menus(dir.path(), base())));
    }

    #[test]
    fn menus_garbage(text in garbage()) {
        let dir = board_dir(&text, PAGE);
        check_menu_setup(&timed("menus", || load_menus(dir.path(), base())));
    }

    #[test]
    fn pages_mutated(text in mutated(PAGE)) {
        let dir = board_dir(MENUS, &text);
        check_menu_setup(&timed("page", || load_menus(dir.path(), base())));
    }

    #[test]
    fn pages_garbage(text in garbage()) {
        let dir = board_dir(MENUS, &text);
        check_menu_setup(&timed("page", || load_menus(dir.path(), base())));
    }

    #[test]
    fn page_art_mutated(text in mutated(ART_TOML)) {
        let dir = board_dir(MENUS, PAGE);
        write(dir.path(), "pages/mark.art.toml", &text);
        check_menu_setup(&timed("page art", || load_menus(dir.path(), base())));
    }
}

/// The valid documents the mutations start from really are valid: otherwise every property
/// above would be testing refusals only.
#[test]
fn the_sample_documents_are_valid() {
    Theme::parse(THEME, base()).unwrap();
    parse_art_toml(ART_TOML, true).unwrap();
    pack::parse(pack_text(), base()).unwrap();
    let dir = board_dir(MENUS, PAGE);
    let setup = load_menus(dir.path(), base());
    assert!(!is_refused(&setup), "{:?}", setup.problem());
    assert!(setup.custom());
}

/// Binary junk, a directory and a link where the files belong: the built-in menu, one reason.
#[test]
fn menu_files_that_are_not_text_files_fall_back() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("menus.toml"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    check_menu_setup(&load_menus(dir.path(), base()));
    assert!(is_refused(&load_menus(dir.path(), base())));

    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("menus.toml")).unwrap();
    let setup = load_menus(dir.path(), base());
    assert!(is_refused(&setup), "{:?}", setup.problem());

    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "real.toml", MENUS);
    std::os::unix::fs::symlink(dir.path().join("real.toml"), dir.path().join("menus.toml"))
        .unwrap();
    let setup = load_menus(dir.path(), base());
    assert!(is_refused(&setup), "{:?}", setup.problem());
}

// ---- the long run ----------------------------------------------------------------------------

/// `cargo test -p russh-bbs-server loader_fuzz_long -- --ignored`: ten times the cases.
#[test]
#[ignore = "long fuzz run; see docs/qa/h4-loader-fuzz.md"]
fn loader_fuzz_long() {
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: 4000,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    runner
        .run(&mutated(THEME), |t| {
            check_theme(&t);
            Ok(())
        })
        .unwrap();
    runner
        .run(&mutated(ART_TOML), |t| {
            check_art_toml(&t);
            Ok(())
        })
        .unwrap();
    runner
        .run(&mutated(pack_text()), |t| {
            check_pack(&t);
            Ok(())
        })
        .unwrap();
    runner
        .run(&garbage(), |t| {
            check_theme(&t);
            check_art_toml(&t);
            check_pack(&t);
            Ok(())
        })
        .unwrap();
}

// ---- regressions found by the properties above ----------------------------------------------

fn short_one_line(what: &str, shown: &str) {
    assert!(
        shown.chars().count() < 450,
        "{what}: {} characters",
        shown.chars().count()
    );
    assert!(!shown.chars().any(char::is_control), "{what}: {shown:?}");
}

/// A refusal quotes what the file said, so what the file said must be cut: a 50 000 character
/// value or key must not become a 50 000 character log line or sysop message.
#[test]
fn a_refusal_never_repeats_a_long_value_or_key_whole() {
    let long = "z".repeat(50_000);
    let theme = |from: &str, to: String| THEME.replacen(from, &to, 1);
    for (what, text) in [
        (
            "style value",
            theme("frame = \"rounded\"", format!("frame = \"{long}\"")),
        ),
        (
            "colour value",
            theme("surface = \"#0b0618\"", format!("surface = \"{long}\"")),
        ),
        (
            "colour role",
            theme("[colors]", format!("[colors]\n{long} = \"#000000\"")),
        ),
        (
            "meta key",
            theme("[meta]", format!("[meta]\n{long} = \"x\"")),
        ),
        ("string key", format!("{THEME}\n{long} = \"x\"\n")),
        (
            "name",
            theme("name = \"neon\"", format!("name = \"{long}\"")),
        ),
    ] {
        let error = Theme::parse(&text, base()).unwrap_err().to_string();
        short_one_line(what, &error);
    }
    // A pack: the entry kind, and the theme inside.
    let pack = format!("pack = 1\ntheme = '''\nname = \"x\"\n'''\n[[art]]\nkind = \"{long}\"\n");
    short_one_line(
        "pack kind",
        &pack::parse(&pack, base()).unwrap_err().to_string(),
    );
    short_one_line(
        "pack key",
        &pack::parse(&format!("pack = 1\ntheme = ''\n{long} = 1\n"), base())
            .unwrap_err()
            .to_string(),
    );
    // Art: the TOML error quotes the offending line, over several lines.
    short_one_line(
        "art key",
        &parse_art_toml(&format!("{long} = 1\n"), true)
            .unwrap_err()
            .to_string(),
    );
    short_one_line(
        "art toml error",
        &parse_art_toml(
            "format = 1\nname = \"a\"\n[[piece]]\nslot = \"menu\"\nrows = [\"a\"]\nbogus = 1\n",
            true,
        )
        .unwrap_err()
        .to_string(),
    );
    // Menus and pages.
    let dir = board_dir(&format!("version = 1\n{long} = 1\n"), PAGE);
    short_one_line(
        "menus key",
        load_menus(dir.path(), base()).problem().unwrap(),
    );
    let dir = board_dir(MENUS, &format!("version = 1\n{long} = 1\n"));
    short_one_line(
        "page key",
        load_menus(dir.path(), base()).problem().unwrap(),
    );
}
