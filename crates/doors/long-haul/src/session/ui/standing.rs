//! Factions and standings: where the captain stands with each, on a bar that runs both ways,
//! with the lines that matter (docks, offices, recruits, bounties) marked on it.
use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::Paragraph;

use super::kit::{Centered, Pal, pad, panel, rpad, trunc};
use crate::content::Faction;
use crate::contracts::{OFFICE_SHUT, PREMIUM_STANDING};
use crate::engine::{BAR_STANDING, SMUGGLER_STANDING};
use crate::session::Session;

impl Session {
    /// What a faction's standing means for this captain right now, in words (from the
    /// engine's published thresholds).
    pub(in crate::session) fn standing_note(f: Faction, v: i16) -> String {
        let mut bits: Vec<String> = Vec::new();
        if matches!(f, Faction::League | Faction::Miners | Faction::Free) && v <= BAR_STANDING {
            bits.push("its docks are closed to you".to_owned());
        }
        if f == Faction::Free && v >= SMUGGLER_STANDING {
            bits.push("military posts turn you away".to_owned());
        }
        if v <= OFFICE_SHUT {
            bits.push("its offices post no jobs".to_owned());
        } else if v >= PREMIUM_STANDING {
            bits.push("better jobs, better pay".to_owned());
        }
        if v <= crate::crew::RECRUIT_REFUSAL {
            bits.push("its recruits refuse you".to_owned());
        }
        if bits.is_empty() {
            "no doors opened or shut yet".to_owned()
        } else {
            bits.join("; ")
        }
    }

    pub(super) fn draw_standing(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let Some(p) = &self.player else { return };
        let block = panel(pal, "FACTIONS AND STANDING", true);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let rows = Layout::vertical(vec![Constraint::Length(2); 5]).split(inner);
        for (f, row) in Faction::ALL.into_iter().zip(rows.iter()) {
            let v = p.standing[f.index()];
            let [top, bottom] =
                Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(*row);
            let [name, bar, val] = Layout::horizontal([
                Constraint::Length(22),
                Constraint::Min(10),
                Constraint::Length(12),
            ])
            .areas(top);
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(if pal.ascii { "* " } else { "● " }, pal.faction(f.index())),
                    Span::styled(pad(f.name(), 20), pal.strong),
                ])),
                name,
            );
            frame.render_widget(
                Centered {
                    value: i32::from(v),
                    max: 100,
                    neg: pal.bad,
                    pos: pal.good,
                    track: pal.dim,
                    ascii: pal.ascii,
                },
                bar,
            );
            let rival = f
                .rival()
                .map_or(String::new(), |r| format!(" vs {}", r.short()));
            frame.render_widget(
                Paragraph::new(format!(
                    " {}{}",
                    rpad(&format!("{v:+}"), 4),
                    trunc(&rival, 8)
                ))
                .style(pal.plain),
                val,
            );
            frame.render_widget(
                Paragraph::new(format!(
                    "  {}",
                    trunc(
                        &Self::standing_note(f, v),
                        usize::from(bottom.width).saturating_sub(2)
                    )
                ))
                .style(pal.dim),
                bottom,
            );
        }
    }
}
