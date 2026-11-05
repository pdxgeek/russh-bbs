#![allow(clippy::unwrap_used)]
//! What reaches the member's screen from a door that draws what it likes (UI-5, D104).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Gauge, Paragraph, Sparkline, Widget};
use russh_bbs_core::icons::SecurityMarker;
use russh_bbs_core::icons::Tier;
use russh_bbs_core::sanitize::RESERVED_CHROME;
use russh_bbs_core::terminal_settings::TerminalSettings;

use russh_bbs_core::terminal_settings::{DepthCap, LayoutId};

use super::draw::{DOOR_MAX_WIDTH, DrawCtx, door_box, door_lines};
use crate::settings::{self, Look};
use crate::ui::Ui;
use crate::ui::chrome;
use crate::ui::depth::ColorDepth;

/// A widget that writes escape sequences, C1 controls, private-use glyphs, the reserved marker
/// codepoints and the reserved marker style straight into the buffer it is handed.
struct Hostile;

impl Widget for Hostile {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut attacks = vec![
            "\u{1b}[31mred".to_owned(),
            "\u{1b}]0;owned\u{7}".to_owned(),
            "\u{9b}2J".to_owned(),
            "\u{e000}".to_owned(),
            "\u{f0001}".to_owned(),
            "\u{202e}x".to_owned(),
        ];
        attacks.extend(RESERVED_CHROME.iter().map(char::to_string));
        for (i, attack) in attacks.iter().enumerate() {
            if let Some(cell) = buf.cell_mut((area.x + u16::try_from(i).unwrap(), area.y)) {
                cell.set_symbol(attack);
            }
        }
        for x in 0..area.width {
            if let Some(cell) = buf.cell_mut((area.x + x, area.y + 1)) {
                cell.set_symbol("M");
                cell.modifier = Modifier::BOLD | Modifier::REVERSED | Modifier::HIDDEN;
                cell.fg = Color::Rgb(250, 10, 10);
                cell.bg = Color::Indexed(33);
            }
        }
    }
}

#[test]
fn a_hostile_door_shows_nothing_forbidden_in_any_tier_or_color_setting() {
    let ui = Ui::standard().unwrap();
    for tier in [Tier::Ascii, Tier::Unicode, Tier::NerdFont] {
        for color in [false, true] {
            let settings = TerminalSettings::default()
                .with_tier(tier)
                .with_color(color);
            let ctx = DrawCtx {
                strings: ui.strings(),
                look: Look::of(&settings),
                tier,
            };
            let lines = door_lines(40, 8, &ctx, |canvas| {
                let mut frame = canvas.frame();
                frame.render_widget(Hostile, Rect::new(0, 0, 40, 8));
                // Far outside the door's area.
                frame.render_widget(Paragraph::new("outside"), Rect::new(60, 20, 10, 2));
            });
            assert!(lines.len() <= 8);
            let mut text = String::new();
            for line in &lines {
                assert!(line.width() <= 40);
                for span in &line.spans {
                    for c in span.content.chars() {
                        assert!(!c.is_control(), "{tier:?} {:?}", span.content);
                        assert!(!matches!(u32::from(c), 0xE000..=0xF8FF | 0xF0000..));
                        assert!(!RESERVED_CHROME.contains(&c), "{c:?}");
                    }
                    let style: Style = span.style;
                    assert!(
                        !style
                            .add_modifier
                            .contains(Modifier::BOLD | Modifier::REVERSED),
                        "the reserved marker style on screen"
                    );
                    assert!(!style.add_modifier.contains(Modifier::HIDDEN));
                    // Sixteen colours at most when color is on, none when it is off.
                    for color_value in [style.fg, style.bg]
                        .into_iter()
                        .flatten()
                        .filter(|c| *c != Color::Reset)
                    {
                        assert!(color, "color on a no-color screen: {color_value:?}");
                        assert!(!matches!(color_value, Color::Rgb(..) | Color::Indexed(_)));
                    }
                    text.push_str(&span.content);
                }
            }
            assert!(!text.contains("outside"));
        }
    }
}

