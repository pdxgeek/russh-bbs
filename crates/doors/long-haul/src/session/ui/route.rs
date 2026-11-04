//! The route planner: choose a path over the known lanes, see its fuel, days, risk and the
//! events it is likely to meet before committing, and fly it one leg at a time.
//!
//! The planner searches the lane graph and sums numbers the engine already computes
//! (`Ship::fuel_for`, `Ship::days_for`, `Player::transit_chance`, `Player::event_weights`).
//! It decides nothing: each leg it flies is an ordinary `Action::Jump`, so every rule, refusal
//! and event is the engine's.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "route sums are small and shown to the player, never stored"
)]

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::Paragraph;
use russh_bbs_door::{Ctx, DoorFrame, DoorKey, DoorStyle};

use super::kit::Pal;
use super::kit::{Bar, key_lines, pad, panel, rpad, trunc};
use crate::engine::Action;
use crate::frontier::Hazard;
use crate::session::ui::chart::Vis;
use crate::session::{Mode, Session};
use crate::ship::Ship;
use crate::transit::Where;

/// What the search tries to minimise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::session) enum Goal {
    /// Drive-days.
    Fastest,
    /// Fuel.
    Thrifty,
    /// The chance of meeting an event.
    Safest,
}

impl Goal {
    pub(super) const ALL: [Self; 3] = [Self::Fastest, Self::Thrifty, Self::Safest];

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Fastest => "Fastest",
            Self::Thrifty => "Thriftiest",
            Self::Safest => "Safest",
        }
    }

    const fn next(self) -> Self {
        match self {
            Self::Fastest => Self::Thrifty,
            Self::Thrifty => Self::Safest,
            Self::Safest => Self::Fastest,
        }
    }
}

/// A route being planned: where to, and how to choose.
#[derive(Debug, Clone, Copy)]
pub(in crate::session) struct Plan {
    pub target: u16,
    pub goal: Goal,
}

/// One jump of a route.
#[derive(Debug, Clone)]
pub(in crate::session) struct Leg {
    pub from: u16,
    pub to: u16,
    pub dist: u16,
    pub fuel: u16,
    pub days: u32,
    pub hazard: Hazard,
    /// The arrival would chart a new system.
    pub fresh: bool,
    /// Chance of an event on this leg, per-mille.
    pub risk_pm: u32,
}

/// A planned route and what it adds up to.
#[derive(Debug, Clone)]
pub(in crate::session) struct Route {
    pub legs: Vec<Leg>,
    pub fuel: u32,
    pub days: u32,
    /// Chance of at least one event, per-mille.
    pub risk_pm: u32,
    /// Expected events by family, in thousandths of an event, largest first.
    pub expected: Vec<(&'static str, u32)>,
    /// The first leg the tank cannot cover, if any.
    pub short_at: Option<usize>,
}

impl Route {
    /// The systems on the route, starting with where the ship is.
    pub(super) fn path(&self) -> Vec<u16> {
        let mut out: Vec<u16> = self.legs.first().map(|l| l.from).into_iter().collect();
        out.extend(self.legs.iter().map(|l| l.to));
        out
    }
}

impl Session {
    /// The best route by `goal` from the ship to `target` over charted lanes (the last hop may
    /// be an uncharted signal), or `None` when there is none.
    pub(in crate::session) fn route_to(&self, target: u16, goal: Goal) -> Option<Route> {
        let player = self.player.as_ref()?;
        let stats = player.stats(&self.content);
        let n = self.frontier.systems.len();
        let start = player.at;
        if start == target || usize::from(target) >= n {
            return None;
        }
        let leg_cost = |from: u16, to: u16, dist: u16| -> u64 {
            let fuel = u64::from(Ship::fuel_for(&stats, dist));
            let days = u64::from(Ship::days_for(&stats, dist));
            let fresh = !self.world.is_charted(&self.frontier, to);
            let at = Where {
                content: &self.content,
                frontier: &self.frontier,
                system: to,
                fresh,
            };
            let risk = u64::from(player.transit_chance(&at, dist));
            let _ = from;
            match goal {
                Goal::Fastest => days * 1000 + fuel,
                Goal::Thrifty => fuel * 1000 + days,
                Goal::Safest => risk * 100 + fuel,
            }
        };
        let mut best = vec![u64::MAX; n];
        let mut prev: Vec<Option<u16>> = vec![None; n];
        let mut heap = BinaryHeap::new();
        best[usize::from(start)] = 0;
        heap.push(Reverse((0u64, start)));
        while let Some(Reverse((cost, at))) = heap.pop() {
            if cost > best[usize::from(at)] {
                continue;
            }
            if at == target {
                break;
            }
            for &(next, dist) in self.frontier.neighbours(at) {
                // Only charted systems are waypoints; an unknown signal can be the last stop.
                match self.vis(next) {
                    Vis::Hidden => continue,
                    Vis::Signal if next != target => continue,
                    _ => {}
                }
                let c = cost + leg_cost(at, next, dist);
                if c < best[usize::from(next)] {
                    best[usize::from(next)] = c;
                    prev[usize::from(next)] = Some(at);
                    heap.push(Reverse((c, next)));
                }
            }
        }
        prev[usize::from(target)]?;
        let mut path = vec![target];
        let mut at = target;
        while let Some(p) = prev[usize::from(at)] {
            path.push(p);
            at = p;
        }
        path.reverse();
        Some(self.summarize(player, &path))
    }

