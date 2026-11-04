//! The tide: the strip across the top of every screen, the day's chart, and the tide table
//! screen (the quay).

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Modifier, Style};
use russh_bbs_door::symbols::Marker;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Axis, BarChart, Chart, Dataset, GraphType, Paragraph, Row, Table};

use super::{Ui, hue};
use crate::tide::{HOLD_FLOOD_LEVEL, MINUTES_PER_DAY, SPRING_AMP, Tide, Warning};

/// `h:mm` for a number of minutes.
pub(super) fn span_text(minutes: i64) -> String {
    format!("{}h{:02}", minutes / 60, minutes % 60)
}

/// `hh:mm` for a minute of the day.
pub(super) fn clock(minute: i64) -> String {
    let m = minute.rem_euclid(MINUTES_PER_DAY);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// The word and style for a warning band, so the band is words as well as colour.
fn warning(ui: &Ui<'_>, w: Warning) -> (&'static str, Style) {
    let p = &ui.pal;
    match w {
        Warning::Calm => ("calm", p.sea()),
        Warning::High => (
            "HIGH WATER",
            p.on(hue::FOG, hue::GOLD).add_modifier(Modifier::empty()),
        ),
        Warning::Flood => ("FLOOD", p.on(hue::SALT, hue::RUST)),
        Warning::Storm => ("STORM SURGE", p.on(hue::SALT, hue::RUST)),
    }
}

/// How the band is flagged when colour is off: a bracket of marks around the word.
fn flag(ui: &Ui<'_>, w: Warning) -> String {
    let (word, _) = warning(ui, w);
    match w {
        Warning::Calm => word.to_owned(),
        Warning::High => format!("{} {word} {}", ui.look.g("▲", "^"), ui.look.g("▲", "^")),
        Warning::Flood | Warning::Storm => {
            format!("{} {word} {}", ui.look.g("▲▲", "^^"), ui.look.g("▲▲", "^^"))
        }
    }
}

/// The strip across the top: the water level as a gauge with the flood line marked, whether it
/// is rising, the time to the turn, and the warning band. With room, a third row draws the
/// day's tide with a marker where the clan's clock stands.
pub(super) fn strip(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let tide = &ui.s.tide;
    let (wait, next_high) = tide.next_turn(ui.minute);
    let next_level = tide.level(ui.minute + wait);
    let band = tide.warning(ui.level, ui.s.today);
    let table = tide.table(ui.s.today);
    let rising = next_high;
    let arrow = if rising {
        ui.look.g("↑", "^")
    } else {
        ui.look.g("↓", "v")
    };
    let row1 = Rect::new(area.x, area.y, area.width, 1.min(area.height));
    let rest = Rect::new(
        area.x,
        area.y + row1.height,
        area.width,
        area.height.saturating_sub(row1.height),
    );

    // Row 1: "TIDE [bar] 63 ↑ high in 2h10 (82)   spring tides   [ HIGH WATER ]".
    let word = flag(ui, band);
    let (_, band_style) = warning(ui, band);
    let badge = format!(" {word} ");
    let badge_w = u16::try_from(badge.chars().count()).unwrap_or(0);
    let tail = format!(
        " {:>3} {arrow} {} in {} ({next_level})  {} tides{} ",
        ui.level,
        if next_high { "high" } else { "low" },
        span_text(wait),
        table.phase,
        if table.storm { ", storm" } else { "" },
    );
    let tail_w = u16::try_from(tail.chars().count()).unwrap_or(0);
    let bar_w = row1.width.saturating_sub(9 + tail_w + badge_w).clamp(8, 40);
    let mut spans: Vec<Span<'static>> = vec![
        Span::styled(format!(" {} ", ui.look.g("≈", "~")), p.sea()),
        Span::styled("TIDE ", p.dim),
    ];
    spans.extend(level_bar(ui, ui.level, bar_w));
    spans.push(Span::styled(tail, p.plain));
    let used = u16::try_from(
        spans
            .iter()
            .map(|s| s.content.chars().count())
            .sum::<usize>(),
    )
    .unwrap_or(0);
    f.render_widget(Paragraph::new(Line::from(spans)), row1);
    if used + badge_w <= row1.width {
        let r = Rect::new(row1.right() - badge_w - 1, row1.y, badge_w, 1);
        let style = if ui.look.color && band != Warning::Calm {
            band_style
        } else if band == Warning::Calm {
            p.sea()
        } else {
            p.strong
        };
        f.render_widget(Paragraph::new(Span::styled(badge, style)), r);
    }

    // The rows below: the day's curve, and (with three rows) a caret under the clock.
    if rest.height > 0 {
        curve(ui, f, rest);
    }
}

