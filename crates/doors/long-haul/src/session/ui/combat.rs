//! The combat panel: the range between the ships, the enemy's telegraphed move, the reactor
//! split as three adjustable gauges, the manoeuvre, both hulls and a fight log.
//!
//! The telegraph drawn is `Combat::shown`, which can lie; `Combat::intent` (the truth) is
//! never read here. The reactor split is only a request: `Action::Fight(Orders)` is checked by
//! the engine.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "gauge values are small and clamped"
)]

use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::Paragraph;
use russh_bbs_door::{Ctx, DoorFrame, DoorKey};

use super::kit::{Bar, Pal, key_lines, pad, panel, pips, trunc};
use crate::combat::{Intent, Maneuver, Orders, Phase, Range};
use crate::engine::Action;
use crate::scene::Scene;
use crate::session::Session;

/// The reactor sliders: which is selected and what each holds.
#[derive(Debug, Clone, Copy)]
pub(in crate::session) struct Reactor {
    pub sel: usize,
    pub pts: [u8; 3],
}

impl Reactor {
    pub(in crate::session) const fn new() -> Self {
        Self {
            sel: 0,
            pts: [0; 3],
        }
    }

    /// The split to show: the held one if it still adds up to the budget, else guns and
    /// shields (a new fight, or a changed reactor).
    pub(in crate::session) fn effective(&self, budget: u8) -> [u8; 3] {
        if self.pts.iter().map(|&p| u16::from(p)).sum::<u16>() == u16::from(budget) {
            self.pts
        } else {
            let guns = (budget * 3).div_ceil(5);
            [guns, budget - guns, 0]
        }
    }

    /// Move one point onto (`by` +1) or off (-1) the selected slider, taking it from or giving
    /// it to the others so the total stays the budget.
    fn adjust(pts: &mut [u8; 3], sel: usize, by: i8) {
        if by > 0 {
            let donor = (0..3)
                .filter(|&i| i != sel && pts[i] > 0)
                .max_by_key(|&i| (pts[i], 3 - i));
            if let Some(d) = donor {
                pts[d] -= 1;
                pts[sel] += 1;
            }
        } else if pts[sel] > 0 {
            let taker = (0..3).filter(|&i| i != sel).min_by_key(|&i| (pts[i], i));
            if let Some(t) = taker {
                pts[sel] -= 1;
                pts[t] += 1;
            }
        }
    }
}

const SLIDERS: [(&str, &str); 3] = [
    ("GUNS", "damage at the range you hold"),
    ("SHIELDS", "soak the enemy's volley"),
    ("ENGINES", "dodge, close, open, run"),
];

impl Session {
    /// Keys for the sliders. True when the key was one of them.
    pub(in crate::session) fn combat_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> bool {
        let Some(player) = &self.player else {
            return false;
        };
        let Some(Scene::Combat(fight)) = &player.scene else {
            return false;
        };
        if fight.phase == Phase::Won {
            return false;
        }
        let budget = player.gear(&self.content).budget;
        let mut pts = self.reactor.effective(budget);
        match key {
            DoorKey::Up => self.reactor.sel = (self.reactor.sel + 2) % 3,
            DoorKey::Down => self.reactor.sel = (self.reactor.sel + 1) % 3,
            DoorKey::Left | DoorKey::Right => {
                Reactor::adjust(
                    &mut pts,
                    self.reactor.sel,
                    if key == DoorKey::Right { 1 } else { -1 },
                );
                self.reactor.pts = pts;
            }
            DoorKey::Enter | DoorKey::Char(' ') => {
                self.reactor.pts = pts;
                let orders = Orders {
                    weapons: pts[0],
                    shields: pts[1],
                    engines: pts[2],
                    maneuver: self.maneuver,
                };
                self.act(ctx, &Action::Fight(orders));
            }
            _ => return false,
        }
        self.reactor.pts = pts;
        true
    }

