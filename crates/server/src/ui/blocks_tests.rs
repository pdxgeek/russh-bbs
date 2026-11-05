#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The Blocks border (FIX-UI-7a): block elements that fill the cell, so a terminal that draws
//! box lines from the font and leaves gaps with extra line spacing still shows a solid frame.
//! Chrome only: never on the marker cell or the status row, never in the reserved style, and
//! never in the ASCII tier.

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::terminal_settings::{BorderChoice, LayoutId};

use super::depth::ColorDepth;
use super::layout_tests::{DRAWN, hostile_body, marker_cell_width, text, ui};
use super::styles_tests::{STYLES, shot_borders, style};
use super::theme::{BorderSet, Skin, Theme};

/// Every character [`BLOCKS_BORDER`](super::theme::skin::BLOCKS_BORDER) draws.
const BLOCK_GLYPHS: [char; 8] = [
    '\u{2590}', '\u{258c}', '\u{2584}', '\u{2580}', '\u{2597}', '\u{2596}', '\u{259d}', '\u{2598}',
];

/// A body of plain ASCII, so a whole-screen "only ASCII" check tests the chrome and not the text.
fn ascii_body() -> Vec<String> {
    let mut body = vec!["Signed in as alice.".to_owned(), "x".repeat(2000)];
    body.extend((0..200).map(|i| format!("line {i}")));
    body
}

/// Whether a layout draws a bordered box. Compact and Focus draw none: they are told apart by
/// their rules, so a border choice changes nothing on them.
fn boxed(layout: LayoutId) -> bool {
    !matches!(layout, LayoutId::Compact | LayoutId::Focus)
}

fn blocks_in(buffer: &Buffer, row: Option<u16>) -> usize {
    let mut found = 0;
    for y in 0..buffer.area.height {
        if row.is_some_and(|only| only != y) {
            continue;
        }
        for x in 0..buffer.area.width {
            if buffer[(x, y)]
                .symbol()
                .chars()
                .any(|c| BLOCK_GLYPHS.contains(&c))
            {
                found += 1;
            }
        }
    }
    found
}

#[test]
fn the_blocks_set_is_continuous_and_fills_each_edge_cell() {
    let set = super::theme::skin::BLOCKS_BORDER;
    // Every edge sits against the content, so the corners meet the edges with no step: a corner
    // quadrant is the overlap of the two half blocks that meet there.
    assert_eq!(set.vertical_left, "\u{2590}", "right half, against content");
    assert_eq!(set.vertical_right, "\u{258c}", "left half, against content");
    assert_eq!(
        set.horizontal_top, "\u{2584}",
        "lower half, against content"
    );
    assert_eq!(
        set.horizontal_bottom, "\u{2580}",
        "upper half, against content"
    );
    assert_eq!(set.top_left, "\u{2597}", "lower right quadrant");
    assert_eq!(set.top_right, "\u{2596}", "lower left quadrant");
    assert_eq!(set.bottom_left, "\u{259d}", "upper right quadrant");
    assert_eq!(set.bottom_right, "\u{2598}", "upper left quadrant");
    // One column wide each, and none is in the private-use range or a reserved marker glyph.
    for part in [
        set.vertical_left,
        set.vertical_right,
        set.horizontal_top,
        set.horizontal_bottom,
        set.top_left,
        set.top_right,
        set.bottom_left,
        set.bottom_right,
    ] {
        assert_eq!(part.chars().count(), 1);
        let c = part.chars().next().unwrap();
        assert!(BLOCK_GLYPHS.contains(&c));
        assert!(!russh_bbs_core::sanitize::RESERVED_CHROME.contains(&c));
    }
}

#[test]
fn every_layout_draws_the_blocks_border_in_mono_and_the_ascii_tier() {
    let ui = ui();
    let theme = style(&ui, "phosphor");
    for chosen in LayoutId::ALL {
        for size in [(80, 24), (132, 43)] {
            // Mono: Unicode tier, no colour.
            let mono = shot_borders(
                &DRAWN,
                chosen,
                theme,
                &ascii_body(),
                Tier::Unicode,
                false,
                ColorDepth::Mono,
                size,
                BorderChoice::Blocks,
            );
            let ctx = format!("{chosen:?} {size:?}");
            let last = mono.area.height - 1;
            if boxed(chosen) {
                assert!(blocks_in(&mono, None) > 0, "{ctx}: Blocks is drawn");
                // Both side edges are drawn on every row of the frame.
                let left = (0..mono.area.height)
                    .flat_map(|y| (0..mono.area.width).map(move |x| (x, y)))
                    .filter(|(x, y)| mono[(*x, *y)].symbol() == "\u{2590}")
                    .count();
                assert!(left >= 10, "{ctx}: the left edge is drawn ({left} rows)");
            } else {
                assert_eq!(blocks_in(&mono, None), 0, "{ctx}: this layout draws no box");
            }
            // The status row (marker cell and the rest) never holds a border.
            assert_eq!(blocks_in(&mono, Some(last)), 0, "{ctx}: status row");
            for x in 0..mono.area.width {
                let cell = &mono[(x, last)];
                if cell.modifier.contains(Modifier::REVERSED) {
                    assert!(
                        cell.modifier.contains(Modifier::BOLD),
                        "{ctx}: reserved style"
                    );
                }
            }

            // ASCII tier: the member's Blocks choice falls back to `+ - |`.
            let ascii = shot_borders(
                &DRAWN,
                chosen,
                theme,
                &ascii_body(),
                Tier::Ascii,
                false,
                ColorDepth::Mono,
                size,
                BorderChoice::Blocks,
            );
            let shown = text(&ascii);
            assert!(
                shown.is_ascii(),
                "{ctx}: ASCII tier draws only ASCII:\n{shown}"
            );
            assert_eq!(blocks_in(&ascii, None), 0, "{ctx}");
            if boxed(chosen) {
                assert!(shown.contains('+') && shown.contains('|'), "{ctx}");
            }
        }
    }
}