/// The water level as a bar, the flood line marked and the part above it in the warning colour.
fn level_bar(ui: &Ui<'_>, level: i32, width: u16) -> Vec<Span<'static>> {
    let p = &ui.pal;
    let w = usize::from(width);
    let filled = usize::try_from(level.clamp(0, 100)).unwrap_or(0) * w / 100;
    let line = usize::try_from(HOLD_FLOOD_LEVEL).unwrap_or(70) * w / 100;
    let (on, off) = (ui.look.g("█", "#"), ui.look.g("░", "-"));
    (0..w)
        .map(|i| {
            if i == line {
                Span::styled(ui.look.g("┃", "|"), p.rust())
            } else if i < filled {
                let style = if i > line {
                    p.rust()
                } else if i * 100 / w.max(1) >= 60 {
                    p.gold()
                } else {
                    p.sea()
                };
                Span::styled(on, style)
            } else {
                Span::styled(off, p.slate())
            }
        })
        .collect()
}

/// The day's tide as a one-line curve (and a caret under it with room): columns before the
/// clan's clock are dim, the clock's column is marked, and water over the flood line is drawn
/// in the warning colour.
fn curve(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let w = usize::from(area.width.saturating_sub(14));
    if w < 8 || area.height == 0 {
        return;
    }
    let start = ui.s.today * MINUTES_PER_DAY;
    let now_col = usize::try_from((ui.minute - start).clamp(0, MINUTES_PER_DAY - 1)).unwrap_or(0)
        * w
        / usize::try_from(MINUTES_PER_DAY).unwrap_or(1440);
    let mut top: Vec<Span<'static>> = vec![Span::styled(" 00:00 ", p.dim)];
    let mut caret: Vec<Span<'static>> = vec![Span::raw("       ")];
    for col in 0..w {
        let minute = start
            + i64::try_from(col).unwrap_or(0) * MINUTES_PER_DAY / i64::try_from(w).unwrap_or(1);
        let level = ui.s.tide.level(minute);
        let glyph = ui
            .look
            .bar_glyph(usize::try_from(level.clamp(0, 100)).unwrap_or(0) * 8 / 100 + 1);
        let base = if level >= HOLD_FLOOD_LEVEL {
            p.rust()
        } else if col < now_col {
            p.slate()
        } else {
            p.sea()
        };
        if col == now_col && area.height < 2 {
            top.push(Span::styled(ui.look.g("◆", "*"), p.gold()));
        } else if col == now_col {
            top.push(Span::styled(
                glyph,
                if ui.look.color {
                    base.add_modifier(Modifier::UNDERLINED)
                } else {
                    p.strong
                },
            ));
        } else {
            top.push(Span::styled(glyph, base));
        }
        caret.push(Span::styled(
            if col == now_col {
                ui.look.g("▲", "^")
            } else {
                " "
            },
            p.gold(),
        ));
    }
    top.push(Span::styled(" 24:00", p.dim));
    let mut lines = vec![Line::from(top)];
    if area.height >= 2 {
        // The labelled caret: "now 14:20" under the clock's column.
        let label = format!("now {}", clock(ui.minute));
        let mut caret_line = caret;
        let at = (7 + now_col + 1).min(7 + w.saturating_sub(label.len()));
        let label_spans: Vec<Span<'static>> = label
            .chars()
            .map(|c| Span::styled(c.to_string(), p.gold()))
            .collect();
        let mut out: Vec<Span<'static>> = Vec::new();
        out.extend(caret_line.drain(..).take(at.min(7 + w)));
        out.extend(label_spans);
        // The day's turning points, as a line of times when there is a third row.
        lines.push(Line::from(out));
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// The chart of today's tide: the curve, the flood line and the clan's clock.
pub(super) fn day_chart(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, title: &str) {
    let p = &ui.pal;
    let start = ui.s.today * MINUTES_PER_DAY;
    let points: Vec<(f64, f64)> = (0..=144)
        .map(|i| {
            let m = i * 10;
            (
                f64::from(m as u32),
                f64::from(ui.s.tide.level(start + i64::from(m))),
            )
        })
        .collect();
    let flood = [
        (0.0, f64::from(HOLD_FLOOD_LEVEL)),
        (1440.0, f64::from(HOLD_FLOOD_LEVEL)),
    ];
    let now_x = f64::from(u32::try_from((ui.minute - start).clamp(0, 1440)).unwrap_or(0));
    let now = [(now_x, 0.0), (now_x, 100.0)];
    let marker = if ui.look.ascii {
        Marker::Dot
    } else {
        Marker::Braille
    };
    let datasets = vec![
        Dataset::default()
            .name("water")
            .marker(marker)
            .graph_type(GraphType::Line)
            .style(p.sea())
            .data(&points),
        Dataset::default()
            .name("flood")
            .marker(marker)
            .graph_type(GraphType::Line)
            .style(p.rust())
            .data(&flood),
        Dataset::default()
            .name("now")
            .marker(marker)
            .graph_type(GraphType::Line)
            .style(p.gold())
            .data(&now),
    ];
    let x_labels: Vec<Span<'_>> = ["00", "06", "12", "18", "24"]
        .iter()
        .map(|t| Span::styled(*t, p.dim))
        .collect();
    let y_labels: Vec<Span<'_>> = ["0", "50", "100"]
        .iter()
        .map(|t| Span::styled(*t, p.dim))
        .collect();
    let chart = Chart::new(datasets)
        .block(ui.panel(title))
        .x_axis(
            Axis::default()
                .style(p.border())
                .bounds([0.0, 1440.0])
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .style(p.border())
                .bounds([0.0, 100.0])
                .labels(y_labels),
        );
    f.render_widget(chart, area);
}

/// The stretches of today (minutes of the day) when the water is over the flood line.
pub(super) fn flood_windows(ui: &Ui<'_>) -> Vec<(i64, i64)> {
    let start = ui.s.today * MINUTES_PER_DAY;
    let mut out: Vec<(i64, i64)> = Vec::new();
    let mut open: Option<i64> = None;
    for m in (0..MINUTES_PER_DAY).step_by(10) {
        let over = ui.s.tide.level(start + m) >= HOLD_FLOOD_LEVEL;
        match (over, open) {
            (true, None) => open = Some(m),
            (false, Some(from)) => {
                out.push((from, m));
                open = None;
            }
            _ => {}
        }
    }
    if let Some(from) = open {
        out.push((from, MINUTES_PER_DAY));
    }
    out
}

/// The highs and lows of today as table rows.
pub(super) fn extremes_rows(ui: &Ui<'_>) -> Vec<Row<'static>> {
    let p = &ui.pal;
    ui.s.tide
        .table(ui.s.today)
        .extremes
        .iter()
        .map(|e| {
            let past = i64::from(e.minute) < (ui.minute - ui.s.today * MINUTES_PER_DAY);
            let style = if past { p.dim } else { p.plain };
            Row::new(vec![
                Span::styled(clock(i64::from(e.minute)), style),
                Span::styled(
                    if e.high {
                        format!("{} high", ui.look.g("▲", "^"))
                    } else {
                        format!("{} low", ui.look.g("▼", "v"))
                    },
                    if e.high { p.sea() } else { p.peat() },
                ),
                Span::styled(format!("{}", e.level), style),
            ])
        })
        .collect()
}

/// The quay: today's tide in full, the highs and lows, and the fortnight's springs and neaps.
pub(super) fn table(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let wide = area.width >= 90;
    let [left, right] = Layout::horizontal([
        Constraint::Percentage(if wide { 60 } else { 55 }),
        Constraint::Min(20),
    ])
    .areas(area);
    day_chart(ui, f, left, "Today's water");
    let [top, bottom] = Layout::vertical([Constraint::Length(8), Constraint::Min(5)]).areas(right);
    let rows = extremes_rows(ui);
    let widths = [
        Constraint::Length(7),
        Constraint::Length(8),
        Constraint::Length(6),
    ];
    let t = ui.s.tide.table(ui.s.today);
    let title = format!("Highs and lows, {} tides", t.phase);
    f.render_widget(
        Table::new(rows, widths)
            .header(Row::new(vec!["time", "turn", "water"]).style(p.dim))
            .block(ui.panel(&title)),
        top,
    );
    // The fortnight: the amplitude of each day's tide as bars, today marked.
    let bars: Vec<(String, u64)> = (0..14)
        .map(|i| {
            let day = ui.s.today + i;
            let n = u64::try_from(Tide::amplitude(day)).unwrap_or(0);
            (
                if i == 0 {
                    "*".to_owned()
                } else {
                    format!("{}", (day % 100).rem_euclid(100))
                },
                n,
            )
        })
        .collect();
    let data: Vec<(&str, u64)> = bars.iter().map(|(l, n)| (l.as_str(), *n)).collect();
    let chart = BarChart::default()
        .block(ui.panel("The fortnight: springs and neaps"))
        .data(&data)
        .bar_width(if bottom.width >= 48 { 3 } else { 2 })
        .bar_gap(0)
        .bar_style(p.sea())
        .value_style(p.dim)
        .label_style(p.dim)
        .max(u64::try_from(SPRING_AMP).unwrap_or(45))
        .bar_set(if ui.look.ascii {
            russh_bbs_door::symbols::bar::THREE_LEVELS
        } else {
            russh_bbs_door::symbols::bar::NINE_LEVELS
        });
    f.render_widget(chart, bottom);
}
