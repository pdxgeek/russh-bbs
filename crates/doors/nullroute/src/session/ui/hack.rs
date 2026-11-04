//! The run screen (node map, trace, here panel, deck, scrolling log) and the debrief.

use russh_bbs_door::layout::{Alignment, Constraint, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Cell, Paragraph, Row, Table};

use super::map::MapView;
use super::{
    AMBER, CYAN, GREEN, Pen, RED, STEEL, Size, VIOLET, bold, clip, cols, fg, meter, rows,
    trace_line,
};
use crate::learn::Stage;
use crate::net::Patrol;
use crate::run::{Run, Status};
use crate::session::{Active, Report, Session};

/// How a log line reads: colour, and the one-character tag in front of it.
pub(super) fn log_style(text: &str) -> (Style, char) {
    let t = text.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&[
        "burned",
        "alarm",
        "thrown out",
        "trace complete",
        "locked.",
        "not enough",
        "patched",
        "is spent",
        "honeypot",
        "refused",
    ]) {
        (bold(RED), '!')
    } else if has(&[
        "trace +", "holds", "camera", "warden", "tracer", "sealed", "echo",
    ]) {
        (fg(AMBER), '~')
    } else if has(&[
        "opens",
        "objective done",
        "jack out clean",
        "goes down",
        "is down",
        "scrubbed",
        "planted",
        "never existed",
        "found a fragment",
        "copied",
        "unrolls",
        "quiet",
        "dark",
    ]) {
        (fg(GREEN), '+')
    } else if has(&["jacked in", "into ", "sweeping", "hidden link", "reads"]) {
        (fg(CYAN), '>')
    } else if has(&["dmg", " on ", "armor", "brace", "blind", "break off"]) {
        (fg(VIOLET), '*')
    } else {
        (Style::new(), '.')
    }
}

/// The scrolling run log: the newest line last, stamped with the cycles spent when it was
/// written.
pub(super) fn log(pen: &mut Pen<'_, '_>, a: &Active, rect: Rect) {
    let room = usize::from(rect.height);
    let width = usize::from(rect.width);
    let skip = a.feed.rows.len().saturating_sub(room);
    let last = a.feed.rows.len().saturating_sub(1);
    let lines: Vec<Line<'static>> = a
        .feed
        .rows
        .iter()
        .enumerate()
        .skip(skip)
        .map(|(i, (t, text))| {
            let (style, tag) = log_style(text);
            // The newest line is bright, the older ones fade.
            let style = if i == last {
                style
            } else {
                style.add_modifier(russh_bbs_door::style::Modifier::DIM)
            };
            Line::from(vec![
                Span::styled(format!("T+{t:02} "), fg(STEEL)),
                Span::styled(format!("{tag} "), style),
                Span::styled(clip(text, width.saturating_sub(7)), style),
            ])
        })
        .collect();
    pen.text(rect, lines);
}

/// The flags of a run as chips: the things the run is under.
pub(super) fn flags(run: &Run) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut chip = |text: String, style: Style| {
        spans.push(Span::styled(text, style));
        spans.push(Span::raw(" "));
    };
    if run.alarm() {
        chip("[ALARM]".into(), bold(RED));
    }
    if run.tracer() {
        chip("[TRACER]".into(), bold(AMBER));
    }
    if run.feed_live() {
        chip("[CAMERA]".into(), fg(AMBER));
    }
    if let Some(w) = run.warden().filter(|w| w.awake && !w.down) {
        chip(format!("[WARDEN T-{}]", w.arrives_in), bold(RED));
    }
    if run.buffer() > 0 {
        chip(format!("soak {}", run.buffer()), fg(CYAN));
    }
    if spans.is_empty() {
        spans.push(Span::styled("quiet", fg(GREEN)));
    }
    spans
}

/// The top row: where you are jacked in and what the job is.
pub(super) fn header(pen: &mut Pen<'_, '_>, s: &Session, a: &Active, rect: Rect) {
    let run = &a.run;
    let net = run.net();
    let (target, effect) = run.objective_target();
    let mut left = vec![
        Span::styled(" NULLROUTE ", bold(CYAN)),
        Span::styled(
            format!(
                "// {} T{} // {effect:?} @ {target}",
                s.corp_name(net.corp),
                net.tier
            ),
            pen.ink.plain,
        ),
    ];
    if run.objective_done() {
        left.push(Span::styled("  OBJECTIVE DONE: jack out", bold(GREEN)));
    }
    pen.f.render_widget(Paragraph::new(Line::from(left)), rect);
    let right = Line::from(vec![
        Span::styled("CY ", pen.ink.dim),
        Span::styled(format!("{} ", run.cycles), bold(AMBER)),
    ])
    .alignment(Alignment::Right);
    pen.f.render_widget(Paragraph::new(right), rect);
}

