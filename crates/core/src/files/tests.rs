//! Unit tests for the file rules that need no store: the virtual path parser (with a fuzz test),
//! the name and tag rules.
#![allow(clippy::unwrap_used)]

use proptest::prelude::*;

use super::path::{self, Entry, PathError, VPath};
use super::text::{self, TextError};
use super::{AreaId, FileId};

fn area(n: i64) -> AreaId {
    AreaId::from_raw(n)
}

fn file(n: i64) -> FileId {
    FileId::from_raw(n)
}

#[test]
fn the_tree_has_these_places_and_no_others() {
    assert_eq!(path::parse("/").unwrap(), VPath::Root);
    assert_eq!(path::parse("").unwrap(), VPath::Root);
    assert_eq!(path::parse(".").unwrap(), VPath::Root);
    assert_eq!(path::parse("/3").unwrap(), VPath::Area(area(3)));
    assert_eq!(path::parse("/3-docs/").unwrap(), VPath::Area(area(3)));
    assert_eq!(
        path::parse("3-anything-at-all").unwrap(),
        VPath::Area(area(3))
    );
    assert_eq!(
        path::parse("/3-docs/12-manual.txt").unwrap(),
        VPath::AreaEntry(area(3), Entry::Id(file(12)))
    );
    assert_eq!(
        path::parse("/3/12").unwrap(),
        VPath::AreaEntry(area(3), Entry::Id(file(12)))
    );
    assert_eq!(
        path::parse("/3/new name.txt").unwrap(),
        VPath::AreaEntry(area(3), Entry::Name("new name.txt".into()))
    );
    // A name that starts with digits but not as `<id>` or `<id>-...` is a name.
    assert_eq!(
        path::parse("/3/12abc.txt").unwrap(),
        VPath::AreaEntry(area(3), Entry::Name("12abc.txt".into()))
    );
    assert_eq!(path::parse("/3/pending").unwrap(), VPath::Pending(area(3)));
    assert_eq!(
        path::parse("/3/pending/9-x").unwrap(),
        VPath::PendingEntry(area(3), Entry::Id(file(9)))
    );
    assert_eq!(path::parse("/downloads").unwrap(), VPath::Downloads);
    assert_eq!(
        path::parse("/downloads/4-x").unwrap(),
        VPath::DownloadEntry(Entry::Id(file(4)))
    );
    // Empty and `.` components are skipped.
    assert_eq!(
        path::parse("//3/./12//").unwrap(),
        path::parse("3/12").unwrap()
    );
    // Not places.
    for bad in [
        "/docs",
        "/0",
        "/-1",
        "/3/pending/9/x",
        "/3/12/extra",
        "/downloads/4/x",
        "/99999999999999999999",
    ] {
        assert_eq!(path::parse(bad), Err(PathError::NoSuchPlace), "{bad}");
    }
}

#[test]
fn traversal_absolute_tricks_and_odd_encodings_are_refused() {
    for bad in ["..", "/..", "/3/..", "../etc/passwd", "/3/../4", "/3/12/.."] {
        assert_eq!(path::parse(bad), Err(PathError::Dots), "{bad}");
    }
    for bad in [
        "/3/a\\b",
        "/3/a\0b",
        "/3/a\u{1b}[31mb",
        "/3/a\nb",
        "/3/a\rb",
        "/3/\u{202e}txt.exe",
        "/3/a\u{200b}b",
        "/3/\u{e000}",
        "/3/\u{7f}",
        "/3/a\tb",
    ] {
        assert_eq!(path::parse(bad), Err(PathError::OddCharacters), "{bad:?}");
    }
    assert_eq!(
        path::parse(&format!("/3/{}", "a".repeat(256))),
        Err(PathError::TooLong)
    );
    assert_eq!(path::parse(&"a/".repeat(600)), Err(PathError::TooLong));
    // A drive letter, a home directory and a URL are just odd names in a place that has no such
    // thing; none leaves the tree.
    for odd in ["C:", "~", "file:///etc/passwd", "//server/share"] {
        let parsed = path::parse(&format!("/3/{odd}"));
        assert!(
            matches!(parsed, Ok(VPath::AreaEntry(_, Entry::Name(_))) | Err(_)),
            "{odd}"
        );
    }
}

#[test]
fn realpath_resolves_dots_inside_the_tree_and_never_above_it() {
    assert_eq!(path::canonical(".").unwrap(), "/");
    assert_eq!(path::canonical("/3-docs/..").unwrap(), "/");
    assert_eq!(
        path::canonical("/3-docs/12-x/../13-y").unwrap(),
        "/3-docs/13-y"
    );
    assert_eq!(path::canonical("/../../..").unwrap(), "/");
    assert_eq!(path::canonical("3").unwrap(), "/3");
    assert_eq!(path::canonical("/nonsense"), Err(PathError::NoSuchPlace));
    assert_eq!(path::canonical("/3/\u{1b}"), Err(PathError::OddCharacters));
}

