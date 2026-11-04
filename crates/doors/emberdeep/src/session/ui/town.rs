//! Cindermere: the town hub (a skyline, the day's news and the list of places) and the screen of
//! a hero who died below.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Clear, List, ListItem, ListState, Paragraph};

use super::Cx;
use super::art::{Skyline, Vignette};
use super::kit::{
    BLOOD, BONE, Chip, EMBER, EMBER_DIM, EMBER_HOT, Keys, MOSS, Meter, STONE, clip, frac, inner,
    panel, wrap_to,
};
use crate::game::Game;
use crate::rules::RECOVERY_DAYS;

/// The places of the hub, in the order the list shows them: key, name.
pub(crate) const PLACES: [(char, &str); 11] = [
    ('b', "Tavern"),
    ('m', "Market"),
    ('i', "Gear"),
    ('f', "Forge"),
    ('t', "Temple"),
    ('g', "Guild hall"),
    ('l', "Library"),
    ('w', "Town works"),
    ('s', "Skills"),
    ('h', "Hero sheet"),
    ('r', "The hall"),
];

/// A few words about what a place offers this hero today.
fn status(cx: &Cx<'_>, game: &Game, key: char) -> (String, bool) {
    let c = cx.content;
    let h = &game.hero;
    let fx = game.town_fx(c);
    match key {
        'b' => {
            if let Some(t) = &h.contract {
                let name = c.contract(&t.id).map_or("a contract", |d| d.name.as_str());
                (
                    if t.done {
                        format!("turn in: {name}")
                    } else {
                        format!("on: {name}")
                    },
                    t.done,
                )
            } else {
                (format!("{} postings today", game.board(c).len()), false)
            }
        }
        'm' => (format!("{} stalls", game.stock(c).len()), false),
        'i' => (format!("{} spare piece(s)", h.pack.len()), false),
        'f' => (format!("forge level {}", fx.forge.max(1)), false),
        't' => {
            let hurt = h.hp < game.derived(c).max_hp || !h.wounds.is_empty();
            (
                if hurt {
                    format!("tend you: {}c", game.heal_price(c))
                } else {
                    "you are well".to_owned()
                },
                hurt,
            )
        }
        'g' => (
            format!(
                "drill {}c, {} today",
                game.train_price(),
                game.today.trained
            ),
            false,
        ),
        'l' => (
            if fx.library {
                format!("{} relic(s) to read", h.relics)
            } else {
                "reading room shut".to_owned()
            },
            false,
        ),
        'w' => {
            let done = c
                .projects
                .project
                .iter()
                .filter(|p| game.world.is_done(c, &p.id))
                .count();
            (
                format!("{done} of {} finished", c.projects.project.len()),
                false,
            )
        }
        's' => {
            let pts = u32::from(h.skill_points) + u32::from(h.attr_points);
            (
                if pts > 0 {
                    format!("{pts} point(s) to spend")
                } else {
                    "no points".to_owned()
                },
                pts > 0,
            )
        }
        'h' => (format!("level {}", h.level), false),
        'r' => (
            format!(
                "{} fallen",
                game.world
                    .fallen
                    .iter()
                    .filter(|f| f.rescued_by.is_none())
                    .count()
            ),
            false,
        ),
        _ => (String::new(), false),
    }
}

pub(super) fn draw(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect) {
    let game = cx.game;
    let [head, gauges, body, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(6),
        Constraint::Length(1),
    ])
    .areas(area);
    super::header(cx, frame, head, "Cindermere");
    super::gauges(cx, frame, gauges, None);
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Fill(1)]).areas(body);

    // Left: the skyline over the day's news.
    let sky_h = if body.height >= 22 {
        11
    } else if body.height >= 16 {
        8
    } else {
        7
    }
    .min(body.height.saturating_sub(5));
    let [sky, news] = Layout::vertical([Constraint::Length(sky_h), Constraint::Min(3)]).areas(left);
    frame.render_widget(Clear, sky);
    frame.render_widget(panel("Under the hill", true), sky);
    let c = cx.content;
    let built = u32::try_from(
        c.projects
            .project
            .iter()
            .filter(|p| game.world.is_done(c, &p.id))
            .count(),
    )
    .unwrap_or(0);
    frame.render_widget(
        Skyline {
            look: cx.look,
            day: game.day,
            built,
            glow: frac(i64::from(game.hero.turns), 25),
        },
        inner(sky),
    );
    draw_news(cx, frame, news, game);

    // Right: the places, then the way down.
    draw_places(cx, frame, right, game);

    frame.render_widget(
        Keys {
            chips: &[
                Chip::new("Up/Down Enter", "open a place"),
                Chip::new("d", "descend"),
                Chip::new("q", "leave the door"),
            ],
            roles: cx.roles,
        },
        keys,
    );
}

fn draw_news(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, game: &Game) {
    frame.render_widget(Clear, area);
    frame.render_widget(panel("Tavern board", false), area);
    let body = inner(area);
    let w = usize::from(body.width);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let news: Vec<&String> = game.world.news.iter().rev().take(3).collect();
    if news.is_empty() {
        lines.push(Line::styled("Nothing yet this season.", cx.roles.dim));
    }
    for n in news {
        for l in wrap_to(n, w.saturating_sub(2), 2) {
            lines.push(Line::from(vec![
                Span::styled(
                    if cx.look.ascii { "- " } else { "▪ " },
                    Style::new().fg(EMBER_DIM),
                ),
                Span::styled(l, cx.roles.dim),
            ]));
        }
    }
    let h = &game.hero;
    if !h.wounds.is_empty() {
        let list: Vec<String> = h
            .wounds
            .iter()
            .map(|(id, d)| {
                format!(
                    "{} {d}d",
                    cx.content
                        .wound(id)
                        .map_or(id.as_str(), |x| x.name.as_str())
                )
            })
            .collect();
        lines.push(Line::styled(
            clip(&format!("Wounds: {}", list.join(", ")), w),
            Style::new().fg(BLOOD),
        ));
    }
    if h.debt > 0 {
        lines.push(Line::styled(
            format!("You owe the crew {}c.", h.debt),
            Style::new().fg(EMBER),
        ));
    }
    frame.render_widget(Paragraph::new(lines), body);
}

