//! The hero sheet (tabs for the numbers, the marks the hero carries and the moves they know) and
//! the skill tree, each a list with a detail pane for the row under the cursor. The same
//! list-and-detail widget serves the town's places. Nothing here changes the hero: `Enter` on the
//! skill tree sends the same `Action::Buy` the number keys do.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Clear, List, ListItem, ListState, Paragraph, Tabs};

use super::Cx;
use super::kit::{
    BLOOD, BONE, Chip, DEEP_BRIGHT, EMBER, EMBER_HOT, Keys, MOSS, Meter, STONE, STONE_DARK, clip,
    frac, inner, panel, vitality,
};
use crate::content::Attr;
use crate::rules::{ATTR_MAX, LEVEL_MAX, xp_for_level};
use crate::session::{NODE_KEYS, effect_summary};
use crate::town::item_name;

/// One selectable row: how it reads in the list, and what the detail pane says.
pub(super) struct Row {
    pub label: Vec<Span<'static>>,
    pub detail_title: String,
    pub detail: Vec<Line<'static>>,
}

/// A list on the left, the detail of the row under the cursor on the right.
pub(super) fn list_detail(
    cx: &Cx<'_>,
    frame: &mut DoorFrame<'_>,
    area: Rect,
    title: &str,
    rows: &[Row],
    cursor: usize,
) {
    let narrow = area.width < 60;
    let (left_area, right_area) = if narrow {
        // A narrow window shows the list over a short detail pane.
        let [a, b] = Layout::vertical([Constraint::Fill(3), Constraint::Min(5)]).areas(area);
        (a, b)
    } else {
        let [a, b] =
            Layout::horizontal([Constraint::Percentage(55), Constraint::Fill(1)]).areas(area);
        (a, b)
    };
    frame.render_widget(Clear, left_area);
    frame.render_widget(panel(title, true), left_area);
    let body = inner(left_area);
    let cursor = cursor.min(rows.len().saturating_sub(1));
    let items: Vec<ListItem<'static>> = rows
        .iter()
        .map(|r| ListItem::new(Line::from(r.label.clone())))
        .collect();
    let mut state = ListState::default();
    if !rows.is_empty() {
        state.select(Some(cursor));
    }
    let marker = if cx.look.ascii { "> " } else { "▸ " };
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol(marker)
            .highlight_style(Style::new().add_modifier(Modifier::BOLD).fg(EMBER_HOT)),
        body,
        &mut state,
    );
    frame.render_widget(Clear, right_area);
    frame.render_widget(panel("Detail", false), right_area);
    let dbody = inner(right_area);
    if let Some(row) = rows.get(cursor) {
        let mut lines = vec![Line::styled(
            clip(&row.detail_title, usize::from(dbody.width)),
            Style::new().fg(EMBER).add_modifier(Modifier::BOLD),
        )];
        lines.extend(row.detail.iter().cloned());
        frame.render_widget(
            Paragraph::new(lines).wrap(russh_bbs_door::widgets::Wrap { trim: false }),
            dbody,
        );
    }
}

fn attr_doc(attr: Attr) -> &'static str {
    match attr {
        Attr::Might => {
            "Strength and toughness. Adds to damage with heavy weapons and decides hauling and hardiness."
        }
        Attr::Finesse => {
            "Speed and precision. Adds to damage with light and ranged weapons, and to slipping blows."
        }
        Attr::Wits => "Perception, lore and craft. Spots traps, opens locks and works runes.",
        Attr::Spirit => {
            "Will and faith. Steadies a hero at the brink of death and feeds the ember-priest's rites."
        }
    }
}

fn bar(f: f32, width: usize, ascii: bool) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let full = ((f.clamp(0.0, 1.0) * width as f32).round() as usize).min(width);
    let (on, off) = if ascii { ('#', '.') } else { ('█', '░') };
    let mut s = String::new();
    s.extend(std::iter::repeat_n(on, full));
    s.extend(std::iter::repeat_n(off, width - full));
    s
}