    pub(super) fn combat_keys(&self, pal: &Pal, width: u16) -> Vec<Line<'static>> {
        let won = matches!(
            self.player.as_ref().and_then(|p| p.scene.as_ref()),
            Some(Scene::Combat(c)) if c.phase == Phase::Won
        );
        if won {
            return key_lines(
                pal,
                &[("1", " board the wreck"), ("2", " leave it")],
                width,
                1,
            );
        }
        key_lines(
            pal,
            &[
                ("arrows", " split reactor"),
                ("Enter", " fire round"),
                ("1-4", " preset+fire"),
                ("c/o/h", " close/open/hold"),
                ("r", " run"),
            ],
            width,
            2,
        )
    }

    /// The text of the enemy's shown move as an arrow row between the ships.
    fn intent_arrow(intent: Intent, ascii: bool, width: usize) -> String {
        let (glyph, fill) = match (intent, ascii) {
            (Intent::Close, false) => ("◀", "◀"),
            (Intent::Close, true) => ("<", "<"),
            (Intent::Open | Intent::Flee, false) => ("▶", "▶"),
            (Intent::Open | Intent::Flee, true) => (">", ">"),
            (Intent::Volley, false) => (".", "."),
            (Intent::Volley, true) => ("-", "-"),
            (Intent::Brace, false) => ("▒", "▒"),
            (Intent::Brace, true) => ("#", "#"),
            (Intent::Grapple, false) => ("═", "═"),
            (Intent::Grapple, true) => ("=", "="),
        };
        let _ = glyph;
        fill.repeat(width.min(8))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one panel: range, hulls, reactor, log"
    )]
    pub(super) fn draw_combat(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let Some(p) = &self.player else { return };
        let Some(Scene::Combat(c)) = &p.scene else {
            return;
        };
        let def = c.def(&self.content);
        let hull_max = u32::from(
            self.content
                .hulls
                .get(usize::from(p.ship.hull))
                .map_or(1, |h| h.hull_points)
                .max(1),
        );
        let [range_area, mid_area, log_area] = Layout::vertical([
            Constraint::Length(5),
            Constraint::Length(6),
            Constraint::Min(3),
        ])
        .areas(area);

        // Range: you, the gap, the enemy; the band names above; the telegraph below.
        let block = panel(
            pal,
            &format!("{} - {} - round {}", def.name, def.faction.name(), c.round),
            true,
        );
        let inner = block.inner(range_area);
        frame.render_widget(block, range_area);
        let w = usize::from(inner.width);
        let bands = [Range::Far, Range::Mid, Range::Close];
        let third = w / 3;
        let mut names = String::new();
        for b in bands {
            let label = if b == c.range {
                format!("[{}]", b.name().to_uppercase())
            } else {
                b.name().to_owned()
            };
            let cell = format!("{:^third$}", label);
            names.push_str(&cell);
        }
        let gap = match c.range {
            Range::Far => w.saturating_sub(24),
            Range::Mid => w.saturating_sub(24) * 55 / 100,
            Range::Close => 3,
        };
        let you = if pal.ascii { "^ YOU" } else { "▲ YOU" };
        let foe = if pal.ascii { "FOE *" } else { "FOE ◆" };
        let dots = if pal.ascii { "." } else { "·" };
        let arrow = Self::intent_arrow(c.shown, pal.ascii, gap / 2);
        let lead = gap.saturating_sub(arrow.chars().count());
        let strip = Line::from(vec![
            Span::styled(you, pal.amber),
            Span::styled(format!(" {}", dots.repeat(lead / 2)), pal.deep),
            Span::styled(
                arrow,
                if matches!(c.shown, Intent::Volley | Intent::Grapple) {
                    pal.bad
                } else {
                    pal.star
                },
            ),
            Span::styled(format!("{} ", dots.repeat(lead - lead / 2)), pal.deep),
            Span::styled(foe, pal.bad),
        ]);
        let say = if c.phase == Phase::Won {
            Line::styled("The enemy is a wreck, venting.", pal.good)
        } else {
            Line::from(vec![
                Span::styled("It is ", pal.dim),
                Span::styled(trunc(c.shown.telegraph(), w.saturating_sub(10)), pal.strong),
                Span::styled(".", pal.dim),
            ])
        };
        frame.render_widget(
            Paragraph::new(vec![Line::styled(names, pal.hull), strip, say]),
            inner,
        );

        // Hulls on the left, reactor on the right.
        let [hull_area, react_area] =
            Layout::horizontal([Constraint::Percentage(46), Constraint::Min(24)]).areas(mid_area);
        let block = panel(pal, "HULLS", false);
        let inner = block.inner(hull_area);
        frame.render_widget(block, hull_area);
        let rows = Layout::vertical([Constraint::Length(1); 4]).split(inner);
        let e_pct = u32::try_from(c.e_hull.max(0)).unwrap_or(0) * 100 / u32::from(def.hull.max(1));
        Self::gauge_row(
            frame,
            rows[0],
            pal,
            "ENEMY",
            e_pct * 10,
            pal.level(e_pct),
            &format!("{}/{}", c.e_hull.max(0), def.hull),
        );
        let s_pct =
            u32::try_from(c.e_shield.max(0)).unwrap_or(0) * 100 / u32::from(def.shield.max(1));
        Self::gauge_row(
            frame,
            rows[1],
            pal,
            "SHIELD",
            s_pct * 10,
            pal.star,
            &format!("{}/{}", c.e_shield.max(0), def.shield),
        );
        let h_pct = u32::from(p.ship.hull_hp) * 100 / hull_max;
        Self::gauge_row(
            frame,
            rows[2],
            pal,
            "YOU",
            h_pct * 10,
            pal.level(h_pct),
            &format!("{}/{hull_max}", p.ship.hull_hp),
        );
        if let Some(row) = rows.get(3) {
            let run = p.run_chance(&self.content, def, c.range);
            let mut bits = format!("run {run}%");
            if c.boarders > 0 {
                bits.push_str(&format!("  boarders {}", c.boarders));
            }
            if c.ward > 0 {
                bits.push_str(&format!("  ward {}%", c.ward));
            }
            frame.render_widget(Paragraph::new(bits).style(pal.dim), *row);
        }

        let block = panel(pal, "REACTOR", c.phase != Phase::Won);
        let inner = block.inner(react_area);
        frame.render_widget(block, react_area);
        if c.phase != Phase::Won {
            let budget = p.gear(&self.content).budget;
            let pts = self.reactor.effective(budget);
            let rows = Layout::vertical([Constraint::Length(1); 4]).split(inner);
            for (i, (name, hint)) in SLIDERS.iter().enumerate() {
                let sel = i == self.reactor.sel;
                let mark = if sel {
                    if pal.ascii { ">" } else { "▶" }
                } else {
                    " "
                };
                let fire = if i == 0 {
                    format!(
                        "{}% reach",
                        Self::reach_pct(
                            p.firepower(&self.content, c.range),
                            p.firepower(&self.content, Range::Far)
                                .max(p.firepower(&self.content, Range::Mid))
                                .max(p.firepower(&self.content, Range::Close))
                        )
                    )
                } else {
                    String::new()
                };
                let _ = hint;
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(mark, pal.amber),
                        Span::styled(pad(name, 8), if sel { pal.strong } else { pal.dim }),
                        Span::styled(
                            pips(u32::from(pts[i]), u32::from(budget), pal.ascii),
                            if sel { pal.amber } else { pal.plain },
                        ),
                        Span::styled(format!(" {}", pts[i]), pal.plain),
                        Span::styled(format!("  {fire}"), pal.dim),
                    ])),
                    rows[i],
                );
            }
            let m = |which: Maneuver, name: &str| {
                Span::styled(
                    format!(" {name} "),
                    if self.maneuver == which {
                        pal.sel
                    } else {
                        pal.dim
                    },
                )
            };
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(" Move ", pal.dim),
                    m(Maneuver::Hold, "hold"),
                    m(Maneuver::Close, "close"),
                    m(Maneuver::Open, "open"),
                ])),
                rows[3],
            );
        } else {
            frame.render_widget(
                Paragraph::new("The guns are cold. Choose what to do with the wreck.")
                    .style(pal.dim),
                inner,
            );
        }

        // The fight log: the newest lines that fit.
        let block = panel(pal, "FIGHT LOG", false);
        let inner = block.inner(log_area);
        frame.render_widget(block, log_area);
        let room = usize::from(inner.height);
        let skip = self.log.len().saturating_sub(room);
        let lines: Vec<Line<'static>> = self
            .log
            .iter()
            .skip(skip)
            .map(|(t, s)| Line::styled(trunc(t, usize::from(inner.width)), pal.role(*s)))
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
        if c.phase == Phase::Won {
            // Wreck choices are the numbered menu; list them over the log's first rows.
            let menu = self.scene_menu(p);
            let lines: Vec<Line<'static>> = menu
                .iter()
                .enumerate()
                .map(|(i, (label, _, _))| {
                    Line::from(vec![
                        Span::styled(format!("[{}] ", i + 1), pal.amber),
                        Span::styled(label.clone(), pal.strong),
                    ])
                })
                .collect();
            let h = u16::try_from(lines.len()).unwrap_or(2).min(inner.height);
            let top = Rect::new(inner.x, inner.y, inner.width, h);
            frame.render_widget(Paragraph::new(lines).style(pal.plain), top);
        }
        let _ = Bar::new(0, pal.plain, pal);
    }

    /// Weapon power at the held band as a percent of the best band (how well the guns suit it).
    fn reach_pct(now: u32, best: u32) -> u32 {
        if best == 0 { 0 } else { now * 100 / best }
    }
}
