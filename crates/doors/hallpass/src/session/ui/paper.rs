//! The school's printed things: the enrolment card, the end-of-term report, the school paper,
//! the hall of legend and the yearbook.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Modifier;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Block, BorderType, Paragraph, Wrap};

use super::{Size, Ui, centred, fit, hue, wrap};
use crate::state::Status;

/// `GREYWATER` in three rows of box drawing.
const BANNER: [&str; 3] = [
    "╔═╗ ╦═╗ ╔═╗ ╦ ╦ ╦ ╦ ╔═╗ ╔╦╗ ╔═╗ ╦═╗",
    "║ ╦ ╠╦╝ ║╣  ╚╦╝ ║║║ ╠═╣  ║  ║╣  ╠╦╝",
    "╚═╝ ╩╚═ ╚═╝  ╩  ╚╩╝ ╩ ╩  ╩  ╚═╝ ╩╚═",
];

fn banner(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let pal = &ui.pal;
    let lines: Vec<Line<'static>> = if ui.look.ascii {
        vec![
            Line::styled("G R E Y W A T E R", pal.chalk()).centered(),
            Line::styled("=================", pal.slate()).centered(),
            Line::styled("A C A D E M Y", pal.ink()).centered(),
        ]
    } else {
        let mut v: Vec<Line<'static>> = BANNER
            .iter()
            .map(|l| Line::styled((*l).to_owned(), pal.chalk()).centered())
            .collect();
        if area.height > 3 {
            v.push(Line::styled("·  A C A D E M Y  ·", pal.ink()).centered());
        }
        v
    };
    f.render_widget(Paragraph::new(lines), area);
}

/// The enrolment card: who do you want to be known as?
pub(super) fn enrol(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let pal = &ui.pal;
    let [head, intro, cards, foot] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(3),
        Constraint::Min(6),
        Constraint::Length(1),
    ])
    .areas(area);
    banner(ui, f, head);
    let text = ui
        .body
        .iter()
        .skip(1)
        .take(2)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let lines: Vec<Line<'static>> = wrap(&text, usize::from(intro.width).saturating_sub(6))
        .into_iter()
        .map(|l| Line::styled(l, pal.chalk_soft()).centered())
        .collect();
    f.render_widget(Paragraph::new(lines), intro);
    // The traits, two rows each.
    let inner = ui.framed(f, cards, "Who do you want to be known as?");
    let mut lines: Vec<Line<'static>> = Vec::new();
    let cursor = ui.cursor();
    for (i, it) in ui.items.iter().enumerate() {
        let on = i == cursor;
        let blurb =
            ui.g.content
                .traits
                .iter()
                .find(|t| t.name == it.label)
                .map(|t| t.blurb.clone());
        let marker = if on { ui.look.g("▸", ">") } else { " " };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {marker} {} ", it.key),
                if on { pal.bar_on() } else { pal.brass() },
            ),
            Span::styled(
                format!(" {}", it.label),
                if on { pal.bar_on() } else { pal.chalk() },
            ),
        ]));
        if let Some(b) = blurb {
            lines.push(Line::styled(
                format!(
                    "      {}",
                    fit(&b, usize::from(inner.width).saturating_sub(7))
                ),
                pal.slate(),
            ));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
    f.render_widget(
        Paragraph::new(Line::styled(
            "The school keeps a file on everybody. Mind yours.",
            pal.slate(),
        ))
        .centered(),
        foot,
    );
}

