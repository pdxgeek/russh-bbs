//! The crew: a roster table, a detail pane for the crew member on the highlight (traits,
//! injuries and scars, skills, bonds, age), and the orders that concern them.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Row, Table, TableState};

use super::{Size, Ui, fit};
use crate::crew::{Crew, RETIRE_AGE};
use crate::types::{Attr, Trade};

/// Whether the order with this tag concerns crew member `id` (or the gate, which concerns all).
pub(super) fn about(tag: &str, id: u16) -> bool {
    tag.starts_with("recruit:")
        || tag.starts_with(&format!("tend:{id}"))
        || tag.starts_with(&format!("post:{id}@"))
        || tag.starts_with(&format!("train:{id}:"))
}

fn status(ui: &Ui<'_>, c: &Crew) -> (String, Style) {
    let p = &ui.pal;
    if ui.clan.social.is_away(c.id) {
        ("away".to_owned(), p.gold())
    } else if ui
        .clan
        .trip
        .as_ref()
        .is_some_and(|t| t.party.contains(&c.id))
    {
        ("out".to_owned(), p.sea())
    } else if !c.injuries.is_empty() {
        (format!("hurt {}", c.injuries.len()), p.rust())
    } else if let Some(post) = c.post {
        (format!("post {post}"), p.sea())
    } else {
        ("idle".to_owned(), p.dim)
    }
}

/// The roster as a table with the highlight on the selected crew member.
fn roster(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let clan = ui.clan;
    let rows: Vec<Row<'static>> = clan
        .crew
        .iter()
        .map(|c| {
            let (_, style) = status(ui, c);
            let bonded = !clan.partners(c.id).is_empty();
            let mark = if ui.clan.social.is_away(c.id) {
                ui.look.g("»", ">")
            } else if !c.injuries.is_empty() {
                ui.look.g("+", "+")
            } else if bonded {
                ui.look.g("♥", "&")
            } else {
                " "
            };
            Row::new(vec![
                Span::styled(fit(&c.name, 16), p.plain),
                Span::styled(c.trade.name(), p.peat()),
                Span::styled(format!("{}", c.age), p.dim),
                Span::styled(mark, style),
            ])
        })
        .collect();
    let title = format!("Crew {}/{}", clan.crew.len(), clan.hold.crew_cap(&ui.s.cat));
    let table = Table::new(
        rows,
        [
            Constraint::Min(10),
            Constraint::Length(10),
            Constraint::Length(3),
            Constraint::Length(1),
        ],
    )
    .header(Row::new(vec!["name", "trade", "age", ""]).style(p.dim))
    .row_highlight_style(p.selected)
    .highlight_symbol(ui.look.g("▸", ">"))
    .block(ui.panel(&title));
    let mut state = TableState::default()
        .with_selected(Some(ui.s.crew_sel.min(clan.crew.len().saturating_sub(1))));
    f.render_stateful_widget(table, area, &mut state);
}

/// Candidates at the gate, with the key that takes each on.
fn gate(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let rows: Vec<Row<'static>> = ui
        .clan
        .candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let key = ui
                .entry(&format!("recruit:{i}"))
                .map_or(' ', |(n, _)| Ui::key_of(n));
            Row::new(vec![
                Span::styled(format!("[{key}]"), p.accent),
                Span::styled(fit(&c.name, 12), p.plain),
                Span::styled(c.trade.name(), p.peat()),
                Span::styled(format!("{}", c.age), p.dim),
                Span::styled(format!("{}t", c.price(&ui.s.cat)), p.gold()),
            ])
        })
        .collect();
    f.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(4),
                Constraint::Min(8),
                Constraint::Length(9),
                Constraint::Length(3),
                Constraint::Length(5),
            ],
        )
        .block(ui.panel("At the gate")),
        area,
    );
}

fn skill_rows(ui: &Ui<'_>, c: &Crew, width: u16, rows: usize) -> Vec<Line<'static>> {
    let p = &ui.pal;
    let mut skills: Vec<(Trade, u8)> = Trade::ALL
        .iter()
        .map(|&t| (t, c.level(t)))
        .filter(|(t, l)| *l > 0 || *t == c.trade)
        .collect();
    skills.sort_by_key(|(t, l)| (std::cmp::Reverse(*l), t.index()));
    // One to a row when there is room; two columns when there is not.
    let columns = if skills.len() <= rows { 1 } else { 2 };
    let col_w = width / columns;
    let bar_w = col_w.saturating_sub(15).max(3);
    let cell = |(t, l): (Trade, u8)| -> Vec<Span<'static>> {
        let mut spans = vec![Span::styled(
            format!("{:<11}", t.name()),
            if t == c.trade { p.peat() } else { p.dim },
        )];
        spans.extend(ui.bar(u32::from(l), 10, bar_w, p.sea()));
        spans.push(Span::styled(format!(" {l:>2} "), p.plain));
        spans
    };
    skills
        .chunks(usize::from(columns))
        .map(|group| {
            let mut spans = Vec::new();
            for s in group {
                spans.extend(cell(*s));
            }
            Line::from(spans)
        })
        .collect()
}

