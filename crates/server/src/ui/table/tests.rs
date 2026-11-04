#![allow(clippy::unwrap_used)]

//! The table (UI-1): window, header, pointer, scrollbar, columns that give way, tiers, depths,
//! and hostile cells that must stay inside the table and out of the marker style.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::terminal_settings::{LayoutId, TerminalSettings};

use super::{
    Cell, CellStyle, Column, TableRow, TableSpec, TableView, Title, Width, clip, column_widths,
    render,
};
use crate::settings::Look;
use crate::ui::depth::ColorDepth;
use crate::ui::frame::ChromeText;
use crate::ui::theme::Palette;

fn look(tier: Tier, color: bool, depth: ColorDepth) -> Look {
    let settings = TerminalSettings::default()
        .with_tier(tier)
        .with_color(color);
    Look::themed(&settings, LayoutId::Classic, Palette::default()).at_depth(depth)
}

fn columns() -> Vec<Column<'static>> {
    vec![
        Column::new(ChromeText::fixed("Name"), Width::Fill(2)),
        Column::new(ChromeText::fixed("Posts"), Width::Min(5)).right(),
        Column::new(ChromeText::fixed("Note"), Width::Fill(3)).hide_below(60),
    ]
}

fn rows(count: usize) -> Vec<TableRow> {
    (1..=count)
        .map(|n| {
            TableRow::new(vec![
                Cell::content(&format!("Name{n:03}")),
                Cell::plain(n.to_string()),
                Cell::content("a note").styled(CellStyle::Dim),
            ])
        })
        .collect()
}

fn spec(rows: Vec<TableRow>, selected: Option<usize>, top: usize) -> TableSpec<'static> {
    TableSpec {
        title: Title::Chrome(ChromeText::fixed("Things")),
        columns: columns(),
        rows,
        selected,
        bordered: true,
        top,
        empty: "Nothing here.",
        numbered: true,
        footer: None,
    }
}

