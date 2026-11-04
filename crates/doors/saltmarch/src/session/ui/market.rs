//! The market at the Tidecourt: prices with sparklines, what the clan holds, and the keys to
//! sell and buy.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Row, Sparkline, Table, Wrap};

use super::{Size, Ui};
use crate::types::Resource;
use crate::world::World;

/// A trail of prices as a string of bar glyphs scaled between its own lowest and highest.
fn spark(ui: &Ui<'_>, trail: &[u16], width: usize) -> String {
    let take = trail.len().saturating_sub(width);
    let trail = &trail[take..];
    let (lo, hi) = trail
        .iter()
        .fold((u16::MAX, 0u16), |(lo, hi), &p| (lo.min(p), hi.max(p)));
    let span = u32::from(hi.saturating_sub(lo)).max(1);
    let mut out: String = trail
        .iter()
        .map(|&p| {
            let level = if hi == lo {
                4
            } else {
                1 + usize::try_from(u32::from(p - lo) * 7 / span).unwrap_or(0)
            };
            ui.look.bar_glyph(level)
        })
        .collect();
    while out.chars().count() < width {
        out.insert_str(0, ui.look.bar_glyph(0));
    }
    out
}

/// The percentage change from yesterday to today, if there is a yesterday.
fn change(trail: &[u16]) -> Option<i32> {
    let [.., before, now] = trail[..] else {
        return None;
    };
    let before = i32::from(before).max(1);
    Some((i32::from(now) - before) * 100 / before)
}

fn price(p: u16) -> String {
    format!("{}.{:02}", p / 100, p % 100)
}

/// The resource a sell or buy tag is about.
fn resource_of(tag: &str) -> Option<Resource> {
    let mut parts = tag.split(':');
    matches!(parts.next(), Some("sell" | "buy"))
        .then(|| parts.next().and_then(Resource::from_id))
        .flatten()
}

pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let world = &ui.s.world;
    let roomy = ui.look.size >= Size::Roomy;
    let (table_area, side) = if roomy {
        let [a, b] = Layout::horizontal([Constraint::Min(60), Constraint::Length(36)]).areas(area);
        (a, Some(b))
    } else {
        (area, None)
    };
    let spark_w = if roomy { 14 } else { 10 };
    let rows: Vec<Row<'static>> = Resource::ALL
        .iter()
        .map(|&r| {
            let trail = world.price_trail(r);
            let now = world.prices[r.index()];
            let base = r.base_price() * 100;
            let trend = change(&trail);
            let (trend_text, trend_style): (String, Style) = match trend {
                Some(t) if t > 0 => (format!("{}{t}%", ui.look.g("▲", "^")), p.gold()),
                Some(t) if t < 0 => (format!("{}{}%", ui.look.g("▼", "v"), -t), p.sea()),
                Some(_) => (format!("{}0%", ui.look.g("•", "=")), p.dim),
                None => ("-".to_owned(), p.dim),
            };
            let on_cursor = ui
                .entries
                .get(ui.s.cursor)
                .and_then(|e| resource_of(&e.tag))
                == Some(r);
            let price_style = if now > base + base / 10 {
                p.gold()
            } else if now + base / 10 < base {
                p.sea()
            } else {
                p.plain
            };
            let have = ui.clan.stock.get(r);
            let pick = |style: Style| if on_cursor { p.selected } else { style };
            let key = |prefix: &str| {
                ui.entries_like(&[prefix])
                    .find(|(_, e)| resource_of(&e.tag) == Some(r))
                    .map_or_else(
                        || Span::styled(" - ", pick(p.slate())),
                        |(i, _)| Span::styled(format!("[{}]", Ui::key_of(i)), pick(p.accent)),
                    )
            };

            Row::new(vec![
                Line::from(Span::styled(
                    r.name(),
                    if on_cursor { p.selected } else { p.plain },
                )),
                Line::from(Span::styled(
                    format!("{have}"),
                    pick(if have == 0 { p.slate() } else { p.salt() }),
                )),
                Line::from(Span::styled(price(now), pick(price_style))),
                Line::from(Span::styled(trend_text, pick(trend_style))),
                Line::from(Span::styled(
                    if trail.len() < 3 {
                        "none yet".to_owned()
                    } else {
                        spark(ui, &trail, spark_w)
                    },
                    pick(if trail.len() < 3 { p.slate() } else { p.sea() }),
                )),
                Line::from(key("sell:")),
                Line::from(key("buy:")),
            ])
        })
        .collect();
    f.render_widget(
        Table::new(
            rows,
            [
                Constraint::Min(14),
                Constraint::Length(5),
                Constraint::Length(7),
                Constraint::Length(6),
                Constraint::Length(u16::try_from(spark_w).unwrap_or(10)),
                Constraint::Length(5),
                Constraint::Length(5),
            ],
        )
        .header(
            Row::new(vec![
                "goods",
                "held",
                "price",
                "day",
                "fortnight",
                "sell",
                "buy",
            ])
            .style(p.dim),
        )
        .block(if ui.look.size == Size::Tiny {
            russh_bbs_door::widgets::Block::new()
        } else {
            ui.panel("The Tidecourt market (tallies)")
        }),
        table_area,
    );
    if let Some(side) = side {
        detail(ui, f, side);
    }
}

/// The resource on the lightbar (or the first), in full: a big sparkline and the spread.
fn detail(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let r = ui
        .entries
        .get(ui.s.cursor)
        .and_then(|e| resource_of(&e.tag))
        .unwrap_or(Resource::Salt);
    let trail = ui.s.world.price_trail(r);
    let [top, bottom] = Layout::vertical([Constraint::Length(8), Constraint::Min(4)]).areas(area);
    let data: Vec<u64> = trail.iter().map(|&x| u64::from(x)).collect();
    let spark_w = Sparkline::default()
        .block(ui.panel(&format!("{}: two weeks", r.name())))
        .data(&data)
        .style(p.sea())
        .bar_set(if ui.look.ascii {
            russh_bbs_door::symbols::bar::THREE_LEVELS
        } else {
            russh_bbs_door::symbols::bar::NINE_LEVELS
        });
    f.render_widget(spark_w, top);
    let prices = &ui.s.world.prices;
    let base = r.base_price() * 100;
    let now = prices[r.index()];
    let lines = vec![
        Line::from(vec![
            Span::styled("price now   ", p.dim),
            Span::styled(price(now), p.salt()),
            Span::styled(format!("   usual {}", price(base)), p.dim),
        ]),
        Line::from(vec![
            Span::styled("you sell at ", p.dim),
            Span::styled(format!("{}", World::sell_value(prices, r, 1)), p.moss()),
            Span::styled("  (90% of the price)", p.dim),
        ]),
        Line::from(vec![
            Span::styled("you buy at  ", p.dim),
            Span::styled(format!("{}", World::buy_cost(prices, r, 1)), p.gold()),
            Span::styled("  (115% of the price)", p.dim),
        ]),
        Line::styled(
            "What clans sell pushes tomorrow's price down; what they buy pushes it up. Prices drift back to the usual.",
            p.dim,
        ),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(ui.panel("The spread")),
        bottom,
    );
}
