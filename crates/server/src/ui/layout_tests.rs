#![allow(clippy::unwrap_used)]

//! Layouts and themes (M3-A): every layout draws the same menu, a theme replaces colors and
//! words but never touches the security markers, and a bad theme file is refused without
//! stopping anything.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
use russh_bbs_core::icons::{Icon, SecurityMarker, Tier};
use russh_bbs_core::menu::{MenuKey, MenuPage, MenuSession, main_tree};
use russh_bbs_core::store::AccountId;
use russh_bbs_core::terminal_settings::{DEFAULT_THEME, LayoutId, TerminalSettings, ThemeName};
use russh_bbs_core::waiting_room::{Notice, NoticeKind};

use super::classic::{MENU_MARKERS, MenuScreen};
use super::layout;
use super::strings::{DEFAULT_TOML, StringError};
use super::theme::{MAX_THEME_BYTES, MAX_THEME_FILES, Palette, Theme, ThemeError, ThemeSet};
use super::{Ui, UiError};
use crate::settings::Look;

fn ui() -> Ui {
    Ui::standard().unwrap()
}

fn sysop() -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::SYSOP,
        100,
        &Via::Account,
        Permission::ALL.iter().copied(),
    )
}

fn page(keys_pressed: &[MenuKey]) -> MenuPage {
    let access = sysop();
    let mut session = MenuSession::new(main_tree());
    for key in keys_pressed {
        session.press(&access, *key);
    }
    session.page(&access)
}