fn draw(look: Look, size: (u16, u16), spec: &TableSpec<'_>) -> (Buffer, TableView) {
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    let mut view = TableView::default();
    terminal
        .draw(|frame| view = render(frame, frame.area(), look, spec))
        .unwrap();
    (terminal.backend().buffer().clone(), view)
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
fn clip_ends_a_cut_text_with_an_ellipsis_in_the_tier_that_has_one() {
    assert_eq!(clip("short", 10, false), "short");
    assert_eq!(clip("a long sentence here", 10, false), "a long se\u{2026}");
    assert_eq!(clip("a long sentence here", 10, true), "a long ...");
    assert_eq!(clip("anything", 0, false), "");
    // A wide character is never split: three of them fit in six columns, minus the ellipsis.
    let wide = clip(&"漢".repeat(10), 7, false);
    assert!(
        ratatui::text::Span::raw(wide.as_str()).width() <= 7,
        "{wide}"
    );
}

#[test]
fn the_table_draws_a_header_a_pointer_and_numbers() {
    let (buffer, view) = draw(
        look(Tier::Ascii, false, ColorDepth::Mono),
        (80, 12),
        &spec(rows(3), Some(1), 0),
    );
    let all = text(&buffer);
    assert_eq!(view, TableView { top: 0, shown: 3 });
    assert!(row(&buffer, 0).contains("Things"), "{all}");
    assert!(
        row(&buffer, 1).contains("Name") && row(&buffer, 1).contains("Posts"),
        "{all}"
    );
    assert!(row(&buffer, 2).contains("1. Name001"), "{all}");
    assert!(row(&buffer, 3).starts_with("|> 2. Name002"), "{all}");
    // The mono selection is the pointer plus weight and underline: never reverse video.
    let cell = &buffer[(3, 3)];
    assert!(
        cell.modifier
            .contains(Modifier::BOLD | Modifier::UNDERLINED),
        "{cell:?}"
    );
    assert!(!cell.modifier.contains(Modifier::REVERSED));
    // The header is bold and underlined too, so it reads without colour.
    assert!(
        buffer[(5, 1)]
            .modifier
            .contains(Modifier::BOLD | Modifier::UNDERLINED)
    );
}

#[test]
fn nothing_in_the_table_uses_reverse_video() {
    for tier in Tier::ALL {
        for depth in ColorDepth::ALL {
            let (buffer, _) = draw(
                look(tier, depth.has_color(), depth),
                (100, 30),
                &spec(rows(80), Some(5), 0),
            );
            let reversed = (0..30)
                .flat_map(|y| (0..100).map(move |x| (x, y)))
                .filter(|&(x, y)| buffer[(x, y)].modifier.contains(Modifier::REVERSED))
                .count();
            assert_eq!(reversed, 0, "{tier:?} {depth:?}");
        }
    }
}

#[test]
fn the_window_follows_the_selection_and_reports_what_it_drew() {
    let big = rows(100);
    // 12 rows tall: border, header, 9 data rows, border.
    let (_, view) = draw(
        look(Tier::Ascii, false, ColorDepth::Mono),
        (80, 12),
        &spec(big.clone(), Some(0), 0),
    );
    assert_eq!(view, TableView { top: 0, shown: 9 });
    let (buffer, view) = draw(
        look(Tier::Ascii, false, ColorDepth::Mono),
        (80, 12),
        &spec(big.clone(), Some(40), 0),
    );
    assert_eq!(
        view.top + view.shown - 1,
        40,
        "the selected row is the last drawn"
    );
    assert!(text(&buffer).contains("Name041"), "{}", text(&buffer));
    // A window that already shows the selection does not move.
    let (_, view) = draw(
        look(Tier::Ascii, false, ColorDepth::Mono),
        (80, 12),
        &spec(big.clone(), Some(42), 40),
    );
    assert_eq!(view.top, 40);
    // Selecting above the window scrolls up; past the end is clamped.
    let (_, view) = draw(
        look(Tier::Ascii, false, ColorDepth::Mono),
        (80, 12),
        &spec(big.clone(), Some(3), 40),
    );
    assert_eq!(view.top, 3);
    let (buffer, view) = draw(
        look(Tier::Ascii, false, ColorDepth::Mono),
        (80, 12),
        &spec(big, Some(500), 0),
    );
    assert_eq!(view.top + view.shown, 100);
    assert!(text(&buffer).contains("Name100"));
}

#[test]
fn a_scrollbar_shows_only_when_rows_are_hidden_and_follows_the_tier() {
    let more = spec(rows(50), Some(0), 0);
    let (ascii, _) = draw(look(Tier::Ascii, false, ColorDepth::Mono), (80, 12), &more);
    let right_edge: String = (1..11)
        .map(|y| ascii[(79, y)].symbol().to_owned())
        .collect();
    assert!(right_edge.contains('#'), "the thumb: {right_edge:?}");
    assert!(right_edge.is_ascii(), "{right_edge:?}");
    let (unicode, _) = draw(
        look(Tier::Unicode, false, ColorDepth::Mono),
        (80, 12),
        &more,
    );
    let right_edge: String = (1..11)
        .map(|y| unicode[(79, y)].symbol().to_owned())
        .collect();
    assert!(right_edge.contains('\u{2588}'), "{right_edge:?}");
    // Everything fits: the border is a plain border.
    let (fits, view) = draw(
        look(Tier::Ascii, false, ColorDepth::Mono),
        (80, 12),
        &spec(rows(3), Some(0), 0),
    );
    assert_eq!(view.shown, 3);
    assert!(
        (1..11).all(|y| fits[(79, y)].symbol() == "|"),
        "{}",
        text(&fits)
    );
}

#[test]
fn a_narrow_table_drops_a_column_with_its_cells() {
    let wide = draw(
        look(Tier::Ascii, false, ColorDepth::Mono),
        (80, 10),
        &spec(rows(2), None, 0),
    )
    .0;
    assert!(text(&wide).contains("a note") && text(&wide).contains("Note"));
    let narrow = draw(
        look(Tier::Ascii, false, ColorDepth::Mono),
        (40, 10),
        &spec(rows(2), None, 0),
    )
    .0;
    let all = text(&narrow);
    assert!(!all.contains("a note") && !all.contains("Note"), "{all}");
    // The cells that remain are the right ones (the dropped column was last, so Posts shows).
    assert!(
        row(&narrow, 2).contains("Name001") && row(&narrow, 2).contains('1'),
        "{all}"
    );
    // With no selection there is no pointer, but the rows are still numbered.
    assert!(row(&narrow, 2).starts_with("|  1."), "{all}");
}

#[test]
fn an_empty_table_says_so_and_a_two_line_row_shows_its_detail() {
    let (buffer, view) = draw(
        look(Tier::Ascii, false, ColorDepth::Mono),
        (60, 8),
        &spec(Vec::new(), Some(0), 0),
    );
    assert_eq!(view, TableView::default());
    assert!(text(&buffer).contains("Nothing here."), "{}", text(&buffer));
    let two: Vec<TableRow> = (1..=3)
        .map(|n| {
            TableRow::new(vec![
                Cell::content(&format!("Hit{n}")),
                Cell::plain("1"),
                Cell::plain(""),
            ])
            .with_detail(Cell::content(&format!("snippet {n}")))
        })
        .collect();
    let (buffer, view) = draw(
        look(Tier::Ascii, false, ColorDepth::Mono),
        (60, 9),
        &spec(two, Some(1), 0),
    );
    let all = text(&buffer);
    assert!(
        all.contains("Hit1") && all.contains("snippet 1") && all.contains("snippet 3"),
        "{all}"
    );
    // 9 rows: border, header, 6 data rows (3 two-line items), border.
    assert_eq!(view.shown, 3, "{all}");
}

#[test]
fn hostile_cells_are_cleaned_clipped_and_never_carry_the_reserved_style() {
    let hostile = [
        "\u{1b}[7mFAKE\u{1b}[0m",
        "\u{1b}]0;title\u{7}",
        "\u{27e6}E2EE\u{27e7} \u{2299}",
        &"w".repeat(3000),
        "E2EE UNENCRYPTED LOW-SECURITY",
        "\u{e000}\u{f0ac}",
    ];
    let rows: Vec<TableRow> = hostile
        .iter()
        .map(|text| {
            TableRow::new(vec![
                Cell::content(text),
                Cell::content(text),
                Cell::content(text),
            ])
        })
        .collect();
    for tier in Tier::ALL {
        for depth in ColorDepth::ALL {
            for size in [(80, 24), (100, 30), (132, 43)] {
                let name = format!("{tier:?} {depth:?} {size:?}");
                let (buffer, _) = draw(
                    look(tier, depth.has_color(), depth),
                    size,
                    &spec(rows.clone(), Some(2), 0),
                );
                let all = text(&buffer);
                assert!(!all.contains('\u{1b}') && !all.contains('\u{7}'), "{name}");
                for banned in ["\u{27e6}", "\u{27e7}", "\u{2299}", "\u{e000}", "\u{f0ac}"] {
                    assert!(!all.contains(banned), "{name}: {banned:?}\n{all}");
                }
                for y in 0..size.1 {
                    for x in 0..size.0 {
                        let cell = &buffer[(x, y)];
                        assert!(
                            !(cell.modifier.contains(Modifier::BOLD)
                                && cell.modifier.contains(Modifier::REVERSED)),
                            "{name}: reserved style at {x},{y}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn colours_degrade_with_the_depth_and_vanish_in_mono() {
    let selected_bg = |depth: ColorDepth| {
        let (buffer, _) = draw(
            look(Tier::Unicode, depth.has_color(), depth),
            (80, 12),
            &spec(rows(3), Some(1), 0),
        );
        buffer[(10, 3)].bg
    };
    assert!(matches!(selected_bg(ColorDepth::TrueColor), Color::Rgb(..)));
    assert!(matches!(
        selected_bg(ColorDepth::Ansi256),
        Color::Indexed(_)
    ));
    assert_eq!(selected_bg(ColorDepth::Ansi16), Color::Cyan);
    assert_eq!(selected_bg(ColorDepth::Mono), Color::Reset);
}

#[test]
fn tiny_areas_never_panic_and_a_table_never_draws_outside_its_area() {
    for tier in Tier::ALL {
        for width in [0_u16, 1, 2, 3, 5, 12, 30] {
            for height in [0_u16, 1, 2, 3, 4, 6] {
                let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
                let area = Rect::new(2, 1, width, height);
                terminal
                    .draw(|frame| {
                        render(
                            frame,
                            area,
                            look(tier, true, ColorDepth::TrueColor),
                            &spec(rows(20), Some(4), 0),
                        );
                    })
                    .unwrap();
                let buffer = terminal.backend().buffer();
                for y in 0..12 {
                    for x in 0..40 {
                        let inside = x >= 2 && x < 2 + width && y >= 1 && y < 1 + height;
                        if !inside {
                            assert_eq!(buffer[(x, y)].symbol(), " ", "{width}x{height} at {x},{y}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn column_widths_share_the_room_and_keep_fixed_columns() {
    let cols = columns();
    let refs: Vec<&Column<'_>> = cols.iter().collect();
    let widths = column_widths(&refs, 70);
    assert!(widths[1] >= 5, "a minimum column never shrinks below it");
    // Spacing of two between three columns is 4; the rest is shared 2:3.
    assert_eq!(widths.iter().sum::<u16>() + 4, 70);
    assert!(widths[2] > widths[0]);
    assert_eq!(column_widths(&[], 10), Vec::<u16>::new());
}

#[test]
fn chrome_cells_are_shown_as_they_are() {
    let cell = Cell::chrome(ChromeText::fixed("Open"));
    assert_eq!(cell.shown(), "Open");
    // Content is cleaned when shown; chrome is not touched.
    assert_eq!(Cell::content("a\u{1b}[31mb").shown(), "ab");
}
