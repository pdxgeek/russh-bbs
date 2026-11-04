//! The Tidecourt and what hangs off it: the town square, the post, the league table and the
//! Hall of Tides.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::symbols::Marker;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::canvas::{Canvas, Points};
use russh_bbs_door::widgets::{Paragraph, Row, Table, Wrap};

use super::{Ground, Size, Ui, fit, hue, landscape};
use crate::lore::{Daypart, Water};
use crate::map::SIZE;
use crate::types::Resource;
use crate::world::World;

/// The league as `(name, score, is_us)`, best first. Our own score is live; the others are as
/// they were when they last saved.
pub(super) fn standings(ui: &Ui<'_>) -> Vec<(String, u32, bool)> {
    let mine = ui.clan.score(&ui.s.cat);
    let mut out: Vec<(String, u32, bool)> =
        ui.s.world
            .standings()
            .into_iter()
            .filter(|c| c.name != ui.clan.name)
            .map(|c| (c.name.clone(), c.score, false))
            .collect();
    out.push((ui.clan.name.clone(), mine, true));
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// The rank of our clan, from 1.
pub(super) fn our_rank(ui: &Ui<'_>) -> usize {
    standings(ui).iter().position(|c| c.2).map_or(1, |i| i + 1)
}

/// A small standings table for the morning report.
pub(super) fn standings_panel(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, rows: usize) {
    let p = &ui.pal;
    let all = standings(ui);
    let rank = our_rank(ui);
    let title = format!("Standings: you are {} of {}", ordinal(rank), all.len());
    let top = all
        .iter()
        .enumerate()
        .filter(|(i, c)| *i < rows.saturating_sub(1) || c.2);
    let width = usize::from(area.width).saturating_sub(18).max(6);
    let lead = all.first().map_or(1, |c| c.1.max(1));
    let table_rows: Vec<Row<'static>> = top
        .take(rows.max(1))
        .map(|(i, (name, score, us))| {
            let style = if *us { p.gold() } else { p.plain };
            Row::new(vec![
                Span::styled(format!("{:>2}", i + 1), p.dim),
                Span::styled(fit(name, width), style),
                Span::styled(format!("{score:>4}"), style),
            ])
        })
        .collect();
    let _ = lead;
    f.render_widget(
        Table::new(
            table_rows,
            [
                Constraint::Length(3),
                Constraint::Min(6),
                Constraint::Length(5),
            ],
        )
        .block(ui.panel(&title)),
        area,
    );
}

fn ordinal(n: usize) -> String {
    let suffix = match (n % 100, n % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// The biggest movers in the market since yesterday: `(resource, percent)`.
fn movers(ui: &Ui<'_>) -> Vec<(Resource, i32)> {
    let mut out: Vec<(Resource, i32)> = Resource::ALL
        .iter()
        .filter_map(|&r| {
            let trail = ui.s.world.price_trail(r);
            let [.., before, now] = trail[..] else {
                return None;
            };
            let before = i32::from(before).max(1);
            Some((r, (i32::from(now) - before) * 100 / before))
        })
        .filter(|(_, pct)| *pct != 0)
        .collect();
    out.sort_by_key(|(_, pct)| std::cmp::Reverse(pct.abs()));
    out.truncate(3);
    out
}

/// The town square.
pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let pic_h = (area.height * 2 / 5).clamp(6, 14);
    let [pic, rest] = Layout::vertical([Constraint::Length(pic_h), Constraint::Min(6)]).areas(area);
    let block = ui.panel("The Tidecourt");
    let inner = block.inner(pic);
    f.render_widget(block, pic);
    let day = crate::lore::Daypart::of(ui.minute);
    f.render_widget(
        landscape(
            ui.look,
            p,
            (inner.width, inner.height),
            Ground::Court,
            day,
            Water::of(ui.level),
            0xC0DE,
        ),
        inner,
    );
    let [menu, news] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Min(24)]).areas(rest);
    let items: Vec<_> = ui.entries.iter().enumerate().collect();
    ui.orders(f, menu, "Where to", items);
    let mut lines = vec![Line::from(Span::styled(
        "The Tidecourt stands on stilts where no tide has ever reached it.",
        p.dim,
    ))];
    let m = movers(ui);
    if m.is_empty() {
        lines.push(Line::from(Span::styled(
            "The market is quiet: prices are where they were yesterday.",
            p.plain,
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Market movers since last night:",
            p.plain,
        )));
        for (r, pct) in m {
            let (arrow, style) = if pct > 0 {
                (ui.look.g("▲", "^"), p.gold())
            } else {
                (ui.look.g("▼", "v"), p.sea())
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {arrow} "), style),
                Span::styled(format!("{} {pct:+}%", r.name()), style),
            ]));
        }
    }
    lines.push(Line::from(Span::styled(
        format!(
            "{} hold{} registered this season; you stand {}.",
            ui.s.world.clans.len().max(1),
            if ui.s.world.clans.len() <= 1 {
                " is"
            } else {
                "s are"
            },
            ordinal(our_rank(ui))
        ),
        p.plain,
    )));
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(ui.panel("Notices")),
        news,
    );
}