#[test]
fn listed_names_round_trip_through_the_parser() {
    let dir = path::area_dir_name(area(7), "Docs & Manuals!");
    assert_eq!(dir, "7-docs-manuals");
    assert_eq!(path::area_dir_name(area(8), "!!!"), "8");
    let name = path::file_entry_name(file(41), "Router manual (v2).pdf");
    assert_eq!(
        path::parse(&format!("/{dir}/{name}")).unwrap(),
        VPath::AreaEntry(area(7), Entry::Id(file(41)))
    );
    assert_eq!(
        path::download_path(area(7), "Docs & Manuals!", file(41), "a.txt"),
        "/7-docs-manuals/41-a.txt"
    );
    let long = path::file_entry_name(file(1), &"é".repeat(300));
    assert!(long.len() <= path::MAX_COMPONENT_BYTES);
}

proptest! {
    /// Whatever bytes a client sends, the parser answers, never panics, and anything it accepts
    /// is made only of ids and names that survive the shared sanitizer unchanged.
    #[test]
    fn the_path_parser_never_panics_and_accepts_only_clean_places(raw in "\\PC{0,200}") {
        if let Ok(parsed) = path::parse(&raw) {
            for name in names_in(&parsed) {
                let cleaned = crate::sanitize::SingleLine::new(255).sanitize(&name);
                prop_assert_eq!(cleaned.as_str(), name.as_str());
                prop_assert!(!name.contains('/') && !name.contains('\\') && name != ".." && name != ".");
            }
        }
        let _ = path::canonical(&raw);
    }

    /// Paths built from path-like pieces (separators, dots, digits, a few odd characters) are
    /// never accepted with a `..` in them, and `canonical` always returns something `parse`
    /// accepts or an error.
    #[test]
    fn dotted_paths_are_refused_or_normalized_safely(
        pieces in prop::collection::vec(
            prop::sample::select(vec![
                "..", ".", "", "3", "3-docs", "12-x", "pending", "downloads", "a b", "\\", "%2e%2e",
                "\u{202e}", "~", "x\0y",
            ]),
            0..8,
        ),
        lead in prop::bool::ANY,
    ) {
        let raw = format!("{}{}", if lead { "/" } else { "" }, pieces.join("/"));
        if pieces.contains(&"..") && path::parse(&raw).is_ok() {
            prop_assert!(false, "accepted a path with .. in it: {raw:?}");
        }
        if let Ok(canonical) = path::canonical(&raw) {
            prop_assert!(canonical.starts_with('/'));
            prop_assert!(!canonical.contains(".."));
            prop_assert!(path::parse(&canonical).is_ok());
        }
    }
}

fn names_in(path: &VPath) -> Vec<String> {
    let entry = |e: &Entry| match e {
        Entry::Name(n) => vec![n.clone()],
        Entry::Id(_) => Vec::new(),
    };
    match path {
        VPath::AreaEntry(_, e) | VPath::PendingEntry(_, e) | VPath::DownloadEntry(e) => entry(e),
        VPath::Root | VPath::Area(_) | VPath::Pending(_) | VPath::Downloads => Vec::new(),
    }
}

#[test]
fn a_file_name_is_cleaned_for_display_and_kept_as_sent_for_data() {
    let name = text::prepare_file_name("report  final.txt").unwrap();
    assert_eq!(name.original, "report  final.txt");
    // Anything the sanitizer would change is cleaned in the display form only.
    let name = text::prepare_file_name("a\u{1b}[31mb\u{202e}.exe").unwrap();
    assert!(!name.display.as_str().contains('\u{1b}'));
    assert!(!name.display.as_str().contains('\u{202e}'));
    assert_eq!(name.original, "a\u{1b}[31mb\u{202e}.exe");
    // Separators cannot survive into a name.
    assert!(
        !text::prepare_file_name("a/b\\c")
            .unwrap()
            .display
            .as_str()
            .contains(['/', '\\'])
    );
    assert_eq!(text::prepare_file_name("   "), Err(TextError::Empty));
    assert_eq!(
        text::prepare_file_name("\u{1b}\u{7f}"),
        Err(TextError::Empty)
    );
    assert!(matches!(
        text::prepare_file_name(&"x".repeat(256)),
        Err(TextError::TooLong { .. })
    ));
    assert!(matches!(
        text::prepare_file_name(&"x".repeat(121)),
        Err(TextError::TooLong { .. })
    ));
    assert!(text::prepare_file_name(&"x".repeat(120)).is_ok());
}

#[test]
fn tags_are_refused_not_rewritten() {
    assert_eq!(text::prepare_tag("  Hardware ").unwrap(), "hardware");
    assert_eq!(text::prepare_tag("a_b-9").unwrap(), "a_b-9");
    assert_eq!(text::prepare_tag(""), Err(TextError::Empty));
    for bad in ["two words", "ünï", "a/b", "a\u{1b}", "x'y", "a.b"] {
        assert_eq!(text::prepare_tag(bad), Err(TextError::BadTag), "{bad:?}");
    }
    assert!(matches!(
        text::prepare_tag(&"a".repeat(25)),
        Err(TextError::TooLong { .. })
    ));
}