fn stat_row(cx: &Cx<'_>, name: &str, value: String, doc: &str) -> Row {
    Row {
        label: vec![
            Span::styled(format!("{name:<14}"), cx.roles.plain),
            Span::styled(value.clone(), cx.roles.strong),
        ],
        detail_title: name.to_owned(),
        detail: doc_lines(cx, &format!("{value}. {doc}")),
    }
}

/// The text as one paragraph; the pane wraps it to its own width.
fn doc_lines(cx: &Cx<'_>, text: &str) -> Vec<Line<'static>> {
    vec![Line::styled(text.to_owned(), cx.roles.plain)]
}

fn stats_rows(cx: &Cx<'_>) -> Vec<Row> {
    let game = cx.game;
    let h = &game.hero;
    let d = game.derived(cx.content);
    let mut rows = Vec::new();
    for attr in Attr::ALL {
        let v = d.attr(attr);
        let f = frac(i64::from(v), i64::from(ATTR_MAX) + 4);
        rows.push(Row {
            label: vec![
                Span::styled(format!("{:<8}", attr.label()), cx.roles.plain),
                Span::styled(bar(f, 12, cx.look.ascii), Style::new().fg(EMBER)),
                Span::styled(format!(" {v:>2}"), cx.roles.strong),
            ],
            detail_title: attr.label().to_owned(),
            detail: {
                let mut lines = doc_lines(cx, attr_doc(attr));
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    format!(
                        "Base {}, with everything you carry and know: {v}.",
                        h.attrs[attr.index()]
                    ),
                    cx.roles.dim,
                ));
                lines
            },
        });
    }
    rows.push(stat_row(
        cx,
        "Armor",
        d.armor.to_string(),
        "Taken off every blow.",
    ));
    rows.push(stat_row(
        cx,
        "Damage",
        format!("+{}", d.damage),
        "Added to every hit you land.",
    ));
    rows.push(stat_row(
        cx,
        "Crit",
        format!("{}%", d.crit),
        "Chance a hit lands as a critical.",
    ));
    rows.push(stat_row(
        cx,
        "Evade",
        format!("{}%", d.evade),
        "Helps you slip a blow, whatever your stance.",
    ));
    rows.push(stat_row(
        cx,
        "Death save",
        format!("+{}%", d.death_save),
        "Added to the chance of staying on your feet at the brink.",
    ));
    rows.push(stat_row(
        cx,
        "Carry",
        d.carry.to_string(),
        "Salvage you can haul before the climb costs extra.",
    ));
    rows.push(stat_row(
        cx,
        "Climb ease",
        d.climb_ease.to_string(),
        "Turns off the climb back to town.",
    ));
    rows.push(stat_row(
        cx,
        "Loot",
        format!("+{}%", d.loot),
        "More coin and salvage from what you find.",
    ));
    rows.push(stat_row(
        cx,
        "Trap sense",
        format!("+{}", d.trap_sense),
        "Helps you spot traps before they spring.",
    ));
    rows.push(stat_row(
        cx,
        "Fatigue guard",
        format!("{}%", d.fatigue_guard),
        "Slows the weariness that builds as you delve.",
    ));
    for (what, base, kit) in [
        ("Weapon", &h.weapon, &h.wkit),
        ("Armour", &h.armour, &h.akit),
    ] {
        let name = item_name(cx.content, base, kit);
        let core = kit
            .core
            .as_ref()
            .and_then(|c| cx.content.core(c))
            .map_or("no core".to_owned(), |c| c.name.clone());
        rows.push(Row {
            label: vec![
                Span::styled(format!("{what:<8}"), cx.roles.plain),
                Span::styled(clip(&name, 22), cx.roles.strong),
            ],
            detail_title: name.clone(),
            detail: {
                let mut v = doc_lines(cx, &format!("Wear {}%. Core: {core}.", kit.wear));
                for a in &kit.affixes {
                    if let Some(af) = cx.content.affix(a) {
                        v.push(Line::styled(
                            format!("{}: {}", af.name, effect_summary(cx.content, &af.effects)),
                            cx.roles.dim,
                        ));
                    }
                }
                v
            },
        });
    }
    rows
}

