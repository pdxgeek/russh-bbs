//! The skirmish panel: three rows a side, the initiative order, what each foe means to do,
//! and health gauges, with the party's morale. The orders are the engine's (`Fight`), shown
//! as verbs with the keys that aim them, and the keys also stand on the cards they aim at.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::Paragraph;

use super::{Size, Ui, fit};
use crate::combat::{Fight, Odds, Side, Unit};
use crate::lore::Behaviour;

/// What a foe means to do, in a few words.
pub(super) const fn intent(b: Behaviour) -> &'static str {
    match b {
        Behaviour::Bruiser => "hits the front",
        Behaviour::Lurker => "goes for the weakest",
        Behaviour::Hurler => "hurls at the weakest",
        Behaviour::Skittish => "hits, then bolts",
        Behaviour::Swarm => "swarms",
    }
}

/// A name that tells units of one kind apart: "Bog-hound 2".
fn label(fight: &Fight, i: usize) -> String {
    let u = &fight.units[i];
    let name = if u.side == Side::Party {
        super::short_name(&u.name).to_owned()
    } else {
        u.name.clone()
    };
    let same: Vec<usize> = fight
        .units
        .iter()
        .enumerate()
        .filter(|(_, o)| o.side == u.side && o.name == u.name)
        .map(|(n, _)| n)
        .collect();
    if same.len() > 1 {
        let nth = same.iter().position(|&n| n == i).unwrap_or(0) + 1;
        format!("{name} {nth}")
    } else {
        name
    }
}

fn hp_style(ui: &Ui<'_>, u: &Unit) -> Style {
    let pct = i32::from(u.hp.max(0)) * 100 / i32::from(u.max_hp.max(1));
    if pct > 60 {
        ui.pal.moss()
    } else if pct > 30 {
        ui.pal.gold()
    } else {
        ui.pal.rust()
    }
}

/// The badges on a unit: braced, netted, hexed, guarded, exposed, special cooling.
fn badges(u: &Unit) -> String {
    let mut out = Vec::new();
    if u.braced {
        out.push("brace".to_owned());
    }
    if u.netted > 0 {
        out.push(format!("net{}", u.netted));
    }
    if u.hexed > 0 {
        out.push(format!("hex{}", u.hexed));
    }
    if u.guarded > 0 {
        out.push(format!("guard{}", u.guarded));
    }
    if u.exposed {
        out.push("open".to_owned());
    }
    if u.cool > 0 {
        out.push(format!("wait{}", u.cool));
    }
    out.join(" ")
}

/// The key of the first order aimed at unit `i`.
fn key_for(ui: &Ui<'_>, i: usize) -> Option<char> {
    ["strike", "throw", "special", "mend", "shove"]
        .iter()
        .find_map(|v| ui.entry(&format!("{v}:{i}")))
        .map(|(k, _)| Ui::key_of(k))
}

