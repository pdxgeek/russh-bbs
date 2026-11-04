//! The combat panel: each enemy on a card with its health, its row, its status and, above all, the
//! action it has announced (reading intent is the core skill); the order of the round; the hero's
//! health, focus, ward, stance and row; the hero's actions with their costs; and the fight log.
//! No rule lives here: the numbers are the engine's, and the keys are the ones `fight_key` takes.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Color, Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Clear, Paragraph, Tabs};

use super::Cx;
use super::kit::{
    BLOOD, Chip, DEEP_BRIGHT, EMBER, EMBER_HOT, Keys, MOSS, Meter, STONE, STONE_DARK, STONE_LIGHT,
    clip, frac, inner, panel, pips,
};
use crate::combat::Stance;
use crate::content::{Intent, Reach, Row};
use crate::game::Game;

/// How an announced action is drawn: a glyph, a colour and a hint about what answers it.
fn intent_look(intent: Intent, ascii: bool) -> (&'static str, Color, &'static str) {
    let g = |u: &'static str, a: &'static str| if ascii { a } else { u };
    match intent {
        Intent::Strike => (g("▸", ">"), EMBER, "a plain blow"),
        Intent::Heavy => (
            g("▲", "^"),
            BLOOD,
            "double damage; an evasive stance is never caught",
        ),
        Intent::Shoot => (g("◆", "*"), EMBER_HOT, "shot from range: any row"),
        Intent::Brace => (
            g("■", "#"),
            DEEP_BRIGHT,
            "takes half damage, does not attack",
        ),
        Intent::Rally => (g("●", "o"), MOSS, "allies hit harder next round"),
        Intent::Advance => (g("»", ">"), STONE_LIGHT, "closing on your back row"),
        Intent::Breathe => (g("▼", "v"), BLOOD, "furnace breath: any row, heavy fire"),
    }
}

fn row_word(row: Row) -> &'static str {
    if row == Row::Front { "front" } else { "back" }
}

pub(super) fn draw(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect) {
    let game = cx.game;
    let Some(fight) = game.fight.as_ref() else {
        return;
    };
    let d = game.derived(cx.content);
    let moves: Vec<String> = d.moves.iter().cloned().collect();
    let alive: Vec<usize> = fight.alive().map(|(i, _)| i).collect();
    let target = cx.s.current_target(game, &alive);

    // The hero's actions as chips.
    let mut chips = vec![Chip::new("x", "attack")];
    for (n, id) in moves.iter().enumerate().take(9) {
        if let Some(mv) = cx.content.mv(id) {
            let c = Chip::new(format!("{}", n + 1), format!("{} ({}f)", mv.name, mv.cost));
            chips.push(if u32::from(fight.focus) >= mv.cost {
                c
            } else {
                c.off()
            });
        }
    }
    chips.push(Chip::new(
        "b",
        if fight.row == Row::Front {
            "step back"
        } else {
            "step forward"
        },
    ));
    chips.push(Chip::new("f", "flee"));
    if fight.round == 0 && !fight.paid && !fight.guardian {
        chips.push(Chip::new("z", "quick (1t)"));
    }
    chips.push(Chip::new("Up/Down", "aim").off());
    let action_rows = Keys::rows(&chips, area.width).min(3);
    let log_h: u16 = if area.height >= 30 {
        6
    } else if area.height >= 22 {
        4
    } else {
        3
    };
    let [head, order, hero, enemies, actions, log] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(2),
        Constraint::Length(action_rows),
        Constraint::Length(log_h),
    ])
    .areas(area);

    let tag = if fight.guardian { " · GUARDIAN" } else { "" };
    super::header(
        cx,
        frame,
        head,
        &format!("Fight · round {}{tag}", fight.round + 1),
    );

    // The order of the round: the hero acts first, then each enemy as it stands. Names when they
    // fit, numbers and intent glyphs when they do not.
    let with_names = {
        let names: usize = fight
            .alive()
            .map(|(_, e)| {
                cx.content
                    .monster(&e.id)
                    .map_or(1, |m| m.name.chars().count().min(14))
                    + 9
            })
            .sum();
        names + 12 <= usize::from(area.width)
    };
    let mut order_spans: Vec<Span<'static>> = vec![
        Span::styled("Order ", cx.roles.dim),
        Span::styled(
            "You",
            Style::new().fg(EMBER_HOT).add_modifier(Modifier::BOLD),
        ),
    ];
    for (i, e) in fight.alive() {
        let name = cx.content.monster(&e.id).map_or("?", |m| m.name.as_str());
        let (g, color, _) = intent_look(e.intent, cx.look.ascii);
        order_spans.push(Span::styled(
            if cx.look.ascii { " > " } else { " ▸ " },
            cx.roles.dim,
        ));
        let label = if with_names {
            format!("({}) {}", i + 1, clip(name, 14))
        } else {
            format!("({})", i + 1)
        };
        order_spans.push(Span::styled(
            label,
            if i == target {
                cx.roles.strong
            } else {
                cx.roles.plain
            },
        ));
        order_spans.push(Span::styled(format!(" {g}"), Style::new().fg(color)));
    }
    frame.render_widget(Paragraph::new(Line::from(order_spans)), order);

    draw_hero(cx, frame, hero, game, d.max_hp);
    let used = draw_enemies(cx, frame, enemies, game, target);
    let spare = Rect::new(
        enemies.x,
        enemies.y + used,
        enemies.width,
        enemies.height.saturating_sub(used),
    );
    draw_you(cx, frame, spare, game);
    frame.render_widget(
        Keys {
            chips: &chips,
            roles: cx.roles,
        },
        actions,
    );
    super::log_panel(cx, frame, log);
}