/// The end of a term: the ending, the numbers, the hall.
pub(super) fn over(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let [left, right] = if area.width >= 90 {
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area)
    } else {
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(area)
    };
    let expelled = g.player.status == Status::Expelled;
    let inner = ui.framed(f, left, if expelled { "Expelled" } else { "End of term" });
    let mut lines: Vec<Line<'static>> = Vec::new();
    if expelled {
        lines.push(Line::from(vec![
            Span::styled(" EXPELLED ", pal.stamp()),
            Span::styled("  The term is over for you.", pal.red()),
        ]));
    } else {
        lines.push(Line::styled("The term is over.", pal.chalk()));
    }
    if let Some(r) = g.latest_result() {
        if let Some(e) = g.content.endings.iter().find(|e| e.id == r.ending) {
            lines.push(Line::default());
            lines.push(Line::styled(e.name.to_uppercase(), pal.lamp()));
            for l in wrap(&e.text, usize::from(inner.width)) {
                lines.push(Line::styled(l, pal.chalk_soft()));
            }
        }
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled("legend ", pal.slate()),
            Span::styled(format!("{}", r.legend), pal.lamp()),
            Span::styled("   file ", pal.slate()),
            Span::styled(format!("{}", r.file), pal.red()),
            Span::styled("   average grade ", pal.slate()),
            Span::styled(format!("{}", r.grades), pal.moss()),
        ]));
    }
    lines.push(Line::default());
    lines.push(Line::styled(
        "A new term starts when the school year turns over.",
        pal.slate(),
    ));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    hall_panel(ui, f, right, "Hall of legend", 5);
}

/// The ranked hall of legend, with bars.
fn hall_panel(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, title: &str, limit: usize) {
    let g = ui.g;
    let pal = &ui.pal;
    let inner = ui.framed(f, area, title);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let top = g
        .world
        .hall
        .iter()
        .map(|e| e.legend)
        .max()
        .unwrap_or(1)
        .max(1);
    if g.world.hall.is_empty() {
        lines.push(Line::styled("Nobody has made the hall yet.", pal.slate()));
    }
    for (i, e) in g.world.hall.iter().take(limit).enumerate() {
        let medal = match i {
            0 => ui.look.g("①", "1"),
            1 => ui.look.g("②", "2"),
            2 => ui.look.g("③", "3"),
            _ => " ",
        };
        let name_w = 14usize;
        let bar_w = inner
            .width
            .saturating_sub(u16::try_from(name_w + 9).unwrap_or(20))
            .max(3);
        let mut spans = vec![
            Span::styled(
                format!(
                    "{medal}{} ",
                    if i > 2 {
                        format!("{}", i + 1)
                    } else {
                        String::new()
                    }
                ),
                pal.lamp(),
            ),
            Span::styled(
                format!("{:<name_w$}", fit(&e.name, name_w)),
                if i == 0 {
                    pal.chalk()
                } else {
                    pal.chalk_soft()
                },
            ),
        ];
        spans.extend(ui.bar(
            e.legend,
            top,
            bar_w,
            if i == 0 { pal.lamp() } else { pal.brass() },
        ));
        spans.push(Span::styled(format!(" {}", e.legend), pal.chalk_soft()));
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The school paper: headlines, what is in force, a taste of the hall, and the rumours.
pub(super) fn news(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let [mast, body, menu] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(4),
        Constraint::Length(1),
    ])
    .areas(area);
    // The masthead of the paper.
    let block = Block::bordered()
        .border_type(if ui.look.ascii {
            BorderType::Plain
        } else {
            BorderType::Double
        })
        .border_style(pal.chalk_soft());
    let inner = block.inner(mast);
    f.render_widget(block, mast);
    let day = (g.term_day() + 1).max(1);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "THE GREYWATER GAZETTE",
                pal.chalk().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("   term {}, day {day}   ", g.player.term),
                pal.slate(),
            ),
            Span::styled("price: one rumour", pal.slate()),
        ]))
        .centered(),
        inner,
    );
    let [left, right] = if body.width >= 90 {
        Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)]).areas(body)
    } else {
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(body)
    };
    // ---- headlines.
    let inner = ui.framed(f, left, "Headlines");
    let mut lines: Vec<Line<'static>> = Vec::new();
    if g.world.paper.is_empty() {
        lines.push(Line::styled(
            "A quiet week. Suspiciously quiet.",
            pal.slate(),
        ));
    }
    for h in g.world.paper.iter().rev().take(6) {
        let d = h.day - g.world.term_start + 1;
        let head = format!("day {d:<2} ");
        let room = usize::from(inner.width).saturating_sub(head.chars().count());
        let mut first = true;
        for l in wrap(&h.text, room).into_iter().take(2) {
            lines.push(Line::from(vec![
                Span::styled(
                    if first {
                        head.clone()
                    } else {
                        " ".repeat(head.chars().count())
                    },
                    pal.slate(),
                ),
                Span::styled(l, pal.chalk_soft()),
            ]));
            first = false;
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
    // ---- in force and rumours.
    let inner = ui.framed(f, right, "In force and in whispers");
    let mut lines: Vec<Line<'static>> = Vec::new();
    for a in &g.world.active {
        if let Some(e) = g.content.effect(&a.id) {
            let left = a.until.map_or(String::new(), |u| {
                let n = (u - g.player.day).max(0);
                format!(" ({n} day{})", if n == 1 { "" } else { "s" })
            });
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", ui.look.g("§", "*")), pal.lamp()),
                Span::styled(format!("{}{left}", e.name), pal.chalk_soft()),
            ]));
        }
    }
    if g.world.active.is_empty() {
        lines.push(Line::styled("Nothing out of the ordinary.", pal.slate()));
    }
    lines.push(Line::default());
    for n in g.player.news.iter().rev().take(4) {
        for l in wrap(n, usize::from(inner.width).saturating_sub(2))
            .into_iter()
            .take(2)
        {
            lines.push(Line::styled(
                format!("{} {l}", ui.look.g("›", ">")),
                pal.chalk_soft(),
            ));
        }
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    ui.chips(f, menu, false);
}