    /// Add up a path.
    fn summarize(&self, player: &crate::player::Player, path: &[u16]) -> Route {
        let stats = player.stats(&self.content);
        let mut legs = Vec::new();
        let mut fuel_total = 0u32;
        let mut days_total = 0u32;
        let mut miss = 1_000_000u64;
        let mut by_family: Vec<(&'static str, u64)> = Vec::new();
        let mut short_at = None;
        let mut tank = i64::from(player.ship.fuel);
        for (i, w) in path.windows(2).enumerate() {
            let (from, to) = (w[0], w[1]);
            let dist = self.frontier.lane(from, to).unwrap_or(0);
            let fuel = Ship::fuel_for(&stats, dist);
            let days = Ship::days_for(&stats, dist);
            let fresh = !self.world.is_charted(&self.frontier, to);
            let at = Where {
                content: &self.content,
                frontier: &self.frontier,
                system: to,
                fresh,
            };
            let risk_pm = player.transit_chance(&at, dist);
            miss = miss * u64::from(1000 - risk_pm.min(1000)) / 1000;
            // Which families an event on this leg would come from.
            let weights = player.event_weights(&at);
            let total: u64 = weights.iter().map(|(_, wt)| u64::from(*wt)).sum();
            if total > 0 {
                for (ix, wt) in &weights {
                    let family = crate::transit::FAMILIES
                        .iter()
                        .find(|f| **f == self.content.transit.event[*ix].family.as_str())
                        .copied()
                        .unwrap_or("other");
                    let share = u64::from(risk_pm) * u64::from(*wt) / total;
                    match by_family.iter_mut().find(|(f, _)| *f == family) {
                        Some((_, v)) => *v += share,
                        None => by_family.push((family, share)),
                    }
                }
            }
            tank -= i64::from(fuel);
            if tank < 0 && short_at.is_none() {
                short_at = Some(i);
            }
            fuel_total += u32::from(fuel);
            days_total += days;
            legs.push(Leg {
                from,
                to,
                dist,
                fuel,
                days,
                hazard: self.frontier.system(to).map_or(Hazard::None, |s| s.hazard),
                fresh,
                risk_pm,
            });
        }
        by_family.sort_by_key(|&(f, v)| (Reverse(v), f));
        Route {
            legs,
            fuel: fuel_total,
            days: days_total,
            risk_pm: (1_000_000 - miss) as u32 / 1000,
            expected: by_family.into_iter().map(|(f, v)| (f, v as u32)).collect(),
            short_at,
        }
    }

    pub(in crate::session) fn route_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let Some(plan) = self.plan else {
            self.mode = Mode::Chart;
            return;
        };
        match key {
            DoorKey::Escape | DoorKey::Char('q') => self.mode = Mode::Chart,
            DoorKey::Char('g' | 'm') | DoorKey::Left | DoorKey::Right => {
                let goal = if key == DoorKey::Left {
                    // Three goals: back is two steps forward.
                    plan.goal.next().next()
                } else {
                    plan.goal.next()
                };
                self.plan = Some(Plan { goal, ..plan });
            }
            DoorKey::Enter | DoorKey::Char('j') => self.fly_next_leg(ctx),
            _ => {}
        }
    }