/// The trace bar and, when there is a row for it, the flags.
pub(super) fn trace(pen: &mut Pen<'_, '_>, run: &Run, rect: Rect) {
    let top = Rect::new(rect.x, rect.y, rect.width, 1);
    pen.f.render_widget(
        Paragraph::new(trace_line(run.trace, usize::from(rect.width), &pen.ink)),
        top,
    );
    if rect.height >= 2 {
        let low = Rect::new(rect.x, rect.y + 1, rect.width, 1);
        pen.f
            .render_widget(Paragraph::new(Line::from(flags(run))), low);
    }
}

/// The script slots as chips on one row (the small layout).
fn chips(pen: &mut Pen<'_, '_>, run: &Run, c: &crate::content::Content, rect: Rect) {
    let n = run.deck.len().max(1);
    let each = usize::from(rect.width) / n;
    let mut spans = Vec::new();
    for (i, slot) in run.deck.iter().enumerate() {
        let script = run.script(c, slot.script);
        let (style, mark) = stage_look(run.stage(c, i));
        let name = script.as_ref().map_or("?", |s| s.name.as_str());
        let power = script.as_ref().map_or(0, |s| s.power);
        let key = char::from(b'a' + u8::try_from(i).unwrap_or(0));
        let text = format!(
            "[{key}]{} p{power}{mark}",
            clip(name, each.saturating_sub(7).max(3))
        );
        spans.push(Span::styled(
            format!("{text:<each$}"),
            if slot.left == 0 { fg(STEEL) } else { style },
        ));
    }
    pen.f.render_widget(Paragraph::new(Line::from(spans)), rect);
}

/// The colour and the mark of a script's degradation state.
pub(super) fn stage_look(stage: Stage) -> (Style, &'static str) {
    match stage {
        Stage::Fresh => (fg(GREEN), ""),
        Stage::Wary => (fg(AMBER), "~"),
        Stage::Patched => (bold(RED), "!"),
    }
}

/// The deck as a table: key, script, power, charges, state.
fn deck_table(pen: &mut Pen<'_, '_>, run: &Run, c: &crate::content::Content, rect: Rect) {
    let rows: Vec<Row<'static>> = run
        .deck
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let script = run.script(c, slot.script);
            let stage = run.stage(c, i);
            let (style, _) = stage_look(stage);
            let word = match stage {
                Stage::Fresh => "fresh",
                Stage::Wary => "wary",
                Stage::Patched => "PATCHED",
            };
            let left = if slot.left == u32::MAX {
                "inf".to_owned()
            } else {
                format!("x{}", slot.left)
            };
            let key = char::from(b'a' + u8::try_from(i).unwrap_or(0));
            Row::new([
                Cell::from(format!("[{key}]")).style(pen.ink.accent),
                Cell::from(script.as_ref().map_or("?".to_owned(), |s| s.name.clone())),
                Cell::from(format!("p{}", script.as_ref().map_or(0, |s| s.power))),
                Cell::from(left),
                Cell::from(word).style(style),
            ])
            .style(if slot.left == 0 {
                fg(STEEL)
            } else {
                Style::new()
            })
        })
        .collect();
    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Length(7),
        ],
    )
    .header(Row::new(["", "script", "pow", "left", "state"]).style(pen.ink.dim))
    .column_spacing(1);
    pen.f.render_widget(table, rect);
}

/// The node you stand on and the ways out, numbered as the keys are.
fn here(pen: &mut Pen<'_, '_>, s: &Session, a: &Active, rect: Rect) {
    let run = &a.run;
    let c = &*s.content;
    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Some(v) = run.view(c, run.at) {
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", v.name), bold(CYAN)),
            Span::styled(format!("{} d{}", v.kind, v.depth), pen.ink.dim),
        ]));
    }
    let exits = run.exits();
    if exits.is_empty() {
        lines.push(Line::styled(
            "no known exits: scan for hidden links",
            pen.ink.dim,
        ));
    }
    for (i, id) in exits.iter().enumerate().take(9) {
        let Some(v) = run.view(c, *id) else { continue };
        let (state, style) = if v.honeypot {
            ("HONEYPOT".to_owned(), bold(RED))
        } else if v.patrol == Patrol::BlackWall && !v.wall_breached {
            ("black wall".to_owned(), bold(RED))
        } else if let Some((lock, level)) = v.lock {
            (format!("{} lock {level}", lock.word()), fg(AMBER))
        } else if v.open {
            ("open".to_owned(), fg(GREEN))
        } else {
            ("locked?".to_owned(), fg(AMBER))
        };
        let ice = match v.patrol {
            Patrol::Sentry if !v.patrol_off => " sentry",
            Patrol::Tracer if !v.patrol_off => " tracer",
            _ => "",
        };
        let width = usize::from(rect.width);
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", i + 1), bold(AMBER)),
            Span::styled(format!("{:<6}", v.name), pen.ink.plain),
            Span::styled(
                clip(&format!("{state}{ice}"), width.saturating_sub(9)),
                style,
            ),
        ]));
    }
    pen.text(rect, lines);
}

