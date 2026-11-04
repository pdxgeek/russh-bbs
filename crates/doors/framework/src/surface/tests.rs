//! The compositing pass: a hostile widget, random buffers, colours, wide symbols and the ASCII
//! tier.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

use proptest::prelude::*;
use ratatui::style::Color;
use ratatui::widgets::{Block, Gauge, Paragraph, Sparkline, Widget};
use russh_bbs_core::sanitize::RESERVED_CHROME;

use super::*;
use crate::canvas::DoorPalette;

const ALL_DEPTHS: [ColorDepth; 4] = [
    ColorDepth::TrueColor,
    ColorDepth::Ansi256,
    ColorDepth::Ansi16,
    ColorDepth::Mono,
];

fn surface(width: u16, height: u16) -> DoorSurface {
    DoorSurface::new(width, height, DoorPalette::mono())
}

fn plain(clean: &CleanBuffer) -> Vec<String> {
    clean.rows_text()
}

fn private_use(c: char) -> bool {
    matches!(u32::from(c), 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x10_0000..=0x10_FFFD)
}

/// Invisible characters the sanitizer removes, listed independently of it.
fn invisible(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD | 0x200B | 0x200E..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x2069
            | 0xFEFF | 0x2800 | 0xFFFC | 0xE0000..=0xE0FFF
    )
}

/// Everything a cell on the member's screen must satisfy, whatever the door drew.
fn assert_clean_cell(cell: &Cell, depth: ColorDepth) {
    let symbol = cell.symbol();
    for c in symbol.chars() {
        assert!(!c.is_control(), "control in {symbol:?}");
        assert!(!private_use(c), "private use in {symbol:?}");
        assert!(!invisible(c), "invisible in {symbol:?}");
        assert!(
            !RESERVED_CHROME.contains(&c),
            "marker codepoint in {symbol:?}"
        );
    }
    assert!(!symbol.is_empty(), "an empty symbol");
    assert!(matches!(Span::raw(symbol).width(), 1 | 2), "{symbol:?}");
    let m = cell.modifier;
    assert!(
        !m.contains(Modifier::BOLD | Modifier::REVERSED),
        "the reserved marker style"
    );
    assert!(!m.intersects(Modifier::HIDDEN | Modifier::SLOW_BLINK | Modifier::RAPID_BLINK));
    assert_eq!(cell.diff_option, ratatui::buffer::CellDiffOption::None);
    for color in [cell.fg, cell.bg] {
        match depth {
            ColorDepth::TrueColor => {}
            ColorDepth::Ansi256 => assert!(!matches!(color, Color::Rgb(..)), "{color:?}"),
            ColorDepth::Ansi16 => assert!(
                !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
                "{color:?}"
            ),
            ColorDepth::Mono => assert_eq!(color, Color::Reset),
        }
    }
}

fn assert_clean(clean: &CleanBuffer, width: u16, height: u16, depth: ColorDepth) {
    assert_eq!((clean.width(), clean.height()), (width, height));
    assert_eq!(
        clean.buffer().content.len(),
        usize::from(width) * usize::from(height)
    );
    for cell in &clean.buffer().content {
        assert_clean_cell(cell, depth);
    }
}

/// A widget that writes whatever it likes into the buffer it is given.
struct Hostile;

