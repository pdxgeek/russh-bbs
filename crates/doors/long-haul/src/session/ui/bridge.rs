//! The bridge: instruments for hull, fuel, reactor and hold, the crew on watch, a local chart,
//! the date and the alerts.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "gauge values are small and clamped"
)]

use russh_bbs_door::DoorFrame;
use russh_bbs_door::DoorStyle;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::Paragraph;

use super::chart::Zoom;
use super::kit::{Bar, Pal, credits, pad, panel, pips, trunc};
use crate::player::Player;
use crate::session::Session;
use crate::ship::Ship;

impl Session {
    /// A labelled gauge row: `LABEL  [bar]  value`.
    pub(super) fn gauge_row(
        frame: &mut DoorFrame<'_>,
        row: Rect,
        pal: &Pal,
        label: &str,
        pm: u32,
        style: Style,
        value: &str,
    ) {
        let vw = u16::try_from(value.chars().count())
            .unwrap_or(12)
            .min(row.width * 2 / 3);
        let [l, b, v] = Layout::horizontal([
            Constraint::Length(7),
            Constraint::Min(4),
            Constraint::Length(vw + 1),
        ])
        .areas(row);
        frame.render_widget(Paragraph::new(label.to_owned()).style(pal.dim), l);
        frame.render_widget(Bar::new(pm, style, pal), b);
        frame.render_widget(Paragraph::new(format!(" {value}")).style(pal.plain), v);
    }

    /// What needs the captain's eye, worst first.
    pub(in crate::session) fn alerts(&self, p: &Player) -> Vec<(DoorStyle, String)> {
        let mut out = Vec::new();
        let stats = p.stats(&self.content);
        let cheapest = self
            .frontier
            .neighbours(p.at)
            .iter()
            .map(|&(_, d)| Ship::fuel_for(&stats, d))
            .min();
        if let Some(c) = cheapest
            && p.ship.fuel < c
        {
            out.push((
                DoorStyle::Bad,
                format!(
                    "Fuel {} is below the cheapest lane ({c}). Refuel or skim.",
                    p.ship.fuel
                ),
            ));
        }
        let max_hp = self
            .content
            .hulls
            .get(usize::from(p.ship.hull))
            .map_or(1, |h| h.hull_points)
            .max(1);
        if u32::from(p.ship.hull_hp) * 100 / u32::from(max_hp) <= 50 {
            out.push((
                DoorStyle::Bad,
                format!("Hull at {}/{max_hp}. Repair at a yard.", p.ship.hull_hp),
            ));
        }
        if p.worst_wear_pct() >= 60 {
            out.push((
                DoorStyle::Bad,
                format!("A module is {}% worn.", p.worst_wear_pct()),
            ));
        }
        let due = p.debt.next_due - p.last_day;
        if p.debt.missed > 0 {
            out.push((
                DoorStyle::Bad,
                format!(
                    "{} missed payment(s). Three repossess the ship.",
                    p.debt.missed
                ),
            ));
        } else if due <= 2 && p.debt.principal > 0 && p.cash < p.weekly_payment() {
            out.push((
                DoorStyle::Bad,
                format!(
                    "Payment {} cr due in {} d; you hold {}.",
                    credits(p.weekly_payment()),
                    due.max(0),
                    credits(p.cash)
                ),
            ));
        }
        if !p.crew.is_empty() && p.rations_until < p.last_day {
            out.push((
                DoorStyle::Bad,
                "The galley is empty. Stock it at a dock.".to_owned(),
            ));
        }
        for c in &p.crew {
            if c.restless() {
                out.push((
                    DoorStyle::Bad,
                    format!("{} is restless (loyalty {}).", c.name, c.loyalty),
                ));
            } else if !c.fit() {
                out.push((
                    DoorStyle::Dim,
                    format!("{} is hurt for {} d.", c.name, c.hurt_days),
                ));
            }
        }
        for c in &p.contracts {
            if c.due - p.last_day <= 2 {
                out.push((
                    DoorStyle::Bad,
                    format!(
                        "A {} job is due in {} d.",
                        c.kind.name(),
                        (c.due - p.last_day).max(0)
                    ),
                ));
            }
        }
        if p.days_left <= 0 {
            out.push((DoorStyle::Dim, "No drive-days left today.".to_owned()));
        }
        if let Some(plan) = self.plan {
            out.push((
                DoorStyle::Accent,
                format!(
                    "Route set to {}. [j] to fly it.",
                    self.system_name(plan.target)
                ),
            ));
        }
        out
    }

