//! The text views of the map and board screens, for the look-around key and for windows too
//! small for the full screens. Every screen the chart, planner or standings add has one here;
//! the older screens' text lives beside their keys in `screens.rs` and `session.rs`.

use russh_bbs_door::DoorStyle;

use super::chart::Vis;
use super::route::Goal;
use crate::scene::Scene;
use crate::session::Session;
use crate::ship::Ship;

type Line = (String, DoorStyle);

fn plain(t: impl Into<String>) -> Line {
    (t.into(), DoorStyle::Plain)
}

impl Session {
    /// The star chart in words: where the ship is, what the cursor is on, every shown lane
    /// from the ship with its cost, and the nearest known systems.
    pub(in crate::session) fn chart_lines(&self) -> Vec<Line> {
        let Some(p) = &self.player else {
            return Vec::new();
        };
        let stats = p.stats(&self.content);
        let mut out = vec![(format!("STAR CHART. {}.", self.place()), DoorStyle::Accent)];
        let range = if stats.burn == 0 {
            0
        } else {
            u32::from(p.ship.fuel) * 100 / stats.burn
        };
        out.push(plain(format!(
            "Fuel {}/{} carries you about {range} spans. Lanes from here:",
            p.ship.fuel, stats.fuel_cap
        )));
        let mut near: Vec<(u16, u16)> = self.frontier.neighbours(p.at).to_vec();
        near.sort_by_key(|&(id, d)| (d, id));
        for (i, (id, dist)) in near.into_iter().enumerate() {
            let fuel = Ship::fuel_for(&stats, dist);
            let days = Ship::days_for(&stats, dist);
            let name = self.system_name(id);
            let mut notes = Vec::new();
            if self.vis(id) == Vis::Charted
                && let Some(s) = self.frontier.system(id)
            {
                if s.hazard != crate::frontier::Hazard::None {
                    notes.push(s.hazard.name().to_owned());
                }
                let letters: String = s
                    .stations
                    .iter()
                    .filter_map(|&x| self.frontier.station(x))
                    .map(|st| {
                        self.content.economies[usize::from(st.economy)]
                            .letter
                            .as_str()
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if !letters.is_empty() {
                    notes.push(format!("stations {letters}"));
                }
            } else {
                notes.push("unknown: survey data".to_owned());
            }
            let short = p.ship.fuel < fuel;
            out.push((
                format!(
                    "{} {name}: {dist} spans, {fuel} fuel, {days} days{}{}",
                    if id == self.chart.cursor { '>' } else { ' ' },
                    if notes.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", notes.join(", "))
                    },
                    if short { "  NOT ENOUGH FUEL" } else { "" }
                ),
                if short {
                    DoorStyle::Dim
                } else if id == self.chart.cursor {
                    DoorStyle::Selected
                } else {
                    DoorStyle::Plain
                },
            ));
            let _ = i;
        }
        // The cursor, if it is not a neighbour.
        let c = self.chart.cursor;
        if c != p.at {
            out.push(plain(format!("Cursor on {}.", self.system_name(c))));
            if let Some(r) = self.route_to(c, Goal::Fastest) {
                out.push(plain(format!(
                    "Route: {} jump(s), {} fuel, {} days, {}% to meet an event{}.",
                    r.legs.len(),
                    r.fuel,
                    r.days,
                    r.risk_pm / 10,
                    if r.short_at.is_some() {
                        "; NOT ENOUGH FUEL"
                    } else {
                        ""
                    }
                )));
            }
        }
        out.push((
            "[Up/Down/Left/Right] move cursor  [Enter] plan a route  [Esc] back".to_owned(),
            DoorStyle::Strong,
        ));
        out
    }

    pub(in crate::session) fn route_lines(&self) -> Vec<Line> {
        let (Some(_), Some(plan)) = (&self.player, self.plan) else {
            return vec![plain("No route set.")];
        };
        let mut out = vec![(
            format!(
                "ROUTE PLANNER to {}. Goal: {}.",
                self.system_name(plan.target),
                plan.goal.name()
            ),
            DoorStyle::Accent,
        )];
        match self.route_to(plan.target, plan.goal) {
            None => out.push((
                "No known route reaches that system.".to_owned(),
                DoorStyle::Bad,
            )),
            Some(r) => {
                for (i, l) in r.legs.iter().enumerate() {
                    out.push((
                        format!(
                            "{}. {} to {}: {} spans, {} fuel, {} days, {}% event{}",
                            i + 1,
                            self.system_name(l.from),
                            self.system_name(l.to),
                            l.dist,
                            l.fuel,
                            l.days,
                            l.risk_pm / 10,
                            if r.short_at.is_some_and(|s| i >= s) {
                                "  NOT ENOUGH FUEL"
                            } else {
                                ""
                            }
                        ),
                        DoorStyle::Plain,
                    ));
                }
                out.push(plain(format!(
                    "Total: {} fuel, {} days, {}% chance of meeting something.",
                    r.fuel,
                    r.days,
                    r.risk_pm / 10
                )));
                let top: Vec<String> = r
                    .expected
                    .iter()
                    .take(3)
                    .map(|(f, m)| format!("{f} {:.1}", f64::from(*m) / 1000.0))
                    .collect();
                if !top.is_empty() {
                    out.push(plain(format!("Likely: {}.", top.join(", "))));
                }
            }
        }
        out.push((
            "[Enter] fly next leg  [g] change goal  [Esc] back".to_owned(),
            DoorStyle::Strong,
        ));
        out
    }

    pub(in crate::session) fn standing_lines(&self) -> Vec<Line> {
        let Some(p) = &self.player else {
            return Vec::new();
        };
        let mut out = vec![("FACTIONS AND STANDING".to_owned(), DoorStyle::Accent)];
        for f in crate::content::Faction::ALL {
            let v = p.standing[f.index()];
            out.push(plain(format!(
                "{}: {v:+}. {}.",
                f.name(),
                Self::standing_note(f, v)
            )));
        }
        out.push(("[Esc] back".to_owned(), DoorStyle::Strong));
        out
    }

    /// Extra text for the board and fight screens: the rooms with their exits, and the reactor.
    pub(in crate::session) fn scene_extra_lines(&self) -> Vec<Line> {
        let Some(p) = &self.player else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match &p.scene {
            Some(Scene::Boarding(b)) => {
                let def = b.def(&self.content);
                out.push(("Map of the wreck:".to_owned(), DoorStyle::Dim));
                for (i, r) in def.rooms.iter().enumerate() {
                    let exits: Vec<&str> = r
                        .exits
                        .iter()
                        .map(|&e| def.rooms[usize::from(e)].name.as_str())
                        .collect();
                    out.push(plain(format!(
                        "{}{}: exits to {}{}",
                        if usize::from(b.at) == i {
                            "(you are here) "
                        } else {
                            ""
                        },
                        r.name,
                        exits.join(", "),
                        if r.lock > 0 && b.opened & (1u16 << i) == 0 {
                            format!("; sealed, lock {}", r.lock)
                        } else {
                            String::new()
                        }
                    )));
                }
            }
            Some(Scene::Combat(c)) if c.phase != crate::combat::Phase::Won => {
                let budget = p.gear(&self.content).budget;
                let pts = self.reactor.effective(budget);
                out.push(plain(format!(
                    "Reactor split: guns {}, shields {}, engines {} of {budget}. [Left/Right] move a point, [Enter] fire.",
                    pts[0], pts[1], pts[2]
                )));
            }
            _ => {}
        }
        out
    }
}