impl Widget for Hostile {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut attacks: Vec<String> = [
            "\u{1b}[31mred\u{1b}[0m",
            "\u{1b}]0;owned\u{7}",
            "\u{1b}[2J",
            "\u{1b}",
            "\u{9b}31m",
            "\u{9d}0;t\u{9c}",
            "\u{90}x\u{9c}",
            "a\u{1b}[2Jb",
            "\u{7}",
            "\u{0}",
            "\r",
            "\n",
            "\u{e000}",
            "\u{f8ff}",
            "\u{f0001}",
            "\u{202e}x",
            "\u{200b}",
            "\u{feff}",
            "\u{2800}",
            "e\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}\u{301}",
            "a long symbol that is far more than one grapheme cluster",
            "",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        attacks.extend(RESERVED_CHROME.iter().map(char::to_string));
        // The markers inside a larger cluster.
        attacks.extend(RESERVED_CHROME.iter().map(|c| format!("x{c}y")));
        let width = usize::from(area.width.max(1));
        for (i, attack) in attacks.iter().enumerate() {
            let x = area.x + u16::try_from(i % width).unwrap_or(0);
            let y = area.y + u16::try_from(i / width).unwrap_or(0);
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_symbol(attack);
            }
        }
        // The reserved style, hidden text, blinking, a skip flag, a forced width.
        let row = area.y + area.height.saturating_sub(1);
        for (i, modifier) in [
            Modifier::BOLD | Modifier::REVERSED,
            Modifier::BOLD | Modifier::REVERSED | Modifier::ITALIC,
            Modifier::HIDDEN,
            Modifier::SLOW_BLINK,
            Modifier::RAPID_BLINK,
        ]
        .into_iter()
        .enumerate()
        {
            if let Some(cell) = buf.cell_mut((area.x + u16::try_from(i).unwrap_or(0), row)) {
                cell.set_symbol("M");
                cell.modifier = modifier;
                cell.fg = Color::Rgb(1, 2, 3);
                cell.bg = Color::Indexed(200);
            }
        }
        if let Some(cell) = buf.cell_mut((area.x, row.saturating_sub(1))) {
            cell.set_symbol("S");
            cell.set_diff_option(ratatui::buffer::CellDiffOption::Skip);
        }
        if let Some(cell) = buf.cell_mut((area.x + 1, row.saturating_sub(1))) {
            cell.set_symbol("F");
            cell.set_diff_option(ratatui::buffer::CellDiffOption::ForcedWidth(
                std::num::NonZeroU16::new(40).unwrap(),
            ));
        }
    }
}

#[test]
fn a_hostile_widget_shows_none_of_its_attacks_on_screen() {
    for depth in ALL_DEPTHS {
        for ascii_icons in [false, true] {
            let mut s = surface(40, 12);
            let drew = s.draw(|canvas| {
                canvas
                    .frame()
                    .render_widget(Hostile, Rect::new(0, 0, 40, 12));
            });
            assert_eq!(drew, Drew::Done);
            let clean = s.composite(Composite::new(depth, ascii_icons));
            assert_clean(&clean, 40, 12, depth);
            let text = plain(&clean).join("\n");
            assert!(!text.contains('\u{1b}'));
            // The sequence is gone and the text around it stays: a cell holding
            // `ESC[31mred ESC[0m` is three columns, too many for one cell, so it keeps its
            // first character.
            assert!(text.starts_with('r'), "{text}");
            assert!(!text.contains("[31m") && !text.contains("[0m"), "{text}");
        }
    }
}

#[test]
fn the_reserved_marker_style_is_removed_but_bold_and_reverse_alone_are_kept() {
    let mut s = surface(6, 1);
    s.draw(|canvas| {
        let mut frame = canvas.frame();
        for (x, modifier) in [
            (0, Modifier::BOLD | Modifier::REVERSED),
            (1, Modifier::BOLD),
            (2, Modifier::REVERSED),
            (
                3,
                Modifier::BOLD | Modifier::REVERSED | Modifier::UNDERLINED,
            ),
            (4, Modifier::ITALIC | Modifier::DIM),
        ] {
            frame.render_widget(
                Paragraph::new("x").style(Style::new().add_modifier(modifier)),
                Rect::new(x, 0, 1, 1),
            );
        }
    });
    for depth in ALL_DEPTHS {
        let clean = s.composite(Composite::new(depth, false));
        let cell = |x| &clean.buffer()[(x, 0)];
        assert_eq!(cell(0).modifier, Modifier::REVERSED);
        assert_eq!(cell(1).modifier, Modifier::BOLD);
        assert_eq!(cell(2).modifier, Modifier::REVERSED);
        assert_eq!(cell(3).modifier, Modifier::REVERSED | Modifier::UNDERLINED);
        assert_eq!(cell(4).modifier, Modifier::ITALIC | Modifier::DIM);
    }
}

#[test]
fn a_widget_given_a_huge_or_far_away_area_draws_only_what_fits() {
    let mut s = surface(10, 4);
    s.draw(|canvas| {
        let mut frame = canvas.frame();
        frame.render_widget(
            Paragraph::new("x".repeat(5000)).block(Block::bordered()),
            Rect::new(0, 0, 5000, 5000),
        );
        frame.render_widget(Paragraph::new("far"), Rect::new(u16::MAX - 1, 2, 50, 50));
        frame.render_widget(Paragraph::new("below"), Rect::new(0, 9, 5, 1));
        frame.render_widget(Paragraph::new("gone"), Rect::new(10, 0, 5, 1));
        frame.render_widget(Paragraph::new("zero"), Rect::new(0, 0, 0, 0));
        // The whole area is the door's, relative to itself.
        assert_eq!(frame.area(), Rect::new(0, 0, 10, 4));
    });
    let clean = s.composite(Composite::new(ColorDepth::Mono, false));
    assert_clean(&clean, 10, 4, ColorDepth::Mono);
    let rows = plain(&clean);
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|r| r.chars().count() <= 10), "{rows:?}");
    assert!(!rows.join("").contains("far") && !rows.join("").contains("below"));
}

