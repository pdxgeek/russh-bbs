#![allow(clippy::unwrap_used)]
//! What reaches the member's screen from a door that draws what it likes (UI-5, D103).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Gauge, Paragraph, Sparkline, Widget};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::sanitize::RESERVED_CHROME;
use russh_bbs_core::terminal_settings::TerminalSettings;

use super::draw::{DrawCtx, door_lines};
use crate::settings::Look;
use crate::ui::Ui;

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
