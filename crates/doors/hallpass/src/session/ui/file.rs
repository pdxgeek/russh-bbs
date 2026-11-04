//! The file: the headmistress's folder on you, drawn as a document that fills up. Each entry
//! is a typed line with its weight; the consequences the file has reached are stamped in
//! detention red; the rest of the sheet is your grades, skills and standing.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::Paragraph;

use super::plan::meter_line;
use super::{Size, Ui, fit, hue, wrap};
use crate::state::map_get;

/// The document: the entries on the file, newest last.
fn document(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let p = &g.player;
    let inner = ui.framed(f, area, "Pupil file");
    let w = usize::from(inner.width);
    let rule = ui.look.g("─", "-").repeat(w);
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        "OFFICE OF THE HEADMISTRESS",
        pal.chalk(),
    )]));
    let pts = p.file_points();
    let full = g.content.rules.file_full;
    lines.push(Line::from(vec![
        Span::styled(fit(&p.name, w.saturating_sub(18)), pal.ink()),
        Span::styled("  CONFIDENTIAL", pal.red()),
    ]));
    lines.push(Line::styled(rule.clone(), pal.slate()));
    let room = usize::from(inner.height).saturating_sub(lines.len() + 3);
    if p.file.is_empty() {
        for l in wrap("No entries. The headmistress has not heard of you.", w) {
            lines.push(Line::styled(l, pal.moss()));
        }
        for l in wrap(
            "(Yet. A clean sheet is a thing to be proud of, and to waste.)",
            w,
        ) {
            lines.push(Line::styled(l, pal.slate()));
        }
    }
    // The newest entries that fit.
    let skip = p.file.len().saturating_sub(room);
    for (n, e) in p.file.iter().enumerate().skip(skip) {
        let text = g
            .content
            .file_kinds
            .iter()
            .find(|k| k.id == e.kind)
            .map_or(e.kind.as_str(), |k| k.text.as_str());
        let day = e.day - g.world.term_start + 1;
        let weight = ui.look.g("▪", "*").repeat(usize::from(e.sev).min(4));
        let head = format!("{:>2}. day {day:<2} ", n + 1);
        let room = w.saturating_sub(head.chars().count() + weight.chars().count() + 1);
        lines.push(Line::from(vec![
            Span::styled(head, pal.slate()),
            Span::styled(format!("{:<room$}", fit(text, room)), pal.chalk_soft()),
            Span::styled(format!(" {weight}"), pal.red()),
        ]));
    }
    if skip > 0 {
        lines.insert(
            3,
            Line::styled(format!("   ... {skip} earlier entries"), pal.slate()),
        );
    }
    f.render_widget(Paragraph::new(lines), inner);
    // The footer of the page: how thick the folder is, and what is owed.
    let y = inner.bottom().saturating_sub(3);
    let foot = Rect::new(inner.x, y, inner.width, 3);
    let mut foot_lines = vec![Line::styled(rule, pal.slate())];
    foot_lines.push(ui.gauge(
        "thick",
        u32::try_from(pts.max(0)).unwrap_or(0),
        u32::try_from(full.max(1)).unwrap_or(1),
        inner.width,
        if pts * 2 >= full {
            pal.red()
        } else {
            pal.brass()
        },
        &format!("{pts}/{full}"),
    ));
    let mut owed = Vec::new();
    if p.detention > 0 {
        owed.push(Span::styled(" DETENTION ", pal.stamp()));
        owed.push(Span::styled(format!(" {} owed  ", p.detention), pal.red()));
    }
    if p.summons {
        owed.push(Span::styled(" SUMMONED ", pal.stamp()));
    }
    if owed.is_empty() {
        owed.push(Span::styled(
            "A week of good conduct strikes a point.",
            pal.slate(),
        ));
    }
    foot_lines.push(Line::from(owed));
    f.render_widget(Paragraph::new(foot_lines), foot);
}

/// What the file has reached, and what comes next.
fn consequences(ui: &Ui<'_>, width: u16) -> Vec<Line<'static>> {
    let g = ui.g;
    let pal = &ui.pal;
    let pts = g.player.file_points();
    let w = usize::from(width);
    let mut lines = Vec::new();
    for t in &g.content.tiers {
        let reached = pts >= t.at;
        let head = format!("{:>2} ", t.at);
        let (style, mark) = if reached {
            (pal.red(), ui.look.g("■", "#"))
        } else {
            (pal.slate(), ui.look.g("□", "."))
        };
        let room = w.saturating_sub(head.chars().count() + 2).max(8);
        let text_style = if reached {
            pal.chalk_soft()
        } else {
            pal.slate()
        };
        // Two lines of the text where there is room, the second under the first.
        for (n, l) in wrap(&t.text, room).into_iter().take(2).enumerate() {
            if n == 0 {
                lines.push(Line::from(vec![
                    Span::styled(mark.to_owned(), style),
                    Span::styled(format!(" {head}"), style),
                    Span::styled(l, text_style),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw(" ".repeat(head.chars().count() + 2)),
                    Span::styled(l, text_style),
                ]));
            }
        }
    }
    lines.push(Line::from(vec![
        Span::styled(
            ui.look.g("■", "#").to_owned(),
            if pts >= g.content.rules.file_full {
                pal.red()
            } else {
                pal.slate()
            },
        ),
        Span::styled(
            format!(" {:>2} expulsion at term end", g.content.rules.file_full),
            if pts >= g.content.rules.file_full {
                pal.red()
            } else {
                pal.slate()
            },
        ),
    ]));
    lines
}

fn record(ui: &Ui<'_>, width: u16) -> Vec<Line<'static>> {
    let g = ui.g;
    let pal = &ui.pal;
    let p = &g.player;
    let mut lines = consequences(ui, width);
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled("pulled off ", pal.slate()),
        Span::styled(format!("{}", p.pulled), pal.moss()),
        Span::styled("  caught ", pal.slate()),
        Span::styled(format!("{}", p.caught), pal.red()),
        Span::styled("  traced ", pal.slate()),
        Span::styled(format!("{}", p.pinned), pal.red()),
        Span::styled("  big ", pal.slate()),
        Span::styled(format!("{}", p.big), pal.lamp()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("cool ", pal.slate()),
        Span::styled(format!("{}", p.cool), pal.chalk_soft()),
        Span::styled("  notoriety ", pal.slate()),
        Span::styled(format!("{}", p.notoriety), pal.brass()),
        Span::styled("  heat ", pal.slate()),
        Span::styled(format!("{}", p.heat), pal.red()),
    ]));
    let traits: Vec<&str> = p
        .traits
        .iter()
        .filter_map(|t| g.content.trait_spec(t))
        .map(|t| t.name.as_str())
        .collect();
    lines.push(Line::from(vec![
        Span::styled("known as ", pal.slate()),
        Span::styled(traits.join(", "), pal.ink()),
    ]));
    lines
}

