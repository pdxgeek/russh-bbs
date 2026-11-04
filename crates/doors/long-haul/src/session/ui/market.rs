//! Station services: the market (prices with a sparkline across the stations you know), the
//! job board with a detail pane, the dock chooser, the yard, the loan office and the news.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "table geometry and gauge values are small and clamped"
)]

use russh_bbs_door::DoorFrame;
use russh_bbs_door::DoorStyle;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Cell, Paragraph, Row, Sparkline, Table, Wrap};

use super::kit::{Bar, Pal, credits, pad, panel, rpad, trunc};
use crate::content::GoodClass;
use crate::contracts::Kind;
use crate::frontier::Role;
use crate::session::{Mode, Session};

/// The first row of a window of `room` rows that keeps `cursor` in view.
fn window(cursor: usize, room: usize, len: usize) -> usize {
    cursor
        .saturating_sub(room.saturating_sub(1))
        .min(len.saturating_sub(room))
}

impl Session {
    /// Prices of one good at every station the captain has seen, nearest first.
    fn price_landscape(&self, good: u8) -> Vec<(u16, u32)> {
        let Some(p) = &self.player else {
            return Vec::new();
        };
        let hops = self.frontier.hops_from(p.at);
        let mut out: Vec<(u8, u16, u32)> = Vec::new();
        for intel in &p.intel {
            let Some(st) = self.frontier.station(intel.station) else {
                continue;
            };
            if let Some(slot) = st.goods.iter().position(|m| m.good == good)
                && let Some(&price) = intel.prices.get(slot)
            {
                let h = hops.get(usize::from(st.system)).copied().unwrap_or(u8::MAX);
                out.push((h, st.id, price));
            }
        }
        out.sort_by_key(|&(h, id, _)| (h, id));
        out.into_iter().map(|(_, id, pr)| (id, pr)).collect()
    }

