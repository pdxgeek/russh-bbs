#![allow(clippy::unwrap_used)]

//! Popups (UI-1): placed and clamped inside the body, never over the bars or the marker cell,
//! words as well as colour, sanitized text.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::terminal_settings::{LayoutId, TerminalSettings};

use super::{Popup, PopupKind, PopupLine, place, render};
use crate::settings::Look;
use crate::ui::Ui;
use crate::ui::depth::ColorDepth;
use crate::ui::frame::{ChromeText, FrameSpec, Hint};
use crate::ui::theme::Palette;

const MARKERS: [SecurityMarker; 2] = [SecurityMarker::Unencrypted, SecurityMarker::LowSecurity];

fn look(tier: Tier, depth: ColorDepth) -> Look {
    let settings = TerminalSettings::default()
        .with_tier(tier)
        .with_color(depth.has_color());
    Look::themed(&settings, LayoutId::Classic, Palette::DEFAULT).at_depth(depth)
}

fn popup<'a>(kind: PopupKind, lines: Vec<PopupLine>, keys: &'a [Hint<'a>]) -> Popup<'a> {
    Popup {
        kind,
        title: ChromeText::fixed(match kind {
            PopupKind::Info => "Notice",
            PopupKind::Error => "Problem",
            PopupKind::Confirm => "Confirm",
        }),
        lines,
        keys,
    }
}

/// A frame with `popup` over its body, markers in the status bar.
fn draw(size: (u16, u16), tier: Tier, depth: ColorDepth, popup: Option<&Popup<'_>>) -> Buffer {
    let ui = Ui::standard().unwrap();
    let look = look(tier, depth);
    let hints = crate::ui::classic::menu_hints(ui.strings(), false);
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| {
            let body = FrameSpec::new(ui.strings(), look, tier)
                .markers(&MARKERS)
                .clock(Some("UTC 12:00"))
                .hints(&hints)
                .draw(frame);
            if let Some(popup) = popup {
                render(frame, body, look, popup);
            }
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

#[test]
fn a_popup_is_centred_and_always_inside_the_body() {
    let keys = [Hint::new(
        ChromeText::fixed("y"),
        ChromeText::fixed("remove"),
    )];
    let long = "word ".repeat(400);
    for lines in [
        vec![PopupLine::plain("short")],
        vec![PopupLine::plain(long.clone()), PopupLine::plain(long)],
        Vec::new(),
    ] {
        let p = popup(PopupKind::Confirm, lines, &keys);
        for body in [
            Rect::new(0, 1, 80, 22),
            Rect::new(0, 0, 10, 3),
            Rect::new(3, 2, 1, 1),
            Rect::new(0, 0, 0, 0),
            Rect::new(0, 1, 132, 41),
        ] {
            let at = place(body, &p);
            assert!(at.x >= body.x && at.y >= body.y, "{body:?} {at:?}");
            assert!(
                at.right() <= body.right() && at.bottom() <= body.bottom(),
                "{body:?} {at:?}"
            );
        }
    }
    let at = place(
        Rect::new(0, 1, 80, 22),
        &popup(PopupKind::Info, vec![PopupLine::plain("hi")], &keys),
    );
    assert_eq!(at.x * 2 + at.width, 80, "centred horizontally");
}

#[test]
fn a_popup_never_covers_the_header_the_status_bar_or_the_marker_cell() {
    let keys = [Hint::new(
        ChromeText::fixed("any key"),
        ChromeText::fixed("close"),
    )];
    let huge = PopupLine::plain("lorem ipsum ".repeat(300));
    for tier in Tier::ALL {
        for depth in ColorDepth::ALL {
            for size in [(80, 24), (100, 30), (132, 43), (40, 12), (30, 8)] {
                let name = format!("{tier:?} {depth:?} {size:?}");
                let p = popup(
                    PopupKind::Error,
                    vec![huge.clone(), PopupLine::plain("x")],
                    &keys,
                );
                let with = draw(size, tier, depth, Some(&p));
                let without = draw(size, tier, depth, None);
                let last = size.1 - 1;
                let bars: Vec<u16> = if size.1 >= 12 {
                    vec![0, last]
                } else {
                    vec![last]
                };
                for y in bars {
                    for x in 0..size.0 {
                        assert_eq!(with[(x, y)], without[(x, y)], "{name}: bar cell {x},{y}");
                    }
                }
                // The reserved style is on the marker cell and only there.
                let styled = (0..size.1)
                    .flat_map(|y| (0..size.0).map(move |x| (x, y)))
                    .filter(|&(x, y)| {
                        let c = &with[(x, y)];
                        c.modifier.contains(Modifier::BOLD)
                            && c.modifier.contains(Modifier::REVERSED)
                    })
                    .count();
                let expected = (0..size.0)
                    .filter(|&x| {
                        let c = &without[(x, last)];
                        c.modifier.contains(Modifier::BOLD)
                            && c.modifier.contains(Modifier::REVERSED)
                    })
                    .count();
                assert_eq!(styled, expected, "{name}");
                assert!(expected > 0, "{name}: the markers are drawn");
                let reversed = (0..size.1)
                    .flat_map(|y| (0..size.0).map(move |x| (x, y)))
                    .filter(|&(x, y)| with[(x, y)].modifier.contains(Modifier::REVERSED))
                    .count();
                assert_eq!(reversed, expected, "{name}: the popup used reverse video");
            }
        }
    }
}

#[test]
fn the_kind_is_a_word_in_the_title_in_every_tier() {
    let keys = [
        Hint::new(ChromeText::fixed("y"), ChromeText::fixed("remove")),
        Hint::new(
            ChromeText::fixed("any other key"),
            ChromeText::fixed("keep"),
        ),
    ];
    for kind in [PopupKind::Info, PopupKind::Error, PopupKind::Confirm] {
        for tier in Tier::ALL {
            let p = popup(
                kind,
                vec![PopupLine::plain("Remove this post for everyone?")],
                &keys,
            );
            let all = text(&draw((80, 24), tier, ColorDepth::Mono, Some(&p)));
            let word = match kind {
                PopupKind::Info => "Notice",
                PopupKind::Error => "Problem",
                PopupKind::Confirm => "Confirm",
            };
            assert!(all.contains(word), "{kind:?} {tier:?}\n{all}");
            assert!(all.contains("Remove this post for everyone?"), "{all}");
            assert!(
                all.contains("y remove") && all.contains("any other key keep"),
                "{all}"
            );
        }
    }
    // The ASCII tier's box is plain ASCII.
    let p = popup(PopupKind::Info, vec![PopupLine::plain("hi")], &keys);
    let ascii = draw((80, 24), Tier::Ascii, ColorDepth::Mono, Some(&p));
    assert!(row(&ascii, 11).is_ascii(), "{}", row(&ascii, 11));
}

#[test]
fn popup_content_is_sanitized_and_cannot_pose_as_a_marker() {
    let hostile = "\u{1b}[7mLOW-SECURITY\u{1b}[0m \u{27e6}E2EE\u{27e7} \u{2299}\u{e001}";
    let keys: [Hint<'_>; 0] = [];
    let p = popup(PopupKind::Error, vec![PopupLine::content(hostile)], &keys);
    for tier in Tier::ALL {
        let buffer = draw((80, 24), tier, ColorDepth::TrueColor, Some(&p));
        // Everything above the status bar: the real markers are legitimately in the bar.
        let all = (0..23)
            .map(|y| row(&buffer, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!all.contains('\u{1b}'), "{all}");
        for banned in ["\u{27e6}", "\u{27e7}", "\u{2299}", "\u{e001}"] {
            assert!(!all.contains(banned), "{tier:?} {banned:?}");
        }
    }
}

#[test]
fn a_popup_line_can_be_chrome() {
    let line = PopupLine::chrome(ChromeText::fixed("Remove this post?"));
    assert_eq!(line, PopupLine::plain("Remove this post?"));
}