/// The hall of legend.
pub(super) fn hall(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let [menu, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(4)]).areas(area);
    ui.chips(f, menu, false);
    let limit = usize::from(body.height).saturating_sub(2).max(3);
    let r = if ui.look.size >= Size::Roomy {
        centred(body, body.width.min(90), body.height)
    } else {
        body
    };
    hall_panel(ui, f, r, "Hall of legend", limit);
}

/// The yearbook: the terms gone by, and yours.
pub(super) fn yearbook(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let [menu, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(4)]).areas(area);
    ui.chips(f, menu, false);
    let [left, right] = if body.width >= 90 {
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(body)
    } else {
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(body)
    };
    let inner = ui.framed(f, left, "The yearbook");
    let mut lines: Vec<Line<'static>> = Vec::new();
    if g.world.yearbook.is_empty() {
        lines.push(Line::styled(
            "No term has ended yet. The pages are blank.",
            pal.slate(),
        ));
    }
    for y in g
        .world
        .yearbook
        .iter()
        .rev()
        .take(usize::from(inner.height) / 2 + 1)
    {
        lines.push(Line::from(vec![
            Span::styled(format!(" TERM {} ", y.term), pal.band()),
            Span::styled(
                format!(
                    " {}",
                    fit(&y.name, usize::from(inner.width).saturating_sub(12))
                ),
                pal.chalk(),
            ),
        ]));
        lines.push(Line::styled(
            format!(
                "        {} legend, {} changes to the school",
                y.legend, y.changes
            ),
            pal.chalk_soft(),
        ));
    }
    f.render_widget(Paragraph::new(lines), inner);
    let inner = ui.framed(f, right, "Your terms");
    let mut lines: Vec<Line<'static>> = Vec::new();
    if g.player.history.is_empty() {
        lines.push(Line::styled("This is your first term.", pal.slate()));
    }
    for r in g
        .player
        .history
        .iter()
        .rev()
        .take(usize::from(inner.height) / 2)
    {
        let ending = g
            .content
            .endings
            .iter()
            .find(|e| e.id == r.ending)
            .map_or("?", |e| e.name.as_str());
        lines.push(Line::from(vec![
            Span::styled(format!("term {} ", r.term), pal.slate()),
            Span::styled(ending.to_owned(), pal.lamp()),
        ]));
        lines.push(Line::styled(
            format!("  legend {} file {} grades {}", r.legend, r.file, r.grades),
            pal.chalk_soft(),
        ));
    }
    f.render_widget(Paragraph::new(lines), inner);
    let _ = hue::INK;
}