#[test]
fn the_marker_cell_is_not_covered_by_blocks_in_any_style_layout_or_depth() {
    let ui = ui();
    for name in STYLES {
        let theme = style(&ui, name);
        for chosen in LayoutId::ALL {
            for depth in [ColorDepth::Mono, ColorDepth::Ansi16, ColorDepth::TrueColor] {
                for tier in [Tier::Ascii, Tier::Unicode] {
                    for size in [(80, 24), (132, 43)] {
                        let buffer = shot_borders(
                            &DRAWN,
                            chosen,
                            theme,
                            &hostile_body(),
                            tier,
                            depth.has_color(),
                            depth,
                            size,
                            BorderChoice::Blocks,
                        );
                        let ctx = format!("{name} {chosen:?} {depth:?} {tier:?} {size:?}");
                        let last = buffer.area.height - 1;
                        let cell_width = marker_cell_width(tier, size.0);
                        let mut marker = false;
                        for y in 0..buffer.area.height {
                            for x in 0..buffer.area.width {
                                let cell = &buffer[(x, y)];
                                let reversed = cell.modifier.contains(Modifier::REVERSED);
                                if y == last && x < cell_width {
                                    assert!(
                                        !cell.symbol().chars().any(|c| BLOCK_GLYPHS.contains(&c)),
                                        "{ctx}: block glyph on the marker cell ({x},{y})"
                                    );
                                    assert!(
                                        cell.fg == Color::Reset && cell.bg == Color::Reset,
                                        "{ctx}: marker cell coloured"
                                    );
                                    marker |= reversed && cell.modifier.contains(Modifier::BOLD);
                                } else {
                                    assert!(
                                        !reversed,
                                        "{ctx}: reverse outside the marker ({x},{y})"
                                    );
                                }
                            }
                        }
                        assert!(marker, "{ctx}: the marker keeps its reserved style");
                        assert_eq!(blocks_in(&buffer, Some(last)), 0, "{ctx}: status row");
                    }
                }
            }
        }
    }
}

#[test]
fn a_style_can_name_blocks_and_the_ascii_tier_still_falls_back() {
    let ui = ui();
    let text_file = "name = \"chunky\"\n[style.borders]\nframe = \"blocks\"\npane = \"blocks\"\npopup = \"blocks\"\n";
    let theme = Theme::parse(text_file, ui.strings()).unwrap();
    assert_eq!(
        theme.skin().border(super::theme::Element::Frame),
        BorderSet::Blocks
    );
    assert_eq!(
        theme.skin().border(super::theme::Element::Pane),
        BorderSet::Blocks
    );
    assert_eq!(
        theme.skin().border(super::theme::Element::Popup),
        BorderSet::Blocks
    );
    assert_ne!(Skin::default().frame, BorderSet::Blocks);
    for chosen in [LayoutId::Classic, LayoutId::Dashboard, LayoutId::Split] {
        for size in [(80, 24), (132, 43)] {
            let unicode = shot_borders(
                &DRAWN,
                chosen,
                &theme,
                &ascii_body(),
                Tier::Unicode,
                false,
                ColorDepth::Mono,
                size,
                BorderChoice::ThemeDefault,
            );
            assert!(blocks_in(&unicode, None) > 0, "{chosen:?} {size:?}");
            let ascii = shot_borders(
                &DRAWN,
                chosen,
                &theme,
                &ascii_body(),
                Tier::Ascii,
                false,
                ColorDepth::Mono,
                size,
                BorderChoice::ThemeDefault,
            );
            assert!(
                text(&ascii).is_ascii(),
                "{chosen:?} {size:?}\n{}",
                text(&ascii)
            );
            assert_eq!(blocks_in(&ascii, None), 0);
        }
    }
    // A member who chose ASCII beats a style that names Blocks.
    let ascii = shot_borders(
        &DRAWN,
        LayoutId::Classic,
        &theme,
        &ascii_body(),
        Tier::Unicode,
        false,
        ColorDepth::Mono,
        (80, 24),
        BorderChoice::Ascii,
    );
    assert_eq!(blocks_in(&ascii, None), 0);
}