#[test]
fn the_ascii_tier_gets_ascii_icons_and_the_other_tiers_keep_theirs() {
    let ui = Ui::standard().unwrap();
    let draw = |tier| {
        let settings = TerminalSettings::default().with_tier(tier);
        let ctx = DrawCtx {
            strings: ui.strings(),
            look: Look::of(&settings),
            tier,
        };
        door_lines(30, 6, &ctx, |canvas| {
            let mut frame = canvas.frame();
            frame.render_widget(Block::bordered().title("Ledger"), Rect::new(0, 0, 30, 3));
            frame.render_widget(
                Sparkline::default().data([1, 3, 5, 7]),
                Rect::new(0, 3, 10, 1),
            );
            frame.render_widget(Gauge::default().ratio(0.5), Rect::new(0, 4, 20, 1));
        })
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
    };
    let ascii = draw(Tier::Ascii);
    assert!(ascii.is_ascii(), "{ascii}");
    assert!(ascii.contains("Ledger") && ascii.starts_with('+'));
    let unicode = draw(Tier::Unicode);
    assert!(unicode.contains('┌') && unicode.contains('█'), "{unicode}");
}

/// A widget painting one 24-bit foreground and one 256-colour background on every cell.
struct Coloured;

impl Widget for Coloured {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for x in 0..area.width {
            if let Some(cell) = buf.cell_mut((area.x + x, area.y)) {
                cell.set_symbol("c");
                cell.fg = Color::Rgb(250, 10, 10);
                cell.bg = Color::Indexed(33);
            }
        }
    }
}

/// The colours of the first drawn line of a door at `depth` (colour on) in a 12-column surface.
fn colours_at(depth: ColorDepth) -> Vec<(Option<Color>, Option<Color>)> {
    let ui = Ui::standard().unwrap();
    let settings = TerminalSettings::default()
        .with_tier(Tier::Unicode)
        .with_color(true);
    let ctx = DrawCtx {
        strings: ui.strings(),
        look: Look::of(&settings).at_depth(depth),
        tier: Tier::Unicode,
    };
    let lines = door_lines(12, 2, &ctx, |canvas| {
        canvas
            .frame()
            .render_widget(Coloured, Rect::new(0, 0, 12, 1));
    });
    lines[0]
        .spans
        .iter()
        .map(|span| (span.style.fg, span.style.bg))
        .collect()
}

#[test]
fn truecolor_clients_get_the_doors_24_bit_colour() {
    let colours = colours_at(ColorDepth::TrueColor);
    assert_eq!(
        colours,
        [(Some(Color::Rgb(250, 10, 10)), Some(Color::Indexed(33)))]
    );
}

#[test]
fn a_256_colour_client_gets_indexed_colour_and_no_24_bit() {
    let colours = colours_at(ColorDepth::Ansi256);
    assert_eq!(colours.len(), 1);
    for (fg, bg) in colours {
        assert!(matches!(fg, Some(Color::Indexed(_))), "{fg:?}");
        assert_eq!(bg, Some(Color::Indexed(33)));
    }
}

#[test]
fn a_16_colour_client_gets_named_colours_only() {
    for (fg, bg) in colours_at(ColorDepth::Ansi16) {
        for c in [fg, bg].into_iter().flatten() {
            assert!(!matches!(c, Color::Rgb(..) | Color::Indexed(_)), "{c:?}");
        }
        assert!(fg.is_some() && bg.is_some());
    }
}

#[test]
fn a_mono_client_gets_no_colour() {
    for (fg, bg) in colours_at(ColorDepth::Mono) {
        assert!(fg.is_none_or(|c| c == Color::Reset), "{fg:?}");
        assert!(bg.is_none_or(|c| c == Color::Reset), "{bg:?}");
    }
}

#[test]
fn the_ascii_tier_keeps_the_depth_the_client_has() {
    let ui = Ui::standard().unwrap();
    let settings = TerminalSettings::default()
        .with_tier(Tier::Ascii)
        .with_color(true);
    let ctx = DrawCtx {
        strings: ui.strings(),
        look: Look::of(&settings).at_depth(ColorDepth::TrueColor),
        tier: Tier::Ascii,
    };
    let lines = door_lines(12, 2, &ctx, |canvas| {
        canvas
            .frame()
            .render_widget(Coloured, Rect::new(0, 0, 12, 1));
    });
    assert_eq!(lines[0].spans[0].style.fg, Some(Color::Rgb(250, 10, 10)));
}