#[test]
fn a_widget_that_resizes_or_moves_the_buffer_cannot_enlarge_the_output() {
    struct Grow;
    impl Widget for Grow {
        fn render(self, _area: Rect, buf: &mut Buffer) {
            buf.resize(Rect::new(0, 0, 300, 200));
            if let Some(cell) = buf.cell_mut((250, 150)) {
                cell.set_symbol("Z");
            }
            buf.area = Rect::new(3, 2, 300, 200);
        }
    }
    struct Shrink;
    impl Widget for Shrink {
        fn render(self, _area: Rect, buf: &mut Buffer) {
            buf.resize(Rect::new(0, 0, 2, 1));
        }
    }
    for hostile in [0, 1] {
        let mut s = surface(12, 5);
        s.draw(|canvas| {
            let mut frame = canvas.frame();
            frame.render_widget(Paragraph::new("hello"), Rect::new(0, 0, 12, 5));
            if hostile == 0 {
                frame.render_widget(Grow, Rect::new(0, 0, 12, 5));
            } else {
                frame.render_widget(Shrink, Rect::new(0, 0, 12, 5));
            }
        });
        let clean = s.composite(Composite::new(ColorDepth::Ansi16, false));
        assert_clean(&clean, 12, 5, ColorDepth::Ansi16);
    }
}

#[test]
fn a_panic_in_a_door_is_caught_and_shows_a_notice() {
    let mut s = surface(40, 3);
    s.draw(|canvas| {
        canvas.text(0, 2, "half drawn", DoorStyle::Plain);
    });
    let drew = s.draw(|canvas| {
        canvas.text(0, 0, "before", DoorStyle::Plain);
        panic!("a widget fell over");
    });
    assert_eq!(drew, Drew::Failed);
    let rows = plain(&s.composite(Composite::new(ColorDepth::Mono, false)));
    assert_eq!(rows[0], "This door had a problem drawing.");
    assert_eq!(rows[2], "", "the half-drawn screen is gone");
}

#[test]
fn colors_are_degraded_to_the_members_depth() {
    let mut s = surface(4, 1);
    s.draw(|canvas| {
        let mut frame = canvas.frame();
        frame.render_widget(
            Paragraph::new("ab").style(
                Style::new()
                    .fg(Color::Rgb(255, 0, 0))
                    .bg(Color::Indexed(21)),
            ),
            Rect::new(0, 0, 2, 1),
        );
        frame.render_widget(
            Paragraph::new("cd").style(Style::new().fg(Color::Cyan).bg(Color::Reset)),
            Rect::new(2, 0, 2, 1),
        );
    });
    let at = |depth| {
        let clean = s.composite(Composite::new(depth, false));
        let a = clean.buffer()[(0, 0)].clone();
        let c = clean.buffer()[(2, 0)].clone();
        ((a.fg, a.bg), (c.fg, c.bg))
    };
    assert_eq!(
        at(ColorDepth::TrueColor),
        (
            (Color::Rgb(255, 0, 0), Color::Indexed(21)),
            (Color::Cyan, Color::Reset)
        )
    );
    assert_eq!(
        at(ColorDepth::Ansi256),
        (
            (Color::Indexed(196), Color::Indexed(21)),
            (Color::Cyan, Color::Reset)
        )
    );
    assert_eq!(
        at(ColorDepth::Ansi16),
        ((Color::LightRed, Color::Blue), (Color::Cyan, Color::Reset))
    );
    assert_eq!(
        at(ColorDepth::Mono),
        ((Color::Reset, Color::Reset), (Color::Reset, Color::Reset))
    );
}

