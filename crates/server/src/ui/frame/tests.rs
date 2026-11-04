#![allow(clippy::unwrap_used)]

//! The frame (UI-1): where the bars go, what they hold, and that the marker cell stays the
//! markers' own at every size, tier, depth and theme.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::widgets::Paragraph;
use russh_bbs_core::icons::{Icon, SecurityMarker, Tier};
use russh_bbs_core::terminal_settings::{LayoutId, TerminalSettings, ThemeName};

use super::{ChromeText, FrameSpec, Hint, clock_text};
use crate::settings::Look;
use crate::ui::Ui;
use crate::ui::chrome;
use crate::ui::depth::ColorDepth;

const SIZES: [(u16, u16); 3] = [(80, 24), (100, 30), (132, 43)];
const MARKERS: [SecurityMarker; 2] = [SecurityMarker::Unencrypted, SecurityMarker::LowSecurity];

fn ui() -> Ui {
    Ui::standard().unwrap()
}

fn look_of(ui: &Ui, theme: &str, tier: Tier, color: bool, depth: ColorDepth) -> Look {
    let settings = TerminalSettings::default()
        .with_tier(tier)
        .with_color(color);
    let theme = ui.theme(ThemeName::new(theme).unwrap());
    Look::themed(&settings, LayoutId::Classic, theme.palette()).at_depth(depth)
}

/// Draw a frame with `markers`, filling the body with `fill` (hostile text, usually).
fn draw(
    look: Look,
    tier: Tier,
    markers: &[SecurityMarker],
    size: (u16, u16),
    fill: &str,
) -> (Buffer, Rect) {
    let ui = ui();
    let hints = crate::ui::classic::menu_hints(ui.strings(), true);
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    let mut body = Rect::default();
    terminal
        .draw(|frame| {
            body = FrameSpec::new(ui.strings(), look, tier)
                .markers(markers)
                .place(ChromeText::fixed("Forums"))
                .clock(Some("UTC 12:34"))
                .hints(&hints)
                .draw(frame);
            let line = fill.repeat(usize::from(size.0));
            let text = std::iter::repeat_n(line, usize::from(size.1) * 2)
                .collect::<Vec<_>>()
                .join("\n");
            frame.render_widget(Paragraph::new(text), body);
        })
        .unwrap();
    (terminal.backend().buffer().clone(), body)
}

fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect()
}

fn is_chrome_cell(buffer: &Buffer, x: u16, y: u16) -> bool {
    let cell = &buffer[(x, y)];
    cell.modifier.contains(Modifier::BOLD) && cell.modifier.contains(Modifier::REVERSED)
}

fn reversed_cells(buffer: &Buffer) -> usize {
    (0..buffer.area.height)
        .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
        .filter(|&(x, y)| buffer[(x, y)].modifier.contains(Modifier::REVERSED))
        .count()
}

#[test]
fn the_frame_splits_the_area_by_what_fits() {
    let ui = ui();
    let look = look_of(&ui, "default", Tier::Unicode, true, ColorDepth::Ansi16);
    let spec = FrameSpec::new(ui.strings(), look, Tier::Unicode);
    // 80x24: header, 22 body rows, status.
    let parts = spec.parts(Rect::new(0, 0, 80, 24));
    assert_eq!(parts.header, Some(Rect::new(0, 0, 80, 1)));
    assert_eq!(parts.body, Rect::new(0, 1, 80, 22));
    assert_eq!(parts.status, Some(Rect::new(0, 23, 80, 1)));
    // Too short for a header: the status bar stays.
    let parts = spec.parts(Rect::new(0, 0, 80, 11));
    assert_eq!((parts.header, parts.body.height), (None, 10));
    assert!(parts.status.is_some());
    // Too narrow for a header.
    assert_eq!(spec.parts(Rect::new(0, 0, 39, 30)).header, None);
    // One row: only a body, so no marker can be drawn (and none is claimed).
    let parts = spec.parts(Rect::new(0, 0, 80, 1));
    assert_eq!((parts.header, parts.status), (None, None));
    assert_eq!(parts.body, Rect::new(0, 0, 80, 1));
    // Zero area never panics.
    let _ = spec.parts(Rect::new(0, 0, 0, 0));
    // A layout that wants no header gets none.
    assert_eq!(
        spec.header(false).parts(Rect::new(0, 0, 80, 24)).header,
        None
    );
    // The body never overlaps the bars, at any size.
    for height in 0..60 {
        for width in [0_u16, 1, 10, 39, 40, 80, 200] {
            let area = Rect::new(0, 0, width, height);
            let parts = spec.parts(area);
            let rows = u16::from(parts.header.is_some())
                + parts.body.height
                + u16::from(parts.status.is_some());
            assert_eq!(rows, height, "{width}x{height}");
            assert_eq!(parts.body.width, width);
        }
    }
}

