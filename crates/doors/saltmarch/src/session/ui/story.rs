//! The faction boards and the journal: standing with the three factions, the day's contracts
//! and the quest chains the clan has heard of.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Wrap};

use super::{Ui, fit};
use crate::story::{Faction, Progress, TIERS};

fn standing_lines(ui: &Ui<'_>) -> Vec<Line<'static>> {
    let p = &ui.pal;
    Faction::ALL
        .iter()
        .map(|f| {
            let rep = ui.clan.story.rep[f.index()];
            let tier = usize::from(ui.clan.story.tier(*f));
            Line::from(vec![
                Span::styled(format!("{:<15}", f.name()), p.plain),
                Span::styled(
                    format!("{:<9}", TIERS[tier]),
                    if tier == 0 { p.rust() } else { p.gold() },
                ),
                Span::styled(format!("{rep:>4}"), p.dim),
            ])
        })
        .collect()
}

/// The contract board: what is in hand and what is on offer today.
pub(super) fn board(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Min(28)]).areas(area);
    let items: Vec<_> = ui.entries.iter().enumerate().collect();
    ui.orders(f, left, "The boards", &items);
    let mut lines = standing_lines(ui);
    lines.push(Line::raw(""));
    let width = usize::from(right.width).saturating_sub(4);
    match ui.clan.story.contract.as_ref() {
        Some(a) => {
            if let Some(def) = ui.s.cat.contracts.iter().find(|c| c.id == a.id) {
                lines.push(Line::styled(format!("In hand: {}", def.title), p.salt()));
                lines.push(Line::styled(def.text.clone(), p.plain));
                let left_days = (a.deadline - ui.s.today + 1).max(0);
                lines.push(Line::styled(
                    format!(
                        "{} {} x{} - {} day(s) left, {}/{} done",
                        def.faction.name(),
                        def.kind.word(),
                        def.qty,
                        left_days,
                        a.progress,
                        def.qty
                    ),
                    p.dim,
                ));
            }
        }
        None => lines.push(Line::styled(
            "Nothing in hand. Take one from today's board.",
            p.dim,
        )),
    }
    lines.push(Line::raw(""));
    for def in crate::board::offers(&ui.s.cat, ui.clan, ui.s.world.seed, ui.s.today) {
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", def.faction.id()), p.reed()),
            Span::styled(fit(&def.title, width.saturating_sub(24)), p.plain),
            Span::styled(
                format!(" {}-{}t +{}", def.pay_lo, def.pay_hi, def.rep),
                p.gold(),
            ),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(ui.panel("Standing and offers")),
        right,
    );
}

/// The journal: every chain the clan has heard of, and where it stands in each.
pub(super) fn journal(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(45), Constraint::Min(30)]).areas(area);
    let items: Vec<_> = ui.entries.iter().enumerate().collect();
    ui.orders(f, left, "The journal", &items);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let has = |m: &str| ui.clan.has_mark(m);
    let mut heard = 0;
    for q in &ui.s.cat.quests {
        let prog = q.progress(has);
        if prog == Progress::Unheard {
            continue;
        }
        heard += 1;
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", q.faction.id()), p.reed()),
            Span::styled(q.title.clone(), p.salt()),
        ]));
        match prog {
            Progress::Ended(i) => lines.push(Line::styled(
                format!("  Ended: {}", q.ends[i].text),
                p.gold(),
            )),
            Progress::Under(n) => {
                let last = &q.steps[n.saturating_sub(1)];
                lines.push(Line::styled(
                    format!("  {}/{} {}", n, q.steps.len(), last.text),
                    p.plain,
                ));
            }
            Progress::Unheard => {}
        }
    }
    if heard == 0 {
        lines.push(Line::styled(
            "No chain has begun. Faction scenes on the marsh open them; standing opens more.",
            p.dim,
        ));
    }
    if let Some(h) = ui.clan.story.heirloom() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!("Heirloom: {} ({})", h.name(), h.effect()),
            p.gold(),
        ));
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(ui.panel("Chains heard of")),
        right,
    );
}
