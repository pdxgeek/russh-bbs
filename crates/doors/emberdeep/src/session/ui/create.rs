//! Making a hero: a calling to choose (with a card for the one under the cursor), two traits and
//! four attribute points. The same keys as ever (a digit, a letter, `Enter`, `Backspace`), plus
//! the arrow keys to browse.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Clear, List, ListItem, ListState, Paragraph};

use super::kit::{
    Chip, DEEP_BRIGHT, EMBER, EMBER_DIM, EMBER_HOT, Keys, Look, MOSS, Meter, Roles, Rule,
    STONE_DARK, clip, frac, inner, panel, wrap_to,
};
use crate::content::Attr;
use crate::rules::{ATTR_MAX, CREATION_POINTS, CREATION_TRAITS};
use crate::session::{Create, CreateStep, Session, TRAIT_KEYS, effect_summary};

pub(super) fn draw(
    s: &Session,
    frame: &mut DoorFrame<'_>,
    area: Rect,
    c: &Create,
    roles: Roles,
    look: Look,
) {
    let [head, body, note, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(6),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .areas(area);
    let mark = if look.ascii { "*" } else { "◆" };
    let step = match c.step {
        CreateStep::Calling => "1 of 3: choose a calling",
        CreateStep::Traits => "2 of 3: choose two traits",
        CreateStep::Points => "3 of 3: spend four points",
    };
    let title = format!("{mark} EMBERDEEP · A new hero · {step} ");
    let tw = u16::try_from(title.chars().count())
        .unwrap_or(u16::MAX)
        .min(area.width);
    frame.render_widget(
        Paragraph::new(Line::styled(
            title,
            Style::new().fg(EMBER).add_modifier(Modifier::BOLD),
        )),
        Rect::new(head.x, head.y, tw, 1),
    );
    frame.render_widget(
        Rule {
            glyph: if look.ascii { "-" } else { "━" },
            from: EMBER_DIM,
            to: STONE_DARK,
        },
        Rect::new(head.x + tw, head.y, head.width.saturating_sub(tw), 1),
    );
    let (chips, note_text): (Vec<Chip>, String) = match c.step {
        CreateStep::Calling => {
            callings(s, frame, body, c, roles, look);
            (
                vec![
                    Chip::new("1-6", "choose"),
                    Chip::new("Up/Down", "browse"),
                    Chip::new("Enter", "pick"),
                    Chip::new("Esc", "leave"),
                ],
                c.note.clone().unwrap_or_default(),
            )
        }
        CreateStep::Traits => {
            traits(s, frame, body, c, roles, look);
            (
                vec![
                    Chip::new("a-x", "pick or drop"),
                    Chip::new("Arrows", "browse"),
                    Chip::new(
                        "Enter",
                        if c.traits.len() == CREATION_TRAITS {
                            "continue"
                        } else {
                            "continue (pick two)"
                        },
                    ),
                    Chip::new("Backspace", "back"),
                ],
                String::new(),
            )
        }
        CreateStep::Points => {
            points(s, frame, body, c, roles);
            (
                vec![
                    Chip::new("1-4", "add a point"),
                    Chip::new("Enter", "begin"),
                    Chip::new("Backspace", "undo"),
                ],
                String::new(),
            )
        }
    };
    if !note_text.is_empty() {
        let lines: Vec<Line<'static>> = wrap_to(&note_text, usize::from(note.width), 2)
            .into_iter()
            .map(|l| Line::styled(l, roles.dim))
            .collect();
        frame.render_widget(Paragraph::new(lines), note);
    }
    frame.render_widget(
        Keys {
            chips: &chips,
            roles,
        },
        keys,
    );
}

fn cx_dim(roles: Roles) -> Style {
    roles.dim
}

fn callings(
    s: &Session,
    frame: &mut DoorFrame<'_>,
    area: Rect,
    c: &Create,
    roles: Roles,
    look: Look,
) {
    let [left, right] =
        Layout::horizontal([Constraint::Length(22), Constraint::Fill(1)]).areas(area);
    frame.render_widget(Clear, left);
    frame.render_widget(panel("Callings", true), left);
    let items: Vec<ListItem<'static>> = s
        .content
        .callings
        .calling
        .iter()
        .enumerate()
        .map(|(i, k)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("[{}] ", i + 1), roles.accent),
                Span::styled(k.name.clone(), roles.plain),
            ]))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(c.calling));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol(if look.ascii { "> " } else { "▸ " })
            .highlight_style(Style::new().fg(EMBER_HOT).add_modifier(Modifier::BOLD)),
        inner(left),
        &mut state,
    );
    frame.render_widget(Clear, right);
    frame.render_widget(panel("", false), right);
    let rb = inner(right);
    let Some(k) = s.content.callings.calling.get(c.calling) else {
        return;
    };
    let mut y = rb.y;
    let blurb = wrap_to(&k.blurb, usize::from(rb.width), 3);
    let n = u16::try_from(blurb.len()).unwrap_or(0);
    frame.render_widget(
        Paragraph::new(
            blurb
                .into_iter()
                .map(|l| Line::styled(l, roles.plain))
                .collect::<Vec<_>>(),
        ),
        Rect::new(rb.x, y, rb.width, n),
    );
    y += n + 1;
    for (i, attr) in Attr::ALL.iter().enumerate() {
        if y >= rb.bottom() {
            return;
        }
        frame.render_widget(
            Meter::heated(
                attr.label(),
                frac(i64::from(k.attrs[i]), i64::from(ATTR_MAX)),
                format!("{}", k.attrs[i]),
                0.4,
                0.88,
            )
            .labelled(9, roles.dim, roles.plain),
            Rect::new(rb.x, y, rb.width, 1),
        );
        y += 1;
    }
    let mut lines: Vec<Line<'static>> =
        vec![Line::styled(format!("{} hit points", k.hp), roles.plain)];
    let weapon = s
        .content
        .weapon(&k.weapon)
        .map_or(k.weapon.clone(), |w| w.name.clone());
    let armour = s
        .content
        .armour(&k.armour)
        .map_or(k.armour.clone(), |w| w.name.clone());
    lines.push(Line::styled(
        clip(&format!("{weapon}, {armour}"), usize::from(rb.width)),
        roles.dim,
    ));
    let moves: Vec<&str> = k
        .moves
        .iter()
        .filter_map(|m| s.content.mv(m))
        .map(|m| m.name.as_str())
        .collect();
    lines.push(Line::styled(
        clip(
            &format!("Moves: {}", moves.join(", ")),
            usize::from(rb.width),
        ),
        Style::new().fg(DEEP_BRIGHT),
    ));
    lines.push(Line::styled(
        clip(
            &format!("Knacks: {}", k.tags.join(", ")),
            usize::from(rb.width),
        ),
        Style::new().fg(MOSS),
    ));
    let first: Vec<&str> = s
        .content
        .nodes
        .node
        .iter()
        .filter(|n| n.calling == k.id && n.tier == 1)
        .map(|n| n.name.as_str())
        .collect();
    lines.push(Line::styled(
        clip(
            &format!("First skills: {}", first.join(", ")),
            usize::from(rb.width),
        ),
        cx_dim(roles),
    ));
    if y < rb.bottom() {
        frame.render_widget(
            Paragraph::new(lines),
            Rect::new(rb.x, y, rb.width, rb.bottom() - y),
        );
    }
}