/// The post: no letters from other clans yet, and the notices that are true today.
pub(super) fn letters(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Min(24)]).areas(area);
    let tomorrow = ui.s.tide.table(ui.s.today + 1);
    let mut lines = vec![
        Line::from(Span::styled("Letters", p.accent)),
        Line::from(Span::styled(
            "  None waiting. Letters between clan leaders arrive with the social update.",
            p.dim,
        )),
        Line::raw(""),
        Line::from(Span::styled("Notices pinned at the Tidecourt", p.accent)),
        Line::from(vec![
            Span::styled(format!("  {} ", ui.look.g("≈", "~")), p.sea()),
            Span::styled(
                format!(
                    "The harbour-reeve says tomorrow's tides are {}{}.",
                    tomorrow.phase,
                    if tomorrow.storm {
                        ", with a storm surge"
                    } else {
                        ""
                    }
                ),
                p.plain,
            ),
        ]),
    ];
    let gate = ui.clan.candidates.len();
    lines.push(Line::from(vec![
        Span::styled(format!("  {} ", ui.look.g("▸", ">")), p.peat()),
        Span::styled(
            format!(
                "{gate} hand{} wait{} at your gate for work.",
                if gate == 1 { "" } else { "s" },
                if gate == 1 { "s" } else { "" }
            ),
            p.plain,
        ),
    ]));
    for (r, pct) in movers(ui) {
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", ui.look.g("◈", "$")), p.gold()),
            Span::styled(format!("{} is {pct:+}% on the day.", r.name()), p.plain),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(ui.panel("The post")),
        left,
    );
    let night: Vec<Line<'static>> = if ui.s.report.is_empty() {
        vec![Line::styled("Nothing to read from the last night.", p.dim)]
    } else {
        ui.s.report
            .iter()
            .take(usize::from(right.height).saturating_sub(2))
            .map(|l| Line::styled(l.clone(), p.plain))
            .collect()
    };
    f.render_widget(
        Paragraph::new(night)
            .wrap(Wrap { trim: true })
            .block(ui.panel("The last ledger")),
        right,
    );
}

