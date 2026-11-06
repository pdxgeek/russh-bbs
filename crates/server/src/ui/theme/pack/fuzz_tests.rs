//! H-4: a pack import is all or nothing, whatever the pack holds and whatever is in the way.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::Path;

use proptest::prelude::*;

use super::*;
use crate::fuzz::mutated;
use crate::ui::Ui;
use crate::ui::strings::StringTable;

fn base() -> StringTable {
    Ui::standard().unwrap().strings().clone()
}

fn empty_board(base: &StringTable) -> ThemeSet {
    ThemeSet {
        themes: vec![Theme::builtin(base.clone())],
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// A pack for the style `harbour` with two art files, so an install writes three files.
fn sample() -> &'static str {
    static TEXT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TEXT.get_or_init(|| {
        let art = "format = 1\nname = \"harbour\"\nstyle = \"harbour\"\n[[piece]]\nslot = \"menu\"\nrows = [\"hi\"]\n";
        format!(
            "pack = 1\ntheme = '''\nname = \"harbour\"\n[colors]\nborder = \"cyan\"\n'''\n\
             [[art]]\nkind = \"art.toml\"\ntext = '''\n{art}'''\n\
             [[art]]\nkind = \"ans\"\nslot = \"login\"\nhex = \"{}\"\n",
            hex(b"\x1b[1;36mHARBOUR\x1b[0m\r\nhello")
        )
    })
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn the_sample_pack_is_valid_and_writes_three_files() {
    let base = base();
    let pack = parse(sample(), &base).unwrap();
    assert_eq!(pack.art_files(), 2);
    let dir = tempfile::tempdir().unwrap();
    let (t, a) = (dir.path().join("t"), dir.path().join("a"));
    let written = install(&pack, &empty_board(&base), &t, &a).unwrap();
    assert_eq!(written.len(), 3);
}

/// Whichever one of the files an install would write is already there, the install fails and
/// both directories hold exactly what they held before.
#[test]
fn a_file_in_the_way_of_any_target_stops_the_whole_import() {
    let base = base();
    let pack = parse(sample(), &base).unwrap();
    // Learn the three target paths from a dry run elsewhere.
    let scratch = tempfile::tempdir().unwrap();
    let (st, sa) = (scratch.path().join("t"), scratch.path().join("a"));
    let targets = install(&pack, &empty_board(&base), &st, &sa).unwrap();
    for victim in &targets {
        let in_art = victim.starts_with(&sa);
        let dir = tempfile::tempdir().unwrap();
        let (t, a) = (dir.path().join("t"), dir.path().join("a"));
        fs::create_dir_all(&t).unwrap();
        fs::create_dir_all(&a).unwrap();
        let name = victim.file_name().unwrap();
        // A file, and then a directory, squatting on the name.
        for as_dir in [false, true] {
            let spot = if in_art { a.join(name) } else { t.join(name) };
            let _ = fs::remove_file(&spot);
            let _ = fs::remove_dir(&spot);
            if as_dir {
                fs::create_dir(&spot).unwrap();
            } else {
                fs::write(&spot, "keep me").unwrap();
            }
            let before = (listing(&t), listing(&a));
            let result = install(&pack, &empty_board(&base), &t, &a);
            assert!(result.is_err(), "{victim:?} as_dir={as_dir}");
            assert_eq!((listing(&t), listing(&a)), before, "{victim:?}");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, failure_persistence: None, ..ProptestConfig::default() })]

    /// A pack that parses installs whole and loads back with no art problem; one that does not
    /// parse never gets as far as a write (there is no `Pack` to install).
    #[test]
    fn an_accepted_pack_installs_whole_and_loads_back(text in mutated(sample())) {
        let base = base();
        let pack = match parse(&text, &base) {
            Ok(pack) => pack,
            Err(error) => {
                let shown = error.to_string();
                prop_assert!(shown.chars().count() < 450, "{} characters", shown.chars().count());
                prop_assert!(!shown.chars().any(char::is_control), "{shown:?}");
                return Ok(());
            }
        };
        let dir = tempfile::tempdir().unwrap();
        let (t, a) = (dir.path().join("t"), dir.path().join("a"));
        let board = empty_board(&base);
        let Ok(written) = install(&pack, &board, &t, &a) else {
            // Refused (a style that is installed, a cap): nothing may be left behind.
            prop_assert!(listing(&t).is_empty() && listing(&a).is_empty());
            return Ok(());
        };
        prop_assert_eq!(written.len(), pack.art_files() + 1);
        let mut themes = empty_board(&base);
        prop_assert_eq!(themes.add_dir(&t, &base), 1);
        let mut art = ArtSet::default();
        art.add_dir(&a);
        prop_assert!(art.problems().is_empty(), "{:?}", art.problems());
    }
}