fn traits(
    s: &Session,
    frame: &mut DoorFrame<'_>,
    area: Rect,
    c: &Create,
    roles: Roles,
    look: Look,
) {
    let [lists, detail] = Layout::vertical([
        Constraint::Length(10.min(area.height.saturating_sub(2))),
        Constraint::Min(2),
    ])
    .areas(area);
    let cols3 = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Fill(1),
    ])
    .split(lists);
    let all = &s.content.traits.traits;
    let half = 8;
    for (col, rect) in cols3.iter().copied().enumerate() {
        frame.render_widget(Clear, rect);
        let title = if col == 0 {
            format!("Traits ({} of {CREATION_TRAITS})", c.traits.len())
        } else {
            String::new()
        };
        frame.render_widget(panel(&title, col == 0), rect);
        let items: Vec<ListItem<'static>> = (0..half)
            .filter_map(|row| {
                let at = row + col * half;
                all.get(at).map(|t| {
                    let chosen = c.traits.contains(&at);
                    let key = TRAIT_KEYS.chars().nth(at).unwrap_or('?');
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("[{key}]"), roles.accent),
                        Span::styled(
                            if chosen {
                                if look.ascii { "*" } else { "◆" }
                            } else {
                                " "
                            },
                            Style::new().fg(EMBER_HOT),
                        ),
                        Span::styled(
                            clip(&t.name, 17),
                            if chosen {
                                Style::new().fg(EMBER_HOT)
                            } else {
                                roles.plain
                            },
                        ),
                    ]))
                })
            })
            .collect();
        let mut state = ListState::default();
        if c.cursor / half == col {
            state.select(Some(c.cursor % half));
        }
        frame.render_stateful_widget(
            List::new(items)
                .highlight_symbol(if look.ascii { ">" } else { "▸" })
                .highlight_style(Style::new().add_modifier(Modifier::BOLD)),
            inner(rect),
            &mut state,
        );
    }
    if let Some(t) = all.get(c.cursor) {
        let text = format!("{}: {}", t.name, t.text);
        let lines: Vec<Line<'static>> =
            wrap_to(&text, usize::from(detail.width), usize::from(detail.height))
                .into_iter()
                .map(|l| Line::styled(l, roles.dim))
                .collect();
        frame.render_widget(Paragraph::new(lines), detail);
    }
}

fn points(s: &Session, frame: &mut DoorFrame<'_>, area: Rect, c: &Create, roles: Roles) {
    frame.render_widget(Clear, area);
    frame.render_widget(panel("Attributes", true), area);
    let b = inner(area);
    let spent: u8 = c.points.iter().sum();
    let left = CREATION_POINTS.saturating_sub(spent);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Points left ", roles.dim),
            Span::styled(
                format!("{left}"),
                if left == 0 { roles.good } else { roles.strong },
            ),
        ])),
        Rect::new(b.x, b.y, b.width, 1),
    );
    let Some(k) = s.content.callings.calling.get(c.calling) else {
        return;
    };
    for (i, attr) in Attr::ALL.iter().enumerate() {
        let Ok(dy) = u16::try_from(i) else { break };
        let y = b.y + 2 + dy;
        if y >= b.bottom() {
            break;
        }
        let v = k.attrs[i] + u32::from(c.points[i]);
        frame.render_widget(
            Meter::heated(
                &format!("[{}] {}", i + 1, attr.label()),
                frac(i64::from(v), i64::from(ATTR_MAX)),
                format!("{v} (+{})", c.points[i]),
                0.4,
                0.88,
            )
            .labelled(13, roles.accent, roles.plain),
            Rect::new(b.x, y, b.width, 1),
        );
    }
    let y = b.y + 7;
    if y < b.bottom() {
        let lines: Vec<Line<'static>> = wrap_to(&k.blurb, usize::from(b.width), 3)
            .into_iter()
            .map(|l| Line::styled(l, roles.dim))
            .collect();
        frame.render_widget(
            Paragraph::new(lines),
            Rect::new(b.x, y, b.width, b.bottom() - y),
        );
    }
    let _ = effect_summary;
}
