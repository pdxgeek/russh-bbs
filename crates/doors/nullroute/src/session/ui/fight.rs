//! The encounter screen: the countermeasure and its integrity, the telegraphed beat, the pattern
//! you have learned, your scripts with their cost and what each would do now, and the fight log.

use russh_bbs_door::layout::{Alignment, Constraint, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Cell, Paragraph, Row, Table};

use super::hack::{header, log, stage_look, trace};
use super::{AMBER, CYAN, GREEN, Pen, RED, STEEL, Size, VIOLET, bar, bold, clip, cols, fg, rows};
use crate::cm::{Encounter, ROUNDS_MAX};
use crate::content::{CmKind, Family, Intent};
use crate::learn::Stage;
use crate::session::{Active, Session};

/// The colour a beat is drawn in: danger red for what hurts, amber for what prepares, cyan for
/// the rest.
fn intent_color(i: Intent) -> russh_bbs_door::style::Color {
    match i {
        Intent::Strike | Intent::Trace | Intent::Counter => RED,
        Intent::Charge | Intent::Seal => AMBER,
        Intent::Guard | Intent::Mend => VIOLET,
        Intent::Probe => CYAN,
    }
}

/// A few rows of art for each kind of machine, in box drawing and blocks (the framework gives the
/// ASCII tier its own stand-ins).
fn art(kind: CmKind) -> [&'static str; 4] {
    match kind {
        CmKind::Sentry => [
            "  \u{2554}\u{2550}\u{2550}\u{2550}\u{2557}  ",
            "  \u{2551}\u{25C9} \u{25C9}\u{2551}  ",
            "  \u{2560}\u{2550}\u{2564}\u{2550}\u{2563}  ",
            "  \u{255A}\u{2550}\u{2569}\u{2550}\u{255D}  ",
        ],
        CmKind::Tracer => [
            " \u{2500}\u{2500}\u{25B6}\u{25B6}\u{25B6}\u{25B6}\u{25B6} ",
            " \u{2591}\u{2592}\u{2593}\u{2588}\u{2593}\u{2592}\u{2591}  ",
            " \u{2500}\u{2500}\u{25B6}\u{25B6}\u{25B6}\u{25B6}\u{25B6} ",
            " \u{2591}\u{2591}\u{2592}\u{2593}\u{2588}\u{2588}\u{2593} ",
        ],
        CmKind::Wall => [
            " \u{2588}\u{2588}\u{2588}\u{2593}\u{2588}\u{2588}\u{2588}\u{2593}\u{2588}\u{2588} ",
            " \u{2593}\u{2588}\u{2588}\u{2588}\u{2593}\u{2588}\u{2588}\u{2588}\u{2593}\u{2588} ",
            " \u{2588}\u{2588}\u{2588}\u{2593}\u{2588}\u{2588}\u{2588}\u{2593}\u{2588}\u{2588} ",
            " \u{2593}\u{2588}\u{2588}\u{2588}\u{2593}\u{2588}\u{2588}\u{2588}\u{2593}\u{2588} ",
        ],
        CmKind::Warden => [
            " \u{256D}\u{2500}\u{2500}\u{25C9}\u{2500}\u{2500}\u{256E} ",
            " \u{2502}\u{2593}\u{2588}\u{2588}\u{2588}\u{2593}\u{2502} ",
            " \u{251C}\u{2500}\u{2524}\u{2588}\u{251C}\u{2500}\u{2524} ",
            " \u{2570}\u{2500}\u{2500}\u{252C}\u{2500}\u{2500}\u{256F} ",
        ],
    }
}

/// The machine's state: integrity, armor, weakness, and anything it is under.
fn machine(pen: &mut Pen<'_, '_>, e: &Encounter, rect: Rect, with_art: bool) {
    let w = usize::from(rect.width);
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(format!("{} ", e.name), bold(VIOLET)),
        Span::styled(format!("({})", e.kind.word()), pen.ink.dim),
    ]));
    if with_art {
        for row in art(e.kind) {
            lines.push(Line::styled(row, fg(VIOLET)));
        }
    }
    let frac = f64::from(e.integrity) / f64::from(e.max.max(1));
    let mut spans = vec![Span::styled("INTEGRITY ", pen.ink.dim)];
    let bar_w = w.saturating_sub(10 + 8).max(6);
    spans.extend(bar(bar_w, frac, RED, false));
    spans.push(Span::styled(
        format!(" {}/{}", e.integrity, e.max),
        pen.ink.plain,
    ));
    lines.push(Line::from(spans));
    lines.push(Line::from(vec![
        Span::styled("armor ", pen.ink.dim),
        Span::styled(
            e.armor_now().to_string(),
            if e.armor_now() > e.armor {
                bold(AMBER)
            } else {
                pen.ink.plain
            },
        ),
        Span::styled("   weak to ", pen.ink.dim),
        Span::styled(format!("{} locks", e.weak.word()), fg(GREEN)),
    ]));
    let mut notes = Vec::new();
    if e.stun > 0 {
        notes.push(("blind", fg(GREEN)));
    }
    if e.charged {
        notes.push(("NEXT STRIKE x2", bold(RED)));
    }
    if let Some(f) = e.sealed {
        notes.push((
            if f == Family::Break {
                "BREAK SEALED"
            } else {
                "a family is sealed"
            },
            bold(AMBER),
        ));
    }
    if !notes.is_empty() {
        let mut spans = Vec::new();
        for (text, style) in notes {
            spans.push(Span::styled(format!("[{text}] "), style));
        }
        lines.push(Line::from(spans));
    }
    pen.text(rect, lines);
}

