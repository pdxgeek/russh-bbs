//! The scheme planner: the lists you pick a target, method, twists, getaway, helpers and time
//! from, each with a panel beside it that says what the highlighted choice means, and the
//! planner itself, a dossier with the odds of each phase and the reasons behind them.
//!
//! Every number is the engine's (`Game::chance`, `Game::readiness`, `Game::trace_chance`); the
//! screen picks the words and the bars.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Wrap};

use super::{Screen, Size, Ui, fit, hue, wrap};
use crate::game::{Action, Phase};
use crate::session::{Cmd, Mode};

/// A signed number with its sign, for the reasons list.
fn signed(v: i32) -> String {
    format!("{v:+}")
}

/// A percentage bar for a chance: red under 40, brass under 65, moss above.
fn odds_style(ui: &Ui<'_>, pct: i32) -> Style {
    if pct < 40 {
        ui.pal.red()
    } else if pct < 65 {
        ui.pal.brass()
    } else {
        ui.pal.moss()
    }
}

/// A number as a small bar `n` out of `max`, for the cost lines of a method.
fn meter(ui: &Ui<'_>, label: &str, n: i32, max: i32, style: Style, width: u16) -> Line<'static> {
    ui.gauge(
        label,
        u32::try_from(n.max(0)).unwrap_or(0),
        u32::try_from(max.max(1)).unwrap_or(1),
        width,
        style,
        &format!("{n}"),
    )
}

/// The detail panel text for the item the lightbar is on.
fn detail_lines(ui: &Ui<'_>, width: u16) -> Vec<Line<'static>> {
    let g = ui.g;
    let pal = &ui.pal;
    let w = usize::from(width);
    let mut out: Vec<Line<'static>> = Vec::new();
    let Some(item) = ui.current() else {
        return out;
    };
    let text = |out: &mut Vec<Line<'static>>, s: &str, style: Style| {
        for l in wrap(s, w) {
            out.push(Line::styled(l, style));
        }
    };
    match (&item.cmd, ui.screen) {
        (Cmd::To(Mode::Scheme(id)), Screen::Plans) => {
            if let Some(s) = g.player.scheme(*id) {
                let r = g.readiness(s);
                if let Some(m) = g.content.method(&s.method) {
                    out.push(Line::styled(m.name.clone(), pal.chalk()));
                    text(&mut out, &m.blurb, pal.chalk_soft());
                }
                out.push(Line::from(vec![
                    Span::styled("scouting ", pal.slate()),
                    Span::styled(
                        format!("{}/{}", r.scout_have, r.scout_need),
                        pal.chalk_soft(),
                    ),
                ]));
                out.push(if r.ready() {
                    Line::styled("READY: press Enter to see it through", pal.moss())
                } else {
                    Line::styled("still being set up", pal.brass())
                });
            }
        }
        (Cmd::To(Mode::DraftTarget), _) => {
            text(
                &mut out,
                "Pick who or what it is aimed at, then how. A target is always chosen from a list.",
                pal.chalk_soft(),
            );
        }
        (Cmd::To(Mode::DraftMethod(t)), _) => {
            if let Some(target) = g.content.target(t) {
                out.push(Line::styled(target.name.clone(), pal.chalk()));
                out.push(Line::styled(format!("a {}", target.kind), pal.ink()));
                if let Some(npc) = target.npc.as_deref().and_then(|n| g.content.npc(n)) {
                    text(&mut out, &npc.blurb, pal.chalk_soft());
                    let wary = g.player.wary_of(&npc.id);
                    let mut spans = vec![Span::styled("wary of you ", pal.slate())];
                    spans.extend(ui.pips(usize::try_from(wary).unwrap_or(0), 3, pal.red()));
                    out.push(Line::from(spans));
                }
                if let Some(place) = target.place.as_deref().and_then(|p| g.content.place(p)) {
                    out.push(Line::styled(
                        format!("found at the {}", place.name),
                        pal.slate(),
                    ));
                }
            }
        }
        (Cmd::Do(Action::Draft { method, .. }), _) => {
            if let Some(m) = g.content.method(method) {
                out.push(Line::styled(m.name.clone(), pal.chalk()));
                text(&mut out, &m.blurb, pal.chalk_soft());
                out.push(meter(ui, "hard", m.difficulty, 40, pal.red(), width));
                out.push(meter(ui, "legend", m.legend, 40, pal.lamp(), width));
                out.push(meter(ui, "heat", m.heat, 30, pal.red(), width));
                if m.big {
                    out.push(Line::from(vec![
                        Span::styled(" BIG ", pal.stamp()),
                        Span::styled(" it changes the school for everyone", pal.red()),
                    ]));
                }
                if !m.needs.is_empty() {
                    let names: Vec<String> = m
                        .needs
                        .iter()
                        .map(|i| g.content.item(i).map_or(i.clone(), |x| x.name.clone()))
                        .collect();
                    out.push(Line::styled(
                        format!("needs {}", names.join(", ")),
                        pal.brass(),
                    ));
                }
                if !m.roles.is_empty() {
                    out.push(Line::styled(
                        format!("helpers: {}", m.roles.join(", ")),
                        pal.ink(),
                    ));
                }
            }
        }
        (Cmd::Do(Action::AddTwist(_, id) | Action::RemoveTwist(_, id)), _) => {
            if let Some(t) = g.content.twists.iter().find(|t| t.id == *id) {
                out.push(Line::styled(t.name.clone(), pal.chalk()));
                text(&mut out, &t.blurb, pal.chalk_soft());
                out.push(meter(ui, "hard", t.difficulty, 40, pal.red(), width));
                out.push(Line::from(vec![
                    Span::styled("legend ", pal.slate()),
                    Span::styled(format!("{}%", signed(t.legend_pct)), pal.lamp()),
                    Span::styled("  heat ", pal.slate()),
                    Span::styled(signed(t.heat), pal.red()),
                ]));
            }
        }
        (Cmd::Do(Action::SetGetaway(_, id)), _) => {
            if let Some(a) = g.content.getaway(id) {
                out.push(Line::styled(a.name.clone(), pal.chalk()));
                text(&mut out, &a.blurb, pal.chalk_soft());
                out.push(Line::from(vec![
                    Span::styled("escape ", pal.slate()),
                    Span::styled(signed(a.escape), pal.moss()),
                    Span::styled("  traced ", pal.slate()),
                    Span::styled(signed(a.suspicion), pal.red()),
                ]));
                if a.lies_low {
                    out.push(Line::styled("lies low: the heat cools faster", pal.ink()));
                }
            }
        }
        (Cmd::Do(Action::Recruit { npc, role, .. }), _) => {
            if let Some(n) = g.content.npc(npc) {
                out.push(Line::styled(n.name.clone(), pal.chalk()));
                out.push(Line::styled(format!("as {role}"), pal.ink()));
                text(&mut out, &n.blurb, pal.chalk_soft());
                let bond = g.bond(npc);
                out.push(bond_line(ui, bond, width));
            }
        }
        (Cmd::Do(Action::Schedule { scheme, day, slot }), _) => {
            if let Some(s) = g.player.scheme(*scheme) {
                let place = g.scheme_place(s).unwrap_or_default();
                let slot = usize::from(*slot);
                out.push(Line::styled(
                    format!(
                        "{} - {}",
                        g.content.periods.get(slot).map_or("?", |p| p.name.as_str()),
                        g.activity(*day, slot)
                    ),
                    pal.chalk(),
                ));
                let open = g.place_open(&place, *day, slot);
                out.push(if open {
                    Line::styled("the place is open", pal.moss())
                } else {
                    Line::styled("the place is shut then", pal.red())
                });
                if s.scout == 0 {
                    out.push(Line::styled(
                        "scout it to learn who will be about",
                        pal.slate(),
                    ));
                } else {
                    let n = g.witnesses_at(&place, *day, slot);
                    out.push(Line::styled(
                        format!("{n} staff or prefects about"),
                        if n == 0 { pal.moss() } else { pal.red() },
                    ));
                }
            }
        }
        (Cmd::Do(Action::Talk(id)), _) => {
            if let Some(t) = g.content.talks.iter().find(|t| t.id == *id)
                && let Some(n) = g.content.npc(&t.npc)
            {
                out.push(Line::styled(n.name.clone(), pal.chalk()));
                out.push(Line::styled(n.title.clone(), pal.ink()));
                text(&mut out, &n.blurb, pal.chalk_soft());
                out.push(bond_line(ui, g.bond(&n.id), width));
            }
        }
        _ => {
            for l in &ui.body {
                text(&mut out, l, pal.chalk_soft());
            }
        }
    }
    out
}

/// A bond from -100 to 100 as a line with the friend and rival thresholds marked.
pub(super) fn bond_line(ui: &Ui<'_>, bond: i32, width: u16) -> Line<'static> {
    let rules = &ui.g.content.rules;
    let word = if bond >= rules.friend_bond {
        "friend"
    } else if bond <= rules.rival_bond {
        "rival"
    } else {
        "known"
    };
    let tail = if usize::from(width) >= 36 {
        format!(" {bond:+} {word}")
    } else {
        format!(" {bond:+}")
    };
    meter_line(ui, "bond", 7, bond, tail, width, true)
}

/// A value from -100 to 100 on a track with its middle marked and the marker where the value
/// is. With `marks`, the friend and rival thresholds are marked too and the marker takes their
/// colours.
pub(super) fn meter_line(
    ui: &Ui<'_>,
    label: &str,
    label_w: usize,
    value: i32,
    tail: String,
    width: u16,
    marks: bool,
) -> Line<'static> {
    let pal = &ui.pal;
    let rules = &ui.g.content.rules;
    let room = usize::from(width)
        .saturating_sub(label_w + tail.chars().count() + 1)
        .max(9);
    // The track is split in two halves; the marker sits where the value is.
    let half = i32::try_from(room / 2).unwrap_or(1);
    let at = |b: i32| usize::try_from(half + (b.clamp(-100, 100) * half) / 100).unwrap_or(0);
    let (pos, friend_at, rival_at) = (at(value), at(rules.friend_bond), at(rules.rival_bond));
    let mid = usize::try_from(half).unwrap_or(0);
    let mut spans = vec![Span::styled(
        format!("{:<label_w$}", fit(label, label_w)),
        pal.slate(),
    )];
    for i in 0..=room {
        let (sym, style) = if i == pos {
            (
                ui.look.g("◆", "O"),
                if marks && value >= rules.friend_bond {
                    pal.moss()
                } else if marks && value <= rules.rival_bond {
                    pal.red()
                } else {
                    pal.lamp()
                },
            )
        } else if marks && i == friend_at && i != mid {
            (ui.look.g("╵", "'"), pal.moss())
        } else if marks && i == rival_at && i != mid {
            (ui.look.g("╵", "'"), pal.red())
        } else if i == mid {
            (ui.look.g("┼", "+"), pal.slate())
        } else {
            (ui.look.g("─", "-"), pal.slate())
        };
        spans.push(Span::styled(sym, style));
    }
    spans.push(Span::styled(tail, pal.chalk_soft()));
    Line::from(spans)
}

/// A list screen: the orders on the left, what the highlighted one means on the right.
pub(super) fn list(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let (title, side) = match ui.screen {
        Screen::Plans => ("Your schemes", "The scheme"),
        Screen::DraftTarget => ("Aimed at", "The target"),
        Screen::DraftMethod => ("What will you do?", "The trick"),
        Screen::Twists => ("Twists", "The twist"),
        Screen::Getaway => ("Getaway", "The getaway"),
        Screen::Recruit => ("Who will help?", "The helper"),
        Screen::Schedule => ("When?", "The time"),
        Screen::Talk => ("Who do you want a word with?", "The person"),
        _ => ("", ""),
    };
    let [left, right] = if area.width >= 100 {
        Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)]).areas(area)
    } else {
        Layout::horizontal([Constraint::Percentage(54), Constraint::Percentage(46)]).areas(area)
    };
    ui.orders(f, left, title);
    let inner = ui.framed(f, right, side);
    let mut lines: Vec<Line<'static>> = Vec::new();
    // The screen's own sentence first, on the roomier screens.
    if ui.look.size >= Size::Roomy
        && let Some(first) = ui.body.first()
    {
        for l in wrap(first, usize::from(inner.width)) {
            lines.push(Line::styled(l, ui.pal.slate()));
        }
        lines.push(Line::default());
    }
    lines.extend(detail_lines(ui, inner.width));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// One row of the dossier: a caption, its words, and a tick or a cross.
