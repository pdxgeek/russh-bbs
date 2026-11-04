//! The Tidecourt and what hangs off it: the town square, the post, the league table and the
//! Hall of Tides.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::symbols::Marker;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::canvas::{Canvas, Points};
use russh_bbs_door::widgets::{Paragraph, Row, Table, Wrap};

use super::{Ground, Size, Ui, fit, hue, landscape};
use crate::lore::{Daypart, Water};
use crate::map::SIZE;
use crate::types::Resource;
use crate::world::World;

/// The league as `(name, renown, is_us)`, best first. Our own renown is live; the others are as
/// they were when they last saved. This is the one place the league's number is chosen.
pub(super) fn standings(ui: &Ui<'_>) -> Vec<(String, u32, bool)> {
    let mine = ui.s.renown().total();
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
pub(super) fn movers(ui: &Ui<'_>) -> Vec<(Resource, i32)> {
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
    ui.orders(f, menu, "Where to", &items);
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
    let name_w = usize::from(left.width).saturating_sub(28).clamp(8, 24);
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
                Constraint::Length(7),
                Constraint::Length(bar_w),
            ],
        )
        .header(Row::new(vec!["#", "clan", "renown", ""]).style(p.dim))
        .block(ui.panel("The league table")),
        left,
    );
    let rn = ui.s.renown();
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
            Line::from(vec![
                Span::styled(format!("Your renown {}: ", rn.total()), p.gold()),
                Span::styled(
                    format!(
                        "hold {}, crew {}, deeds {}, Deep {}, pacts {}, raids {} (at most {}), wealth {}{}",
                        rn.hold,
                        rn.crew,
                        rn.deeds,
                        rn.deep,
                        rn.pacts,
                        rn.raids,
                        crate::social::renown::RAIDS_MAX,
                        rn.wealth,
                        if rn.penalty > 0 {
                            format!(", betrayals -{}", rn.penalty)
                        } else {
                            String::new()
                        }
                    ),
                    p.dim,
                ),
            ]),
        ])
        .wrap(Wrap { trim: true }),
        note,
    );
}

/// The Hall of Tides: the clan's record, its retired crew and its dead, this season's records
/// so far and how the last season is remembered.
pub(super) fn hall(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let clan = ui.clan;
    let roomy = ui.look.size >= Size::Roomy;
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(52), Constraint::Min(30)]).areas(area);
    let [entries, record] = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(if roomy { 9 } else { 6 }),
    ])
    .areas(left);
    let width = usize::from(entries.width).saturating_sub(4);
    let mut lines: Vec<Line<'static>> = Vec::new();
    if clan.hall.is_empty() {
        lines.push(Line::styled(
            "The hall is bare. Crew who retire, and crew the marsh keeps, are written here.",
            p.dim,
        ));
    }
    let rows = usize::from(entries.height).saturating_sub(2);
    for h in clan.hall.iter().rev().take(rows) {
        lines.push(Line::from(vec![
            Span::styled(format!("S{} ", h.season), p.gold()),
            Span::styled(format!("{} ", ui.look.g("†", "+")), p.slate()),
            Span::styled(fit(&h.text, width.saturating_sub(5)), p.plain),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines).block(ui.panel("Hall of Tides")),
        entries,
    );
    let (st, so) = (&clan.stats, &clan.social);
    let mut rec = vec![
        Line::from(vec![
            Span::styled("Raids   ", p.dim),
            Span::styled(
                format!(
                    "{} won, {} lost, {} beaten off, {} suffered",
                    so.raids_won, so.raids_lost, so.repelled, so.raided
                ),
                p.plain,
            ),
        ]),
        Line::from(vec![
            Span::styled("Trips   ", p.dim),
            Span::styled(
                format!(
                    "{} out, {} home, {} fights won of {}",
                    st.trips, st.returned, st.fights_won, st.fights
                ),
                p.plain,
            ),
        ]),
        Line::from(vec![
            Span::styled("Crew    ", p.dim),
            Span::styled(
                format!(
                    "{} bonds, {} lost, {} seasons played",
                    st.bonds,
                    st.lost,
                    clan.seasons_played.max(1)
                ),
                p.plain,
            ),
        ]),
    ];
    if roomy {
        rec.push(Line::from(vec![
            Span::styled("Hold    ", p.dim),
            Span::styled(
                format!(
                    "{} shifts, {} goods crafted, {} tallies earned",
                    st.shifts, st.crafted, st.earned
                ),
                p.plain,
            ),
        ]));
        if !so.grudges.is_empty() {
            rec.push(Line::from(vec![
                Span::styled("Remembers  ", p.dim),
                Span::styled(so.grudges.join(", "), p.rust()),
            ]));
        }
    }
    f.render_widget(
        Paragraph::new(rec)
            .wrap(Wrap { trim: true })
            .block(ui.panel(&format!("The {} clan", clan.name))),
        record,
    );
    // The season's records so far, and how the last season is remembered.
    let now = crate::social::hall::SeasonRecord::of(&ui.s.world, ui.s.today);
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::styled(
        format!("Season {} so far", ui.s.world.season),
        p.accent,
    ));
    if now.records.is_empty() {
        lines.push(Line::styled("  no records yet", p.dim));
    }
    for h in &now.records {
        lines.push(Line::from(vec![
            Span::styled(format!("  {}: ", h.record.title()), p.dim),
            Span::styled(format!("{} ({})", fit(&h.clan, 18), h.value), p.plain),
        ]));
    }
    if let Some(last) = ui.s.world.social.book.last() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!("Season {} in the book", last.season),
            p.accent,
        ));
        for (i, (name, score)) in last.top.iter().enumerate().take(3) {
            lines.push(Line::styled(
                format!("  {}. {} ({score})", i + 1, fit(name, 20)),
                p.plain,
            ));
        }
        lines.push(Line::styled(last.ending.clone(), p.dim));
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(ui.panel("The season's records")),
        right,
    );
    let _ = (World::new(0, 0, 0), hue::SEA, Daypart::Day);
}