/// The telegraph: what it does on this beat, what follows, what has been seen.
fn telegraph(pen: &mut Pen<'_, '_>, e: &Encounter, rect: Rect) {
    let now = e.intent();
    let tone = intent_color(now);
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("IT WILL  ", pen.ink.dim),
        Span::styled(
            format!(" {} ", now.word()),
            bold(tone).add_modifier(russh_bbs_door::style::Modifier::UNDERLINED),
        ),
        Span::styled(format!("  {}", now.gloss()), fg(tone)),
    ]));
    let forecast = e.forecast(3);
    let mut next = vec![Span::styled("THEN      ", pen.ink.dim)];
    if forecast.is_empty() {
        next.push(Span::styled("? ? ?", fg(STEEL)));
        next.push(Span::styled(
            format!("  (watch a full cycle of {} or scan it)", e.pattern.len()),
            pen.ink.dim,
        ));
    } else {
        for i in forecast {
            next.push(Span::styled(format!("{} ", i.word()), fg(intent_color(i))));
        }
    }
    lines.push(Line::from(next));
    let mut seen = vec![Span::styled("SEEN      ", pen.ink.dim)];
    if e.seen.is_empty() {
        seen.push(Span::styled("nothing yet", fg(STEEL)));
    }
    for i in &e.seen {
        seen.push(Span::styled(
            format!("{} ", i.word()),
            fg(intent_color(*i)).add_modifier(russh_bbs_door::style::Modifier::DIM),
        ));
    }
    lines.push(Line::from(seen));
    // The moves, as pips: the machine's patience.
    let total = ROUNDS_MAX as usize;
    let used = (e.round as usize).min(total);
    let mut pips = vec![Span::styled("MOVE      ", pen.ink.dim)];
    for i in 0..total {
        let (ch, style) = match i.cmp(&used) {
            std::cmp::Ordering::Less => ('\u{25A0}', fg(AMBER)),
            std::cmp::Ordering::Equal => ('\u{25A0}', bold(CYAN)),
            std::cmp::Ordering::Greater => ('\u{25A1}', fg(STEEL)),
        };
        pips.push(Span::styled(format!("{ch} "), style));
    }
    pips.push(Span::styled(
        format!("{}/{}", used + 1, ROUNDS_MAX),
        pen.ink.plain,
    ));
    lines.push(Line::from(pips));
    pen.text(rect, lines);
}