fn form_row(
    ui: &Ui<'_>,
    caption: &str,
    value: Vec<Span<'static>>,
    ok: Option<bool>,
) -> Line<'static> {
    let pal = &ui.pal;
    let mut spans = vec![Span::styled(format!("{caption:<8}"), pal.slate())];
    spans.extend(value);
    match ok {
        Some(true) => spans.push(Span::styled(
            format!(" {}", ui.look.g("✓", "+")),
            pal.moss(),
        )),
        Some(false) => spans.push(Span::styled(format!(" {}", ui.look.g("✗", "x")), pal.red())),
        None => {}
    }
    Line::from(spans)
}

/// The planner: the dossier of one scheme and the odds.
pub(super) fn planner(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let Mode::Scheme(id) = ui.s.mode else {
        return;
    };
    let (Some(s), true) = (g.player.scheme(id), true) else {
        return;
    };
    let (Some(m), Some(t)) = (g.content.method(&s.method), g.content.target(&s.target)) else {
        return;
    };
    let r = g.readiness(s);
    let chip_rows = {
        let per_row = usize::from(area.width / 12).max(1);
        u16::try_from(ui.items.len().div_ceil(per_row))
            .unwrap_or(2)
            .min(3)
    };
    let [top, menu] =
        Layout::vertical([Constraint::Min(6), Constraint::Length(chip_rows)]).areas(area);
    let [left, right] = if top.width >= 100 {
        Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)]).areas(top)
    } else {
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(top)
    };
    // ---- the dossier
    let title = format!("Scheme #{id}");
    let inner = ui.framed(f, left, &title);
    let w = usize::from(inner.width).saturating_sub(10);
    let place = g.scheme_place(s).and_then(|p| g.content.place(&p).cloned());
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(form_row(
        ui,
        "target",
        vec![Span::styled(fit(&t.name, w), pal.chalk())],
        None,
    ));
    lines.push(form_row(
        ui,
        "method",
        vec![Span::styled(fit(&m.name, w), pal.chalk())],
        None,
    ));
    if let Some(p) = &place {
        let open = p.lock.is_none() || p.unlock.iter().any(|c| g.holds(c));
        lines.push(form_row(
            ui,
            "where",
            vec![Span::styled(fit(&p.name, w), pal.chalk_soft())],
            Some(open),
        ));
    }
    let twists: Vec<String> = s
        .twists
        .iter()
        .filter_map(|x| g.content.twist(x))
        .map(|x| x.name.clone())
        .collect();
    lines.push(form_row(
        ui,
        "twists",
        vec![Span::styled(
            fit(
                &if twists.is_empty() {
                    "none".to_owned()
                } else {
                    twists.join(", ")
                },
                w,
            ),
            pal.chalk_soft(),
        )],
        None,
    ));
    lines.push(form_row(
        ui,
        "getaway",
        vec![Span::styled(
            fit(
                g.content
                    .getaway(&s.getaway)
                    .map_or("?", |x| x.name.as_str()),
                w,
            ),
            pal.chalk_soft(),
        )],
        None,
    ));
    if !m.needs.is_empty() {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, item) in m.needs.iter().enumerate() {
            let have = g.player.count(item) > 0;
            if i > 0 {
                spans.push(Span::styled(", ", pal.slate()));
            }
            spans.push(Span::styled(
                g.content
                    .item(item)
                    .map_or(item.clone(), |x| x.name.clone()),
                if have { pal.moss() } else { pal.red() },
            ));
        }
        lines.push(form_row(
            ui,
            "items",
            spans,
            Some(r.missing_items.is_empty()),
        ));
    }
    let roles = g.needed_roles(s);
    if !roles.is_empty() || !s.allies.is_empty() {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, role) in roles.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(", ", pal.slate()));
            }
            match s.allies.iter().find(|(_, r)| r == role) {
                Some((n, _)) => spans.push(Span::styled(
                    format!(
                        "{role}: {}",
                        g.content.npc(n).map_or("?", |x| x
                            .name
                            .split_whitespace()
                            .last()
                            .unwrap_or("?"))
                    ),
                    pal.moss(),
                )),
                None => spans.push(Span::styled(format!("{role}: ?"), pal.red())),
            }
        }
        if roles.is_empty() {
            spans.push(Span::styled(
                format!("{} helping", s.allies.len()),
                pal.chalk_soft(),
            ));
        }
        lines.push(form_row(
            ui,
            "allies",
            spans,
            Some(r.missing_roles.is_empty()),
        ));
    }
    let when = s.when.map_or("not set".to_owned(), |(d, sl)| {
        format!(
            "{} day {}, {}",
            ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][crate::game::Game::weekday(d)],
            d - g.world.term_start + 1,
            g.content
                .periods
                .get(usize::from(sl))
                .map_or("?", |p| p.name.as_str())
        )
    });
    lines.push(form_row(
        ui,
        "when",
        vec![Span::styled(fit(&when, w), pal.chalk_soft())],
        Some(r.scheduled),
    ));
    let mut scout = ui.pips(
        usize::from(r.scout_have),
        usize::from(r.scout_need.max(r.scout_have)),
        pal.ink(),
    );
    scout.push(Span::styled(
        format!(" {}/{}", r.scout_have, r.scout_need),
        pal.chalk_soft(),
    ));
    lines.push(form_row(
        ui,
        "scout",
        scout,
        Some(r.scout_have >= r.scout_need),
    ));
    f.render_widget(Paragraph::new(lines), inner);

    // ---- the odds
    let inner = ui.framed(f, right, "The odds");
    let phases = [Phase::Sneak, Phase::Act, Phase::Escape];
    let shown = ui.s.tab % phases.len();
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, phase) in phases.iter().enumerate() {
        if let Some(c) = g.chance(s, *phase, 0, 0) {
            let mark = if i == shown {
                ui.look.g("▸", ">")
            } else {
                " "
            };
            let pct = u32::try_from(c.total).unwrap_or(0);
            let mut spans = vec![Span::styled(
                format!("{mark}{:<7}", phase.name()),
                if i == shown { pal.lamp() } else { pal.slate() },
            )];
            let bar_w = inner.width.saturating_sub(8 + 5).max(4);
            spans.extend(ui.bar(pct, 100, bar_w, odds_style(ui, c.total)));
            spans.push(Span::styled(format!(" {:>3}%", c.total), pal.chalk()));
            lines.push(Line::from(spans));
        }
    }
    lines.push(Line::from(vec![
        Span::styled(" traced  ", pal.slate()),
        Span::styled(format!("{}%", g.trace_chance(s, 0)), pal.red()),
        Span::styled(" if it goes clean", pal.slate()),
    ]));
    // The reasons behind the phase shown.
    let phase = phases[shown];
    if let Some(c) = g.chance(s, phase, 0, 0) {
        lines.push(Line::styled(
            format!("why {} is {}%:", phase.name(), c.total),
            pal.ink(),
        ));
        let room = usize::from(inner.height).saturating_sub(lines.len() + 2);
        let parts: Vec<(&String, i32)> = c
            .parts
            .iter()
            .filter(|(_, v)| *v != 0)
            .map(|(n, v)| (n, *v))
            .collect();
        let cols = if inner.width >= 44 { 2 } else { 1 };
        let colw = usize::from(inner.width) / cols;
        for chunk in parts.chunks(cols).take(room) {
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (n, v) in chunk {
                let style = if *v > 0 { pal.moss() } else { pal.red() };
                let text = format!(" {:>3} {}", signed(*v), fit(n, colw.saturating_sub(6)));
                let pad = colw.saturating_sub(text.chars().count());
                spans.push(Span::styled(text, style));
                spans.push(Span::raw(" ".repeat(pad)));
            }
            lines.push(Line::from(spans));
        }
    }
    // The verdict.
    let verdict = if r.ready() {
        Line::from(vec![
            Span::styled(" READY ", pal.band()),
            Span::styled(" go when the time comes", pal.moss()),
        ])
    } else {
        let mut why: Vec<String> = Vec::new();
        if !r.missing_items.is_empty() {
            why.push("items".into());
        }
        if !r.missing_roles.is_empty() {
            why.push("helpers".into());
        }
        if r.scout_have < r.scout_need {
            why.push("scouting".into());
        }
        if !r.scheduled {
            why.push("a time".into());
        }
        Line::from(vec![
            Span::styled(" NOT READY ", pal.stamp()),
            Span::styled(format!(" needs {}", why.join(", ")), pal.red()),
        ])
    };
    let y = inner.bottom().saturating_sub(1);
    let body_h = y.saturating_sub(inner.y);
    f.render_widget(
        Paragraph::new(lines),
        Rect::new(inner.x, inner.y, inner.width, body_h),
    );
    f.render_widget(
        Paragraph::new(verdict),
        Rect::new(inner.x, y, inner.width, 1),
    );
    ui.chips(f, menu, false);
    let _ = hue::INK;
}