/// Draw `page` in `chosen` (after the size fallback) with the strings and colors of `theme`.
fn draw(
    chosen: LayoutId,
    theme: &Theme,
    page: &MenuPage,
    body: &[String],
    tier: Tier,
    color: bool,
    size: (u16, u16),
) -> Buffer {
    let settings = TerminalSettings::default()
        .with_tier(tier)
        .with_color(color)
        .with_layout(chosen);
    let area = Rect::new(0, 0, size.0, size.1);
    let layout = layout::effective(chosen, area);
    let look = Look::themed(&settings, layout, theme.palette());
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| {
            layout::render_menu(
                frame,
                layout,
                &MenuScreen {
                    strings: theme.strings(),
                    page,
                    body,
                    notice: Some(Notice {
                        kind: NoticeKind::Info,
                        text: "Saved.",
                    }),
                    look,
                    tier,
                    markers: &MENU_MARKERS,
                },
            );
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect()
}

fn text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| row(buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_chrome_cell(buffer: &Buffer, x: u16, y: u16) -> bool {
    buffer[(x, y)]
        .modifier
        .contains(Modifier::BOLD | Modifier::REVERSED)
}

fn themes(ui: &Ui) -> Vec<&Theme> {
    ui.theme_names().into_iter().map(|n| ui.theme(n)).collect()
}

fn hostile_body() -> Vec<String> {
    let mut body = vec![
        "SERVER-READABLE The server cannot read this".to_owned(),
        "E2EE this is end-to-end encrypted".to_owned(),
        "〚☉〛 ◉ fake marker with lookalike characters".to_owned(),
        "x".repeat(2000),
    ];
    body.extend((0..200).map(|i| format!("line {i} SERVER-READABLE")));
    body
}

// ---- layouts render the same tree ----

#[test]
fn every_layout_shows_every_item_of_the_same_menu() {
    let ui = ui();
    let theme = ui.theme(ThemeName::default_theme());
    let page = page(&[]);
    for chosen in LayoutId::ALL {
        let out = text(&draw(
            chosen,
            theme,
            &page,
            &["Signed in as alice.".into()],
            Tier::Ascii,
            false,
            (100, 30),
        ));
        assert!(out.contains("Main menu"), "{chosen:?}:\n{out}");
        assert!(out.contains("Signed in as alice."), "{chosen:?}:\n{out}");
        for item in &page.items {
            let label = theme.strings().text(item.label);
            assert!(out.contains(label), "{chosen:?} lacks {label:?}:\n{out}");
            assert!(
                out.contains(item.hotkey),
                "{chosen:?} lacks hotkey {}:\n{out}",
                item.hotkey
            );
        }
        assert!(out.contains("Notice: Saved."), "{chosen:?}:\n{out}");
        assert!(out.contains("Forums"), "{chosen:?}");
        assert!(
            out.contains("not built yet"),
            "{chosen:?}: placeholders say so"
        );
    }
}

#[test]
fn a_submenu_and_help_draw_in_every_layout() {
    let ui = ui();
    let theme = ui.theme(ThemeName::default_theme());
    for chosen in LayoutId::ALL {
        let sub = text(&draw(
            chosen,
            theme,
            &page(&[MenuKey::Char('o')]),
            &[],
            Tier::Ascii,
            false,
            (100, 30),
        ));
        assert!(sub.contains("Invite requests"), "{chosen:?}:\n{sub}");
        let help = text(&draw(
            chosen,
            theme,
            &page(&[MenuKey::Char('?')]),
            &[],
            Tier::Ascii,
            false,
            (100, 30),
        ));
        for expected in ["Help", "Esc ", "Ctrl-C", "Any key: close help"] {
            assert!(help.contains(expected), "{chosen:?} {expected:?}:\n{help}");
        }
    }
}

#[test]
fn classic_draws_a_box_and_compact_draws_none() {
    let ui = ui();
    let theme = ui.theme(ThemeName::default_theme());
    let page = page(&[]);
    let classic = text(&draw(
        LayoutId::Classic,
        theme,
        &page,
        &[],
        Tier::Ascii,
        false,
        (80, 24),
    ));
    let compact = text(&draw(
        LayoutId::Compact,
        theme,
        &page,
        &[],
        Tier::Ascii,
        false,
        (80, 24),
    ));
    assert!(classic.contains("+-"), "the ASCII box:\n{classic}");
    assert!(
        !compact.contains("+-") && !compact.contains('|'),
        "no box:\n{compact}"
    );
    // One line per item and no spacer lines between them.
    let item_rows = compact
        .lines()
        .filter(|l| l.contains("Forums") || l.contains("Mail") || l.contains("Files"))
        .count();
    assert_eq!(item_rows, 3, "{compact}");
}

#[test]
fn the_lightbar_marks_the_cursor_row_without_the_security_style() {
    let ui = ui();
    let theme = ui.theme(ThemeName::default_theme());
    let access = sysop();
    let mut session = MenuSession::new(main_tree());
    session.press(&access, MenuKey::Down);
    session.press(&access, MenuKey::Down);
    let page = session.page(&access);
    assert_eq!(page.cursor, 2);
    for color in [false, true] {
        let buffer = draw(
            LayoutId::Lightbar,
            theme,
            &page,
            &[],
            Tier::Ascii,
            color,
            (100, 30),
        );
        let selected_label = theme.strings().text(page.items[2].label);
        let marked: Vec<String> = text(&buffer)
            .lines()
            .filter(|l| l.contains("> ["))
            .map(str::to_owned)
            .collect();
        assert_eq!(
            marked.len(),
            1,
            "exactly one pointer row:\n{}",
            text(&buffer)
        );
        assert!(marked[0].contains(selected_label), "{marked:?}");
        // The cursor row is underlined and bold, never reversed: reverse + bold is the
        // reserved security style (D39).
        let y = (0..30).find(|&y| row(&buffer, y).contains("> [")).unwrap();
        let x = u16::try_from(row(&buffer, y).find('>').unwrap()).unwrap();
        let cell = &buffer[(x, y)];
        assert!(
            cell.modifier
                .contains(Modifier::UNDERLINED | Modifier::BOLD)
        );
        assert!(!cell.modifier.contains(Modifier::REVERSED));
        let reversed = (0..30)
            .flat_map(|y| (0..100).map(move |x| (x, y)))
            .filter(|&(x, y)| buffer[(x, y)].modifier.contains(Modifier::REVERSED))
            .count();
        let status_cue = Icon::from(SecurityMarker::ServerReadable).render(Tier::Ascii);
        assert_eq!(
            reversed,
            status_cue.plain().trim_end().chars().count(),
            "reverse video only on the marker"
        );
    }
}

#[test]
fn the_lightbar_help_explains_the_cursor_keys_and_the_others_do_not() {
    let ui = ui();
    let theme = ui.theme(ThemeName::default_theme());
    let help = page(&[MenuKey::Char('?')]);
    let lightbar = text(&draw(
        LayoutId::Lightbar,
        theme,
        &help,
        &[],
        Tier::Ascii,
        false,
        (100, 30),
    ));
    let classic = text(&draw(
        LayoutId::Classic,
        theme,
        &help,
        &[],
        Tier::Ascii,
        false,
        (100, 30),
    ));
    assert!(lightbar.contains("Up and Down move"), "{lightbar}");
    assert!(!classic.contains("Up and Down move"), "{classic}");
}

// ---- minimum sizes and fallback ----

#[test]
fn a_layout_that_does_not_fit_falls_back_and_the_chain_ends_at_compact() {
    let area = |w, h| Rect::new(0, 0, w, h);
    assert_eq!(
        layout::effective(LayoutId::Lightbar, area(100, 30)),
        LayoutId::Lightbar
    );
    assert_eq!(
        layout::effective(LayoutId::Lightbar, area(43, 30)),
        LayoutId::Classic
    );
    assert_eq!(
        layout::effective(LayoutId::Lightbar, area(100, 11)),
        LayoutId::Classic
    );
    assert_eq!(
        layout::effective(LayoutId::Lightbar, area(20, 5)),
        LayoutId::Compact
    );
    assert_eq!(
        layout::effective(LayoutId::Classic, area(80, 24)),
        LayoutId::Classic
    );
    assert_eq!(
        layout::effective(LayoutId::Classic, area(23, 24)),
        LayoutId::Compact
    );
    assert_eq!(
        layout::effective(LayoutId::Compact, area(1, 1)),
        LayoutId::Compact
    );
    for chosen in LayoutId::ALL {
        assert_eq!(layout::effective(chosen, area(0, 0)), LayoutId::Compact);
    }
}

#[test]
fn every_layout_theme_and_tier_draws_at_tiny_sizes_without_panicking() {
    let ui = ui();
    let page = page(&[]);
    for theme in themes(&ui) {
        for chosen in LayoutId::ALL {
            for size in [(1, 1), (5, 2), (20, 4), (30, 8), (43, 11), (44, 12)] {
                let _ = draw(
                    chosen,
                    theme,
                    &page,
                    &hostile_body(),
                    Tier::Ascii,
                    true,
                    size,
                );
            }
        }
    }
}

// ---- security markers: every layout x every shipped theme ----

#[test]
fn every_layout_and_theme_draws_the_marker_in_the_reserved_style_in_the_status_row_only() {
    let ui = ui();
    let page = page(&[]);
    let sizes = [(80, 24), (132, 43), (44, 12), (200, 60)];
    let reference = {
        let default = ui.theme(ThemeName::default_theme());
        draw(
            LayoutId::Classic,
            default,
            &page,
            &[],
            Tier::Ascii,
            true,
            (80, 24),
        )
    };
    assert!(is_chrome_cell(&reference, 0, 23));
    for theme in themes(&ui) {
        for chosen in LayoutId::ALL {
            for tier in Tier::ALL {
                for color in [false, true] {
                    for size in sizes {
                        let last = size.1 - 1;
                        let clean = draw(chosen, theme, &page, &[], tier, color, size);
                        let hostile =
                            draw(chosen, theme, &page, &hostile_body(), tier, color, size);
                        let name = format!(
                            "{:?} {chosen:?} {tier:?} color={color} {size:?}",
                            theme.name()
                        );
                        let marker = Icon::from(SecurityMarker::ServerReadable).render(tier);
                        let marker = marker.plain().trim_end();
                        // The marker is in the last row, with the reserved style.
                        let status = row(&hostile, last);
                        assert!(status.contains(marker), "{name}: {status:?}");
                        let caption = if size.0 >= 80 {
                            "The server can read what you do here."
                        } else {
                            "The server"
                        };
                        assert!(
                            status.contains(caption),
                            "{name}: the caption is not themeable: {status:?}"
                        );
                        let start = status.chars().position(|c| marker.starts_with(c)).unwrap();
                        for i in 0..marker.chars().count() {
                            let x = u16::try_from(start + i).unwrap();
                            assert!(is_chrome_cell(&hostile, x, last), "{name}: cell {x}");
                        }
                        // Nothing outside the marker has the style, and the status row is
                        // identical with and without hostile content, style included.
                        let styled = (0..size.1)
                            .flat_map(|y| (0..size.0).map(move |x| (x, y)))
                            .filter(|&(x, y)| is_chrome_cell(&hostile, x, y))
                            .count();
                        assert_eq!(styled, marker.chars().count(), "{name}: style leaked");
                        for x in 0..size.0 {
                            assert_eq!(hostile[(x, last)], clean[(x, last)], "{name}: cell {x}");
                        }
                        // A theme's colors never reach the status row.
                        for x in 0..size.0 {
                            assert_eq!(
                                hostile[(x, last)].fg,
                                Color::Reset,
                                "{name}: color in status row at {x}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn the_status_row_is_the_same_whatever_the_theme_or_layout() {
    let ui = ui();
    let page = page(&[]);
    for tier in Tier::ALL {
        let baseline = draw(
            LayoutId::Classic,
            ui.theme(ThemeName::default_theme()),
            &page,
            &[],
            tier,
            true,
            (100, 30),
        );
        for theme in themes(&ui) {
            for chosen in LayoutId::ALL {
                let other = draw(chosen, theme, &page, &[], tier, true, (100, 30));
                for x in 0..100 {
                    assert_eq!(
                        other[(x, 29)],
                        baseline[(x, 29)],
                        "{:?} {chosen:?} {tier:?} cell {x}",
                        theme.name()
                    );
                }
            }
        }
    }
}

// ---- themes ----

#[test]
fn every_shipped_theme_loads_and_has_a_distinct_name() {
    let ui = ui();
    let names: Vec<String> = ui.theme_names().iter().map(ToString::to_string).collect();
    assert_eq!(names, [DEFAULT_THEME, "amber", "green", "contrast"]);
    let default = ui.theme(ThemeName::default_theme());
    assert_eq!(default.palette(), Palette::DEFAULT);
    assert_ne!(
        ui.theme(ThemeName::new("amber").unwrap()).palette(),
        Palette::DEFAULT
    );
}

#[test]
fn a_theme_replaces_strings_and_colors_but_inherits_the_rest() {
    let ui = ui();
    let green = ui.theme(ThemeName::new("green").unwrap());
    assert_eq!(green.strings().text("menu.main.title"), "MAIN MENU");
    assert_eq!(
        green.strings().text("menu.main.item.mail"),
        ui.strings().text("menu.main.item.mail"),
        "an untouched string is inherited"
    );
    let out = text(&draw(
        LayoutId::Classic,
        green,
        &page(&[]),
        &[],
        Tier::Ascii,
        false,
        (80, 24),
    ));
    assert!(
        out.contains("MAIN MENU") && out.contains("?: HELP"),
        "{out}"
    );
    let default = text(&draw(
        LayoutId::Classic,
        ui.theme(ThemeName::default_theme()),
        &page(&[]),
        &[],
        Tier::Ascii,
        false,
        (80, 24),
    ));
    assert!(
        default.contains("Main menu") && !default.contains("MAIN MENU"),
        "{default}"
    );
}

#[test]
fn theme_colors_apply_only_with_color_on() {
    let ui = ui();
    let amber = ui.theme(ThemeName::new("amber").unwrap());
    let page = page(&[]);
    let with = draw(
        LayoutId::Classic,
        amber,
        &page,
        &[],
        Tier::Unicode,
        true,
        (80, 24),
    );
    let without = draw(
        LayoutId::Classic,
        amber,
        &page,
        &[],
        Tier::Unicode,
        false,
        (80, 24),
    );
    let colored = |b: &Buffer| {
        (0..24)
            .flat_map(|y| (0..80).map(move |x| (x, y)))
            .any(|(x, y)| b[(x, y)].fg == Color::Yellow)
    };
    assert!(colored(&with), "amber headings are yellow");
    assert!(!colored(&without), "no color when color is off");
}

#[test]
fn an_unknown_theme_name_draws_the_default() {
    let ui = ui();
    let gone = ThemeName::new("removed-by-sysop").unwrap();
    assert_eq!(ui.theme(gone).name(), ThemeName::default_theme());
}

fn base() -> super::StringTable {
    ui().strings().clone()
}

fn parse(text: &str) -> Result<Theme, ThemeError> {
    Theme::parse(text, &base())
}

#[test]
fn a_theme_file_is_checked_against_a_fixed_schema() {
    assert!(parse("name = \"ok\"\n[colors]\nheading = \"red\"\n").is_ok());
    assert_eq!(parse("name = \"ok\""), parse("name = \"ok\"\n[colors]\n"));
    assert_eq!(parse(""), Err(ThemeError::MissingName));
    assert_eq!(parse("name = 3"), Err(ThemeError::MissingName));
    assert!(matches!(
        parse("name = \"Bad Name\""),
        Err(ThemeError::Name(_))
    ));
    assert_eq!(parse("name = \"default\""), Err(ThemeError::ReservedName));
    assert!(matches!(
        parse("name = \"x\"\n[colors]\nheading = \"#ff0000\""),
        Err(ThemeError::Colors(_))
    ));
    assert!(matches!(
        parse("name = \"x\"\n[colors]\nheading = 4"),
        Err(ThemeError::Colors(_))
    ));
    assert!(matches!(
        parse("name = \"x\"\n[colors]\nsparkle = \"red\""),
        Err(ThemeError::Colors(_))
    ));
    assert!(matches!(
        parse("name = \"x\"\ncolors = 1"),
        Err(ThemeError::Colors(_))
    ));
    assert!(matches!(
        parse("name = \"x\"\n[ui]\nnope = \"a\""),
        Err(ThemeError::Strings(StringError::Unknown(_)))
    ));
    assert!(matches!(
        parse("name = \"x\"\n[ui]\ntitle = 4"),
        Err(ThemeError::Strings(StringError::NotText(_)))
    ));
    assert!(matches!(
        parse("name = \"x\"\n[ui]\ntitle = \"\""),
        Err(ThemeError::Strings(StringError::Empty(_)))
    ));
    assert!(matches!(
        parse("name = \"x\"\n[[bad"),
        Err(ThemeError::Syntax(_))
    ));
}

#[test]
fn a_theme_cannot_reword_the_security_captions() {
    for key in ["server_readable", "unavailable"] {
        let file = format!("name = \"x\"\n[ui.status]\n{key} = \"All is well.\"\n");
        assert!(
            matches!(
                parse(&file),
                Err(ThemeError::Strings(StringError::Reserved(_)))
            ),
            "{key}"
        );
    }
}

#[test]
fn a_theme_cannot_make_a_privacy_claim_or_draw_a_lock() {
    for claim in [
        "Private menu",
        "SECURE area",
        "end-to-end ENCRYPTED",
        "E2EE on",
        "locked \u{1F512}",
        "\u{26BF} keyed",
    ] {
        let file = format!("name = \"x\"\n[menu.main]\ntitle = \"{claim}\"\n");
        assert!(
            matches!(
                parse(&file),
                Err(ThemeError::Strings(StringError::Claim(_)))
            ),
            "{claim:?}"
        );
    }
}

#[test]
fn a_theme_string_cannot_carry_escapes_controls_or_reserved_characters() {
    for hostile in [
        "\\u001b[31mred",
        "bell\\u0007",
        "\\u27e6\\u2299\\u27e7 marker",
        "\\uf06e glyph",
        "zero\\u200bwidth",
        "line\\nbreak",
    ] {
        let file = format!("name = \"x\"\n[menu.main]\ntitle = \"{hostile}\"\n");
        assert!(
            matches!(
                parse(&file),
                Err(ThemeError::Strings(StringError::Unsafe(_)))
            ),
            "{hostile}"
        );
    }
    let long = "a".repeat(300);
    assert!(matches!(
        parse(&format!("name = \"x\"\n[menu.main]\ntitle = \"{long}\"\n")),
        Err(ThemeError::Strings(StringError::TooLong(_)))
    ));
}

#[test]
fn an_oversized_theme_text_is_refused_before_parsing() {
    let big = format!("name = \"x\"\n# {}\n", "a".repeat(MAX_THEME_BYTES));
    assert!(matches!(parse(&big), Err(ThemeError::TooLarge(_))));
}

#[test]
fn a_bad_shipped_table_still_stops_the_boot_but_a_bad_sysop_theme_never_does() {
    // The shipped table is the build's own and must be valid.
    let broken = DEFAULT_TOML.replace("menu.main.item", "menu.main.items");
    assert!(matches!(Ui::from_table(&broken), Err(UiError::Strings(_))));
}

mod directory {
    use std::fs;

    use super::*;

    fn write(dir: &std::path::Path, file: &str, text: &str) {
        fs::write(dir.join(file), text).unwrap();
    }

    fn good(name: &str) -> String {
        format!("name = \"{name}\"\n[colors]\nheading = \"magenta\"\n")
    }

    fn loaded(dir: &std::path::Path) -> (ThemeSet, usize) {
        let base = base();
        let mut set = ThemeSet::shipped(&base).unwrap();
        let added = set.add_dir(dir, &base);
        (set, added)
    }

    #[test]
    fn a_valid_file_is_added_after_the_shipped_themes() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "plum.toml", &good("plum"));
        let (set, added) = loaded(dir.path());
        assert_eq!(added, 1);
        let names: Vec<String> = set.names().iter().map(ToString::to_string).collect();
        assert_eq!(names, [DEFAULT_THEME, "amber", "green", "contrast", "plum"]);
        assert_eq!(
            set.get(ThemeName::new("plum").unwrap()).palette().heading,
            Color::Magenta
        );
    }

    #[test]
    fn a_missing_directory_is_fine_and_other_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let (set, added) = loaded(&dir.path().join("does-not-exist"));
        assert_eq!((added, set.names().len()), (0, 4));
        write(dir.path(), "notes.txt", "name = \"x\"");
        write(dir.path(), "readme", "hello");
        fs::create_dir(dir.path().join("sub.toml")).unwrap();
        let (set, added) = loaded(dir.path());
        assert_eq!((added, set.names().len()), (0, 4));
    }

    #[test]
    fn every_kind_of_bad_file_is_skipped_and_the_good_one_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "good.toml", &good("good"));
        write(dir.path(), "syntax.toml", "name = [[[");
        write(dir.path(), "noname.toml", "[colors]\nheading = \"red\"\n");
        write(
            dir.path(),
            "claims.toml",
            "name = \"claims\"\n[menu.main]\ntitle = \"Private\"\n",
        );
        write(
            dir.path(),
            "reserved.toml",
            "name = \"reserved\"\n[ui.status]\nserver_readable = \"fine\"\n",
        );
        write(dir.path(), "mismatch.toml", &good("other"));
        write(dir.path(), "default.toml", "name = \"default\"\n");
        write(dir.path(), "amber.toml", &good("amber"));
        write(
            dir.path(),
            "huge.toml",
            &format!("name = \"huge\"\n# {}", "a".repeat(MAX_THEME_BYTES + 10)),
        );
        fs::write(dir.path().join("binary.toml"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
        let (set, added) = loaded(dir.path());
        assert_eq!(added, 1, "only the good file");
        let names: Vec<String> = set.names().iter().map(ToString::to_string).collect();
        assert_eq!(names, [DEFAULT_THEME, "amber", "green", "contrast", "good"]);
        assert_eq!(
            set.get(ThemeName::new("amber").unwrap()).palette(),
            ui().theme(ThemeName::new("amber").unwrap()).palette(),
            "a sysop file cannot replace a shipped theme"
        );
    }

    #[test]
    fn only_a_bounded_number_of_files_are_read() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(MAX_THEME_FILES + 10) {
            write(dir.path(), &format!("t{i}.toml"), &good(&format!("t{i}")));
        }
        let (_, added) = loaded(dir.path());
        assert!(added <= MAX_THEME_FILES, "{added}");
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_never_ends_is_read_only_up_to_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/dev/zero", dir.path().join("zero.toml")).unwrap();
        let (set, added) = loaded(dir.path());
        assert_eq!((added, set.names().len()), (0, 4));
    }
}
