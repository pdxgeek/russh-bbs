#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Theme packs (UI-7): every shipped style survives an export and an import, hostile packs are
//! refused with a reason, an older theme file migrates forward, and an install never overwrites.

use std::fs;

use super::*;
use crate::ui::Ui;
use crate::ui::strings::StringTable;

const STYLES: [&str; 9] = [
    "phosphor",
    "neon",
    "paper",
    "amber",
    "deepspace",
    "ansi",
    "green",
    "contrast",
    "amber",
];

struct World {
    base: StringTable,
    themes: ThemeSet,
    art: ArtSet,
}

fn world() -> World {
    let ui = Ui::standard().unwrap();
    let base = ui.strings().clone();
    World {
        themes: ThemeSet::shipped(&base).unwrap(),
        art: ArtSet::bundled().unwrap(),
        base,
    }
}

/// A board that has only the built-in style, to import into.
fn empty_board(base: &StringTable) -> ThemeSet {
    ThemeSet {
        themes: vec![Theme::builtin(base.clone())],
    }
}

fn name(text: &str) -> ThemeName {
    ThemeName::new(text).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// A minimal valid pack for a style called `harbour`.
fn pack_text(extra: &str) -> String {
    format!("pack = 1\ntheme = '''\nname = \"harbour\"\n[colors]\nborder = \"cyan\"\n'''\n{extra}")
}

#[test]
fn every_shipped_style_round_trips_through_a_pack() {
    let w = world();
    for style in STYLES {
        let id = name(style);
        let text = export(&w.themes, &w.art, id).unwrap();
        assert!(text.len() < MAX_PACK_BYTES, "{style}: {} bytes", text.len());
        let pack = parse(&text, &w.base).unwrap_or_else(|e| panic!("{style}: {e}"));
        let original = w.themes.get(id);
        let got = pack.theme();
        assert_eq!(got.name(), original.name(), "{style}");
        assert_eq!(got.palette(), original.palette(), "{style}");
        assert_eq!(got.skin(), original.skin(), "{style}");
        assert_eq!(got.strings(), original.strings(), "{style}");
        assert_eq!(got.title(), original.title(), "{style}");

        // Install into a fresh board and load it the way the server does at boot.
        let dir = tempfile::tempdir().unwrap();
        let (themes_dir, art_dir) = (dir.path().join("themes"), dir.path().join("art"));
        install(&pack, &empty_board(&w.base), &themes_dir, &art_dir).unwrap();
        let mut themes = empty_board(&w.base);
        assert_eq!(themes.add_dir(&themes_dir, &w.base), 1, "{style}");
        assert_eq!(themes.get(id).palette(), original.palette(), "{style}");
        let mut art = ArtSet::default();
        art.add_dir(&art_dir);
        assert!(art.problems().is_empty(), "{style}: {:?}", art.problems());
        let mine = |set: &ArtSet| {
            set.pieces()
                .iter()
                .filter(|p| p.style() == Some(id))
                .map(|p| (p.slot(), p.grid().clone(), p.fallback().map(str::to_owned)))
                .collect::<Vec<_>>()
        };
        let mut before = mine(&w.art);
        let mut after = mine(&art);
        before.sort_by_key(|p| p.0.name());
        after.sort_by_key(|p| p.0.name());
        assert_eq!(after, before, "{style}: art differs");
        // Six of the nine styles ship art.
        if ["phosphor", "neon", "paper", "amber", "deepspace", "ansi"].contains(&style) {
            assert!(!after.is_empty(), "{style} lost its art");
        }
        // And the exported pack is itself stable: exporting what was imported gives the same file.
        let again = export(&themes, &art, id).unwrap();
        assert_eq!(again, text, "{style}: second export differs");
    }
}

#[test]
fn the_built_in_style_has_no_file_to_export() {
    let w = world();
    assert_eq!(
        export(&w.themes, &w.art, ThemeName::default_theme()),
        Err(PackError::NotExportable("default".to_owned()))
    );
    assert_eq!(
        export(&w.themes, &w.art, name("nope")),
        Err(PackError::Unknown("nope".to_owned()))
    );
}

#[test]
fn an_export_holds_only_the_styles_own_files() {
    let w = world();
    let text = export(&w.themes, &w.art, name("neon")).unwrap();
    // No board setting, path, address or secret-shaped word comes from the style.
    for word in [
        "password",
        "token",
        "secret",
        "/home",
        "/etc",
        "ssh-ed25519",
        "BEGIN",
    ] {
        assert!(
            !text.to_lowercase().contains(&word.to_lowercase()),
            "{word}"
        );
    }
    // The board name override is applied to loaded tables, never to the authored file.
    let ui = Ui::standard().unwrap().with_board(
        crate::ui::Board::from_config(&crate::config::BoardConfig {
            name: "Zebra Harbour BBS".into(),
            ..crate::config::BoardConfig::default()
        })
        .unwrap(),
    );
    let ui_text = export(&ui.themes, ui.art(), name("neon")).unwrap();
    assert!(!ui_text.contains("Zebra"));
}

#[test]
fn a_hand_edited_pack_imports() {
    let w = world();
    let text = export(&w.themes, &w.art, name("neon"))
        .unwrap()
        .replace("name = \"neon\"", "name = \"neon2\"")
        .replace("style = \"neon\"", "style = \"neon2\"");
    let pack = parse(&text, &w.base).unwrap();
    assert_eq!(pack.theme().name(), name("neon2"));
    assert!(pack.art_files() > 0);
}

// ---- hostile packs -------------------------------------------------------------------------

fn refused(text: &str) -> PackError {
    parse(text, &world().base).expect_err("this pack must be refused")
}

#[test]
fn an_oversized_pack_is_refused_before_it_is_read() {
    let big = format!("pack = 1\ntheme = ''\n# {}\n", "x".repeat(MAX_PACK_BYTES));
    assert!(matches!(refused(&big), PackError::TooLarge(_)));
}

#[test]
fn a_wrong_version_is_refused() {
    for version in ["0", "2", "99"] {
        let text = pack_text("").replace("pack = 1", &format!("pack = {version}"));
        assert!(matches!(refused(&text), PackError::Version(_)), "{version}");
    }
    assert!(matches!(refused("theme = ''\n"), PackError::Syntax(_)));
}

#[test]
fn unknown_fields_are_refused_at_every_level() {
    assert!(matches!(
        refused(&pack_text("surprise = 1\n")),
        PackError::Syntax(_)
    ));
    let art = "[[art]]\nkind = \"art.toml\"\ntext = 'x'\nrun = \"rm -rf /\"\n";
    assert!(matches!(refused(&pack_text(art)), PackError::Syntax(_)));
    // And inside the theme file: the theme schema refuses them.
    let text = "pack = 1\ntheme = '''\nname = \"x\"\n[exec]\ncmd = \"sh\"\n'''\n";
    assert!(matches!(refused(text), PackError::Theme(_)));
}

#[test]
fn a_pack_cannot_name_the_reserved_style_or_a_marker() {
    // There is no key for an attribute, a marker or its position: each spelling is an unknown
    // key, and a refusal.
    for body in [
        "[style.marker]\nreverse = true\n",
        "[style.borders]\nmarker = \"plain\"\n",
        "[colors]\nmarker = \"red\"\n",
        "[colors]\nreverse = \"red\"\n",
        "[style.title]\nmodifier = \"bold reverse\"\n",
        "[marker]\nposition = \"body\"\n",
    ] {
        let text = format!("pack = 1\ntheme = '''\nname = \"x\"\n{body}'''\n");
        assert!(
            matches!(refused(&text), PackError::Theme(_)),
            "{body} was not refused"
        );
    }
    // The status line's caption keys are reserved.
    let text = "pack = 1\ntheme = '''\nname = \"x\"\n[status]\nunavailable = \"fine\"\n'''\n";
    assert!(matches!(refused(text), PackError::Theme(_)));
}

#[test]
fn marker_glyphs_control_characters_and_private_use_in_strings_are_refused() {
    let key = "title";
    for bad in [
        r"\u27E6\u2299\u27E7 Hi",
        r"\u2299",
        r"\u001B[31mred",
        r"bell\u0007",
        r"\uE0A0 branch",
        r"zero\u200Bwidth",
    ] {
        let text = format!("pack = 1\ntheme = '''\nname = \"x\"\n{key} = \"{bad}\"\n'''\n");
        let err = refused(&text);
        assert!(
            matches!(err, PackError::Theme(_) | PackError::Syntax(_)),
            "{bad}: {err}"
        );
    }
}

#[test]
fn a_string_cannot_claim_privacy() {
    let w = world();
    let key = w
        .base
        .keys()
        .find(|k| k.ends_with("title"))
        .unwrap()
        .to_owned();
    let (section, leaf) = key.rsplit_once('.').unwrap();
    let text = format!(
        "pack = 1\ntheme = '''\nname = \"x\"\n[{section}]\n{leaf} = \"A private board\"\n'''\n"
    );
    assert!(matches!(
        refused(&text),
        PackError::Theme(ThemeError::Strings(_))
    ));
}

#[test]
fn art_must_belong_to_the_pack_style() {
    // A generic piece (no style) would replace the board's art for every style.
    let generic = "format = 1\nname = \"harbour\"\n[[piece]]\nslot = \"menu\"\nrows = [\"hi\"]\n";
    let entry = format!("[[art]]\nkind = \"art.toml\"\ntext = '''\n{generic}'''\n");
    assert!(matches!(
        refused(&pack_text(&entry)),
        PackError::Art { entry: 1, .. }
    ));
    let other = generic.replace("name = \"harbour\"", "name = \"harbour\"\nstyle = \"neon\"");
    let entry = format!("[[art]]\nkind = \"art.toml\"\ntext = '''\n{other}'''\n");
    assert!(matches!(
        refused(&pack_text(&entry)),
        PackError::Art { entry: 1, .. }
    ));
    let mine = generic.replace(
        "name = \"harbour\"",
        "name = \"harbour\"\nstyle = \"harbour\"",
    );
    let entry = format!("[[art]]\nkind = \"art.toml\"\ntext = '''\n{mine}'''\n");
    assert_eq!(
        parse(&pack_text(&entry), &world().base)
            .unwrap()
            .art_files(),
        1
    );
}

#[test]
fn art_marker_glyphs_become_replacement_characters() {
    let art = "format = 1\nname = \"harbour\"\nstyle = \"harbour\"\n[[piece]]\nslot = \"menu\"\nrows = [\"\u{27E6}\u{2299}\u{27E7} \u{E0A0} \\u001B\"]\n";
    let entry = format!("[[art]]\nkind = \"art.toml\"\ntext = '''\n{art}'''\n");
    // The pack parses (art is cleaned, not refused); read the piece back as the loader does.
    let w = world();
    let pack = parse(&pack_text(&entry), &w.base).unwrap();
    let dir = tempfile::tempdir().unwrap();
    install(
        &pack,
        &empty_board(&w.base),
        &dir.path().join("t"),
        &dir.path().join("a"),
    )
    .unwrap();
    let mut set = ArtSet::default();
    set.add_dir(&dir.path().join("a"));
    let row = set.pieces()[0].grid().row_text(0, false);
    for reserved in russh_bbs_core::sanitize::RESERVED_CHROME {
        assert!(!row.contains(*reserved), "{row:?}");
    }
    assert!(row.chars().all(|c| !c.is_control()));
}

#[test]
fn an_ans_bomb_is_refused() {
    // More hex than the cap: refused before it is decoded.
    let huge = "ff".repeat(ANS_MAX_BYTES + 1);
    let entry = format!("[[art]]\nkind = \"ans\"\nslot = \"menu\"\nhex = \"{huge}\"\n");
    assert!(matches!(
        refused(&pack_text(&entry)),
        PackError::Art { entry: 1, .. }
    ));
    // Within the byte cap but a screen of cursor-down: the picture is over the row cap.
    let tall = "\x1b[100B".repeat(40) + "x";
    let entry = format!(
        "[[art]]\nkind = \"ans\"\nslot = \"menu\"\nhex = \"{}\"\n",
        hex(tall.as_bytes())
    );
    let err = refused(&pack_text(&entry));
    assert!(matches!(err, PackError::Art { entry: 1, .. }), "{err}");
    // Odd or non-hex digits.
    for bad in ["abc", "zz", "12 3"] {
        let entry = format!("[[art]]\nkind = \"ans\"\nslot = \"menu\"\nhex = \"{bad}\"\n");
        assert!(
            matches!(refused(&pack_text(&entry)), PackError::Art { .. }),
            "{bad}"
        );
    }
    // Too many entries.
    let many =
        "[[art]]\nkind = \"ans\"\nslot = \"menu\"\nhex = \"41\"\n".repeat(MAX_ART_ENTRIES + 1);
    assert_eq!(refused(&pack_text(&many)), PackError::TooManyArt);
}

#[test]
fn a_valid_ans_is_cleaned_and_installed_under_the_style() {
    let w = world();
    // Colour, a cursor move, and an escape that must never reach a terminal.
    let ans = b"\x1b[1;36mHARBOUR\x1b[0m\r\n\x1b]0;title\x07done";
    let entry = format!(
        "[[art]]\nkind = \"ans\"\nslot = \"menu\"\nhex = \"{}\"\n",
        hex(ans)
    );
    let pack = parse(&pack_text(&entry), &w.base).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (t, a) = (dir.path().join("t"), dir.path().join("a"));
    install(&pack, &empty_board(&w.base), &t, &a).unwrap();
    assert!(a.join("harbour.menu.ans").is_file());
    let mut set = ArtSet::default();
    set.add_dir(&a);
    assert_eq!(set.pieces().len(), 1);
    assert_eq!(set.pieces()[0].style(), Some(name("harbour")));
    let text: String = (0..set.pieces()[0].grid().height())
        .map(|y| set.pieces()[0].grid().row_text(y, false))
        .collect();
    assert!(
        !text.contains('\x1b') && !text.contains("title"),
        "{text:?}"
    );
}

#[test]
fn a_second_ans_for_a_slot_and_a_bad_kind_are_refused() {
    let one = "[[art]]\nkind = \"ans\"\nslot = \"menu\"\nhex = \"41\"\n";
    assert!(matches!(
        refused(&pack_text(&one.repeat(2))),
        PackError::Art { entry: 2, .. }
    ));
    let bad = "[[art]]\nkind = \"script\"\ntext = \"x\"\n";
    assert!(matches!(
        refused(&pack_text(bad)),
        PackError::Art { entry: 1, .. }
    ));
}

// ---- versions ------------------------------------------------------------------------------

/// A theme file as UI-2 wrote them: no `version`.
const OLD_THEME: &str = "name = \"oldie\"\n[colors]\nheading = \"magenta\"\nborder = \"blue\"\n";

#[test]
fn an_old_theme_file_migrates_forward() {
    let w = world();
    // A bare file is version 1 and loads.
    let theme = Theme::parse(OLD_THEME, &w.base).unwrap();
    assert_eq!(theme.name(), name("oldie"));
    // An export writes the current version in front of the authored text.
    let exported = theme.export_text().unwrap();
    assert!(exported.starts_with("version = 2\n"), "{exported}");
    // The migrated text is the same theme, and is stable from then on.
    let again = Theme::parse(&exported, &w.base).unwrap();
    assert_eq!(again.palette(), theme.palette());
    assert_eq!(again.export_text().unwrap(), exported);
    // As a pack: an old file inside a pack is migrated and installed in the new version.
    let text = format!("pack = 1\ntheme = '''\n{OLD_THEME}'''\n");
    let pack = parse(&text, &w.base).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (t, a) = (dir.path().join("t"), dir.path().join("a"));
    install(&pack, &empty_board(&w.base), &t, &a).unwrap();
    let written = fs::read_to_string(t.join("oldie.toml")).unwrap();
    assert!(written.starts_with("version = 2\n"));
}

#[test]
fn every_shipped_file_still_loads_as_version_one_and_the_meta_is_read() {
    let w = world();
    for style in STYLES {
        let theme = w.themes.get(name(style));
        assert!(!theme.title().is_empty(), "{style}");
    }
    let with_meta = "version = 2\nname = \"m\"\n[meta]\ntitle = \"Mauve Hour\"\ncredit = \"Jo\"\n";
    let theme = Theme::parse(with_meta, &w.base).unwrap();
    assert_eq!((theme.title(), theme.credit()), ("Mauve Hour", "Jo"));
}

#[test]
fn a_version_this_server_does_not_read_is_refused() {
    let w = world();
    for version in ["0", "3", "-1", "\"2\"", "2.5", "true"] {
        let text = format!("version = {version}\nname = \"v\"\n");
        let err = Theme::parse(&text, &w.base).unwrap_err();
        assert!(matches!(err, ThemeError::Version(_)), "{version}: {err}");
    }
}

#[test]
fn meta_is_checked_like_any_sysop_string() {
    let w = world();
    for meta in [
        "[meta]\ntitle = \"A private style\"\n",
        "[meta]\ntitle = \"\\u2299\"\n",
        "[meta]\ntitle = \"x\"\nrun = \"y\"\n",
        "[meta]\ncredit = 5\n",
    ] {
        let text = format!("name = \"m\"\n{meta}");
        assert!(
            matches!(
                Theme::parse(&text, &w.base).unwrap_err(),
                ThemeError::Meta(_)
            ),
            "{meta}"
        );
    }
    let long = format!("name = \"m\"\n[meta]\ntitle = \"{}\"\n", "x".repeat(41));
    assert!(matches!(
        Theme::parse(&long, &w.base).unwrap_err(),
        ThemeError::Meta(_)
    ));
}

// ---- install -------------------------------------------------------------------------------

#[test]
fn install_never_overwrites_and_is_all_or_nothing() {
    let w = world();
    let pack = parse(&pack_text(""), &w.base).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (t, a) = (dir.path().join("t"), dir.path().join("a"));
    // A style that is already installed.
    let shipped = parse(&export(&w.themes, &w.art, name("neon")).unwrap(), &w.base).unwrap();
    assert_eq!(
        install(&shipped, &w.themes, &t, &a),
        Err(PackError::Exists("neon".to_owned()))
    );
    assert!(!t.exists(), "a refused install writes nothing");
    // A leftover file of the same name is not replaced.
    fs::create_dir_all(&t).unwrap();
    fs::write(t.join("harbour.toml"), "keep me").unwrap();
    assert_eq!(
        install(&pack, &empty_board(&w.base), &t, &a),
        Err(PackError::Exists("harbour".to_owned()))
    );
    assert_eq!(
        fs::read_to_string(t.join("harbour.toml")).unwrap(),
        "keep me"
    );
    // A failure after the art is written removes the art again.
    fs::remove_file(t.join("harbour.toml")).unwrap();
    let art = "format = 1\nname = \"harbour\"\nstyle = \"harbour\"\n[[piece]]\nslot = \"menu\"\nrows = [\"hi\"]\n";
    let entry = format!("[[art]]\nkind = \"art.toml\"\ntext = '''\n{art}'''\n");
    let with_art = parse(&pack_text(&entry), &w.base).unwrap();
    fs::create_dir_all(t.join("harbour.toml")).unwrap(); // a directory in the way
    assert!(install(&with_art, &empty_board(&w.base), &t, &a).is_err());
    assert_eq!(
        fs::read_dir(&a).map_or(0, Iterator::count),
        0,
        "art was left behind"
    );
}

#[test]
fn install_stops_at_the_loaders_file_caps() {
    let w = world();
    let pack = parse(&pack_text(""), &w.base).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (t, a) = (dir.path().join("t"), dir.path().join("a"));
    fs::create_dir_all(&t).unwrap();
    for i in 0..MAX_THEME_FILES {
        fs::write(t.join(format!("f{i}.toml")), "").unwrap();
    }
    assert_eq!(
        install(&pack, &empty_board(&w.base), &t, &a),
        Err(PackError::Full("themes"))
    );
}

#[test]
fn the_packs_directory_lists_good_and_refused_files() {
    let w = world();
    let dir = tempfile::tempdir().unwrap();
    let good = export(&w.themes, &w.art, name("paper")).unwrap();
    write_export(dir.path(), name("paper"), &good).unwrap();
    fs::write(dir.path().join("bad.pack.toml"), "pack = 7\ntheme = ''\n").unwrap();
    fs::write(dir.path().join("note.txt"), "ignored").unwrap();
    let listed = list_dir(dir.path(), &w.base);
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].file, "bad.pack.toml");
    assert!(listed[0].verdict.is_err());
    assert_eq!(listed[1].verdict, Ok(name("paper")));
    // An export never replaces a file.
    assert!(write_export(dir.path(), name("paper"), &good).is_err());
    // A link is not followed.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/dev/zero", dir.path().join("zero.pack.toml")).unwrap();
        let listed = list_dir(dir.path(), &w.base);
        let zero = listed.iter().find(|f| f.file == "zero.pack.toml").unwrap();
        assert!(zero.verdict.is_err());
    }
}