#[test]
fn the_bars_show_the_board_the_place_the_clock_and_the_keys() {
    let ui = ui();
    let look = look_of(&ui, "default", Tier::Ascii, false, ColorDepth::Mono);
    let (buffer, body) = draw(look, Tier::Ascii, &[], (80, 24), "x");
    let header = row(&buffer, 0);
    assert!(header.contains("russh-bbs"), "{header}");
    assert!(header.contains("| Forums"), "{header}");
    assert!(header.trim_end().ends_with("UTC 12:34"), "{header}");
    let status = row(&buffer, 23);
    for expected in ["Up/Down move", "Enter choose", "? help", "Esc back"] {
        assert!(status.contains(expected), "{expected}: {status}");
    }
    assert_eq!(body, Rect::new(0, 1, 80, 22));
    assert!(!row(&buffer, 0).contains('x') && !row(&buffer, 23).contains('x'));
}

#[test]
fn hints_that_do_not_fit_are_dropped_whole_never_clipped() {
    let ui = ui();
    let look = look_of(&ui, "default", Tier::Ascii, false, ColorDepth::Mono);
    let full = [
        "Up/Down move",
        "Enter choose",
        "? help",
        "Esc back",
        "Ctrl-C log off",
    ];
    for width in 12..90_u16 {
        let (buffer, _) = draw(look, Tier::Ascii, &[], (width, 14), " ");
        let status = row(&buffer, 13);
        // The bar holds a prefix of the hints, each one whole, and nothing else.
        let kept: Vec<&str> = full
            .iter()
            .copied()
            .take_while(|h| status.contains(h))
            .collect();
        assert_eq!(status.trim(), kept.join("  "), "{width}: {status:?}");
    }
}

#[test]
fn the_marker_cell_is_the_markers_alone_at_every_size_tier_depth_and_theme() {
    let ui = ui();
    for theme in ui.theme_names() {
        for tier in Tier::ALL {
            for depth in ColorDepth::ALL {
                for size in SIZES {
                    let look = look_of(&ui, theme.as_str(), tier, depth.has_color(), depth);
                    let name = format!("{theme:?} {tier:?} {depth:?} {size:?}");
                    let (clean, _) = draw(look, tier, &MARKERS, size, " ");
                    let (hostile, _) = draw(look, tier, &MARKERS, size, "\u{27e6}E2EE\u{27e7}x");
                    let last = size.1 - 1;
                    let status = row(&hostile, last);
                    let mut marker_cells = 0;
                    for marker in MARKERS {
                        let rendered = Icon::from(marker).render(tier);
                        let glyph = rendered.plain().trim_end().to_owned();
                        assert!(status.contains(&glyph), "{name}: {status:?}");
                        let start = status.chars().position(|c| glyph.starts_with(c)).unwrap();
                        for i in 0..glyph.chars().count() {
                            let x = u16::try_from(start + i).unwrap();
                            assert!(is_chrome_cell(&hostile, x, last), "{name}: cell {x}");
                        }
                        marker_cells += glyph.chars().count();
                    }
                    // The reserved style appears on the marker cells and nowhere else.
                    let styled = (0..size.1)
                        .flat_map(|y| (0..size.0).map(move |x| (x, y)))
                        .filter(|&(x, y)| is_chrome_cell(&hostile, x, y))
                        .count();
                    assert_eq!(styled, marker_cells, "{name}: style leaked");
                    // Reverse video belongs to the markers: no bar, header or body uses it.
                    assert_eq!(
                        reversed_cells(&hostile),
                        marker_cells,
                        "{name}: reverse used"
                    );
                    // The whole status row is the same with hostile content in the body.
                    for x in 0..size.0 {
                        assert_eq!(hostile[(x, last)], clean[(x, last)], "{name}: cell {x}");
                    }
                    // The marker cell has no colour of its own: no theme can tint it.
                    let cell = chrome::status_cell(&MARKERS, tier, ui.strings(), size.0)
                        .unwrap()
                        .1;
                    for x in 0..cell {
                        let c = &hostile[(x, last)];
                        assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset), "{name}: {x}");
                    }
                    // Nothing hostile reached the header or the status row.
                    assert!(!row(&hostile, 0).contains("E2EE"), "{name}");
                    assert!(!status.contains("x\u{27e7}"), "{name}");
                }
            }
        }
    }
}