#[test]
fn theme_roles_and_a_doors_own_colors_both_work() {
    let palette = DoorPalette::new([
        Style::new(),
        Style::new(),
        Style::new(),
        Style::new().fg(Color::Magenta),
        Style::new(),
        Style::new(),
        Style::new(),
    ]);
    let mut s = DoorSurface::new(4, 1, palette);
    s.draw(|canvas| {
        canvas.text(0, 0, "a", DoorStyle::Accent);
        let own = Style::new().fg(Color::Rgb(0, 200, 0));
        canvas
            .frame()
            .render_widget(Paragraph::new("b").style(own), Rect::new(1, 0, 1, 1));
    });
    let clean = s.composite(Composite::new(ColorDepth::Ansi16, false));
    assert_eq!(clean.buffer()[(0, 0)].fg, Color::Magenta);
    assert_eq!(clean.buffer()[(1, 0)].fg, Color::Green);
}

#[test]
fn wide_symbols_take_two_cells_and_never_overflow_the_row() {
    let mut s = surface(5, 2);
    s.draw(|canvas| {
        canvas.text(0, 0, "a日本", DoorStyle::Plain);
        // A wide symbol in the last column has no room.
        canvas.text(0, 1, "abcd日", DoorStyle::Plain);
    });
    let clean = s.composite(Composite::new(ColorDepth::Mono, false));
    assert_eq!(plain(&clean), vec!["a日本".to_owned(), "abcd".to_owned()]);
    // A door that writes into the cell a wide symbol covers loses that write.
    let mut s = surface(4, 1);
    s.draw(|canvas| {
        canvas.text(0, 0, "日", DoorStyle::Plain);
    });
    s.buf.cell_mut((1, 0)).unwrap().set_symbol("X");
    let clean = s.composite(Composite::new(ColorDepth::Mono, false));
    assert_eq!(plain(&clean), vec!["日".to_owned()]);
    let lines = clean.to_lines();
    assert_eq!(lines[0].to_string(), "日");
}

#[test]
fn the_ascii_tier_replaces_icon_characters_and_keeps_text() {
    let mut s = surface(30, 10);
    s.draw(|canvas| {
        let mut frame = canvas.frame();
        frame.render_widget(
            Block::bordered().title("Résumé 日本"),
            Rect::new(0, 0, 30, 5),
        );
        frame.render_widget(
            Sparkline::default().data([0, 1, 2, 3, 4, 5, 6, 7]),
            Rect::new(1, 6, 8, 1),
        );
        frame.render_widget(
            Gauge::default().ratio(0.5).use_unicode(true),
            Rect::new(0, 8, 20, 1),
        );
    });
    let unicode = plain(&s.composite(Composite::new(ColorDepth::Mono, false))).join("\n");
    assert!(unicode.contains('┌') && unicode.contains('▁'), "{unicode}");
    let ascii = plain(&s.composite(Composite::new(ColorDepth::Mono, true))).join("\n");
    assert!(unicode.contains("Résumé 日本") && ascii.contains("Résumé 日本"));
    let icons: String = ascii.replace("Résumé 日本", "");
    assert!(icons.is_ascii(), "{icons}");
    assert!(ascii.starts_with('+') && ascii.contains('|'), "{ascii}");
}

#[test]
fn lines_keep_a_coloured_blank_and_drop_a_plain_one() {
    let mut s = surface(8, 2);
    s.draw(|canvas| {
        let mut frame = canvas.frame();
        frame.render_widget(
            Paragraph::new("   ").style(Style::new().bg(Color::Blue)),
            Rect::new(0, 0, 3, 1),
        );
        frame.render_widget(Paragraph::new("hi"), Rect::new(0, 1, 2, 1));
    });
    let lines = s
        .composite(Composite::new(ColorDepth::Ansi16, false))
        .to_lines();
    assert_eq!(lines[0].width(), 3, "the blue blanks are drawn");
    assert_eq!(lines[1].to_string(), "hi", "the plain blanks are not");
}

#[test]
fn many_distinct_symbols_stay_clean_past_the_memory_cap() {
    let mut s = surface(100, 40);
    for (i, cell) in s.buf.content.iter_mut().enumerate() {
        // Every cell a different CJK ideograph or a private-use hazard.
        let code = if i % 7 == 0 {
            0xE000 + u32::try_from(i % 0x1000).unwrap()
        } else {
            0x4E00 + u32::try_from(i).unwrap()
        };
        cell.set_symbol(&char::from_u32(code).unwrap().to_string());
    }
    let clean = s.composite(Composite::new(ColorDepth::Ansi16, true));
    assert_eq!(clean.width(), 100);
    for cell in &clean.buffer().content {
        assert_clean_cell(cell, ColorDepth::Ansi16);
    }
}