/// What the run knows about the place: the corporation, the warden, the loot.
fn intel(pen: &mut Pen<'_, '_>, s: &Session, a: &Active, rect: Rect) {
    let run = &a.run;
    let net = run.net();
    let mut lines = Vec::new();
    if let Some(corp) = s.content.corp(net.corp) {
        lines.push(Line::styled(corp.style.clone(), pen.ink.dim));
    }
    lines.push(Line::from(vec![
        Span::styled("known ", pen.ink.dim),
        Span::styled(run.known.count_ones().to_string(), pen.ink.plain),
        Span::styled(" scanned ", pen.ink.dim),
        Span::styled(run.scanned.count_ones().to_string(), pen.ink.plain),
        Span::styled(" of ", pen.ink.dim),
        Span::styled(net.nodes.len().to_string(), pen.ink.plain),
    ]));
    match run.warden() {
        Some(w) if w.down => lines.push(Line::styled("warden down", fg(GREEN))),
        Some(w) if w.awake => lines.push(Line::styled(
            format!("WARDEN HUNTING: {} moves", w.arrives_in),
            bold(RED),
        )),
        Some(_) => lines.push(Line::styled("a warden sleeps here", fg(AMBER))),
        None => {}
    }
    pen.wrapped(rect, lines);
}

/// The run screen.
pub(super) fn draw(pen: &mut Pen<'_, '_>, s: &Session, a: &Active, area: Rect) {
    let c = &*s.content;
    let run = &a.run;
    let hint: &[(&str, &str)] = if a.aim.is_some() {
        &[("1-9", "aim at node"), ("Esc", "cancel")]
    } else {
        &[("1-9", "move"), ("a-f", "script"), ("o", "jack out")]
    };
    let aim_note = a.aim.and_then(|slot| {
        run.deck
            .get(slot)
            .and_then(|d| run.script(c, d.script))
            .map(|sc| format!("Aim {}: pick the node (1-9), Esc cancels", sc.name))
    });
    let pulse = u32::try_from(a.feed.rows.len()).unwrap_or(0) + run.trace;
    let map = |pen: &mut Pen<'_, '_>, rect: Rect, compact: bool| {
        let inner = pen.panel(rect, "NODE MAP", CYAN);
        let ascii = pen.ascii;
        pen.f.render_widget(
            MapView {
                run,
                content: c,
                ascii,
                pulse,
                compact,
            },
            inner,
        );
    };
    match pen.size {
        Size::Small => {
            let [head, tr, mid, logr, deck, foot] = rows(
                area,
                [
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(8),
                    Constraint::Length(4),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ],
            );
            header(pen, s, a, head);
            pen.f.render_widget(
                Paragraph::new(trace_line(run.trace, usize::from(tr.width), &pen.ink)),
                tr,
            );
            let [mapr, herer] = cols(mid, [Constraint::Min(30), Constraint::Length(27)]);
            map(pen, mapr, true);
            let hi = pen.panel(herer, "HERE", CYAN);
            here(pen, s, a, hi);
            let li = pen.ruled(logr, "LOG", CYAN);
            log(pen, a, li);
            chips(pen, run, c, deck);
            small_foot(pen, foot, hint, aim_note.as_deref(), run);
        }
        Size::Medium | Size::Large => {
            let large = pen.size == Size::Large;
            let bottom = if large { 12 } else { 9 };
            let [head, tr, mid, low, foot] = rows(
                area,
                [
                    Constraint::Length(1),
                    Constraint::Length(2),
                    Constraint::Min(10),
                    Constraint::Length(bottom),
                    Constraint::Length(1),
                ],
            );
            header(pen, s, a, head);
            trace(pen, run, tr);
            let right_w = if large { 50 } else { 38 };
            let [mapr, right] = cols(mid, [Constraint::Min(46), Constraint::Length(right_w)]);
            map(pen, mapr, !large);
            if large {
                let [herer, intelr] = rows(right, [Constraint::Min(8), Constraint::Length(6)]);
                let hi = pen.panel(herer, "HERE", CYAN);
                here(pen, s, a, hi);
                let ii = pen.panel(intelr, "INTEL", VIOLET);
                intel(pen, s, a, ii);
            } else {
                let hi = pen.panel(right, "HERE", CYAN);
                here(pen, s, a, hi);
            }
            let [logr, deckr] = cols(low, [Constraint::Min(40), Constraint::Length(right_w)]);
            let li = pen.panel(logr, "RUN LOG", CYAN);
            log(pen, a, li);
            let di = pen.panel(deckr, "DECK", GREEN);
            deck_table(pen, run, c, di);
            let mut keys = hint.to_vec();
            if run.objective_done() {
                keys.push(("!", "objective done"));
            }
            pen.hint(foot, &keys);
            if let Some(note) = aim_note {
                pen.f.render_widget(
                    Paragraph::new(Line::styled(note, bold(AMBER))).alignment(Alignment::Right),
                    foot,
                );
            }
        }
    }
}