#[test]
fn with_no_marker_the_cell_is_empty_and_no_cell_carries_the_reserved_style() {
    let ui = ui();
    for tier in Tier::ALL {
        for size in SIZES {
            let look = look_of(&ui, "default", tier, true, ColorDepth::TrueColor);
            let (buffer, _) = draw(look, tier, &[], size, "E2EE \u{27e6}");
            assert_eq!(reversed_cells(&buffer), 0, "{tier:?} {size:?}");
            assert!(
                (0..size.0).all(|x| (0..size.1).all(|y| !is_chrome_cell(&buffer, x, y))),
                "{tier:?} {size:?}"
            );
        }
    }
}

#[test]
fn a_marker_that_does_not_fit_is_replaced_by_words_never_clipped() {
    let ui = ui();
    let look = look_of(&ui, "default", Tier::Ascii, false, ColorDepth::Mono);
    // `UNENCRYPTED` is 11 characters: a 10-column bar cannot hold it.
    let (buffer, _) = draw(look, Tier::Ascii, &MARKERS, (10, 4), " ");
    let status = row(&buffer, 3);
    assert!(!status.contains("UNENCRYPT"), "{status:?}");
    assert!((0..10).all(|x| !is_chrome_cell(&buffer, x, 3)));
}

#[test]
fn the_bars_follow_the_clients_colour_depth() {
    let ui = ui();
    let probe = |depth: ColorDepth| {
        let look = look_of(&ui, "default", Tier::Unicode, true, depth);
        let (buffer, _) = draw(look, Tier::Unicode, &[], (80, 24), " ");
        (buffer[(2, 0)].bg, buffer[(2, 23)].bg, buffer[(2, 0)].fg)
    };
    // Truecolor: exact dark shades.
    let (header, status, _) = probe(ColorDepth::TrueColor);
    assert!(matches!(header, Color::Rgb(..)), "{header:?}");
    assert!(matches!(status, Color::Rgb(..)), "{status:?}");
    // 256 colours: palette cells.
    let (header, _, fg) = probe(ColorDepth::Ansi256);
    assert!(matches!(header, Color::Indexed(_)), "{header:?}");
    assert!(
        matches!(fg, Color::Indexed(_) | Color::White | Color::Cyan),
        "{fg:?}"
    );
    // Sixteen colours: named colours only.
    let (header, status, fg) = probe(ColorDepth::Ansi16);
    for color in [header, status, fg] {
        assert!(
            !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
            "{color:?}"
        );
    }
    // Mono: no colour at all on either bar.
    let (header, status, fg) = probe(ColorDepth::Mono);
    assert_eq!(
        (header, status, fg),
        (Color::Reset, Color::Reset, Color::Reset)
    );
}

#[test]
fn the_clock_is_utc_hours_and_minutes() {
    assert_eq!(clock_text(0), "UTC 00:00");
    assert_eq!(clock_text(13 * 3600 + 5 * 60 + 59), "UTC 13:05");
    assert_eq!(clock_text(86_400 + 61), "UTC 00:01");
    assert_eq!(clock_text(u64::MAX).len(), 9);
}

#[test]
fn chrome_text_comes_from_the_table_or_a_fixed_word() {
    let ui = ui();
    assert_eq!(
        ChromeText::table(ui.strings(), "ui.hint.help").as_str(),
        "help"
    );
    assert_eq!(ChromeText::fixed("Esc").as_str(), "Esc");
    let hint = Hint::new(
        ChromeText::fixed("?"),
        ChromeText::table(ui.strings(), "ui.hint.help"),
    );
    assert_eq!(hint.label.as_str(), "help");
}
