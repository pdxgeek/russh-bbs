//! The season and the hall: standings, the dead, and what the heroes have changed in the world.
//! Everything shown is what the shared world already keeps (`World`); this screen only reads it.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Direction, Layout, Rect};
use russh_bbs_door::style::{Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Bar, BarChart, BarGroup, Cell, Clear, Paragraph, Row, Table, Tabs};

use super::kit::{
    BLOOD, BONE, Chip, DEEP, DEEP_BRIGHT, EMBER, EMBER_HOT, Keys, MOSS, STONE, clip, inner, panel,
};
use super::{Cx, band_name};
use crate::rules::{DEPTH_MAX, RECOVERY_DAYS};

pub(super) const TABS: [&str; 3] = ["Standings", "The dead", "The world"];

pub(super) fn draw(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect) {
    let game = cx.game;
    let [head, tabs, body, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .areas(area);
    super::header(
        cx,
        frame,
        head,
        &format!(
            "The Hall · season {} · the dungeon remade {}x",
            game.world.season, game.world.epoch
        ),
    );
    let tab = cx.s.ui.tab.min(TABS.len() - 1);
    frame.render_widget(
        Tabs::new(TABS.to_vec())
            .select(tab)
            .style(cx.roles.dim)
            .highlight_style(Style::new().fg(EMBER_HOT).add_modifier(Modifier::BOLD))
            .divider(if cx.look.ascii { "|" } else { "│" }),
        tabs,
    );
    match tab {
        0 => standings(cx, frame, body),
        1 => the_dead(cx, frame, body),
        _ => the_world(cx, frame, body),
    }
    frame.render_widget(
        Keys {
            chips: &[Chip::new("Left/Right", "tab"), Chip::new("Esc", "back")],
            roles: cx.roles,
        },
        keys,
    );
}

fn standings(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect) {
    let game = cx.game;
    let w = &game.world;
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(44), Constraint::Fill(1)]).areas(area);
    frame.render_widget(Clear, left);
    frame.render_widget(panel("Deepest heroes", true), left);
    let lb = inner(left);
    if w.deepest.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "No one has come back from the deep yet.",
                cx.roles.dim,
            )),
            lb,
        );
    } else {
        let bars: Vec<Bar<'_>> = w
            .deepest
            .iter()
            .take(usize::from(lb.height))
            .map(|(name, depth)| {
                Bar::default()
                    .value(u64::from(*depth))
                    .label(Line::from(clip(name, 12)))
                    .text_value(format!("L{depth}"))
                    .style(Style::new().fg(if *name == game.hero.name {
                        EMBER_HOT
                    } else {
                        DEEP_BRIGHT
                    }))
            })
            .collect();
        frame.render_widget(
            BarChart::default()
                .direction(Direction::Horizontal)
                .bar_width(1)
                .bar_gap(0)
                .max(u64::from(DEPTH_MAX))
                .data(BarGroup::default().bars(&bars))
                .label_style(cx.roles.plain)
                .value_style(Style::new().fg(BONE).add_modifier(Modifier::BOLD)),
            lb,
        );
    }
    frame.render_widget(Clear, right);
    frame.render_widget(panel("The guardians", false), right);
    let rb = inner(right);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for band in 1..=5u8 {
        let by = w
            .slain
            .iter()
            .find(|(b, _)| *b == band)
            .map(|(_, who)| who.clone());
        lines.push(Line::from(vec![
            Span::styled(format!("{band} "), cx.roles.accent),
            Span::styled(clip(band_name(band), 18), cx.roles.plain),
        ]));
        lines.push(Line::styled(
            match by {
                Some(who) => format!("    slain by {}", clip(&who, 14)),
                None => "    unbroken".to_owned(),
            },
            if w.is_slain(band) {
                Style::new().fg(MOSS)
            } else {
                Style::new().fg(BLOOD)
            },
        ));
    }
    let h = &game.hero;
    lines.push(Line::styled(
        format!(
            "You: level {}, {} kills, {} death(s)",
            h.best_depth, h.kills, h.deaths
        ),
        cx.roles.strong,
    ));
    frame.render_widget(Paragraph::new(lines), rb);
}

fn the_dead(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect) {
    let game = cx.game;
    frame.render_widget(Clear, area);
    frame.render_widget(panel("In memory", false), area);
    let b = inner(area);
    let mut rows: Vec<Row<'_>> = Vec::new();
    for f in game.world.fallen.iter().rev() {
        let ago = (game.day - f.day).max(0);
        let (state, style) = match &f.rescued_by {
            Some(by) => (
                format!("carried home by {}", clip(by, 12)),
                Style::new().fg(MOSS),
            ),
            None if ago > RECOVERY_DAYS => ("not recovered".to_owned(), Style::new().fg(STONE)),
            None => (
                format!("waiting, {} day(s) left", (RECOVERY_DAYS - ago).max(0)),
                Style::new().fg(BLOOD),
            ),
        };
        rows.push(
            Row::new([
                Cell::from(if cx.look.ascii { "x" } else { "†" }),
                Cell::from(clip(&f.name, 14)),
                Cell::from(format!("level {}", f.depth)),
                Cell::from(format!("{ago}d ago")),
                Cell::from(state),
            ])
            .style(style),
        );
    }
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled("No one has fallen this season.", cx.roles.dim)),
            b,
        );
        return;
    }
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(2),
                Constraint::Length(14),
                Constraint::Length(8),
                Constraint::Length(7),
                Constraint::Fill(1),
            ],
        )
        .column_spacing(1),
        b,
    );
}

fn the_world(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect) {
    let game = cx.game;
    let w = &game.world;
    let c = cx.content;
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Fill(1)]).areas(area);
    frame.render_widget(Clear, left);
    frame.render_widget(panel("What heroes have changed", true), left);
    let lb = inner(left);
    let mut lines: Vec<Line<'static>> = Vec::new();
    if w.shortcuts.is_empty() {
        lines.push(Line::styled("No shortcuts open yet.", cx.roles.dim));
    }
    for (depth, kind) in &w.shortcuts {
        lines.push(Line::from(vec![
            Span::styled(
                if cx.look.ascii { "+ " } else { "▪ " },
                Style::new().fg(MOSS),
            ),
            Span::styled(
                format!("A {} at level {depth}", kind.label()),
                cx.roles.plain,
            ),
        ]));
    }
    for p in &c.projects.project {
        if w.is_done(c, &p.id) {
            lines.push(Line::from(vec![
                Span::styled(
                    if cx.look.ascii { "* " } else { "◆ " },
                    Style::new().fg(EMBER),
                ),
                Span::styled(
                    clip(&p.name, usize::from(lb.width).saturating_sub(2)),
                    cx.roles.plain,
                ),
            ]));
        }
    }
    if let Some(who) = &w.finale {
        lines.push(Line::styled(
            format!("{who} felled the Cold Furnace. The dungeon will remake itself."),
            Style::new().fg(EMBER_HOT),
        ));
    }
    frame.render_widget(Paragraph::new(lines), lb);
    frame.render_widget(Clear, right);
    frame.render_widget(panel("News of the season", false), right);
    let rb = inner(right);
    let news: Vec<Line<'static>> = w
        .news
        .iter()
        .rev()
        .take(usize::from(rb.height))
        .map(|n| {
            Line::from(vec![
                Span::styled(
                    if cx.look.ascii { "- " } else { "▸ " },
                    Style::new().fg(DEEP),
                ),
                Span::styled(
                    clip(n, usize::from(rb.width).saturating_sub(2)),
                    cx.roles.dim,
                ),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(news), rb);
}