    pub(super) fn draw_market(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let (Some(p), Mode::Market(cursor)) = (&self.player, &self.mode) else {
            return;
        };
        let rows = self.rows();
        let [table_area, side] =
            Layout::horizontal([Constraint::Min(46), Constraint::Length(20)]).areas(area);
        let st = p.docked.and_then(|s| self.frontier.station(s));
        let title = st.map_or("MARKET".to_owned(), |s| {
            format!(
                "{} - {}",
                s.name,
                self.content.economies[usize::from(s.economy)].name
            )
        });
        let block = panel(pal, &title, true);
        let inner = block.inner(table_area);
        frame.render_widget(block, table_area);
        let room = usize::from(inner.height).saturating_sub(1);
        let first = window(*cursor, room, rows.len());
        let body: Vec<Row<'_>> = rows
            .iter()
            .enumerate()
            .skip(first)
            .take(room)
            .map(|(i, r)| {
                let g = &self.content.goods[usize::from(r.good)];
                let heat = crate::market::heat_pct(r.mid, g.base);
                let role = match r.role {
                    Role::Produces => "makes",
                    Role::Needs => "needs",
                    Role::Neutral => "",
                };
                let sel = i == *cursor;
                let heat_style = if sel {
                    pal.sel
                } else if heat <= -15 {
                    pal.good
                } else if heat >= 15 {
                    pal.bad
                } else {
                    pal.dim
                };
                Row::new(vec![
                    Cell::from(format!(
                        "{}{}",
                        if g.class == GoodClass::Contraband {
                            "*"
                        } else {
                            " "
                        },
                        g.name
                    )),
                    Cell::from(rpad(&r.stock.to_string(), 5)),
                    Cell::from(rpad(&r.mid.to_string(), 5)),
                    Cell::from(Span::styled(rpad(&format!("{heat:+}%"), 5), heat_style)),
                    Cell::from(rpad(
                        &if r.can_buy {
                            r.buy.to_string()
                        } else {
                            "-".to_owned()
                        },
                        5,
                    )),
                    Cell::from(rpad(&r.sell.to_string(), 5)),
                    Cell::from(rpad(&r.held.to_string(), 3)),
                    Cell::from(role),
                ])
                .style(if sel { pal.sel } else { pal.plain })
            })
            .collect();
        let table = Table::new(
            body,
            [
                Constraint::Length(inner.width.saturating_sub(40).max(6)),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Length(3),
                Constraint::Length(5),
            ],
        )
        .column_spacing(1)
        .header(
            Row::new([
                "Good", "Stock", "  Mid", "   vs", "  Buy", " Sell", "Own", "Role",
            ])
            .style(pal.dim),
        );
        frame.render_widget(table, inner);

        // Detail: this good across the frontier you know.
        let block = panel(pal, "PRICES SEEN", false);
        let inner = block.inner(side);
        frame.render_widget(block, side);
        let Some(row) = rows.get(*cursor) else { return };
        let g = &self.content.goods[usize::from(row.good)];
        let mut landscape = self.price_landscape(row.good);
        if let Some(here) = p.docked {
            landscape.retain(|&(id, _)| id != here);
            landscape.insert(0, (here, u32::try_from(row.mid).unwrap_or(0)));
        }
        let [head, spark, foot] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(2),
        ])
        .areas(inner);
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(g.name.clone(), pal.accent),
                Line::styled(format!("base {} cr - held {} t", g.base, row.held), pal.dim),
                Line::styled(
                    if g.class == GoodClass::Contraband {
                        "CONTRABAND: customs will take it"
                    } else {
                        "legal"
                    },
                    if g.class == GoodClass::Contraband {
                        pal.bad
                    } else {
                        pal.dim
                    },
                ),
            ]),
            head,
        );
        let data: Vec<u64> = landscape.iter().map(|&(_, pr)| u64::from(pr)).collect();
        frame.render_widget(Sparkline::default().data(&data).style(pal.amber), spark);
        let cheap = landscape.iter().min_by_key(|&&(_, pr)| pr);
        let dear = landscape.iter().max_by_key(|&&(_, pr)| pr);
        let nm = |id: u16| {
            self.frontier
                .station(id)
                .map_or("?".to_owned(), |s| s.name.clone())
        };
        let mut lines = Vec::new();
        if let (Some(&(c, cp)), Some(&(d, dp))) = (cheap, dear)
            && landscape.len() > 1
        {
            lines.push(Line::from(vec![
                Span::styled("low  ", pal.good),
                Span::styled(format!("{cp} {}", trunc(&nm(c), 14)), pal.plain),
            ]));
            lines.push(Line::from(vec![
                Span::styled("high ", pal.bad),
                Span::styled(format!("{dp} {}", trunc(&nm(d), 14)), pal.plain),
            ]));
        } else {
            lines.push(Line::styled(
                "Dock elsewhere to compare. Prices you have seen are remembered, not live.",
                pal.dim,
            ));
        }
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), foot);
    }

    pub(super) fn draw_jobs(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let (Some(p), Mode::Jobs(cursor)) = (&self.player, &self.mode) else {
            return;
        };
        let carried_h = (u16::try_from(p.contracts.len()).unwrap_or(4) + 2).min(6);
        let [carried_area, board_area] = if p.contracts.is_empty() {
            [Rect::new(area.x, area.y, area.width, 0), area]
        } else {
            let [a, b] =
                Layout::vertical([Constraint::Length(carried_h), Constraint::Min(5)]).areas(area);
            [a, b]
        };
        if carried_area.height > 0 {
            let block = panel(
                pal,
                &format!(
                    "CARRIED {}/{}",
                    p.contracts.len(),
                    crate::contracts::MAX_CONTRACTS
                ),
                false,
            );
            let inner = block.inner(carried_area);
            frame.render_widget(block, carried_area);
            let rows = Layout::vertical(vec![Constraint::Length(1); usize::from(inner.height)])
                .split(inner);
            for (c, r) in p.contracts.iter().zip(rows.iter()) {
                let left = (c.due - p.last_day).max(0);
                let to = self.frontier.station(c.to).map_or("?", |s| s.name.as_str());
                let [l, b] =
                    Layout::horizontal([Constraint::Min(30), Constraint::Length(16)]).areas(*r);
                frame.render_widget(
                    Paragraph::new(format!(
                        "{} → {} - {} cr",
                        c.kind.name(),
                        trunc(to, 18),
                        credits(i64::from(c.reward))
                    ))
                    .style(pal.plain),
                    l,
                );
                frame.render_widget(
                    Bar::new(
                        (left * 1000 / 14).min(1000) as u32,
                        pal.level((left * 100 / 14).min(100) as u32),
                        pal,
                    ),
                    Rect {
                        width: b.width.saturating_sub(5),
                        ..b
                    },
                );
                frame.render_widget(
                    Paragraph::new(format!("{left:>3} d")),
                    Rect {
                        x: b.x + b.width.saturating_sub(5),
                        width: 5.min(b.width),
                        ..b
                    },
                );
            }
        }
        let offers = self.offers(p);
        let [list_area, detail_area] =
            Layout::horizontal([Constraint::Min(40), Constraint::Length(30)]).areas(board_area);
        let block = panel(pal, "JOB BOARD", true);
        let inner = block.inner(list_area);
        frame.render_widget(block, list_area);
        if offers.is_empty() {
            frame.render_widget(
                Paragraph::new("Nothing posted for you here.").style(pal.dim),
                inner,
            );
        }
        let room = usize::from(inner.height).saturating_sub(1);
        let first = window(*cursor, room, offers.len());
        let body: Vec<Row<'_>> = offers
            .iter()
            .enumerate()
            .skip(first)
            .take(room)
            .map(|(i, o)| {
                let c = &o.contract;
                let to = self.frontier.station(c.to).map_or("?", |s| s.name.as_str());
                let sel = i == *cursor;
                Row::new(vec![
                    Cell::from(c.faction.short()),
                    Cell::from(c.kind.name()),
                    Cell::from(trunc(to, 16)),
                    Cell::from(rpad(&credits(i64::from(c.reward)), 7)),
                    Cell::from(o.locked.map_or(String::new(), |n| format!("needs {n:+}"))),
                ])
                .style(if sel {
                    pal.sel
                } else if o.locked.is_some() {
                    pal.dim
                } else {
                    pal.plain
                })
            })
            .collect();
        frame.render_widget(
            Table::new(
                body,
                [
                    Constraint::Length(8),
                    Constraint::Length(9),
                    Constraint::Length(inner.width.saturating_sub(37).max(6)),
                    Constraint::Length(7),
                    Constraint::Length(9),
                ],
            )
            .column_spacing(1)
            .header(Row::new(["Client", "Kind", "To", "Pay", ""]).style(pal.dim)),
            inner,
        );
        let block = panel(pal, "DETAIL", false);
        let inner = block.inner(detail_area);
        frame.render_widget(block, detail_area);
        if let Some(o) = offers.get(*cursor) {
            let c = &o.contract;
            let to = self.frontier.station(c.to).map_or("?", |s| s.name.as_str());
            let what = match c.kind {
                Kind::Delivery => format!(
                    "{} t {}",
                    c.tons,
                    self.content.goods[usize::from(c.good)].name
                ),
                Kind::Passenger => format!(
                    "{} {}",
                    c.count,
                    self.content.jobs.passenger[usize::from(c.pax)].name
                ),
                Kind::Bounty => format!(
                    "wreck a {}",
                    self.content.enemies.enemy[usize::from(c.pax)].name
                ),
                Kind::Escort => "escort a ward".to_owned(),
                Kind::Survey => format!("survey {}", self.system_name(c.target)),
            };
            let mut lines = vec![
                Line::styled(what, pal.accent),
                Line::styled(
                    format!("to {to} - due in {} d", c.due - p.last_day),
                    pal.plain,
                ),
            ];
            let dest = self.frontier.station(c.to).map(|s| s.system);
            if let Some(d) = dest
                && d != p.at
                && let Some(r) = self.route_to(d, super::route::Goal::Fastest)
            {
                lines.push(Line::styled(
                    format!("{} jumps, {} fuel, {} days", r.legs.len(), r.fuel, r.days),
                    if r.short_at.is_some() {
                        pal.bad
                    } else {
                        pal.dim
                    },
                ));
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled(o.line.replace("{to}", to), pal.dim));
            if let Some(need) = o.locked {
                lines.push(Line::styled(
                    format!("Closed: needs standing {need:+}."),
                    pal.bad,
                ));
            }
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
        }
    }

    pub(super) fn draw_docks(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let (Some(p), Mode::Docks(cursor)) = (&self.player, &self.mode) else {
            return;
        };
        let block = panel(pal, "STATIONS IN THIS SYSTEM", true);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let mut lines = Vec::new();
        for (i, id) in self
            .frontier
            .system(p.at)
            .map(|s| s.stations.clone())
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            if let Some(st) = self.frontier.station(id) {
                let eco = &self.content.economies[usize::from(st.economy)];
                lines.push(Line::styled(
                    format!(
                        "{} {} - {} - size {} (docking costs 1 drive-day)",
                        if i == *cursor { '>' } else { ' ' },
                        st.name,
                        eco.name,
                        st.size
                    ),
                    if i == *cursor { pal.sel } else { pal.plain },
                ));
            }
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }

    pub(super) fn draw_yard(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let Mode::Yard(cursor) = &self.mode else {
            return;
        };
        let items = self.yard_items();
        let block = panel(pal, "SHIPYARD  (negative is what you pay)", true);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let room = usize::from(inner.height);
        let first = window(*cursor, room, items.len());
        let lines: Vec<Line<'static>> = items
            .iter()
            .enumerate()
            .skip(first)
            .take(room)
            .map(|(i, (_, label, money))| {
                let style: Style = if i == *cursor {
                    pal.sel
                } else if *money < 0 {
                    pal.plain
                } else {
                    pal.good
                };
                Line::styled(
                    format!(
                        "{} {} {}",
                        if i == *cursor { '>' } else { ' ' },
                        pad(label, usize::from(inner.width).saturating_sub(14)),
                        rpad(&credits(*money), 10)
                    ),
                    style,
                )
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
    }

    pub(super) fn draw_loan(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let Some(p) = &self.player else { return };
        let block = panel(pal, "LOAN OFFICE", true);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(inner);
        let limit = p.credit_limit().max(1);
        let pm = (p.debt.principal.clamp(0, limit) * 1000 / limit) as u32;
        Self::gauge_row(
            frame,
            rows[0],
            pal,
            "DEBT",
            pm,
            pal.danger(pm / 10),
            &format!(
                "{} / {} limit",
                credits(p.debt.principal),
                credits(p.credit_limit())
            ),
        );
        let due = (p.debt.next_due - p.last_day).max(0);
        Self::gauge_row(
            frame,
            rows[1],
            pal,
            "PAYDAY",
            (1000 - (due * 1000 / 7).min(1000)) as u32,
            pal.hull,
            &format!("{} cr in {due} d", credits(p.weekly_payment())),
        );
        frame.render_widget(
            Paragraph::new(format!(
                "Interest runs {} per mille a day. Three missed payments in a row repossess the ship (so far: {}).",
                crate::engine::INTEREST_PM, p.repossessions
            ))
            .style(pal.dim)
            .wrap(Wrap { trim: true }),
            Rect { height: 2, ..rows[3] },
        );
    }

    pub(super) fn draw_news(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let Some(p) = &self.player else { return };
        let block = panel(
            pal,
            "NEWS REACHING THIS SYSTEM (it travels one hop a night)",
            true,
        );
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let news = self
            .world
            .news_at(&self.content, &self.frontier, p.at, self.world.day);
        let mut lines: Vec<Line<'static>> = Vec::new();
        if news.is_empty() {
            lines.push(Line::styled("Nothing yet.", pal.dim));
        }
        for (age, text) in news.into_iter().take(usize::from(inner.height)) {
            lines.push(Line::from(vec![
                Span::styled(format!("{age:>2}d ago  "), pal.amber),
                Span::styled(text, pal.plain),
            ]));
        }
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
        let _ = DoorStyle::Plain;
    }
}