/// The detail pane.
fn detail(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, c: &Crew) {
    let p = &ui.pal;
    let cat = &ui.s.cat;
    let block = ui.panel(&format!("{}, the {}", c.name, c.trade.name()));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let w = inner.width;
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Age against retirement, and the post.
    let mut age = vec![Span::styled("Age     ", p.dim)];
    age.extend(ui.bar(
        u32::from(c.age),
        u32::from(RETIRE_AGE),
        (w / 3).clamp(6, 20),
        p.slate(),
    ));
    age.push(Span::styled(
        format!(" {} (retires at {RETIRE_AGE})", c.age),
        p.plain,
    ));
    lines.push(Line::from(age));
    if ui.clan.social.is_away(c.id) {
        lines.push(Line::styled(
            "Away on a raid: back with the report.",
            p.gold(),
        ));
    } else if ui
        .clan
        .trip
        .as_ref()
        .is_some_and(|t| t.party.contains(&c.id))
    {
        lines.push(Line::styled("Out on the marsh with the party.", p.sea()));
    }

    // The three attributes, one to a row (on one line when the pane is short).
    if inner.height < 12 {
        let mut row: Vec<Span<'static>> = Vec::new();
        for a in Attr::ALL {
            let v = c.attrs[a.index()];
            row.push(Span::styled(
                format!("{} ", &a.name()[..a.name().len().min(4)]),
                p.dim,
            ));
            row.extend(ui.bar(u32::from(v), 10, 4, p.moss()));
            row.push(Span::styled(format!(" {v:<2} "), p.plain));
        }
        lines.push(Line::from(row));
    }
    for a in Attr::ALL.iter().filter(|_| inner.height >= 12) {
        let v = c.attrs[a.index()];
        let mut row = vec![Span::styled(format!("{:<8}", a.name()), p.dim)];
        row.extend(ui.bar(u32::from(v), 10, (w / 2).clamp(6, 24), p.moss()));
        row.push(Span::styled(format!(" {v:>2}"), p.plain));
        lines.push(Line::from(row));
    }

    // Traits, with the scars marked.
    for id in &c.traits {
        if let Some(t) = cat.trait_def(id) {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "{} ",
                        if t.scar {
                            ui.look.g("✖", "x")
                        } else {
                            ui.look.g("◆", "*")
                        }
                    ),
                    if t.scar { p.rust() } else { p.gold() },
                ),
                Span::styled(
                    format!("{}{}", t.name, if t.scar { " (a scar)" } else { "" }),
                    if t.scar { p.rust() } else { p.salt() },
                ),
                Span::styled(
                    format!(
                        " {}",
                        fit(&t.blurb, usize::from(w).saturating_sub(t.name.len() + 14))
                    ),
                    p.dim,
                ),
            ]));
        }
    }

    // Injuries: days left to heal.
    if c.injuries.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("{} no injuries", ui.look.g("✓", "+")),
            p.moss(),
        )));
    }
    for inj in &c.injuries {
        if let Some(def) = cat.injury(&inj.id) {
            let mut spans = vec![Span::styled(
                format!("{} {:<14}", ui.look.g("✚", "+"), fit(&def.name, 14)),
                p.rust(),
            )];
            spans.extend(ui.bar(
                u32::from(def.days.saturating_sub(inj.days)),
                u32::from(def.days.max(1)),
                (w / 4).clamp(4, 14),
                p.moss(),
            ));
            spans.push(Span::styled(
                format!(
                    " {} day{} to heal",
                    inj.days,
                    if inj.days == 1 { "" } else { "s" }
                ),
                p.dim,
            ));
            lines.push(Line::from(spans));
        }
    }

    // Bonds.
    let partners = ui.clan.partners(c.id);
    let bond_line = if partners.is_empty() {
        Line::from(Span::styled(
            format!(
                "{} no bonds yet: hardship shared makes them",
                ui.look.g("♡", "-")
            ),
            p.dim,
        ))
    } else {
        let names: Vec<String> = partners
            .iter()
            .filter_map(|id| ui.clan.crew_by(*id))
            .map(|x| x.name.clone())
            .collect();
        Line::from(vec![
            Span::styled(format!("{} bonded with ", ui.look.g("♥", "&")), p.gold()),
            Span::styled(names.join(", "), p.salt()),
        ])
    };
    lines.push(bond_line);
    lines.push(Line::from(Span::styled("Skills", p.accent)));
    let used = lines.len();
    lines.extend(skill_rows(
        ui,
        c,
        w,
        usize::from(inner.height).saturating_sub(used),
    ));
    f.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let roomy = ui.look.size >= Size::Roomy;
    let [left, right] = Layout::horizontal([
        Constraint::Length(if roomy { 44 } else { 38 }),
        Constraint::Min(30),
    ])
    .areas(area);
    if roomy && !ui.clan.candidates.is_empty() {
        let gate_h = u16::try_from(ui.clan.candidates.len()).unwrap_or(3) + 3;
        let [r, g] = Layout::vertical([Constraint::Min(6), Constraint::Length(gate_h)]).areas(left);
        roster(ui, f, r);
        gate(ui, f, g);
    } else {
        roster(ui, f, left);
    }
    let orders_h = if roomy { 8 } else { 5 };
    let [top, bottom] =
        Layout::vertical([Constraint::Min(8), Constraint::Length(orders_h)]).areas(right);
    if let Some(c) = ui.clan.crew.get(ui.s.crew_sel) {
        detail(ui, f, top, c);
        let items: Vec<_> = ui
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| about(&e.tag, c.id))
            .collect();
        ui.orders(
            f,
            bottom,
            &format!("Orders for {}", fit(&c.name, 16)),
            &items,
        );
    } else {
        f.render_widget(
            Paragraph::new("The hold has no crew.").block(ui.panel("Crew")),
            top,
        );
    }
}