/// The last row of the small layout: the hints, or the aim prompt.
fn small_foot(
    pen: &mut Pen<'_, '_>,
    rect: Rect,
    hint: &[(&str, &str)],
    aim: Option<&str>,
    run: &Run,
) {
    if let Some(note) = aim {
        pen.f.render_widget(
            Paragraph::new(Line::styled(note.to_owned(), bold(AMBER))),
            rect,
        );
    } else {
        pen.hint(rect, hint);
        let flags_line = Line::from(flags(run)).alignment(Alignment::Right);
        pen.f.render_widget(Paragraph::new(flags_line), rect);
    }
}

/// The debrief: how it ended, the numbers, the log's tail.
pub(super) fn debrief(pen: &mut Pen<'_, '_>, s: &Session, report: &Report, area: Rect) {
    let r = &report.result;
    let (word, tone) = match (r.status, r.objective) {
        (Status::Burned, _) => ("B U R N E D", RED),
        (_, true) => ("E X T R A C T E D", GREEN),
        _ => ("J A C K E D   O U T", AMBER),
    };
    let [banner, body, foot] = rows(
        area,
        [
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(1),
        ],
    );
    let bi = pen.panel(banner, "DEBRIEF", tone);
    pen.f.render_widget(
        Paragraph::new(Line::styled(word, bold(tone))).alignment(Alignment::Center),
        bi,
    );
    let wide = area.width >= 90;
    let [left, right] = if wide {
        cols(body, [Constraint::Length(40), Constraint::Min(30)])
    } else {
        cols(body, [Constraint::Length(30), Constraint::Min(20)])
    };
    let li = pen.panel(left, "THE RUN", tone);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let w = usize::from(li.width).saturating_sub(12);
    lines.push(meter(
        "peak trace",
        10,
        r.peak,
        100,
        w.min(20),
        if r.peak >= 70 { RED } else { AMBER },
        &r.peak.to_string(),
        &pen.ink,
    ));
    let mut row = |k: &str, v: String| {
        lines.push(Line::from(vec![
            Span::styled(format!("{k:<13}"), pen.ink.dim),
            Span::styled(v, pen.ink.plain),
        ]));
    };
    row("job pays", report.contract.reward.to_string());
    row("data copied", r.loot.to_string());
    row("fights", format!("{} ({} put down)", r.fights, r.defeated));
    row("locks broken", r.broken.len().to_string());
    row("fragments", r.fragments.len().to_string());
    row("cycles left", r.cycles_left.to_string());
    if r.planted {
        row("backdoor", "planted".into());
    }
    if r.warden_down {
        row("warden", "put down".into());
    } else if r.warden_woke {
        row("warden", "woke".into());
    }
    pen.text(li, lines);
    let ri = pen.panel(right, "WHAT HAPPENED", CYAN);
    let width = usize::from(ri.width);
    let notes: Vec<Line<'static>> = report
        .notes
        .iter()
        .flat_map(|n| wrap(n, width.saturating_sub(4)))
        .map(|n| {
            let (style, tag) = log_style(&n);
            if n.is_empty() {
                Line::raw("")
            } else {
                Line::from(vec![
                    Span::styled(format!("{tag} "), style),
                    Span::styled(n, style),
                ])
            }
        })
        .collect();
    let skip = notes.len().saturating_sub(usize::from(ri.height));
    pen.text(ri, notes.into_iter().skip(skip).collect());
    pen.hint(foot, &[("Enter", "continue")]);
    let _ = s;
}

/// Break `text` at spaces into lines of at most `width` characters.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    out.push(line);
    out
}