fn draw_places(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, game: &Game) {
    frame.render_widget(Clear, area);
    frame.render_widget(panel("Places", true), area);
    let body = inner(area);
    let ways = game.ways_down(cx.content);
    let down_rows = u16::try_from(1 + ways.len().min(2)).unwrap_or(1);
    let [list_a, down_a] = Layout::vertical([
        Constraint::Min(4),
        Constraint::Length(down_rows.min(body.height.saturating_sub(4))),
    ])
    .areas(body);
    let cursor = cx.s.ui.home.min(PLACES.len() - 1);
    let items: Vec<ListItem<'static>> = PLACES
        .iter()
        .map(|(key, name)| {
            let (text, hot) = status(cx, game, *key);
            let w = usize::from(list_a.width).saturating_sub(2);
            let left = format!("[{key}] {name:<11}");
            let room = w.saturating_sub(left.chars().count());
            ListItem::new(Line::from(vec![
                Span::styled(left, cx.roles.accent),
                Span::styled(
                    clip(&text, room),
                    if hot {
                        Style::new().fg(EMBER_HOT)
                    } else {
                        cx.roles.dim
                    },
                ),
            ]))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(cursor));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol(if cx.look.ascii { "> " } else { "▸ " })
            .highlight_style(Style::new().add_modifier(Modifier::BOLD).fg(EMBER_HOT)),
        list_a,
        &mut state,
    );
    // The way down.
    let mut down: Vec<Line<'static>> = vec![Line::from(vec![
        Span::styled("[d] ", cx.roles.accent),
        Span::styled(
            "Descend to level 1",
            Style::new().fg(EMBER).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" (1 turn)", cx.roles.dim),
    ])];
    for (n, (depth, kind)) in ways.iter().enumerate().take(2) {
        down.push(Line::from(vec![
            Span::styled(format!("[{}] ", n + 1), cx.roles.accent),
            Span::styled(format!("{} to level {depth}", kind.label()), cx.roles.plain),
            Span::styled(format!(" ({} turn)", kind.flat_cost()), cx.roles.dim),
        ]));
    }
    frame.render_widget(Paragraph::new(down), down_a);
}

/// A hero who died below.
pub(super) fn draw_dead(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, depth: u8, since: i64) {
    let game = cx.game;
    let [head, body, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(6),
        Constraint::Length(1),
    ])
    .areas(area);
    super::header(cx, frame, head, "The dark");
    frame.render_widget(panel("You are dead", false), body);
    let b = inner(body);
    let left_days = (RECOVERY_DAYS - (game.day - since)).max(0);
    let stone = if cx.look.ascii {
        ["  ___  ", " /   \\ ", " | + | ", " |RIP| ", "_|___|_"]
    } else {
        ["  ▄▄▄  ", " ▟███▙ ", " █ † █ ", " █RIP█ ", "▀▀▀▀▀▀▀"]
    };
    let mut y = b.y + 1;
    for row in stone {
        if y >= b.bottom() {
            break;
        }
        frame.render_widget(
            Paragraph::new(Line::styled(row, Style::new().fg(STONE))),
            Rect::new(b.x + 2, y, 9.min(b.width), 1),
        );
        y += 1;
    }
    let text_x = b.x + 13.min(b.width.saturating_sub(10));
    let text_w = b.right().saturating_sub(text_x);
    let msg = format!(
        "Your body lies in the dark, on level {depth}. Another hero may carry you home. After that you wake in the temple, weakened, and the gear and pack are lost."
    );
    let mut lines: Vec<Line<'static>> = wrap_to(&msg, usize::from(text_w), 6)
        .into_iter()
        .map(|l| Line::styled(l, cx.roles.plain))
        .collect();
    lines.push(Line::raw(""));
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(text_x, b.y + 1, text_w, b.height.saturating_sub(4)),
    );
    // Eyes in the dark, between the words and the clock.
    let art_top = b.y + 7;
    let bar_y = b.bottom().saturating_sub(2);
    if bar_y > art_top + 1 {
        frame.render_widget(
            Vignette {
                kind: crate::content::RoomKind::Lair,
                look: cx.look,
                warmth: 0.2,
                seed: 5,
            },
            Rect::new(b.x, art_top, b.width, (bar_y - art_top - 1).min(6)),
        );
    }
    frame.render_widget(
        Meter::new(
            "Time left",
            frac(left_days, RECOVERY_DAYS),
            format!("{left_days} day(s)"),
            BLOOD,
            BONE,
        )
        .labelled(10, cx.roles.dim, cx.roles.plain),
        Rect::new(b.x + 1, bar_y, b.width.saturating_sub(2), 1),
    );
    frame.render_widget(
        Keys {
            chips: &[
                Chip::new("g", "give up and wake in the temple"),
                Chip::new("Esc", "leave"),
            ],
            roles: cx.roles,
        },
        keys,
    );
    let _ = (MOSS, EMBER);
}
