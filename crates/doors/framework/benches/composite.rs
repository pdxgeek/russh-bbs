//! What compositing a door's screen costs (UI-5, D104). A plain timing loop, run with
//! `cargo bench -p russh-bbs-door`; it prints the mean time per frame for an 80x24 and a 200x60
//! door area and for three kinds of screen:
//!
//! * `text`: ASCII only, the cheapest case (one comparison per cell);
//! * `widgets`: a bordered table, a gauge, a sparkline and a braille chart, as the example door
//!   draws them (box drawing, blocks and braille, a few dozen distinct symbols);
//! * `hostile`: every cell a different symbol, half of them needing cleaning (private-use
//!   glyphs and marker codepoints), the worst case for the cleaning memo.
//!
//! Each time includes `composite` and `to_lines` (what the board does per frame). The board
//! redraws at most 20 times a second per session (D49), so a frame has 50 ms; the numbers below
//! are in microseconds.
#![allow(clippy::print_stdout, clippy::unwrap_used)]

use std::hint::black_box;
use std::time::Instant;

use russh_bbs_door::buffer::Buffer;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Color, Style};
use russh_bbs_door::symbols::Marker;
use russh_bbs_door::widgets::{
    Axis, Block, Cell, Chart, Dataset, Gauge, GraphType, Paragraph, Row, Sparkline, Table, Widget,
};
use russh_bbs_door::{ColorDepth, Composite, DoorPalette, DoorStyle, DoorSurface};

fn text(width: u16, height: u16) -> DoorSurface {
    let mut surface = DoorSurface::new(width, height, DoorPalette::mono());
    surface.draw(|canvas| {
        for y in 0..height {
            canvas.text(
                0,
                y,
                &"the quick brown fox jumps over the lazy dog ".repeat(8),
                DoorStyle::Plain,
            );
        }
    });
    surface
}

fn widgets(width: u16, height: u16) -> DoorSurface {
    let mut surface = DoorSurface::new(width, height, DoorPalette::mono());
    surface.draw(|canvas| {
        let mut frame = canvas.frame();
        let area = frame.area();
        let [top, middle, bottom] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .areas(area);
        frame.render_widget(
            Gauge::default()
                .use_unicode(true)
                .ratio(0.62)
                .gauge_style(Style::new().fg(Color::Rgb(232, 163, 61))),
            top,
        );
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(middle);
        let rows: Vec<Row<'_>> = (0..usize::from(left.height))
            .map(|n| {
                Row::new([
                    Cell::from(format!("{n}")),
                    Cell::from("heads"),
                    Cell::from("won"),
                ])
            })
            .collect();
        frame.render_widget(
            Table::new(
                rows,
                [
                    Constraint::Length(4),
                    Constraint::Length(8),
                    Constraint::Min(4),
                ],
            )
            .block(Block::bordered().title("Table")),
            left,
        );
        let points: Vec<(f64, f64)> = (0..120u16)
            .map(|x| (f64::from(x), (f64::from(x) / 7.0).sin() * 10.0 + 10.0))
            .collect();
        frame.render_widget(
            Chart::new(vec![
                Dataset::default()
                    .marker(Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::new().fg(Color::Rgb(61, 196, 176)))
                    .data(&points),
            ])
            .block(Block::bordered().title("Chart"))
            .x_axis(Axis::default().bounds([0.0, 120.0]))
            .y_axis(Axis::default().bounds([0.0, 20.0])),
            right,
        );
        let data: Vec<u64> = (0..u64::from(width)).map(|n| n % 9).collect();
        frame.render_widget(
            Sparkline::default().data(&data).block(Block::bordered()),
            bottom,
        );
        frame.render_widget(Paragraph::new(""), Rect::new(0, 0, 0, 0));
    });
    surface
}

/// Writes a different symbol into every cell, half of them private-use glyphs.
struct EveryCellDifferent;

impl Widget for EveryCellDifferent {
    fn render(self, _area: Rect, buf: &mut Buffer) {
        let mut next = 0x4E00u32;
        for (n, cell) in buf.content.iter_mut().enumerate() {
            let symbol = if n % 2 == 0 {
                char::from_u32(0xE000 + u32::try_from(n % 0x1800).unwrap()).unwrap()
            } else {
                next += 1;
                char::from_u32(next).unwrap_or('x')
            };
            cell.set_symbol(&symbol.to_string());
        }
    }
}

fn hostile(width: u16, height: u16) -> DoorSurface {
    let mut surface = DoorSurface::new(width, height, DoorPalette::mono());
    surface.draw(|canvas| {
        let area = canvas.frame().area();
        canvas.frame().render_widget(EveryCellDifferent, area);
    });
    surface
}

fn time(name: &str, surface: &DoorSurface, depth: ColorDepth, ascii: bool) {
    let options = Composite::new(depth, ascii);
    let runs = 300u32;
    // Warm up, then measure.
    for _ in 0..20 {
        black_box(surface.composite(options).to_lines());
    }
    let start = Instant::now();
    for _ in 0..runs {
        black_box(surface.composite(options).to_lines());
    }
    let per_frame = start.elapsed() / runs;
    println!(
        "{name:<8} {:>3}x{:<3} {depth:?}{} {:>8.1} us/frame",
        surface.width(),
        surface.height(),
        if ascii { " ascii" } else { "" },
        per_frame.as_secs_f64() * 1e6,
    );
}

fn main() {
    for (width, height) in [(80, 24), (200, 60)] {
        time("text", &text(width, height), ColorDepth::Ansi16, false);
        for ascii in [false, true] {
            time(
                "widgets",
                &widgets(width, height),
                ColorDepth::Ansi16,
                ascii,
            );
        }
        time(
            "widgets",
            &widgets(width, height),
            ColorDepth::TrueColor,
            false,
        );
        time(
            "hostile",
            &hostile(width, height),
            ColorDepth::Ansi16,
            false,
        );
    }
}
