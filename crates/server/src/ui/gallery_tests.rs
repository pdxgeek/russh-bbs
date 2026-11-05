#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The member style gallery and the sysop Theme packs screen (UI-7): every allowed style is
//! drawn live at the member's size, depth and tier; the board's locks are shown and respected;
//! the marker cell is never covered by a preview; hostile text is cleaned.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use russh_bbs_core::icons::Tier;
use russh_bbs_core::sanitize::RESERVED_CHROME;
use russh_bbs_core::terminal_settings::{
    FileRow, Gallery, GalleryKey, LayoutId, PackListing, PackScreen, StyleRow, TerminalSettings,
    ThemeName,
};

use super::depth::ColorDepth;
use super::gallery::{GalleryCtx, render, render_packs};
use super::layout_tests::{DRAWN, marker_cell_width, text};
use super::styles_tests::STYLES;
use super::{Board, Ui};
use crate::config::BoardConfig;

const SIZES: [(u16, u16); 3] = [(80, 24), (100, 30), (132, 43)];

fn name(style: &str) -> ThemeName {
    ThemeName::new(style).unwrap()
}

fn all_styles() -> Vec<ThemeName> {
    STYLES.iter().map(|s| name(s)).collect()
}

fn screen(
    ui: &Ui,
    size: (u16, u16),
    tier: Tier,
    depth: ColorDepth,
    layout: LayoutId,
    draw: impl FnOnce(&mut ratatui::Frame<'_>, &GalleryCtx<'_>),
) -> Buffer {
    let settings = TerminalSettings::default()
        .with_tier(tier)
        .with_layout(layout)
        .with_color(true);
    let area = Rect::new(0, 0, size.0, size.1);
    let look = ui.look(&settings, area, depth);
    let strings = ui.theme_for(&settings).strings();
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| {
            let ctx = GalleryCtx {
                ui,
                strings,
                look,
                tier,
                markers: &DRAWN,
                clock: Some("UTC 12:00"),
                settings: &settings,
            };
            draw(frame, &ctx);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn gallery_at(
    ui: &Ui,
    gallery: &Gallery,
    size: (u16, u16),
    tier: Tier,
    depth: ColorDepth,
) -> Buffer {
    screen(ui, size, tier, depth, LayoutId::Classic, |frame, ctx| {
        render(frame, gallery, ctx);
    })
}

#[test]
fn every_style_previews_live_at_every_size_depth_and_tier() {
    let ui = Ui::standard().unwrap();
    let mut gallery = Gallery::open(&all_styles(), name("phosphor"));
    let mut drawn: Vec<(ThemeName, String)> = Vec::new();
    for _ in 0..STYLES.len() {
        let style = gallery.selected();
        let title = ui.theme(style).title().to_owned();
        for size in SIZES {
            for (tier, depth) in [
                (Tier::Unicode, ColorDepth::TrueColor),
                (Tier::Unicode, ColorDepth::Ansi256),
                (Tier::Unicode, ColorDepth::Ansi16),
                (Tier::Ascii, ColorDepth::Ansi16),
            ] {
                let buffer = gallery_at(&ui, &gallery, size, tier, depth);
                let shown = text(&buffer);
                assert!(
                    shown.contains(&title),
                    "{style} {size:?} {tier:?}: no title"
                );
                assert!(shown.contains("Style gallery"), "{style}: no heading");
                if tier == Tier::Ascii {
                    // The ASCII tier is plain ASCII all the way down, sample included.
                    assert!(
                        shown.is_ascii(),
                        "{style} {size:?}: not ASCII: {:?}",
                        shown.chars().filter(|c| !c.is_ascii()).collect::<String>()
                    );
                }
            }
        }
        drawn.push((
            style,
            text(&gallery_at(
                &ui,
                &gallery,
                (100, 30),
                Tier::Unicode,
                ColorDepth::TrueColor,
            )),
        ));
        gallery.press(GalleryKey::Next);
    }
    // The previews differ: each style's sample is its own, not one picture relabelled.
    for (i, (a, shown_a)) in drawn.iter().enumerate() {
        for (b, shown_b) in &drawn[i + 1..] {
            let body = |s: &str| s.lines().skip(2).collect::<Vec<_>>().join("\n");
            assert_ne!(body(shown_a), body(shown_b), "{a} and {b} look the same");
        }
    }
}

#[test]
fn the_sample_follows_the_viewers_depth() {
    let ui = Ui::standard().unwrap();
    let gallery = Gallery::open(&all_styles(), name("neon"));
    let truecolor = gallery_at(
        &ui,
        &gallery,
        (100, 30),
        Tier::Unicode,
        ColorDepth::TrueColor,
    );
    let sixteen = gallery_at(&ui, &gallery, (100, 30), Tier::Unicode, ColorDepth::Ansi16);
    let has_rgb = |b: &Buffer| {
        b.content()
            .iter()
            .any(|c| matches!(c.bg, ratatui::style::Color::Rgb(..)))
    };
    assert!(has_rgb(&truecolor));
    assert!(!has_rgb(&sixteen), "a 16-colour viewer was shown truecolor");
}

#[test]
fn the_marker_cell_is_never_covered_by_a_preview() {
    let ui = Ui::standard().unwrap();
    let mut gallery = Gallery::open(&all_styles(), name("phosphor"));
    for _ in 0..STYLES.len() {
        for sample in 0..2 {
            for size in SIZES {
                for tier in [Tier::Unicode, Tier::Ascii] {
                    let buffer = gallery_at(&ui, &gallery, size, tier, ColorDepth::TrueColor);
                    let width = marker_cell_width(tier, size.0);
                    let last = size.1 - 1;
                    for y in 0..size.1 {
                        for x in 0..size.0 {
                            let cell = &buffer[(x, y)];
                            let reversed = cell.modifier.contains(Modifier::REVERSED);
                            let marker_cell = y == last && x < width;
                            assert!(
                                !reversed || marker_cell,
                                "{:?} {size:?} {tier:?} ({x},{y}): reverse video outside the marker cell",
                                gallery.selected()
                            );
                        }
                    }
                    // And the marker cell still carries the reserved style: bold plus reverse.
                    let cell = &buffer[(0, last)];
                    assert!(cell.modifier.contains(Modifier::BOLD | Modifier::REVERSED));
                }
            }
            gallery.press(GalleryKey::Sample);
            let _ = sample;
        }
        gallery.press(GalleryKey::Next);
    }
}

#[test]
fn a_locked_board_shows_one_style_and_says_so() {
    let board = Board::from_config(&BoardConfig {
        styles: "neon".into(),
        ..BoardConfig::default()
    })
    .unwrap();
    let ui = Ui::standard().unwrap().with_board(board);
    assert_eq!(ui.allowed_styles(), vec![name("neon")]);
    let gallery = Gallery::open(&ui.allowed_styles(), name("neon"));
    assert!(gallery.locked());
    let shown = text(&gallery_at(
        &ui,
        &gallery,
        (80, 24),
        Tier::Unicode,
        ColorDepth::TrueColor,
    ));
    assert!(
        shown.contains("the sysop has set this board's style"),
        "{shown}"
    );
    assert!(!shown.contains("Enter"), "a lock offers no pick");
}

#[test]
fn the_gallery_lists_only_the_boards_allowed_styles() {
    let board = Board::from_config(&BoardConfig {
        styles: "neon, paper".into(),
        ..BoardConfig::default()
    })
    .unwrap();
    let ui = Ui::standard().unwrap().with_board(board);
    let allowed = ui.allowed_styles();
    assert_eq!(allowed, vec![name("neon"), name("paper")]);
    let gallery = Gallery::open(&allowed, name("neon"));
    let shown = text(&gallery_at(
        &ui,
        &gallery,
        (100, 30),
        Tier::Unicode,
        ColorDepth::TrueColor,
    ));
    let strip = shown.lines().nth(1).unwrap_or_default();
    assert!(
        strip.contains("Neon Night") && strip.contains("Paper"),
        "{strip}"
    );
    for other in ["Phosphor", "Amber Terminal", "Deep Space", "Classic ANSI"] {
        assert!(!strip.contains(other), "{other} is not allowed: {strip}");
    }
}

#[test]
fn a_long_strip_scrolls_to_keep_the_cursor_on_screen() {
    let ui = Ui::standard().unwrap();
    let mut gallery = Gallery::open(&all_styles(), name("phosphor"));
    for _ in 0..5 {
        gallery.press(GalleryKey::Next);
    }
    assert_eq!(gallery.selected(), name("ansi"));
    let shown = text(&gallery_at(
        &ui,
        &gallery,
        (60, 24),
        Tier::Unicode,
        ColorDepth::TrueColor,
    ));
    assert!(
        shown.lines().nth(1).unwrap().contains("Classic ANSI"),
        "{shown}"
    );
}

#[test]
fn the_packs_screen_cleans_what_a_file_says() {
    let ui = Ui::standard().unwrap();
    let hostile = format!(
        "bad \u{1b}[31mred\u{7} {}",
        RESERVED_CHROME.iter().collect::<String>()
    );
    let listing = PackListing {
        styles: vec![
            StyleRow {
                name: ThemeName::default_theme(),
                title: "default".into(),
                exportable: false,
            },
            StyleRow {
                name: name("neon"),
                title: hostile.clone(),
                exportable: true,
            },
        ],
        files: vec![
            FileRow {
                file: format!("a{}.pack.toml", '\u{1b}'),
                verdict: Err(hostile.clone()),
            },
            FileRow {
                file: "ok.pack.toml".into(),
                verdict: Ok(name("ok")),
            },
        ],
        unavailable: false,
    };
    let mut packs = PackScreen::open();
    packs.listed(listing, Some(&hostile));
    for size in SIZES {
        let buffer = screen(
            &ui,
            size,
            Tier::Unicode,
            ColorDepth::TrueColor,
            LayoutId::Classic,
            |frame, ctx| {
                render_packs(frame, &packs, ctx);
            },
        );
        let shown = text(&buffer);
        assert!(shown.contains("Installed styles"), "{shown}");
        assert!(shown.contains("ready: style ok") || size.1 < 30, "{shown}");
        assert!(
            !shown.chars().any(|c| c != '\n' && c.is_control()),
            "control character on screen"
        );
        // The security markers' reserved glyphs only ever appear in the marker cell.
        let last = size.1 - 1;
        let body: String = (0..last)
            .flat_map(|y| (0..size.0).map(move |x| (x, y)))
            .map(|(x, y)| buffer[(x, y)].symbol().to_owned())
            .collect();
        for glyph in RESERVED_CHROME {
            assert!(!body.contains(*glyph), "{glyph:?} drawn from content");
        }
    }
}

#[test]
fn the_packs_screen_says_when_there_is_no_directory() {
    let ui = Ui::standard().unwrap();
    let mut packs = PackScreen::open();
    packs.listed(
        PackListing {
            unavailable: true,
            ..PackListing::default()
        },
        None,
    );
    let shown = text(&screen(
        &ui,
        (80, 24),
        Tier::Unicode,
        ColorDepth::TrueColor,
        LayoutId::Classic,
        |f, c| {
            render_packs(f, &packs, c);
        },
    ));
    assert!(shown.contains("no packs directory"), "{shown}");
}