fn marks_rows(cx: &Cx<'_>) -> Vec<Row> {
    let h = &cx.game.hero;
    let mut rows = Vec::new();
    let tag = |t: &str, color| Span::styled(format!("{t:<7}"), Style::new().fg(color));
    for id in &h.traits {
        if let Some(t) = cx.content.trait_def(id) {
            rows.push(Row {
                label: vec![
                    tag("trait", DEEP_BRIGHT),
                    Span::styled(t.name.clone(), cx.roles.plain),
                ],
                detail_title: t.name.clone(),
                detail: doc_lines(cx, &t.text),
            });
        }
    }
    for id in &h.scars {
        if let Some(t) = cx.content.scar(id) {
            rows.push(Row {
                label: vec![
                    tag("scar", BLOOD),
                    Span::styled(t.name.clone(), cx.roles.plain),
                ],
                detail_title: t.name.clone(),
                detail: {
                    let mut v = doc_lines(cx, &t.text);
                    v.push(Line::styled("A scar is permanent.", cx.roles.dim));
                    v
                },
            });
        }
    }
    for (id, days) in &h.wounds {
        if let Some(w) = cx.content.wound(id) {
            rows.push(Row {
                label: vec![
                    tag(
                        if w.boon { "boon" } else { "wound" },
                        if w.boon { MOSS } else { EMBER },
                    ),
                    Span::styled(format!("{} ({days}d)", w.name), cx.roles.plain),
                ],
                detail_title: w.name.clone(),
                detail: {
                    let mut v = doc_lines(
                        cx,
                        &format!(
                            "{days} day(s) left. {}.",
                            effect_summary(cx.content, &w.effects)
                        ),
                    );
                    if !w.boon && w.scar_pct > 0 {
                        v.push(Line::styled(
                            "It may leave a scar when it heals.",
                            cx.roles.dim,
                        ));
                    }
                    v
                },
            });
        }
    }
    if rows.is_empty() {
        rows.push(Row {
            label: vec![Span::styled("No marks yet.", cx.roles.dim)],
            detail_title: "Marks".to_owned(),
            detail: doc_lines(
                cx,
                "Traits are chosen, scars are earned, wounds are suffered.",
            ),
        });
    }
    rows
}

fn moves_rows(cx: &Cx<'_>) -> Vec<Row> {
    let d = cx.game.derived(cx.content);
    let mut rows = Vec::new();
    for id in &d.moves {
        if let Some(m) = cx.content.mv(id) {
            rows.push(Row {
                label: vec![
                    Span::styled(format!("{:<20}", clip(&m.name, 19)), cx.roles.plain),
                    Span::styled(format!("{}f", m.cost), Style::new().fg(EMBER_HOT)),
                ],
                detail_title: m.name.clone(),
                detail: {
                    let mut v = doc_lines(cx, &m.text);
                    v.push(Line::styled(
                        format!(
                            "Costs {} focus. Reach: {}.",
                            m.cost,
                            if m.reach == crate::content::Reach::Ranged {
                                "any row"
                            } else {
                                "the front row"
                            }
                        ),
                        cx.roles.dim,
                    ));
                    v
                },
            });
        }
    }
    for tag in &d.tags {
        rows.push(Row {
            label: vec![
                Span::styled(format!("{:<20}", "knack"), Style::new().fg(DEEP_BRIGHT)),
                Span::styled(tag.clone(), cx.roles.plain),
            ],
            detail_title: format!("Knack: {tag}"),
            detail: doc_lines(
                cx,
                "A capability. Rooms offer options to heroes who have it; two builds see different choices in the same room.",
            ),
        });
    }
    rows
}

pub(super) fn sheet_rows(cx: &Cx<'_>, tab: usize) -> Vec<Row> {
    match tab {
        0 => stats_rows(cx),
        1 => marks_rows(cx),
        _ => moves_rows(cx),
    }
}

/// The number of rows the sheet's `tab` has.
pub(crate) fn sheet_len(cx: &Cx<'_>, tab: usize) -> usize {
    sheet_rows(cx, tab).len()
}

pub(super) const SHEET_TABS: [&str; 3] = ["Numbers", "Marks", "Moves & knacks"];