    /// Fly the first leg of the plan; the plan ends on arrival at its target.
    fn fly_next_leg(&mut self, ctx: &mut Ctx<'_>) {
        let Some(plan) = self.plan else { return };
        let Some(route) = self.route_to(plan.target, plan.goal) else {
            self.push("There is no known route there.".to_owned(), DoorStyle::Bad);
            return;
        };
        let Some(first) = route.legs.first() else {
            return;
        };
        let to = first.to;
        if self.act(ctx, &Action::Jump(to)) {
            self.chart.cursor = plan.target;
            let here = self.player.as_ref().map(|p| p.at);
            if here == Some(plan.target) {
                self.plan = None;
                self.mode = Mode::Bridge;
            }
            // A scene opening takes over the screen; the plan stays for after it.
        }
    }

    // ---------------------------------------------------------------- drawing

    #[allow(
        clippy::too_many_lines,
        reason = "one planner screen: legs, meters, map, events"
    )]
    pub(super) fn draw_route(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let (Some(player), Some(plan)) = (&self.player, self.plan) else {
            return;
        };
        let Some(route) = self.route_to(plan.target, plan.goal) else {
            let block = panel(pal, "ROUTE PLANNER", false);
            let inner = block.inner(area);
            frame.render_widget(block, area);
            frame.render_widget(
                Paragraph::new("No known route reaches that system. Chart the lanes between.")
                    .style(pal.bad),
                inner,
            );
            return;
        };
        let wide = area.width >= 70;
        let [left, right] = if wide {
            Layout::horizontal([Constraint::Min(40), Constraint::Length(28)]).areas(area)
        } else {
            Layout::horizontal([Constraint::Min(30), Constraint::Length(0)]).areas(area)
        };
        // Left: the legs, then the meters.
        let [legs_area, meters_area] =
            Layout::vertical([Constraint::Min(5), Constraint::Length(5)]).areas(left);
        let target_name = self.system_name(plan.target);
        let block = panel(pal, &format!("ROUTE  to {}", trunc(&target_name, 20)), true);
        let inner = block.inner(legs_area);
        frame.render_widget(block, legs_area);
        let w = usize::from(inner.width);
        let mut lines = vec![Line::styled(
            format!(
                "{} {} {} {} {}",
                pad("Leg", w.saturating_sub(24).max(8)),
                rpad("Span", 5),
                rpad("Fuel", 5),
                rpad("Days", 5),
                rpad("Evt", 4)
            ),
            pal.dim,
        )];
        let room = usize::from(inner.height).saturating_sub(1);
        for (i, leg) in route.legs.iter().enumerate().take(room) {
            let from = self.system_name(leg.from);
            let to = self.system_name(leg.to);
            let name_w = w.saturating_sub(24).max(8);
            let mut note = String::new();
            if leg.hazard != Hazard::None {
                note.push_str(" !");
            }
            if leg.fresh {
                note.push_str(" ?");
            }
            let cut = route.short_at.is_some_and(|s| i >= s);
            let style = if cut { pal.bad } else { pal.plain };
            lines.push(Line::from(vec![
                Span::styled(
                    pad(
                        &format!("{}>{}{}", trunc(&from, 10), trunc(&to, 12), note),
                        name_w,
                    ),
                    style,
                ),
                Span::styled(format!(" {}", rpad(&leg.dist.to_string(), 5)), pal.dim),
                Span::styled(format!(" {}", rpad(&leg.fuel.to_string(), 5)), style),
                Span::styled(format!(" {}", rpad(&leg.days.to_string(), 5)), style),
                Span::styled(
                    format!(" {}", rpad(&format!("{}%", leg.risk_pm / 10), 4)),
                    pal.role(if leg.risk_pm > 450 {
                        DoorStyle::Bad
                    } else {
                        DoorStyle::Dim
                    }),
                ),
            ]));
        }
        if route.legs.len() > room {
            lines.push(Line::styled(
                format!("... and {} more legs", route.legs.len() - room),
                pal.dim,
            ));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        // Meters: fuel against the tank, days against the allowance, the odds of trouble.
        let block = panel(pal, "COST", false);
        let inner = block.inner(meters_area);
        frame.render_widget(block, meters_area);
        let stats = player.stats(&self.content);
        let rows = Layout::vertical([Constraint::Length(1); 3]).split(inner);
        let label_w = 7u16;
        let meter = |frame: &mut DoorFrame<'_>,
                     row: Rect,
                     label: &str,
                     pm: u32,
                     style: russh_bbs_door::style::Style,
                     text: String,
                     tick: Option<u32>| {
            let [l, b, t] = Layout::horizontal([
                Constraint::Length(label_w),
                Constraint::Min(4),
                Constraint::Length(u16::try_from(text.chars().count() + 1).unwrap_or(12)),
            ])
            .areas(row);
            frame.render_widget(Paragraph::new(label.to_owned()).style(pal.dim), l);
            frame.render_widget(
                Bar::new(pm, style, pal).ticks(tick.into_iter().collect()),
                b,
            );
            frame.render_widget(Paragraph::new(format!(" {text}")).style(pal.plain), t);
        };
        let tank = u32::from(stats.fuel_cap.max(1));
        let fuel_pm = route.fuel.saturating_mul(1000) / tank;
        meter(
            frame,
            rows[0],
            "FUEL",
            fuel_pm,
            if route.short_at.is_some() {
                pal.bad
            } else {
                pal.amber
            },
            format!("{}/{} (tank {})", route.fuel, tank, player.ship.fuel),
            Some(u32::from(player.ship.fuel) * 1000 / tank),
        );
        let allowance = u32::try_from(crate::player::ALLOWANCE).unwrap_or(10);
        meter(
            frame,
            rows[1],
            "DAYS",
            route.days.saturating_mul(1000) / allowance.max(1),
            pal.star,
            format!("{} ({} left today)", route.days, player.days_left.max(0)),
            Some(u32::try_from(player.days_left.max(0)).unwrap_or(0) * 1000 / allowance.max(1)),
        );
        meter(
            frame,
            rows[2],
            "RISK",
            route.risk_pm,
            pal.danger(route.risk_pm / 10),
            format!("{}% to meet something", route.risk_pm / 10),
            None,
        );

        if right.width > 0 {
            let [map_area, ev_area] =
                Layout::vertical([Constraint::Min(6), Constraint::Length(8)]).areas(right);
            let block = panel(pal, "TRACK", false);
            let inner = block.inner(map_area);
            frame.render_widget(block, map_area);
            let path = route.path();
            self.draw_map(
                frame,
                inner,
                pal,
                player.at,
                plan.target,
                Some(&path),
                super::chart::Zoom::Near,
            );
            let block = panel(pal, "LIKELY EVENTS", false);
            let inner = block.inner(ev_area);
            frame.render_widget(block, ev_area);
            let mut lines: Vec<Line<'static>> = Vec::new();
            let mut goals = vec![Span::styled("goal ", pal.dim)];
            for g in Goal::ALL {
                goals.push(Span::styled(
                    format!(" {} ", g.name()),
                    if g == plan.goal { pal.sel } else { pal.dim },
                ));
            }
            lines.push(Line::from(goals));
            for (family, milli) in route
                .expected
                .iter()
                .take(usize::from(inner.height).saturating_sub(2))
            {
                lines.push(Line::from(vec![
                    Span::styled(pad(family, 11), pal.plain),
                    Span::styled(
                        format!("{:.1} expected", f64::from(*milli) / 1000.0),
                        pal.dim,
                    ),
                ]));
            }
            if route.expected.is_empty() {
                lines.push(Line::styled("Quiet lanes.", pal.dim));
            }
            frame.render_widget(Paragraph::new(lines), inner);
        }
    }

    pub(super) fn route_keys(&self, pal: &Pal, width: u16) -> Vec<Line<'static>> {
        key_lines(
            pal,
            &[
                ("Enter", " fly next leg"),
                ("g", " change goal"),
                ("Esc", " back to chart"),
            ],
            width,
            2,
        )
    }

    pub(in crate::session) fn system_name(&self, id: u16) -> String {
        if self.vis(id) == Vis::Charted {
            self.frontier
                .system(id)
                .map_or_else(|| "?".to_owned(), |s| s.name.clone())
        } else {
            "signal".to_owned()
        }
    }
}