/// The league table and a map of where the holds stand.
pub(super) fn league(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let wide = ui.look.size >= Size::Roomy;
    let [left, right] = Layout::horizontal([
        Constraint::Percentage(if wide { 55 } else { 52 }),
        Constraint::Min(24),
    ])
    .areas(area);
    let all = standings(ui);
    let lead = all.first().map_or(1, |c| c.1.max(1));
    let name_w = usize::from(left.width).saturating_sub(26).clamp(8, 24);
    let bar_w = 10u16;
    let rows: Vec<Row<'static>> = all
        .iter()
        .enumerate()
        .map(|(i, (name, score, us))| {
            let style = if *us { p.gold() } else { p.plain };
            let bar = ui.bar(*score, lead, bar_w, if *us { p.gold() } else { p.sea() });
            Row::new(vec![
                Line::from(Span::styled(format!("{:>2}", i + 1), p.dim)),
                Line::from(Span::styled(
                    format!(
                        "{}{}",
                        if *us { ui.look.g("▸", ">") } else { " " },
                        fit(name, name_w)
                    ),
                    style,
                )),
                Line::from(Span::styled(format!("{score:>4}"), style)),
                Line::from(bar),
            ])
        })
        .collect();
    f.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(5),
                Constraint::Length(bar_w),
            ],
        )
        .header(Row::new(vec!["#", "clan", "score", ""]).style(p.dim))
        .block(ui.panel("The league table")),
        left,
    );
    // Where the holds stand on the marsh: a braille plot of the sites.
    let [map_area, note] =
        Layout::vertical([Constraint::Min(8), Constraint::Length(4)]).areas(right);
    let size = f64::from(u32::try_from(SIZE).unwrap_or(64));
    let others: Vec<(f64, f64)> =
        ui.s.world
            .clans
            .iter()
            .filter(|c| c.name != ui.clan.name)
            .filter_map(|c| ui.s.map.sites.get(usize::from(c.site)))
            .map(|&(x, y)| (f64::from(x), size - f64::from(y)))
            .collect();
    let mine: Vec<(f64, f64)> =
        ui.s.map
            .sites
            .get(usize::from(ui.clan.site))
            .map(|&(x, y)| vec![(f64::from(x), size - f64::from(y))])
            .unwrap_or_default();
    let court = vec![(
        f64::from(ui.s.map.tidecourt.0),
        size - f64::from(ui.s.map.tidecourt.1),
    )];
    // The land: every tile that stays dry at the middle of the tide, dimly, so the holds sit
    // on a map and not in the dark.
    let land: Vec<(f64, f64)> =
        ui.s.map
            .iter()
            .filter(|(_, t)| t.elevation >= 50)
            .map(|((x, y), _)| {
                (
                    f64::from(u32::try_from(x).unwrap_or(0)),
                    size - f64::from(u32::try_from(y).unwrap_or(0)),
                )
            })
            .collect();
    let (land_color, other_style, mine_style, court_style) = (
        p.fg(hue::SEA_DEEP).fg.unwrap_or_default(),
        p.sea().add_modifier(russh_bbs_door::style::Modifier::BOLD),
        p.gold().add_modifier(russh_bbs_door::style::Modifier::BOLD),
        p.salt().add_modifier(russh_bbs_door::style::Modifier::BOLD),
    );
    let glyphs = (
        ui.look.g("◆", "o"),
        ui.look.g("⌂", "H"),
        ui.look.g("▣", "T"),
    );
    let canvas = Canvas::default()
        .block(ui.panel("Holds on the marsh"))
        .marker(if ui.look.ascii {
            Marker::Dot
        } else {
            Marker::Braille
        })
        .x_bounds([0.0, size])
        .y_bounds([0.0, size])
        .paint(move |c| {
            c.draw(&Points {
                coords: &land,
                color: land_color,
            });
            c.layer();
            for &(x, y) in &others {
                c.print(x, y, Span::styled(glyphs.0, other_style));
            }
            for &(x, y) in &court {
                c.print(x, y, Span::styled(glyphs.2, court_style));
            }
            for &(x, y) in &mine {
                c.print(x, y, Span::styled(glyphs.1, mine_style));
            }
        });
    f.render_widget(canvas, map_area);
    f.render_widget(
        Paragraph::new(vec![
            Line::styled(
                "Score: buildings, crew skill, bonds, crew and tallies. Renown joins it with the social update.",
                p.dim,
            ),
        ])
        .wrap(Wrap { trim: true }),
        note,
    );
}

/// The Hall of Tides: the clan's record, its retired crew and its dead.
pub(super) fn hall(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let clan = ui.clan;
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Min(26)]).areas(area);
    let width = usize::from(left.width).saturating_sub(4);
    let mut lines: Vec<Line<'static>> = Vec::new();
    if clan.hall.is_empty() {
        lines.push(Line::styled(
            "The hall is bare. Crew who retire, and crew the marsh keeps, are written here.",
            p.dim,
        ));
    }
    let rows = usize::from(left.height).saturating_sub(2);
    for h in clan.hall.iter().rev().take(rows) {
        let text = fit(&h.text, width.saturating_sub(5));
        lines.push(Line::from(vec![
            Span::styled(format!("S{} ", h.season), p.gold()),
            Span::styled(format!("{} ", ui.look.g("†", "+")), p.slate()),
            Span::styled(text, p.plain),
        ]));
    }
    f.render_widget(Paragraph::new(lines).block(ui.panel("Hall of Tides")), left);
    let st = &clan.stats;
    let rows = vec![
        ("seasons played", u32::from(clan.seasons_played).max(1)),
        ("shifts worked", st.shifts),
        ("goods crafted", st.crafted),
        ("expeditions", st.trips),
        ("came home", st.returned),
        ("cut off", st.cut_off),
        ("swept by the flood", st.swept),
        ("skirmishes won", st.fights_won),
        ("of fights fought", st.fights),
        ("bonds formed", st.bonds),
        ("crew lost", st.lost),
        ("tallies earned", st.earned),
    ];
    let table_rows: Vec<Row<'static>> = rows
        .into_iter()
        .map(|(k, v)| {
            Row::new(vec![
                Span::styled(k, p.dim),
                Span::styled(v.to_string(), p.plain),
            ])
        })
        .collect();
    f.render_widget(
        Table::new(table_rows, [Constraint::Min(16), Constraint::Length(7)])
            .block(ui.panel(&format!("The {} clan", clan.name))),
        right,
    );
    let _ = (World::new(0, 0, 0), hue::SEA, Daypart::Day);
}