fn marks(ui: &Ui<'_>, width: u16) -> Vec<Line<'static>> {
    let g = ui.g;
    let pal = &ui.pal;
    let mut lines = Vec::new();
    for s in &g.content.subjects {
        let grade = map_get(&g.player.grades, &s.id).copied().unwrap_or(0);
        let style = if i32::from(grade) >= g.content.rules.honour_roll {
            pal.moss()
        } else if i32::from(grade) < g.content.rules.poor_grade {
            pal.red()
        } else {
            pal.fg_or(hue::INK, pal.accent)
        };
        lines.push(ui.gauge(
            &fit(&s.name, 7),
            u32::from(grade),
            100,
            width,
            style,
            &format!("{grade}"),
        ));
    }
    lines.push(Line::default());
    let mut skills: Vec<Span<'static>> = Vec::new();
    for sk in &g.content.skills {
        let lvl = usize::try_from(g.skill_level(&sk.id)).unwrap_or(0);
        if !skills.is_empty() {
            skills.push(Span::styled("  ", pal.slate()));
        }
        skills.push(Span::styled(format!("{} ", sk.name), pal.slate()));
        skills.extend(ui.pips(lvl, 5, pal.lamp()));
    }
    lines.push(Line::from(skills));
    lines
}

fn standing(ui: &Ui<'_>, width: u16) -> Vec<Line<'static>> {
    let g = ui.g;
    let pal = &ui.pal;
    let mut lines = Vec::new();
    for c in &g.content.cliques {
        let v = g.player.standing_with(&c.id);
        let name = c.name.trim_start_matches("The ");
        lines.push(meter_line(ui, name, 13, v, format!(" {v:+}"), width, false));
    }
    let wary: Vec<String> = g
        .player
        .wary
        .iter()
        .filter_map(|(n, w)| {
            g.content.npc(n).map(|x| {
                format!(
                    "{} {}",
                    x.name.split_whitespace().last().unwrap_or(""),
                    w.level
                )
            })
        })
        .collect();
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled("watching you  ", pal.slate()),
        Span::styled(
            if wary.is_empty() {
                "nobody yet".to_owned()
            } else {
                wary.join(", ")
            },
            pal.red(),
        ),
    ]));
    lines
}

/// The file screen.
pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let pal = &ui.pal;
    let [back, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(4)]).areas(area);
    // The one real item is "back"; show it as a chip.
    ui.chips(f, back, false);
    if ui.look.size >= Size::Roomy && body.width >= 100 {
        let [doc, a, b] = Layout::horizontal([
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Percentage(30),
        ])
        .areas(body);
        document(ui, f, doc);
        let inner = ui.framed(f, a, "Record");
        f.render_widget(Paragraph::new(record(ui, inner.width)), inner);
        let [m, s] =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(b);
        let inner = ui.framed(f, m, "Marks and skills");
        f.render_widget(Paragraph::new(marks(ui, inner.width)), inner);
        let inner = ui.framed(f, s, "Standing");
        f.render_widget(Paragraph::new(standing(ui, inner.width)), inner);
        return;
    }
    let [doc, side] =
        Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)]).areas(body);
    document(ui, f, doc);
    let tab = ui.s.tab % 3;
    let titles = ["Record", "Marks", "Standing"];
    let title = format!("{} {}/3", titles[tab], tab + 1);
    let inner = ui.framed(f, side, &title);
    let lines = match tab {
        0 => record(ui, inner.width),
        1 => marks(ui, inner.width),
        _ => standing(ui, inner.width),
    };
    f.render_widget(Paragraph::new(lines), inner);
    let hint = format!("{} {}", ui.look.g("◂", "<"), ui.look.g("▸", ">"));
    let w = u16::try_from(hint.chars().count()).unwrap_or(0);
    if side.width > w + 4 {
        f.render_widget(
            Paragraph::new(Span::styled(hint, pal.slate())),
            Rect::new(side.right() - w - 2, side.y, w, 1),
        );
    }
    let _: Style = pal.plain;
}
