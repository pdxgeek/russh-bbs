//! The scheme run: sneak, act and escape as a tense sequence. Three stages along the top, the
//! nerve you have to spend, the odds of each way to spend it, and a log of what has happened.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Block, BorderType, Paragraph, Wrap};

use super::{Size, Ui, fit, wrap};
use crate::game::Phase;

const PHASES: [Phase; 3] = [Phase::Sneak, Phase::Act, Phase::Escape];

/// The three stages.
fn track(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, now: usize, flubs: u8) {
    let pal = &ui.pal;
    let g = ui.g;
    let Some(run) = g.player.run.as_ref() else {
        return;
    };
    let Some(scheme) = g.player.scheme(run.scheme) else {
        return;
    };
    let cells = Layout::horizontal([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .split(area);
    for (i, phase) in PHASES.iter().enumerate() {
        let (border, state) = match i.cmp(&now) {
            std::cmp::Ordering::Less => (pal.moss(), "clear"),
            std::cmp::Ordering::Equal => (pal.lamp(), "NOW"),
            std::cmp::Ordering::Greater => (pal.slate(), "ahead"),
        };
        let block = Block::bordered()
            .border_type(if ui.look.ascii {
                BorderType::Plain
            } else if i == now {
                BorderType::Double
            } else {
                BorderType::Rounded
            })
            .border_style(border)
            .title(Line::from(vec![
                Span::styled(format!(" {} ", i + 1), pal.slate()),
                Span::styled(phase.name().to_uppercase(), border),
                Span::styled(" ", pal.slate()),
            ]));
        let inner = block.inner(cells[i]);
        f.render_widget(block, cells[i]);
        let mut spans = Vec::new();
        if i < now {
            spans.push(Span::styled(
                format!("{} {state}", ui.look.g("✓", "+")),
                pal.moss(),
            ));
        } else {
            let odds = g.chance(scheme, *phase, 0, if i == now { flubs } else { 0 });
            match odds {
                Some(c) => {
                    let style = if c.total < 40 {
                        pal.red()
                    } else if c.total < 65 {
                        pal.brass()
                    } else {
                        pal.moss()
                    };
                    spans.push(Span::styled(
                        format!(
                            "{} {:>3}%",
                            if i == now { ui.look.g("▶", ">") } else { " " },
                            c.total
                        ),
                        style,
                    ));
                    spans.push(Span::styled(format!("  {state}"), pal.slate()));
                }
                None => spans.push(Span::styled(state, pal.slate())),
            }
        }
        f.render_widget(Paragraph::new(Line::from(spans)), inner);
    }
}

/// The nerve and the slips.
fn nerve_panel(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, flubs: u8) {
    let pal = &ui.pal;
    let p = &ui.g.player;
    let inner = ui.framed(f, area, "Nerve");
    let mut lines = Vec::new();
    let (full, empty) = (ui.look.g("◆", "#"), ui.look.g("◇", "."));
    let have = usize::from(p.nerve);
    let max = usize::from(p.nerve_max);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for i in 0..max {
        if i < have {
            spans.push(Span::styled(
                format!("{full} "),
                pal.fg_or(super::hue::INK, pal.accent),
            ));
        } else {
            spans.push(Span::styled(format!("{empty} "), pal.slate()));
        }
    }
    lines.push(Line::from(spans));
    lines.push(Line::from(vec![Span::styled(
        format!("{have} of {max} left"),
        pal.chalk_soft(),
    )]));
    let mut slips = vec![Span::styled("slips ", pal.slate())];
    if flubs == 0 {
        slips.push(Span::styled("none yet", pal.moss()));
    } else {
        slips.push(Span::styled(
            ui.look.g("✗", "x").repeat(usize::from(flubs)),
            pal.red(),
        ));
        slips.push(Span::styled(format!(" {flubs}"), pal.red()));
    }
    lines.push(Line::from(slips));
    lines.push(Line::from(vec![
        Span::styled("heat ", pal.slate()),
        Span::styled(format!("{}", p.heat), pal.red()),
    ]));
    f.render_widget(Paragraph::new(lines), inner);
}

/// The ways to spend nerve, each with the odds it buys.
fn spend_panel(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, now: usize, flubs: u8) {
    let pal = &ui.pal;
    let g = ui.g;
    let inner = ui.framed(f, area, "Spend nerve");
    let Some(run) = g.player.run.as_ref() else {
        return;
    };
    let Some(scheme) = g.player.scheme(run.scheme) else {
        return;
    };
    let phase = PHASES.get(now).copied().unwrap_or(Phase::Sneak);
    let mut lines = Vec::new();
    let at = ui.cursor();
    for (i, item) in ui.items.iter().enumerate() {
        let push = u8::try_from(item.key.to_digit(10).unwrap_or(0)).unwrap_or(0);
        let Some(c) = g.chance(scheme, phase, push, flubs) else {
            continue;
        };
        let style: Style = if i == at {
            pal.bar_on()
        } else {
            pal.chalk_soft()
        };
        let bar_w = inner.width.saturating_sub(18).max(4);
        let mut spans = vec![Span::styled(
            format!(" {} spend {} ", item.key, push),
            style,
        )];
        spans.extend(ui.bar(
            u32::try_from(c.total).unwrap_or(0),
            100,
            bar_w,
            if c.total < 40 {
                pal.red()
            } else if c.total < 65 {
                pal.brass()
            } else {
                pal.moss()
            },
        ));
        spans.push(Span::styled(format!(" {:>3}%", c.total), style));
        lines.push(Line::from(spans));
    }
    // Why the odds are what they are.
    if let Some(c) = g.chance(scheme, phase, 0, flubs) {
        let why: Vec<String> = c
            .parts
            .iter()
            .filter(|(_, v)| *v != 0)
            .map(|(n, v)| format!("{n} {v:+}"))
            .collect();
        let room = usize::from(inner.height).saturating_sub(lines.len());
        for l in wrap(&why.join(", "), usize::from(inner.width))
            .into_iter()
            .take(room)
        {
            lines.push(Line::styled(l, pal.slate()));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// What has happened so far, newest at the bottom.
fn log_panel(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let pal = &ui.pal;
    let inner = ui.framed(f, area, "What happened");
    let width = usize::from(inner.width);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (n, m) in ui.s.messages.iter().enumerate() {
        let style = if n + 1 == ui.s.messages.len() {
            pal.lamp()
        } else {
            pal.chalk_soft()
        };
        lines.push(Line::styled(fit(m, width), style));
    }
    if lines.is_empty() {
        let g = ui.g;
        let place = g
            .content
            .place(&g.player.place)
            .map_or("?", |p| p.name.as_str());
        let about = g
            .current_slot()
            .map_or(0, |s| g.witnesses_at(&g.player.place, g.player.day, s));
        lines.push(Line::styled(
            format!("You are in position in the {place}. The corridor is quiet. For now."),
            pal.chalk_soft(),
        ));
        lines.push(Line::styled(
            format!(
                "{about} staff or prefects about. Each phase can go wrong in its own way; nerve buys the odds."
            ),
            pal.slate(),
        ));
    }
    let keep = usize::from(inner.height);
    let skip = lines.len().saturating_sub(keep);
    let lines: Vec<Line<'static>> = lines.into_iter().skip(skip).collect();
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// The run screen.
pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let Some(run) = g.player.run.as_ref() else {
        return;
    };
    let now = usize::from(run.phase);
    let flubs = run.flubs;
    let title_h = 1;
    let mid_h = match ui.look.size {
        Size::Tiny => 6,
        Size::Compact => 7,
        _ => 8,
    };
    let [title, tr, mid, log] = Layout::vertical([
        Constraint::Length(title_h),
        Constraint::Length(3),
        Constraint::Length(mid_h),
        Constraint::Min(3),
    ])
    .areas(area);
    // The scheme being run.
    let scheme = g.player.scheme(run.scheme);
    let what = scheme.map_or(String::new(), |s| {
        format!(
            "{} {} {}",
            g.content.method(&s.method).map_or("?", |m| m.name.as_str()),
            ui.look.g("→", "->"),
            g.content.target(&s.target).map_or("?", |t| t.name.as_str())
        )
    });
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" #{} ", run.scheme), ui.pal.stamp()),
            Span::styled(
                format!(
                    " {}",
                    fit(&what, usize::from(title.width).saturating_sub(8))
                ),
                ui.pal.chalk(),
            ),
        ])),
        title,
    );
    track(ui, f, tr, now, flubs);
    let [l, r] =
        Layout::horizontal([Constraint::Percentage(36), Constraint::Percentage(64)]).areas(mid);
    nerve_panel(ui, f, l, flubs);
    spend_panel(ui, f, r, now, flubs);
    log_panel(ui, f, log);
}