fn draw_hero(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, game: &Game, max_hp: i32) {
    let Some(fight) = game.fight.as_ref() else {
        return;
    };
    let h = &game.hero;
    let [a, gap, b] = Layout::horizontal([
        Constraint::Percentage(52),
        Constraint::Length(2),
        Constraint::Fill(1),
    ])
    .areas(area);
    let _ = gap;
    let hp = h.hp.max(0);
    let [a1, a2] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(a);
    let hpf = frac(i64::from(hp), i64::from(max_hp));
    frame.render_widget(
        Meter::vital("Health", hpf, format!("{hp}/{max_hp}")).labelled(
            7,
            cx.roles.dim,
            cx.roles.plain,
        ),
        a1,
    );
    let mut focus = vec![Span::styled("Focus  ", cx.roles.dim)];
    focus.extend(pips(u32::from(fight.focus), 5, EMBER_HOT));
    focus.push(Span::styled(format!(" {}/5", fight.focus), cx.roles.plain));
    if fight.ward > 0 {
        focus.push(Span::styled(
            format!("   ward {}", fight.ward),
            Style::new().fg(DEEP_BRIGHT).add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(focus)), a2);

    let [b1, b2] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(b);
    let sel = match fight.stance {
        Stance::Aggressive => 0,
        Stance::Guarded => 1,
        Stance::Evasive => 2,
    };
    let names: [&str; 3] = if b.width < 44 {
        ["Aggr", "Guard", "Evas"]
    } else {
        ["Aggressive", "Guarded", "Evasive"]
    };
    let mark = if cx.look.ascii { "*" } else { "●" };
    let narrow: Vec<String> = ["a", "g", "v"]
        .iter()
        .zip(names)
        .enumerate()
        .map(|(i, (k, n))| format!("[{k}]{}{n}", if i == sel { mark } else { " " }))
        .collect();
    frame.render_widget(
        Tabs::new(narrow.iter().map(String::as_str))
            .select(sel)
            .style(cx.roles.dim)
            .highlight_style(Style::new().fg(EMBER_HOT).add_modifier(Modifier::BOLD))
            .divider(if cx.look.ascii { "|" } else { "│" }),
        b1,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("You stand ", cx.roles.dim),
            Span::styled(row_word(fight.row), cx.roles.strong),
            Span::styled(
                if h.fatigue >= 75 { "  weary" } else { "" },
                Style::new().fg(BLOOD),
            ),
        ])),
        b2,
    );
}