#[test]
fn a_member_cap_lowers_the_depth_a_door_is_composited_at() {
    // Through the same call the session makes (`Ui::look`): a 256-colour cap on a truecolor
    // client gives 256 colours; automatic leaves the client's depth alone.
    let ui = Ui::standard().unwrap();
    let area = Rect::new(0, 0, 100, 30);
    let capped = TerminalSettings::default()
        .with_tier(Tier::Unicode)
        .with_color(true)
        .with_depth_cap(DepthCap::Ansi256);
    let auto = TerminalSettings::default()
        .with_tier(Tier::Unicode)
        .with_color(true);
    let ctx = |settings: &TerminalSettings| DrawCtx {
        strings: ui.strings(),
        look: ui.look(settings, area, ColorDepth::TrueColor),
        tier: Tier::Unicode,
    };
    let door = |ctx: &DrawCtx<'_>| {
        door_lines(12, 2, ctx, |canvas| {
            canvas
                .frame()
                .render_widget(Coloured, Rect::new(0, 0, 12, 1));
        })[0]
            .spans[0]
            .style
            .fg
    };
    assert!(matches!(door(&ctx(&capped)), Some(Color::Indexed(_))));
    assert_eq!(door(&ctx(&auto)), Some(Color::Rgb(250, 10, 10)));
}

/// The inner area of a running door's box in a terminal of `cols` by `rows` in `layout`, and
/// the status row split off beneath it.
fn door_area(layout: LayoutId, cols: u16, rows: u16) -> (usize, usize, Rect, Rect) {
    let screen = Rect::new(0, 0, cols, rows);
    let (content, status) = chrome::split_status(screen, &[SecurityMarker::LowSecurity]);
    let (width, height) = settings::inner_size_of(content, door_box(layout));
    (width, height, content, status.unwrap())
}

#[test]
fn a_door_gets_more_than_74_columns_where_the_terminal_has_them() {
    // The old reading box: 74 columns at any size.
    let (w, h, ..) = door_area(LayoutId::Classic, 80, 24);
    assert_eq!((w, h), (78, 21));
    let (w, h, ..) = door_area(LayoutId::Classic, 100, 30);
    assert_eq!((w, h), (98, 25));
    let (w, h, ..) = door_area(LayoutId::Classic, 132, 43);
    assert_eq!((w, h), (130, 38));
    // Wider than the cap: the box stops at the cap and stays centred.
    let (w, ..) = door_area(LayoutId::Classic, 200, 50);
    assert_eq!(w, usize::from(DOOR_MAX_WIDTH) - 2);
}

#[test]
fn the_focus_layout_gives_a_door_the_whole_body() {
    for (cols, rows) in [(80, 24), (100, 30), (132, 43), (200, 60)] {
        let (w, h, content, _) = door_area(LayoutId::Focus, cols, rows);
        // Only the box's border is lost: every column, and every row above the status row.
        assert_eq!(w, usize::from(cols) - 2, "{cols}x{rows}");
        assert_eq!(h, usize::from(content.height) - 2, "{cols}x{rows}");
        assert_eq!(content.height, rows - 1);
    }
}

#[test]
fn the_marker_cell_is_never_inside_a_doors_area() {
    for layout in [LayoutId::Classic, LayoutId::Focus, LayoutId::Lightbar] {
        for (cols, rows) in [(80, 24), (100, 30), (132, 43)] {
            let (w, h, content, status) = door_area(layout, cols, rows);
            // The door's area sits inside the box, which sits inside `content`; the status row
            // is the row below `content` and shares none of its rows.
            assert!(w <= usize::from(content.width) && h <= usize::from(content.height));
            assert_eq!(status.y, content.y + content.height);
            assert_eq!(status.height, 1);
            assert!(content.y + content.height <= status.y);
        }
    }
}