// ---- Property tests over random buffers ----

fn arb_char() -> impl Strategy<Value = char> {
    prop_oneof![
        4 => any::<char>(),
        3 => (0x20u32..0x7f).prop_map(|c| char::from_u32(c).unwrap()),
        2 => (0u32..0xA0).prop_map(|c| char::from_u32(c).unwrap()),
        2 => (0xE000u32..0xF900).prop_map(|c| char::from_u32(c).unwrap()),
        2 => proptest::sample::select(RESERVED_CHROME.to_vec()),
        1 => proptest::sample::select(vec!['\u{1b}', '[', ']', '\u{200d}', '\u{fe0f}', '\u{301}', '\u{2800}', '\u{ffd}', '日', '█', '⣿']),
    ]
}

fn arb_symbol() -> impl Strategy<Value = String> {
    proptest::collection::vec(arb_char(), 0..6).prop_map(|cs| cs.into_iter().collect())
}

fn arb_color() -> impl Strategy<Value = Color> {
    prop_oneof![
        Just(Color::Reset),
        proptest::sample::select(vec![
            Color::Black,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::Gray,
            Color::DarkGray,
            Color::LightRed,
            Color::LightGreen,
            Color::LightYellow,
            Color::LightBlue,
            Color::LightMagenta,
            Color::LightCyan,
            Color::White,
        ]),
        (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(r, g, b)| Color::Rgb(r, g, b)),
        any::<u8>().prop_map(Color::Indexed),
    ]
}

type RawCell = (String, Color, Color, u16, bool);

fn arb_cell() -> impl Strategy<Value = RawCell> {
    (
        arb_symbol(),
        arb_color(),
        arb_color(),
        any::<u16>(),
        any::<bool>(),
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn any_buffer_composites_to_clean_cells_of_the_same_size(
        width in 1u16..30,
        height in 1u16..8,
        cells in proptest::collection::vec(arb_cell(), 240),
        depth in proptest::sample::select(ALL_DEPTHS.to_vec()),
        ascii_icons in any::<bool>(),
    ) {
        let mut s = surface(width, height);
        for (cell, (symbol, fg, bg, bits, skip)) in s.buf.content.iter_mut().zip(&cells) {
            cell.set_symbol(symbol);
            cell.fg = *fg;
            cell.bg = *bg;
            cell.modifier = Modifier::from_bits_truncate(*bits);
            if *skip {
                cell.set_diff_option(ratatui::buffer::CellDiffOption::Skip);
            }
        }
        let options = Composite::new(depth, ascii_icons);
        let clean = s.composite(options);
        assert_clean(&clean, width, height, depth);
        if ascii_icons {
            for cell in &clean.buffer().content {
                for c in cell.symbol().chars() {
                    prop_assert!(
                        !matches!(u32::from(c), 0x2500..=0x259F | 0x2800..=0x28FF | 0xFFFD),
                        "{c:?}"
                    );
                }
            }
        }
        // Cleaning twice changes nothing more.
        let mut again = surface(width, height);
        again.buf = clean.buffer().clone();
        let second = again.composite(options);
        for (i, (a, b)) in clean.buffer().content.iter().zip(&second.buffer().content).enumerate() {
            prop_assert!(a == b, "cell {i}: {a:?} then {b:?}");
        }
        // The lines never carry more than the area's columns or any escape.
        for line in clean.to_lines() {
            prop_assert!(line.width() <= usize::from(width));
            prop_assert!(!line.to_string().contains('\x1b'));
        }
    }

    #[test]
    fn text_through_the_canvas_is_clean_and_inside_whatever_the_input(
        width in 1u16..40,
        height in 1u16..6,
        x in 0u16..60,
        y in 0u16..10,
        text in proptest::collection::vec(arb_char(), 0..60).prop_map(|cs| cs.into_iter().collect::<String>()),
    ) {
        let mut s = surface(width, height);
        s.draw(|canvas| {
            canvas.text(x, y, &text, DoorStyle::Plain);
            canvas.center(y, &text, DoorStyle::Strong);
        });
        let clean = s.composite(Composite::new(ColorDepth::Mono, false));
        assert_clean(&clean, width, height, ColorDepth::Mono);
    }
}