fn draw_enemies(
    cx: &Cx<'_>,
    frame: &mut DoorFrame<'_>,
    area: Rect,
    game: &Game,
    target: usize,
) -> u16 {
    let Some(fight) = game.fight.as_ref() else {
        return 0;
    };
    let alive: Vec<(usize, &crate::combat::Enemy)> = fight.alive().collect();
    let n = u16::try_from(alive.len()).unwrap_or(1).max(1);
    let card_h = if area.height >= n * 5 {
        5
    } else if area.height >= n * 3 {
        3
    } else {
        2
    };
    let gap = u16::from(area.height >= n * card_h + (n - 1));
    let ranged = game
        .weapon(cx.content)
        .is_some_and(|w| w.reach == Reach::Ranged);
    let mut y = area.y;
    for (i, e) in alive {
        if y + card_h > area.bottom() {
            break;
        }
        let rect = Rect::new(area.x, y, area.width, card_h);
        let Some(m) = cx.content.monster(&e.id) else {
            continue;
        };
        let aimed = i == target;
        let reachable = ranged || (fight.row == Row::Front && e.row == Row::Front);
        let body = if card_h >= 5 {
            frame.render_widget(Clear, rect);
            frame.render_widget(panel("", aimed), rect);
            inner(rect)
        } else {
            rect
        };
        let [l1, l2, l3] = [
            Rect::new(body.x, body.y, body.width, 1),
            Rect::new(body.x, body.y + 1, body.width, 1),
            Rect::new(body.x, body.y + 2, body.width, 1),
        ];
        let hp_max = i64::from(m.hp);
        let hpf = frac(i64::from(e.hp), hp_max);
        let [name_a, hp_a] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(28.min(body.width / 2)),
        ])
        .areas(l1);
        let marker = if aimed {
            if cx.look.ascii { ">" } else { "▶" }
        } else {
            " "
        };
        let name_style = if aimed {
            Style::new().fg(EMBER_HOT).add_modifier(Modifier::BOLD)
        } else {
            cx.roles.plain
        };
        let mut name_line = vec![
            Span::styled(format!("{marker}({}) ", i + 1), cx.roles.accent),
            Span::styled(m.name.clone(), name_style),
            Span::styled(format!("  {}", row_word(e.row)), cx.roles.dim),
        ];
        if !reachable {
            name_line.push(Span::styled("  out of reach", Style::new().fg(STONE)));
        }
        frame.render_widget(Paragraph::new(Line::from(name_line)), name_a);
        frame.render_widget(Meter::vital("", hpf, format!("{}/{}", e.hp, m.hp)), hp_a);
        // What it will do next.
        let (g, color, hint) = intent_look(e.intent, cx.look.ascii);
        let danger = matches!(e.intent, Intent::Heavy | Intent::Breathe);
        let mut next = vec![
            Span::styled(
                format!("   next {g} "),
                Style::new().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                e.intent.describe().to_owned(),
                if danger {
                    Style::new().fg(color).add_modifier(Modifier::BOLD)
                } else {
                    cx.roles.plain
                },
            ),
        ];
        let mut marks: Vec<(String, Color)> = Vec::new();
        if e.mark_pct > 0 {
            marks.push((format!("marked +{}%", e.mark_pct), EMBER_HOT));
        }
        if e.sunder > 0 {
            marks.push((format!("sundered {}", e.sunder), EMBER));
        }
        if e.bleed > 0 {
            marks.push((format!("bleeding {}", e.bleed), BLOOD));
        }
        if e.rallied {
            marks.push(("rallied".to_owned(), MOSS));
        }
        if e.interrupted {
            marks.push(("broken: loses its action".to_owned(), DEEP_BRIGHT));
        }
        let used: usize = next.iter().map(|s| s.content.chars().count()).sum();
        if card_h >= 3 {
            let room = usize::from(body.width).saturating_sub(used + 3);
            if room > 12 {
                next.push(Span::styled(
                    format!("  ({})", clip(hint, room - 3)),
                    cx.roles.dim,
                ));
            }
            frame.render_widget(Paragraph::new(Line::from(next)), l2);
            let mut chips: Vec<Span<'static>> = vec![Span::raw("   ")];
            for (text, color) in &marks {
                chips.push(Span::styled(format!("{text}  "), Style::new().fg(*color)));
            }
            if marks.is_empty() {
                chips.push(Span::styled("no marks on it", Style::new().fg(STONE_DARK)));
            }
            frame.render_widget(Paragraph::new(Line::from(chips)), l3);
        } else {
            for (text, color) in &marks {
                next.push(Span::styled(format!("  {text}"), Style::new().fg(*color)));
            }
            frame.render_widget(Paragraph::new(Line::from(next)), l2);
        }
        y += card_h + gap;
    }
    y.saturating_sub(area.y)
}

/// The hero's own panel in the rows the enemies did not use: what they wield and how their stance
/// bends the fight.
fn draw_you(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, game: &Game) {
    let Some(fight) = game.fight.as_ref() else {
        return;
    };
    if area.height < 4 {
        return;
    }
    let h = &game.hero;
    frame.render_widget(panel("You", false), area);
    let body = inner(area);
    let stance = match fight.stance {
        Stance::Aggressive => "Aggressive: hit harder, get hit harder.",
        Stance::Guarded => "Guarded: soak blows, hit softer.",
        Stance::Evasive => {
            "Evasive: slip strikes and never be caught by a heavy blow; hit a little softer."
        }
    };
    let mut lines = vec![Line::styled(stance, cx.roles.plain)];
    for (what, base, kit) in [("Hand", &h.weapon, &h.wkit), ("Body", &h.armour, &h.akit)] {
        lines.push(Line::from(vec![
            Span::styled(format!("{what} "), cx.roles.dim),
            Span::styled(
                format!(
                    "{}  wear {}%",
                    crate::town::item_name(cx.content, base, kit),
                    kit.wear
                ),
                cx.roles.plain,
            ),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(russh_bbs_door::widgets::Wrap { trim: true }),
        body,
    );
}
