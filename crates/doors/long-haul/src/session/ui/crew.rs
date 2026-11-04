//! The crew roster and a detail pane: skill, the four parts of mood, loyalty, the personal
//! goal and its progress, scars and wages; recruits at a dock below the hands.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "gauge values are small and clamped"
)]

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Wrap};

use super::kit::{Bar, Centered, Pal, credits, pad, panel, pips, rpad, trunc};
use crate::crew::{DANGER, FOOD, PAY, TREATMENT};
use crate::session::{Mode, Session};

impl Session {
    pub(super) fn draw_crew(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let (Some(p), Mode::Crew(cursor)) = (&self.player, &self.mode) else {
            return;
        };
        let cursor = *cursor;
        let [list_area, detail_area] =
            Layout::horizontal([Constraint::Min(34), Constraint::Length(36)]).areas(area);
        let title = format!(
            "CREW {}/{} berths - {} cr/wk",
            p.berths_used(),
            p.berths(&self.content),
            credits(p.weekly_wages())
        );
        let block = panel(pal, &title, true);
        let inner = block.inner(list_area);
        frame.render_widget(block, list_area);
        let w = usize::from(inner.width);
        let name_w = w.saturating_sub(23).max(8);
        let mut lines: Vec<Line<'static>> = Vec::new();
        if p.crew.is_empty() {
            lines.push(Line::styled("No crew. You fly alone.", pal.dim));
        }
        for (i, c) in p.crew.iter().enumerate() {
            let sel = i == cursor;
            let tag = if c.sworn {
                "sworn"
            } else if c.restless() {
                "RESTLESS"
            } else if !c.fit() {
                "hurt"
            } else {
                ""
            };
            let row = format!(
                "{}{} {} {} {}",
                if sel {
                    if pal.ascii { '>' } else { '▶' }
                } else {
                    ' '
                },
                pad(&c.name, name_w),
                pad(c.role.name(), 8),
                rpad(&c.loyalty.to_string(), 3),
                pad(tag, 8)
            );
            lines.push(Line::styled(
                trunc(&row, w),
                if sel {
                    pal.sel
                } else if c.restless() || !c.fit() {
                    pal.bad
                } else {
                    pal.plain
                },
            ));
        }
        let recruits = p
            .docked
            .map(|s| p.recruit_list(&self.content, &self.frontier, s, p.last_day))
            .unwrap_or_default();
        if p.docked.is_some() {
            lines.push(Line::styled(
                if recruits.is_empty() {
                    "No one is looking for a berth here today."
                } else {
                    "For hire at this dock:"
                },
                pal.dim,
            ));
        }
        for (j, (_, r)) in recruits.iter().enumerate() {
            let sel = p.crew.len() + j == cursor;
            let row = format!(
                "{}{} {} sk{} {} cr/wk",
                if sel {
                    if pal.ascii { '>' } else { '▶' }
                } else {
                    ' '
                },
                pad(&r.name, name_w),
                pad(r.role.name(), 8),
                r.skill,
                r.wage
            );
            lines.push(Line::styled(
                trunc(&row, w),
                if sel { pal.sel } else { pal.good },
            ));
        }
        frame.render_widget(Paragraph::new(lines), inner);

        // Detail.
        let block = panel(pal, "DETAIL", false);
        let inner = block.inner(detail_area);
        frame.render_widget(block, detail_area);
        if let Some(c) = p.crew.get(cursor) {
            let rows = Layout::vertical([
                Constraint::Length(2),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(2),
            ])
            .split(inner);
            let blurb = self
                .content
                .crew
                .role
                .iter()
                .find(|r| r.role == c.role)
                .map_or(String::new(), |r| r.blurb.clone());
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(vec![
                        Span::styled(c.name.clone(), pal.accent),
                        Span::styled(format!(" - {} - {}", c.role.name(), c.mood_word()), pal.dim),
                    ]),
                    Line::styled(trunc(&blurb, usize::from(inner.width)), pal.dim),
                ]),
                rows[0],
            );
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(pad("Skill", 9), pal.dim),
                    Span::styled(pips(u32::from(c.effective()), 10, pal.ascii), pal.amber),
                    Span::styled(
                        format!(
                            " {}{}",
                            c.effective(),
                            if c.scars > 0 {
                                format!(" ({} scar)", c.scars)
                            } else {
                                String::new()
                            }
                        ),
                        pal.plain,
                    ),
                ])),
                rows[1],
            );
            for (k, (name, part)) in [
                ("Pay", PAY),
                ("Food", FOOD),
                ("Danger", DANGER),
                ("Treated", TREATMENT),
            ]
            .into_iter()
            .enumerate()
            {
                let row = rows[2 + k.min(3)];
                let [l, b] =
                    Layout::horizontal([Constraint::Length(9), Constraint::Min(6)]).areas(row);
                frame.render_widget(Paragraph::new(name).style(pal.dim), l);
                frame.render_widget(
                    Centered {
                        value: i32::from(c.mood[part]),
                        max: 10,
                        neg: pal.bad,
                        pos: pal.good,
                        track: pal.dim,
                        ascii: pal.ascii,
                    },
                    b,
                );
            }
            let [l, b] =
                Layout::horizontal([Constraint::Length(9), Constraint::Min(6)]).areas(rows[6]);
            frame.render_widget(
                Paragraph::new(if c.sworn { "Loyal*" } else { "Loyalty" }).style(pal.dim),
                l,
            );
            frame.render_widget(
                Bar::new(
                    u32::from(c.loyalty) * 10,
                    pal.level(u32::from(c.loyalty)),
                    pal,
                ),
                b,
            );
            let goal_pm = if c.goal.target == 0 {
                0
            } else {
                c.goal.progress.min(c.goal.target) * 1000 / c.goal.target
            };
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(vec![
                        Span::styled("Goal: ", pal.dim),
                        Span::styled(c.goal_text(&self.content, &self.frontier), pal.plain),
                        Span::styled(format!(" ({})", c.goal_progress()), pal.amber),
                    ]),
                    Line::styled(
                        format!(
                            "{} cr/wk{}",
                            c.wage,
                            if c.fit() {
                                String::new()
                            } else {
                                format!(" - hurt {} d", c.hurt_days)
                            }
                        ),
                        pal.dim,
                    ),
                ])
                .wrap(Wrap { trim: true }),
                rows[7],
            );
            let _ = goal_pm;
        } else if let Some((_, r)) = recruits.get(cursor.saturating_sub(p.crew.len())) {
            let lines = vec![
                Line::from(vec![
                    Span::styled(r.name.clone(), pal.accent),
                    Span::styled(format!(" - {} - skill {}", r.role.name(), r.skill), pal.dim),
                ]),
                Line::styled(
                    format!(
                        "Wage {} cr/wk, signing fee {}",
                        r.wage,
                        u32::from(r.wage) * 2
                    ),
                    pal.plain,
                ),
                Line::styled(
                    format!("Wants: {}", r.goal_text(&self.content, &self.frontier)),
                    pal.plain,
                ),
                Line::styled("Meet their goal and they swear to the ship.", pal.dim),
            ];
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
        }
    }
}