fn card(ui: &Ui<'_>, fight: &Fight, i: usize, u: &Unit, w: u16) -> Vec<Line<'static>> {
    let p = &ui.pal;
    let active = fight
        .order
        .get(usize::from(fight.cursor))
        .is_some_and(|&slot| usize::from(slot) == i);
    let mark = if active { ui.look.g("▶", ">") } else { " " };
    let name_style = if u.side == Side::Party {
        p.salt()
    } else {
        p.rust()
    };
    let mut l1 = vec![
        Span::styled(mark, p.gold()),
        Span::styled(
            fit(&label(fight, i), usize::from(w).saturating_sub(1)),
            if active {
                name_style.add_modifier(Modifier::BOLD)
            } else {
                name_style
            },
        ),
    ];
    if u.downed() {
        l1.push(Span::styled(" down", p.dim));
    }
    let hp = u.hp.max(0);
    let tail = format!(" {hp}/{}", u.max_hp);
    let bar_w = w
        .saturating_sub(u16::try_from(tail.len()).unwrap_or(6))
        .max(3);
    let mut l2 = ui.bar(
        u32::try_from(hp).unwrap_or(0),
        u32::try_from(u.max_hp.max(1)).unwrap_or(1),
        bar_w,
        hp_style(ui, u),
    );
    l2.push(Span::styled(tail, p.dim));
    let status = badges(u);
    let what = if !status.is_empty() {
        status
    } else if u.side == Side::Foe {
        intent(u.behaviour).to_owned()
    } else {
        u.trade.map_or(String::new(), |t| t.name().to_owned())
    };
    let mut l3: Vec<Span<'static>> = Vec::new();
    let key = key_for(ui, i);
    if let Some(k) = key {
        l3.push(Span::styled(
            format!("[{k}]"),
            if ui
                .entries
                .get(ui.s.cursor)
                .is_some_and(|e| e.tag.ends_with(&format!(":{i}")))
            {
                p.selected
            } else {
                p.accent
            },
        ));
    }
    let room = usize::from(w).saturating_sub(if key.is_some() { 3 } else { 0 });
    l3.push(Span::styled(
        fit(&what, room),
        if u.side == Side::Party {
            p.peat()
        } else {
            p.dim
        },
    ));
    vec![Line::from(l1), Line::from(l2), Line::from(l3)]
}

fn column(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, fight: &Fight, side: Side, row: u8) {
    let p = &ui.pal;
    let title = ["front", "middle", "back"][usize::from(row)];
    let block = ui.panel(title).border_style(if side == Side::Party {
        p.moss()
    } else {
        p.rust()
    });
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut hidden = 0;
    let per = 3usize;
    let fits = usize::from(inner.height) / per;
    for (i, u) in fight.units.iter().enumerate() {
        if u.side != side || u.gone || u.row != row {
            continue;
        }
        if lines.len() / per >= fits {
            hidden += 1;
            continue;
        }
        lines.extend(card(ui, fight, i, u, inner.width));
    }
    if hidden > 0 {
        lines.push(Line::styled(format!("+{hidden} more"), p.dim));
    }
    if lines.is_empty() {
        lines.push(Line::styled(ui.look.g("·", "."), p.slate()));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn order_strip(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, fight: &Fight) {
    let p = &ui.pal;
    let odds = fight.odds();
    let (odds_style, odds_mark) = match odds {
        Odds::Easy => (p.moss(), "++"),
        Odds::Fair => (p.sea(), "+"),
        Odds::Hard => (p.gold(), "="),
        Odds::Grim => (p.rust(), "--"),
    };
    let mut head = vec![
        Span::styled(format!(" {} ", ui.look.g("x", "x")), p.rust()),
        Span::styled(
            format!("SKIRMISH, round {}", fight.round),
            p.salt().add_modifier(Modifier::BOLD),
        ),
        Span::styled("   odds ", p.dim),
        Span::styled(format!("{odds_mark} {}", odds.word()), odds_style),
        Span::styled(
            if fight.wet {
                format!("   {} knee-deep", ui.look.g("≈", "~"))
            } else {
                "   solid ground".to_owned()
            },
            if fight.wet { p.sea() } else { p.dim },
        ),
    ];
    if fight.limit > 0 {
        head.push(Span::styled(
            format!("   water takes the field at round {}", fight.limit),
            if fight.round + 1 >= fight.limit {
                p.rust()
            } else {
                p.dim
            },
        ));
    }
    head.push(Span::styled("   morale ", p.dim));
    head.extend(ui.bar(
        u32::from(ui.clan.morale),
        100,
        8,
        if ui.clan.morale < 35 {
            p.rust()
        } else {
            p.moss()
        },
    ));
    let mut spans: Vec<Span<'static>> = vec![Span::styled(" order ", p.dim)];
    for (n, &slot) in fight.order.iter().enumerate() {
        let i = usize::from(slot);
        let Some(u) = fight.units.get(i) else {
            continue;
        };
        if !u.alive() {
            continue;
        }
        let now = n == usize::from(fight.cursor);
        let base = if u.side == Side::Party {
            p.moss()
        } else {
            p.rust()
        };
        let style = if now { p.selected } else { base };
        spans.push(Span::styled(
            format!(
                "{}{}",
                if now { ui.look.g("▶", ">") } else { "" },
                fit(&label(fight, i), 12)
            ),
            style,
        ));
        spans.push(Span::styled(ui.look.g(" › ", " > "), p.slate()));
    }
    f.render_widget(
        Paragraph::new(vec![Line::from(head), Line::from(spans)]),
        area,
    );
}

/// The orders as verbs: each with the units it can be aimed at and the key for each.
fn verbs(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, fight: &Fight, actor: &str) {
    let p = &ui.pal;
    let block = ui.panel(&format!("{} to act", fit(actor, 20)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (verb, word) in [
        ("strike", "Strike"),
        ("special", "Special"),
        ("throw", "Throw"),
        ("shove", "Shove"),
        ("mend", "Mend"),
    ] {
        let prefix = format!("{verb}:");
        let mut spans: Vec<Span<'static>> = vec![Span::styled(format!("{word:<8}"), p.dim)];
        let mut any = false;
        for (k, e) in ui.entries_like(&[prefix.as_str()]) {
            let target = e.tag[prefix.len()..].parse::<usize>().ok();
            let name = target.map_or(String::new(), |i| label(fight, i));
            let style = if k == ui.s.cursor {
                p.selected
            } else {
                p.plain
            };
            spans.push(Span::styled(
                format!("[{}]", Ui::key_of(k)),
                if k == ui.s.cursor {
                    p.selected
                } else {
                    p.accent
                },
            ));
            spans.push(Span::styled(format!("{} ", fit(&name, 12)), style));
            any = true;
        }
        if any {
            lines.push(Line::from(spans));
        }
    }
    // The orders that need no aim.
    let mut plain: Vec<Span<'static>> = vec![Span::styled("Or       ", p.dim)];
    for (tag, word) in [
        ("special", "special on the line"),
        ("brace", "brace"),
        ("flee", "flee"),
        ("quick", "quick-resolve"),
    ] {
        if let Some((k, _)) = ui.entry(tag) {
            plain.push(Span::styled(
                format!("[{}]", Ui::key_of(k)),
                if k == ui.s.cursor {
                    p.selected
                } else {
                    p.accent
                },
            ));
            plain.push(Span::styled(
                format!("{word}  "),
                if k == ui.s.cursor {
                    p.selected
                } else {
                    p.plain
                },
            ));
        }
    }
    lines.push(Line::from(plain));
    f.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, fight: &Fight) {
    let roomy = ui.look.size >= Size::Roomy;
    let orders_h = if roomy { 8 } else { 6 };
    let [strip, field, orders] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(7),
        Constraint::Length(orders_h),
    ])
    .areas(area);
    order_strip(ui, f, strip, fight);
    let cols = Layout::horizontal([
        Constraint::Ratio(4, 26),
        Constraint::Ratio(4, 26),
        Constraint::Ratio(5, 26),
        Constraint::Ratio(5, 26),
        Constraint::Ratio(4, 26),
        Constraint::Ratio(4, 26),
    ])
    .split(field);
    // Your line stands back to front, the foes front to back, so the fronts face each other.
    for (col, (side, row)) in cols.iter().zip([
        (Side::Party, 2),
        (Side::Party, 1),
        (Side::Party, 0),
        (Side::Foe, 0),
        (Side::Foe, 1),
        (Side::Foe, 2),
    ]) {
        column(ui, f, *col, fight, side, row);
    }
    let actor = fight
        .order
        .get(usize::from(fight.cursor))
        .and_then(|&s| fight.units.get(usize::from(s)))
        .map_or("the field", |u| u.name.as_str());
    verbs(ui, f, orders, fight, actor);
}