/// Your scripts: the key, the name, the cycles, what it would do now as a bar, and its state.
fn scripts(pen: &mut Pen<'_, '_>, s: &Session, a: &Active, e: &Encounter, rect: Rect) {
    let run = &a.run;
    let c = &*s.content;
    let max_dmg = e.cap().max(1);
    let bar_w = if pen.size == Size::Small { 5 } else { 10 };
    let rows_: Vec<Row<'static>> = run
        .deck
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let script = run.script(c, slot.script);
            let stage = run.stage(c, i);
            let sealed = script
                .as_ref()
                .is_some_and(|sc| e.sealed == Some(sc.family));
            let dmg = run.preview(c, i).unwrap_or(0);
            let tuned = script.as_ref().is_some_and(|sc| e.tuned(sc));
            let (state, style): (&str, Style) = if stage == Stage::Patched {
                ("PATCHED", bold(RED))
            } else if sealed {
                ("SEALED", bold(AMBER))
            } else if slot.left == 0 {
                ("spent", fg(STEEL))
            } else if stage == Stage::Wary {
                ("wary", stage_look(stage).0)
            } else if tuned {
                ("tuned", fg(GREEN))
            } else {
                ("", Style::new())
            };
            let mut dmg_spans = bar(bar_w, f64::from(dmg) / f64::from(max_dmg), GREEN, false);
            dmg_spans.push(Span::styled(format!(" {dmg}"), pen.ink.plain));
            let key = char::from(b'a' + u8::try_from(i).unwrap_or(0));
            Row::new([
                Cell::from(format!("[{key}]")).style(pen.ink.accent),
                Cell::from(clip(script.as_ref().map_or("?", |sc| sc.name.as_str()), 16)),
                Cell::from(format!("cy{}", script.as_ref().map_or(1, |sc| sc.cycles))),
                Cell::from(Line::from(dmg_spans)),
                Cell::from(state).style(style),
            ])
        })
        .collect();
    let table = Table::new(
        rows_,
        [
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
            Constraint::Length(u16::try_from(bar_w + 3).unwrap_or(8)),
            Constraint::Length(7),
        ],
    )
    .header(Row::new(["", "script", "cost", "damage", "state"]).style(pen.ink.dim))
    .column_spacing(1);
    let table_h = u16::try_from(run.deck.len() + 1)
        .unwrap_or(7)
        .min(rect.height);
    pen.f
        .render_widget(table, Rect::new(rect.x, rect.y, rect.width, table_h));
    let extra = rect.y + table_h;
    if pen.size != Size::Small && extra + 1 < rect.y + rect.height {
        let lines = vec![
            Line::from(vec![
                Span::styled("[w] ", pen.ink.accent),
                Span::styled("brace: next strike at 3/4, costs a cycle", pen.ink.dim),
            ]),
            Line::from(vec![
                Span::styled("[x] ", pen.ink.accent),
                Span::styled("break off: costs trace, damage stays", pen.ink.dim),
            ]),
        ];
        pen.text(
            Rect::new(
                rect.x,
                extra + 1,
                rect.width,
                rect.height - (extra - rect.y) - 1,
            ),
            lines,
        );
    }
}

/// The encounter screen.
pub(super) fn draw(pen: &mut Pen<'_, '_>, s: &Session, a: &Active, area: Rect) {
    let Some(e) = a.run.encounter() else { return };
    let run = &a.run;
    match pen.size {
        Size::Small => {
            let [head, tr, body, logr, foot] = rows(
                area,
                [
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(9),
                    Constraint::Length(3),
                    Constraint::Length(1),
                ],
            );
            header(pen, s, a, head);
            pen.f.render_widget(
                Paragraph::new(super::trace_line(
                    run.trace,
                    usize::from(tr.width),
                    &pen.ink,
                )),
                tr,
            );
            let [left, right] = cols(body, [Constraint::Length(34), Constraint::Min(30)]);
            let li = pen.panel(left, "COUNTERMEASURE", VIOLET);
            let [m, t] = rows(li, [Constraint::Length(4), Constraint::Min(4)]);
            machine(pen, e, m, false);
            telegraph(pen, e, t);
            let ri = pen.panel(right, "YOUR DECK", GREEN);
            scripts(pen, s, a, e, ri);
            let lo = pen.ruled(logr, "FIGHT LOG", CYAN);
            log(pen, a, lo);
            pen.hint(foot, &[("a-f", "play"), ("w", "brace"), ("x", "break off")]);
        }
        Size::Medium | Size::Large => {
            let large = pen.size == Size::Large;
            let [head, tr, body, logr, foot] = rows(
                area,
                [
                    Constraint::Length(1),
                    Constraint::Length(2),
                    Constraint::Min(12),
                    Constraint::Length(if large { 10 } else { 7 }),
                    Constraint::Length(1),
                ],
            );
            header(pen, s, a, head);
            trace(pen, run, tr);
            let [left, right] = cols(
                body,
                [
                    Constraint::Percentage(if large { 46 } else { 50 }),
                    Constraint::Min(30),
                ],
            );
            let li = pen.panel(left, "COUNTERMEASURE", VIOLET);
            let [m, t] = rows(
                li,
                [
                    Constraint::Length(if large { 9 } else { 8 }),
                    Constraint::Min(5),
                ],
            );
            machine(pen, e, m, true);
            telegraph(pen, e, t);
            let ri = pen.panel(right, "YOUR DECK", GREEN);
            scripts(pen, s, a, e, ri);
            let lo = pen.panel(logr, "FIGHT LOG", CYAN);
            log(pen, a, lo);
            pen.hint(
                foot,
                &[("a-f", "play a script"), ("w", "brace"), ("x", "break off")],
            );
            pen.f.render_widget(
                Paragraph::new(Line::styled(
                    format!("{} integrity left", e.integrity),
                    pen.ink.dim,
                ))
                .alignment(Alignment::Right),
                foot,
            );
        }
    }
}