pub(super) fn draw_sheet(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect) {
    let [head, strip, tabs, body, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .areas(area);
    super::header(cx, frame, head, "Hero sheet");
    super::gauges(cx, frame, strip, super::depth_opt(cx.game));
    let tab = cx.s.ui.tab.min(SHEET_TABS.len() - 1);
    frame.render_widget(
        Tabs::new(SHEET_TABS.to_vec())
            .select(tab)
            .style(cx.roles.dim)
            .highlight_style(Style::new().fg(EMBER_HOT).add_modifier(Modifier::BOLD))
            .divider(if cx.look.ascii { "|" } else { "│" }),
        tabs,
    );
    let rows = sheet_rows(cx, tab);
    list_detail(cx, frame, body, SHEET_TABS[tab], &rows, cx.s.ui.cursor);
    frame.render_widget(
        Keys {
            chips: &[
                Chip::new("Up/Down", "choose"),
                Chip::new("Left/Right", "tab"),
                Chip::new("Esc", "back"),
            ],
            roles: cx.roles,
        },
        keys,
    );
}

/// The skill rows: one per node of the hero's tree, in tree order.
pub(super) fn skill_rows(cx: &Cx<'_>) -> Vec<Row> {
    let h = &cx.game.hero;
    let mut rows = Vec::new();
    for (i, id) in cx.s.tree(&h.calling).iter().enumerate() {
        let Some(n) = cx.content.node(id) else {
            continue;
        };
        let key = NODE_KEYS.chars().nth(i).unwrap_or('?');
        let taken = h.nodes.contains(id);
        let locked = h.node_locked(cx.content, id);
        let (state, color) = if taken {
            ("taken", MOSS)
        } else if let Some(why) = locked {
            if why.contains("closed") {
                ("closed", BLOOD)
            } else {
                ("locked", STONE)
            }
        } else {
            ("ready", EMBER_HOT)
        };
        let summary = effect_summary(cx.content, &n.effects);
        rows.push(Row {
            label: vec![
                Span::styled(format!("[{key}] "), cx.roles.accent),
                Span::styled(format!("T{} ", n.tier), cx.roles.dim),
                Span::styled(
                    format!("{:<20}", clip(&n.name, 19)),
                    if taken {
                        Style::new().fg(MOSS)
                    } else if locked.is_some() {
                        cx.roles.dim
                    } else {
                        cx.roles.plain
                    },
                ),
                Span::styled(format!(" {state}"), Style::new().fg(color)),
            ],
            detail_title: n.name.clone(),
            detail: {
                let mut v = doc_lines(cx, &n.text);
                v.push(Line::styled(summary, cx.roles.strong));
                let named = |ids: &[String]| -> String {
                    ids.iter()
                        .map(|r| {
                            cx.content
                                .node(r)
                                .map_or(r.as_str(), |x| x.name.as_str())
                                .to_owned()
                        })
                        .collect::<Vec<_>>()
                        .join(" or ")
                };
                if !n.requires.is_empty() {
                    v.push(Line::styled(
                        format!("Needs {}.", named(&n.requires)),
                        cx.roles.dim,
                    ));
                }
                if !n.excludes.is_empty() {
                    v.push(Line::styled(
                        format!("Closes for good: {}.", named(&n.excludes)),
                        Style::new().fg(BLOOD),
                    ));
                }
                v.push(Line::styled(
                    format!("Level {} needed.", 2 * n.tier - 1),
                    cx.roles.dim,
                ));
                let status = if taken {
                    "You have this.".to_owned()
                } else if let Some(why) = h.node_refusal(cx.content, id) {
                    why.to_owned()
                } else {
                    "Press Enter to take it.".to_owned()
                };
                v.push(Line::styled(status, Style::new().fg(color)));
                v
            },
        });
    }
    rows
}

pub(super) fn draw_skills(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect) {
    let h = &cx.game.hero;
    let [head, strip, body, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(5),
        Constraint::Length(2),
    ])
    .areas(area);
    super::header(cx, frame, head, "Skills");
    let [s1, s2] = [
        Rect::new(strip.x, strip.y, strip.width, 1),
        Rect::new(strip.x, strip.y + 1, strip.width, 1),
    ];
    let lo = xp_for_level(h.level);
    let hi = xp_for_level(h.level.saturating_add(1));
    let xp_text = if h.level >= LEVEL_MAX {
        "max level".to_owned()
    } else {
        format!("{}/{hi}xp to level {}", h.xp, h.level + 1)
    };
    frame.render_widget(
        Meter::heated(
            "Level",
            frac(
                i64::from(h.xp.saturating_sub(lo)),
                i64::from(hi.saturating_sub(lo)),
            ),
            xp_text,
            0.4,
            0.88,
        )
        .labelled(7, cx.roles.dim, cx.roles.plain),
        Rect::new(s1.x, s1.y, s1.width.min(56), 1),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Points  ", cx.roles.dim),
            Span::styled(
                format!("{} skill", h.skill_points),
                if h.skill_points > 0 {
                    cx.roles.good
                } else {
                    cx.roles.plain
                },
            ),
            Span::styled(" · ", cx.roles.dim),
            Span::styled(
                format!("{} attribute", h.attr_points),
                if h.attr_points > 0 {
                    cx.roles.good
                } else {
                    cx.roles.plain
                },
            ),
            Span::styled(
                "   A skill you take can close others for good.",
                cx.roles.dim,
            ),
        ])),
        s2,
    );
    let rows = skill_rows(cx);
    list_detail(cx, frame, body, "Skill tree", &rows, cx.s.ui.cursor);
    let mut chips = vec![Chip::new("Enter", "take"), Chip::new("Up/Down", "choose")];
    if h.attr_points > 0 {
        chips.push(Chip::new("M F W S", "raise might, finesse, wits, spirit"));
    } else {
        chips.push(Chip::new("M F W S", "raise attribute").off());
    }
    chips.push(Chip::new("Esc", "back"));
    frame.render_widget(
        Keys {
            chips: &chips,
            roles: cx.roles,
        },
        keys,
    );
}