    #[allow(clippy::too_many_lines, reason = "one screen of instruments")]
    pub(super) fn draw_bridge(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let Some(p) = &self.player else { return };
        let wide = area.width >= 96;
        let [left, right] = if wide {
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(area)
        } else {
            Layout::horizontal([Constraint::Length(40), Constraint::Min(20)]).areas(area)
        };
        let stats = p.stats(&self.content);
        let hull = self.content.hulls.get(usize::from(p.ship.hull));
        let max_hp = u32::from(hull.map_or(1, |h| h.hull_points).max(1));
        let ship_h = 9.min(left.height);
        let [ship_area, alert_area] =
            Layout::vertical([Constraint::Length(ship_h), Constraint::Min(0)]).areas(left);

        let block = panel(pal, hull.map_or("SHIP", |h| h.name.as_str()), true);
        let inner = block.inner(ship_area);
        frame.render_widget(block, ship_area);
        let rows = Layout::vertical([Constraint::Length(1); 7]).split(inner);
        let hull_pct = u32::from(p.ship.hull_hp) * 100 / max_hp;
        Self::gauge_row(
            frame,
            rows[0],
            pal,
            "HULL",
            hull_pct * 10,
            pal.level(hull_pct),
            &format!("{}/{max_hp}", p.ship.hull_hp),
        );
        let cap = u32::from(stats.fuel_cap.max(1));
        let fuel_pct = u32::from(p.ship.fuel) * 100 / cap;
        let jumps = self
            .frontier
            .neighbours(p.at)
            .iter()
            .map(|&(_, d)| Ship::fuel_for(&stats, d))
            .min()
            .map_or(String::new(), |c| {
                format!(" ~{} hops", p.ship.fuel / c.max(1))
            });
        Self::gauge_row(
            frame,
            rows[1],
            pal,
            "FUEL",
            fuel_pct * 10,
            pal.level(fuel_pct),
            &format!("{}/{cap}{jumps}", p.ship.fuel),
        );
        // Reactor: pips, one per point.
        let gear = p.gear(&self.content);
        if let Some(row) = rows.get(2) {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(pad("REACTOR", 7), pal.dim),
                    Span::styled(
                        pips(u32::from(gear.budget), u32::from(gear.budget), pal.ascii),
                        pal.amber,
                    ),
                    Span::styled(format!(" {} pts", gear.budget), pal.plain),
                ])),
                *row,
            );
        }
        let hold = p.hold_used();
        let hold_cap = u32::from(stats.cargo_cap.max(1));
        Self::gauge_row(
            frame,
            rows[3],
            pal,
            "HOLD",
            hold * 1000 / hold_cap,
            pal.star,
            &format!("{hold}/{hold_cap} t"),
        );
        let wear = p.worst_wear_pct();
        Self::gauge_row(
            frame,
            rows[4],
            pal,
            "WEAR",
            u32::from(wear) * 10,
            pal.danger(u32::from(wear)),
            &format!("worst {wear}%"),
        );
        let due = (p.debt.next_due - p.last_day).max(0);
        Self::gauge_row(
            frame,
            rows[5],
            pal,
            "PAYDAY",
            (1000 - (due * 1000 / 7).min(1000)) as u32,
            if p.debt.missed > 0 { pal.bad } else { pal.hull },
            &format!("{} cr in {due} d", credits(p.weekly_payment())),
        );
        if let Some(row) = rows.get(6) {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(pad("DATE", 7), pal.dim),
                    Span::styled(
                        format!(
                            "season {} - voyage day {} - {} jumps",
                            self.frontier.season, p.voyage_days, p.stats.jumps
                        ),
                        pal.plain,
                    ),
                ])),
                *row,
            );
        }

        // Alerts and the hold manifest under the instruments.
        if alert_area.height >= 3 {
            let block = panel(pal, "ALERTS", false);
            let inner = block.inner(alert_area);
            frame.render_widget(block, alert_area);
            let alerts = self.alerts(p);
            let mut lines: Vec<Line<'static>> = if alerts.is_empty() {
                vec![Line::styled("All quiet.", pal.dim)]
            } else {
                alerts
                    .into_iter()
                    .map(|(s, t)| Line::styled(trunc(&t, usize::from(inner.width)), pal.role(s)))
                    .collect()
            };
            let hold_text = if p.ship.cargo.is_empty() {
                "Hold: empty".to_owned()
            } else {
                format!(
                    "Hold: {}",
                    p.ship
                        .cargo
                        .iter()
                        .map(|l| format!(
                            "{} t {}",
                            l.tons,
                            self.content.goods[usize::from(l.good)].name
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            lines.push(Line::styled(
                trunc(&hold_text, usize::from(inner.width)),
                pal.plain,
            ));
            lines.push(Line::styled(
                trunc(&self.place(), usize::from(inner.width)),
                pal.strong,
            ));
            frame.render_widget(Paragraph::new(lines), inner);
        }

        // Right: the local chart over the crew on watch.
        let watch_h = (u16::try_from(p.crew.len().min(7)).unwrap_or(7) + 2)
            .clamp(3, 9)
            .min(right.height / 2);
        let [map_area, watch_area] =
            Layout::vertical([Constraint::Min(4), Constraint::Length(watch_h)]).areas(right);
        let block = panel(pal, "LOCAL SPACE", false);
        let inner = block.inner(map_area);
        frame.render_widget(block, map_area);
        self.draw_map(frame, inner, pal, p.at, p.at, None, Zoom::Near);
        let block = panel(pal, "ON WATCH", false);
        let inner = block.inner(watch_area);
        frame.render_widget(block, watch_area);
        let mut lines: Vec<Line<'static>> = Vec::new();
        if p.crew.is_empty() {
            lines.push(Line::styled("You fly alone.", pal.dim));
        }
        for c in &p.crew {
            let style = if !c.fit() || c.restless() {
                pal.bad
            } else {
                pal.plain
            };
            lines.push(Line::from(vec![
                Span::styled(pad(c.name.as_str(), 14), style),
                Span::styled(pad(c.role.name(), 10), pal.dim),
                Span::styled(
                    pips(u32::from(c.effective()).div_ceil(2), 5, pal.ascii),
                    pal.amber,
                ),
            ]));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}
