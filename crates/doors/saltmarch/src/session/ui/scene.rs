//! An event scene: a framed vignette of the place, the words of the moment, and the choices,
//! each with what it risks. An option that can start a fight says so (the engine's rule: the
//! player should know), and a risk is shown as a number, never hidden.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Modifier;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Wrap};

use super::{Ground, Size, Ui, fit, landscape};
use crate::lore::{Daypart, Water};
use crate::map::TileKind;
use crate::trip::{self, Scene};

/// A seed for the picture that is the same for the same scene in the same place.
fn seed(event: &str, at: (u8, u8)) -> u64 {
    event
        .bytes()
        .fold(u64::from(at.0) << 8 | u64::from(at.1), |h, b| {
            h.wrapping_mul(0x100_0000_01B3).wrapping_add(u64::from(b))
        })
}

pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, scene: &Scene) {
    let p = &ui.pal;
    let Some(t) = ui.clan.trip.as_ref() else {
        return;
    };
    let Some(def) = ui.s.cat.event(&scene.event) else {
        return;
    };
    let kind =
        ui.s.map
            .tile(usize::from(t.at.0), usize::from(t.at.1))
            .map_or(TileKind::Mudflat, |x| x.kind);
    let ground = if t.at == trip::home(&ui.env, ui.clan) {
        Ground::Hold
    } else {
        Ground::Tile(kind)
    };
    let opt_rows = u16::try_from(scene.offered.len()).unwrap_or(4).max(1) + 2;
    let [stage, choices] =
        Layout::vertical([Constraint::Min(10), Constraint::Length(opt_rows)]).areas(area);

    // The frame carries the scene's title; inside it, the picture and then the words.
    let title = if def.rare {
        format!("{} {}", ui.look.g("*", "*"), def.title)
    } else {
        def.title.clone()
    };
    let block = ui.panel(&title);
    let inner = block.inner(stage);
    f.render_widget(block, stage);
    let text_w = usize::from(inner.width).saturating_sub(2);
    let words = super::wrap(&def.text, text_w);
    let text_h = u16::try_from(words.len())
        .unwrap_or(4)
        .min(match ui.look.size {
            Size::Compact => 5,
            _ => 8,
        });
    let pic_h = inner.height.saturating_sub(text_h + 1).clamp(3, 16);
    let [pic, rest] =
        Layout::vertical([Constraint::Length(pic_h), Constraint::Min(2)]).areas(inner);
    f.render_widget(
        landscape(
            ui.look,
            p,
            (pic.width, pic.height),
            ground,
            Daypart::of(ui.minute),
            Water::of(ui.level),
            seed(&scene.event, t.at),
        ),
        pic,
    );
    let mut lines: Vec<Line<'static>> = vec![Line::from(Span::styled(
        format!(
            "{} {} {}, water {} ({})",
            ui.look.g("·", "-"),
            kind.name(),
            Daypart::of(ui.minute),
            ui.level,
            Water::of(ui.level)
        ),
        p.dim,
    ))];
    lines.extend(
        words
            .into_iter()
            .take(usize::from(rest.height).saturating_sub(1))
            .map(|l| Line::from(Span::styled(l, p.plain))),
    );
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), rest);

    // The choices.
    let block = ui.panel("What do you do?");
    let inner = block.inner(choices);
    f.render_widget(block, choices);
    let mut rows: Vec<Line<'static>> = Vec::new();
    for (n, &i) in scene.offered.iter().enumerate() {
        let Some(info) = ui.s.option_info(ui.clan, t, scene, i) else {
            continue;
        };
        let key = ui
            .entry(&format!("choose:{n}"))
            .map_or(Ui::key_of(n), |(k, _)| Ui::key_of(k));
        let on = ui
            .entry(&format!("choose:{n}"))
            .is_some_and(|(k, _)| k == ui.s.cursor);
        let mut spans: Vec<Span<'static>> = vec![Span::styled(
            format!(" {key} "),
            if on { p.selected } else { p.accent },
        )];
        let room = usize::from(inner.width).saturating_sub(40);
        spans.push(Span::styled(
            fit(
                &info.label,
                usize::from(inner.width).saturating_sub(24).max(room),
            ),
            if on { p.selected } else { p.salt() },
        ));
        if let Some(skill) = info.skill {
            spans.push(Span::styled(format!("  [{}]", skill.name()), p.peat()));
        }
        if let Some(risk) = info.risk {
            let style = if risk >= 40 {
                p.rust()
            } else if risk >= 20 {
                p.gold()
            } else {
                p.moss()
            };
            spans.push(Span::styled(
                format!("  {} {risk}% to go badly", ui.look.g("▲", "!")),
                style.add_modifier(Modifier::BOLD),
            ));
        }
        if !info.fight.is_empty() {
            spans.push(Span::styled(
                format!("  {} {}", ui.look.g("×", "x"), info.fight.trim()),
                p.rust(),
            ));
        }
        rows.push(Line::from(spans));
    }
    if rows.is_empty() {
        let on = ui.s.cursor == 0;
        rows.push(Line::styled(
            " a Let the moment pass",
            if on { p.selected } else { p.plain },
        ));
    }
    f.render_widget(Paragraph::new(rows), inner);
}