/// A narrow card of the hero for wide windows: who, what they wield, what ails them.
pub(super) fn card(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect) {
    let game = cx.game;
    let h = &game.hero;
    frame.render_widget(Clear, area);
    frame.render_widget(panel("Hero", false), area);
    let body = inner(area);
    let d = game.derived(cx.content);
    let mut lines: Vec<Line<'static>> = vec![
        Line::styled(
            format!(
                "{} the {}",
                h.name,
                cx.content
                    .calling(&h.calling)
                    .map_or("hero", |c| c.name.as_str())
                    .to_lowercase()
            ),
            cx.roles.strong,
        ),
        Line::styled(format!("Level {}  ·  {} xp", h.level, h.xp), cx.roles.dim),
        Line::raw(""),
    ];
    for (what, base, kit) in [("Hand", &h.weapon, &h.wkit), ("Body", &h.armour, &h.akit)] {
        lines.push(Line::from(vec![
            Span::styled(format!("{what} "), cx.roles.dim),
            Span::styled(
                clip(
                    &item_name(cx.content, base, kit),
                    usize::from(body.width).saturating_sub(6),
                ),
                cx.roles.plain,
            ),
        ]));
        lines.push(Line::styled(
            format!("     wear {}%", kit.wear),
            Style::new().fg(if kit.wear >= 60 { BLOOD } else { STONE }),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!("Armor {}  Dmg +{}", d.armor, d.damage),
        cx.roles.plain,
    ));
    lines.push(Line::styled(
        format!("Coin {}  Salvage {}", h.coin, h.salvage),
        cx.roles.plain,
    ));
    for (id, days) in &h.wounds {
        if let Some(w) = cx.content.wound(id) {
            lines.push(Line::styled(
                format!("{} ({days}d)", w.name),
                Style::new().fg(if w.boon { MOSS } else { BLOOD }),
            ));
        }
    }
    let hpf = frac(i64::from(h.hp.max(0)), i64::from(d.max_hp));
    let _ = (BONE, STONE_DARK, vitality(hpf));
    frame.render_widget(Paragraph::new(lines), body);
}
