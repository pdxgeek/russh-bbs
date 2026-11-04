//! The town's places: the market, gear, forge, temple, guild hall, tavern, library and the town
//! works. Each is a main panel (a table, gauges, a sparkline) beside a detail pane that previews
//! the cost of whatever the cursor is on, over the same key chips the menu answers. The menu
//! (`Session::spot_view`) is still the single source of what can be done; this file only draws it.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Cell, Clear, Paragraph, Row, Sparkline, Table};

use super::Cx;
use super::art::{Vignette, flame_rows};
use super::kit::{
    BLOOD, BONE, Chip, DEEP_BRIGHT, EMBER, EMBER_DIM, EMBER_HOT, Keys, MOSS, Meter, STONE_DARK,
    clip, frac, heat, inner, panel, pips, wrap_to,
};
use crate::content::RoomKind;
use crate::game::{Action, Game};
use crate::rules::{DEBT_LIMIT, TRAIN_LEVEL_MAX, gear_level, xp_for_level};
use crate::session::places::{Entry, Spot};
use crate::town::{TownAct, Wares, Worn, base_band, item_name, repair_cost};

fn title_of(spot: Spot) -> (&'static str, &'static str) {
    match spot {
        Spot::Tavern => ("The Tavern", "the Banked Ember"),
        Spot::Market => ("The Market", "stalls under the hill"),
        Spot::Gear => ("Gear", "what you wield and what you keep"),
        Spot::Forge => ("The Forge", "where the old fires still serve"),
        Spot::Temple => ("The Temple", "of the Low Flame"),
        Spot::Guild => ("The Guild Hall", "drills and old hands"),
        Spot::Library => ("The Library", "dust, relics and patterns"),
        Spot::Works => ("The Town Works", "what Cindermere builds together"),
    }
}

pub(super) fn draw(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, spot: Spot) {
    let game = cx.game;
    let (_, entries) = cx.s.spot_view(game, spot);
    let cursor = cx.s.ui.cursor.min(entries.len().saturating_sub(1));
    let (name, tagline) = title_of(spot);
    let mut chips: Vec<Chip> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let label = match &e.action {
                Action::Town(TownAct::Donate {
                    project,
                    coin,
                    salvage,
                }) => {
                    let name = cx
                        .content
                        .project(project)
                        .map_or(project.as_str(), |p| p.name.as_str());
                    if *coin > 0 {
                        format!("give 25c: {name}")
                    } else {
                        format!("give {salvage}s: {name}")
                    }
                }
                _ => e.label.clone(),
            };
            let mut c = Chip::new(e.key.to_string(), clip(&label, 28));
            c.hot = i == cursor;
            c
        })
        .collect();
    chips.push(Chip::new("Esc", "back"));
    let key_rows = Keys::rows(&chips, area.width).clamp(1, 3);
    let [head, res, body, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(key_rows),
    ])
    .areas(area);
    super::header(cx, frame, head, name);
    let h = &game.hero;
    let purse = format!(
        "Coin {}  Salvage {}  Store {}  Turns {}/25",
        h.coin, h.salvage, h.stash, h.turns
    );
    let room = usize::from(res.width).saturating_sub(purse.chars().count() + 3);
    let mut line = Vec::new();
    if tagline.chars().count() <= room {
        line.push(Span::styled(format!("{tagline}   "), cx.roles.dim));
    }
    line.push(Span::styled(purse, Style::new().fg(EMBER_HOT)));
    frame.render_widget(Paragraph::new(Line::from(line)), res);
    let [main, side] =
        Layout::horizontal([Constraint::Percentage(57), Constraint::Fill(1)]).areas(body);
    frame.render_widget(Clear, main);
    frame.render_widget(panel(name, true), main);
    let mb = inner(main);
    let selected = entries.get(cursor);
    match spot {
        Spot::Market => market(cx, frame, mb, game, &entries, selected),
        Spot::Gear => gear(cx, frame, mb, game, &entries, selected),
        Spot::Forge => forge(cx, frame, mb, game, &entries, selected),
        Spot::Temple => temple(cx, frame, mb, game),
        Spot::Guild => guild(cx, frame, mb, game),
        Spot::Tavern => tavern(cx, frame, mb, game, &entries, selected),
        Spot::Library => library(cx, frame, mb, game),
        Spot::Works => works(cx, frame, mb, game, selected),
    }
    // Atmosphere in whatever height the panel did not use.
    ambient(cx, frame, mb, spot);
    frame.render_widget(Clear, side);
    frame.render_widget(panel("Preview", false), side);
    let sb = inner(side);
    let (title, lines) = match selected {
        Some(e) => detail(cx, game, e, usize::from(sb.width)),
        None => (
            "Nothing to do".to_owned(),
            vec![Line::styled(
                "Nothing here is open to you today.",
                cx.roles.dim,
            )],
        ),
    };
    let mut all = vec![Line::styled(
        clip(&title, usize::from(sb.width)),
        Style::new().fg(EMBER).add_modifier(Modifier::BOLD),
    )];
    all.extend(lines);
    frame.render_widget(
        Paragraph::new(all).wrap(russh_bbs_door::widgets::Wrap { trim: false }),
        sb,
    );
    frame.render_widget(
        Keys {
            chips: &chips,
            roles: cx.roles,
        },
        keys,
    );
}

fn sel_style(on: bool) -> Style {
    if on {
        Style::new().fg(EMBER_HOT).add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    }
}

fn plain_table<'a>(
    cx: &Cx<'_>,
    header: &[&'a str],
    widths: &[Constraint],
    rows: Vec<Row<'a>>,
) -> Table<'a> {
    Table::new(rows, widths.to_vec())
        .header(
            Row::new(header.iter().map(|h| Cell::from(*h)).collect::<Vec<_>>())
                .style(cx.roles.dim.add_modifier(Modifier::UNDERLINED)),
        )
        .column_spacing(1)
}

fn money(coin: u32, cost: u32) -> Span<'static> {
    Span::styled(
        format!("Coin {coin} -> {}", i64::from(coin) - i64::from(cost)),
        Style::new().fg(if coin >= cost { MOSS } else { BLOOD }),
    )
}

/// Short stats for a table cell: `5-9 Wit` (and `r` for ranged), or `arm 2`.
fn stats_of(cx: &Cx<'_>, base: &str) -> String {
    if let Some(w) = cx.content.weapon(base) {
        let attr: String = w.attr.label().chars().take(3).collect();
        format!(
            "{}-{} {attr}{}",
            w.damage[0],
            w.damage[1],
            if w.reach == crate::content::Reach::Ranged {
                " r"
            } else {
                ""
            }
        )
    } else if let Some(a) = cx.content.armour(base) {
        format!("arm {}", a.armor)
    } else {
        String::new()
    }
}

/// The key a menu hands the entry that matches `f`, or `-` when there is none (a sold stall).
fn key_of(entries: &[Entry], f: impl Fn(&TownAct) -> bool) -> String {
    entries
        .iter()
        .find(|e| matches!(&e.action, Action::Town(a) if f(a)))
        .map_or_else(|| "-".to_owned(), |e| e.key.to_string())
}

/// A few rows of scenery under a place's table, where the height allows.
fn ambient(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, spot: Spot) {
    let rows = area.height.min(3);
    if area.height < 12 || rows == 0 {
        return;
    }
    let kind = match spot {
        Spot::Temple => RoomKind::Shrine,
        Spot::Tavern | Spot::Guild => RoomKind::Forge,
        Spot::Library => RoomKind::Vault,
        Spot::Gear | Spot::Forge => RoomKind::Forge,
        Spot::Market | Spot::Works => return,
    };
    frame.render_widget(
        Vignette {
            kind,
            look: cx.look,
            warmth: 0.8,
            seed: 17,
        },
        Rect::new(area.x, area.bottom() - rows, area.width, rows),
    );
}

fn market(
    cx: &Cx<'_>,
    frame: &mut DoorFrame<'_>,
    area: Rect,
    game: &Game,
    entries: &[Entry],
    sel: Option<&Entry>,
) {
    let c = cx.content;
    let stock = game.stock(c);
    let rows: Vec<Row<'_>> = stock
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_sel = matches!(sel.map(|e| &e.action), Some(Action::Town(TownAct::BuyStock(n))) if *n == i);
            let sold = game.today.bought.contains(&u8::try_from(i).unwrap_or(u8::MAX));
            let (name, stats, lv) = match &s.wares {
                Wares::Gear(g) => (
                    item_name(c, &g.base, &g.kit),
                    stats_of(cx, &g.base),
                    gear_level(base_band(c, &g.base)).to_string(),
                ),
                Wares::Core(id) => (
                    c.core(id).map_or_else(String::new, |k| k.name.clone()),
                    "core".to_owned(),
                    "-".to_owned(),
                ),
            };
            Row::new([
                Cell::from(format!(
                    "[{}] {}",
                    key_of(entries, |a| matches!(a, TownAct::BuyStock(n) if *n == i)),
                    clip(&name, 24)
                )),
                Cell::from(clip(&stats, 9)),
                Cell::from(lv),
                Cell::from(if sold { "SOLD".to_owned() } else { format!("{}c", s.price) }),
            ])
            .style(if sold { cx.roles.dim } else { sel_style(is_sel) })
        })
        .collect();
    let n = u16::try_from(rows.len()).unwrap_or(0);
    let table_h = (n + 1).min(area.height);
    frame.render_widget(
        plain_table(
            cx,
            &["Wares", "Stats", "Lv", "Price"],
            &[
                Constraint::Fill(1),
                Constraint::Length(9),
                Constraint::Length(2),
                Constraint::Length(5),
            ],
            rows,
        ),
        Rect::new(area.x, area.y, area.width, table_h),
    );
    // The shelf's price over the last two weeks: it follows the town (the deepest hero in the hall
    // and the market's level) as the days pass.
    let spark_y = area.y + table_h + 1;
    if spark_y + 2 <= area.bottom() {
        let history = price_history(game, c);
        let (lo, hi) = (
            history.iter().min().copied().unwrap_or(0),
            history.iter().max().copied().unwrap_or(0),
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Shelf price, 14 days ", cx.roles.dim),
                Span::styled(format!("{lo}c - {hi}c"), cx.roles.plain),
            ])),
            Rect::new(area.x, spark_y, area.width, 1),
        );
        frame.render_widget(
            Sparkline::default()
                .data(&history)
                .style(Style::new().fg(EMBER)),
            Rect::new(
                area.x,
                spark_y + 1,
                area.width.min(28),
                (area.bottom() - spark_y - 1).min(3),
            ),
        );
    }
}

/// The average asking price of the shelf on each of the last fourteen days (the market is a pure
/// function of the day, so this asks the engine for each day rather than inventing a curve).
fn price_history(game: &Game, content: &crate::content::Content) -> Vec<u64> {
    (0..14)
        .rev()
        .map(|back| {
            let mut g = game.clone();
            g.day -= back;
            let stock = g.stock(content);
            let n = u64::try_from(stock.len()).unwrap_or(0).max(1);
            stock.iter().map(|s| u64::from(s.price)).sum::<u64>() / n
        })
        .collect()
}

fn wear_text(cx: &Cx<'_>, wear: u8) -> String {
    let full = usize::from(100u8.saturating_sub(wear)) * 4 / 100;
    let (on, off) = if cx.look.ascii {
        ('#', '.')
    } else {
        ('█', '░')
    };
    let mut s = String::new();
    s.extend(std::iter::repeat_n(on, full));
    s.extend(std::iter::repeat_n(off, 4 - full));
    format!("{s} {wear}%")
}

fn gear(
    cx: &Cx<'_>,
    frame: &mut DoorFrame<'_>,
    area: Rect,
    game: &Game,
    entries: &[Entry],
    sel: Option<&Entry>,
) {
    let c = cx.content;
    let h = &game.hero;
    let sel_pack = match sel.map(|e| &e.action) {
        Some(Action::Town(TownAct::Equip(i) | TownAct::Sell(i))) => Some(*i),
        _ => None,
    };
    let mut rows: Vec<Row<'_>> = Vec::new();
    for (slot, base, kit) in [("Hand", &h.weapon, &h.wkit), ("Body", &h.armour, &h.akit)] {
        rows.push(Row::new([
            Cell::from(slot),
            Cell::from(clip(&item_name(c, base, kit), 20)),
            Cell::from(clip(&stats_of(cx, base), 9)),
            Cell::from(wear_text(cx, kit.wear)),
        ]));
    }
    for (i, it) in h.pack.iter().enumerate() {
        rows.push(
            Row::new([
                Cell::from(format!(
                    "[{}]",
                    key_of(entries, |a| matches!(a, TownAct::Equip(n) if *n == i))
                )),
                Cell::from(clip(&item_name(c, &it.base, &it.kit), 20)),
                Cell::from(clip(&stats_of(cx, &it.base), 9)),
                Cell::from(wear_text(cx, it.kit.wear)),
            ])
            .style(sel_style(sel_pack == Some(i))),
        );
    }
    for it in &h.found {
        rows.push(
            Row::new([
                Cell::from("pack"),
                Cell::from(clip(&item_name(c, &it.base, &it.kit), 20)),
                Cell::from(clip(&stats_of(cx, &it.base), 9)),
                Cell::from("carried"),
            ])
            .style(cx.roles.dim),
        );
    }
    for id in &h.cores {
        rows.push(
            Row::new([
                Cell::from("core"),
                Cell::from(c.core(id).map_or("?".to_owned(), |k| k.name.clone())),
                Cell::from("loose"),
                Cell::from(""),
            ])
            .style(Style::new().fg(DEEP_BRIGHT)),
        );
    }
    frame.render_widget(
        plain_table(
            cx,
            &["", "Piece", "Stats", "Wear"],
            &[
                Constraint::Length(4),
                Constraint::Fill(1),
                Constraint::Length(8),
                Constraint::Length(9),
            ],
            rows,
        ),
        area,
    );
    let d = game.derived(c);
    let totals_y = area.y + 9.min(area.height.saturating_sub(2));
    if totals_y + 1 < area.bottom() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                format!(
                    "Armor {}  Damage +{}  Crit {}%  Evade {}%",
                    d.armor, d.damage, d.crit, d.evade
                ),
                cx.roles.strong,
            )),
            Rect::new(area.x, totals_y, area.width, 1),
        );
    }
    let y = area.bottom().saturating_sub(1);
    if !h.found.is_empty() && y > area.y + 6 {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "Carried gear is lost if you die; it is banked on arrival.",
                cx.roles.dim,
            )),
            Rect::new(area.x, y, area.width, 1),
        );
    }
}

fn forge(
    cx: &Cx<'_>,
    frame: &mut DoorFrame<'_>,
    area: Rect,
    game: &Game,
    entries: &[Entry],
    sel: Option<&Entry>,
) {
    let c = cx.content;
    let h = &game.hero;
    let d = game.derived(c);
    let fx = game.town_fx(c);
    let fl = fx.forge.max(1);
    let mut y = area.y;
    frame.render_widget(
        Paragraph::new(Line::from({
            let mut v = vec![
                Span::styled("Forge level ", cx.roles.dim),
                Span::styled(format!("{fl}"), cx.roles.strong),
                Span::raw("  "),
            ];
            v.extend(pips(fl.min(4), 4, EMBER));
            v
        })),
        Rect::new(area.x, y, area.width, 1),
    );
    y += 1;
    // Repairs.
    for (worn, base, kit) in [
        (Worn::Weapon, &h.weapon, &h.wkit),
        (Worn::Armour, &h.armour, &h.akit),
    ] {
        let cost = repair_cost(kit.wear, base_band(c, base), fl, d.has_tag("smithcraft"));
        let is_sel =
            matches!(sel.map(|e| &e.action), Some(Action::Town(TownAct::Repair(w))) if *w == worn);
        let health = frac(i64::from(100 - kit.wear.min(100)), 100);
        let [a, b] = Layout::horizontal([Constraint::Percentage(55), Constraint::Fill(1)])
            .areas(Rect::new(area.x, y, area.width, 1));
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{} ", worn.word()), sel_style(is_sel)),
                Span::styled(
                    clip(
                        &item_name(c, base, kit),
                        usize::from(a.width).saturating_sub(8),
                    ),
                    cx.roles.plain,
                ),
            ])),
            a,
        );
        frame.render_widget(
            Meter::vital(
                "",
                health,
                if cost == 0 {
                    "sound".to_owned()
                } else {
                    format!("{cost}c")
                },
            ),
            b,
        );
        y += 1;
    }
    y += 1;
    // Recipes.
    let mut rows: Vec<Row<'_>> = Vec::new();
    for r in &c.recipes.recipe {
        if !h.knows(r) {
            continue;
        }
        let (salvage, coin) = Game::craft_cost(r, &d);
        let ready = game.craft_refusal(c, &r.id).is_none();
        let is_sel =
            matches!(sel.map(|e| &e.action), Some(Action::Town(TownAct::Craft(id))) if *id == r.id);
        rows.push(
            Row::new([
                Cell::from(format!(
                    "[{}] {}",
                    key_of(entries, |a| matches!(a, TownAct::Craft(id) if *id == r.id)),
                    clip(&r.name, 22)
                )),
                Cell::from(format!("{salvage}s")),
                Cell::from(format!("{coin}c")),
                Cell::from(format!("F{}", r.forge)),
                Cell::from(if ready { "ready" } else { "not yet" }),
            ])
            .style(if is_sel {
                sel_style(true)
            } else if ready {
                cx.roles.plain
            } else {
                cx.roles.dim
            }),
        );
    }
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "No recipes yet: find patterns below.",
                cx.roles.dim,
            )),
            Rect::new(area.x, y, area.width, 1),
        );
    } else if y < area.bottom() {
        frame.render_widget(
            plain_table(
                cx,
                &["Recipe", "Salv", "Coin", "Fg", ""],
                &[
                    Constraint::Fill(1),
                    Constraint::Length(4),
                    Constraint::Length(5),
                    Constraint::Length(3),
                    Constraint::Length(7),
                ],
                rows,
            ),
            Rect::new(area.x, y, area.width, area.bottom() - y),
        );
    }
}

fn temple(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, game: &Game) {
    let c = cx.content;
    let h = &game.hero;
    let d = game.derived(c);
    for (i, row) in flame_rows(&cx.look).iter().enumerate() {
        let Ok(dy) = u16::try_from(i) else { break };
        frame.render_widget(
            Paragraph::new(Line::styled(
                *row,
                Style::new().fg(heat(0.9 - 0.2 * f32::from(dy))),
            )),
            Rect::new(area.x + 1, area.y + dy, 4.min(area.width), 1),
        );
    }
    let fx = game.town_fx(c);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(format!("Temple level {}", fx.temple), cx.roles.strong),
            Line::styled(
                format!("Being tended costs {} turn(s).", game.heal_turns(c)),
                cx.roles.dim,
            ),
        ]),
        Rect::new(area.x + 6, area.y, area.width.saturating_sub(6), 2),
    );
    let hpf = frac(i64::from(h.hp.max(0)), i64::from(d.max_hp));
    frame.render_widget(
        Meter::vital("Health", hpf, format!("{}/{}", h.hp.max(0), d.max_hp)).labelled(
            8,
            cx.roles.dim,
            cx.roles.plain,
        ),
        Rect::new(area.x, area.y + 4, area.width, 1),
    );
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (id, days) in &h.wounds {
        if let Some(w) = c.wound(id) {
            lines.push(Line::from(vec![
                Span::styled(
                    if cx.look.ascii { "+ " } else { "✚ " },
                    Style::new().fg(if w.boon { MOSS } else { BLOOD }),
                ),
                Span::styled(
                    format!(
                        "{} ({days}d){}",
                        w.name,
                        if w.boon { " - a boon" } else { "" }
                    ),
                    cx.roles.plain,
                ),
            ]));
        }
    }
    if lines.is_empty() {
        lines.push(Line::styled("No wounds.", cx.roles.dim));
    }
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(
            area.x,
            area.y + 6,
            area.width,
            area.height.saturating_sub(6),
        ),
    );
}

fn guild(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, game: &Game) {
    let h = &game.hero;
    let fx = game.town_fx(cx.content);
    let lo = xp_for_level(h.level);
    let hi = xp_for_level(h.level.saturating_add(1));
    frame.render_widget(
        Meter::heated(
            "Level",
            frac(
                i64::from(h.xp.saturating_sub(lo)),
                i64::from(hi.saturating_sub(lo)),
            ),
            format!("{}/{}xp", h.xp, hi),
            0.4,
            0.88,
        )
        .labelled(7, cx.roles.dim, cx.roles.plain),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let allowed = 1 + fx.guild;
    let mut drills = vec![Span::styled("Drills today ", cx.roles.dim)];
    drills.extend(pips(
        u32::from(game.today.trained).min(allowed),
        allowed,
        EMBER,
    ));
    frame.render_widget(
        Paragraph::new(Line::from(drills)),
        Rect::new(area.x, area.y + 2, area.width, 1),
    );
    let text = format!(
        "The hall drills heroes up to level {TRAIN_LEVEL_MAX}: {allowed} drill(s) a day, {}c and a turn each.",
        game.train_price()
    );
    let lines: Vec<Line<'static>> = wrap_to(&text, usize::from(area.width), 4)
        .into_iter()
        .map(|l| Line::styled(l, cx.roles.plain))
        .collect();
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(
            area.x,
            area.y + 4,
            area.width,
            area.height.saturating_sub(4),
        ),
    );
}

fn tavern(
    cx: &Cx<'_>,
    frame: &mut DoorFrame<'_>,
    area: Rect,
    game: &Game,
    entries: &[Entry],
    sel: Option<&Entry>,
) {
    let c = cx.content;
    let h = &game.hero;
    let mut y = area.y;
    if let Some(t) = &h.contract {
        if let Some(def) = c.contract(&t.id) {
            let left = (i64::from(def.days) - (game.day - t.day)).max(0);
            let mut lines = vec![
                Line::styled("Your contract", cx.roles.dim),
                Line::styled(clip(&def.name, usize::from(area.width)), cx.roles.strong),
                Line::styled(
                    if t.done {
                        "Done: turn it in at the board.".to_owned()
                    } else {
                        format!("{left} day(s) left")
                    },
                    if t.done {
                        cx.roles.good
                    } else {
                        cx.roles.plain
                    },
                ),
            ];
            for l in wrap_to(&def.text, usize::from(area.width), 3) {
                lines.push(Line::styled(l, cx.roles.dim));
            }
            frame.render_widget(
                Paragraph::new(lines),
                Rect::new(area.x, y, area.width, 7.min(area.height)),
            );
            y += 7;
        }
    } else {
        let board = game.board(c);
        let rows: Vec<Row<'_>> = board
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let is_sel = matches!(sel.map(|e| &e.action), Some(Action::Town(TownAct::Take(n))) if *n == i);
                Row::new([
                    Cell::from(format!(
                        "[{}] {}: {}",
                        key_of(entries, |a| matches!(a, TownAct::Take(n) if *n == i)),
                        p.kind.label(),
                        clip(&p.name, 14)
                    )),
                    Cell::from(format!("L{}+", p.depth)),
                    Cell::from(format!("{}c", p.coin)),
                    Cell::from(format!("{}d", p.days)),
                ])
                .style(sel_style(is_sel))
            })
            .collect();
        let n = u16::try_from(rows.len()).unwrap_or(0);
        if rows.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "The board-keeper trusts you with nothing today.",
                    cx.roles.dim,
                )),
                Rect::new(area.x, y, area.width, 1),
            );
            y += 2;
        } else {
            frame.render_widget(
                plain_table(
                    cx,
                    &["The board", "Depth", "Pay", "Days"],
                    &[
                        Constraint::Fill(1),
                        Constraint::Length(4),
                        Constraint::Length(5),
                        Constraint::Length(3),
                    ],
                    rows,
                ),
                Rect::new(area.x, y, area.width, (n + 1).min(area.height)),
            );
            y += n + 2;
        }
    }
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut drinks = vec![Span::styled("Drinks ", cx.roles.dim)];
    drinks.extend(pips(u32::from(game.today.drinks), 3, EMBER));
    drinks.push(Span::styled("   Dice ", cx.roles.dim));
    drinks.extend(pips(u32::from(game.today.gambles), 3, EMBER));
    lines.push(Line::from(drinks));
    if let Some(b) = &h.bond
        && let Some(comp) = c.companion(&b.id)
    {
        let mut v = vec![Span::styled(format!("{}  ", comp.name), cx.roles.plain)];
        v.extend(pips(u32::from(b.level), 3, MOSS));
        lines.push(Line::from(v));
    }
    if y < area.bottom() {
        frame.render_widget(
            Paragraph::new(lines),
            Rect::new(area.x, y, area.width, (area.bottom() - y).min(3)),
        );
        y += 3;
    }
    if h.debt > 0 && y < area.bottom() {
        frame.render_widget(
            Meter::new(
                "Debt",
                frac(i64::from(h.debt), i64::from(DEBT_LIMIT)),
                format!("{}c (the collectors call past {DEBT_LIMIT})", h.debt),
                EMBER_DIM,
                BLOOD,
            )
            .labelled(6, cx.roles.dim, cx.roles.plain),
            Rect::new(area.x, y, area.width, 1),
        );
    }
}

fn library(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, game: &Game) {
    let fx = game.town_fx(cx.content);
    let h = &game.hero;
    let text = if fx.library {
        format!(
            "The reading room is open. You hold {} relic(s) to be read.",
            h.relics
        )
    } else {
        format!(
            "The shelves are dust and the reading room is shut. You hold {} relic(s) no one can read yet. The town could build a reading room.",
            h.relics
        )
    };
    let mut lines: Vec<Line<'static>> = wrap_to(&text, usize::from(area.width), 5)
        .into_iter()
        .map(|l| Line::styled(l, cx.roles.plain))
        .collect();
    lines.push(Line::raw(""));
    let known: Vec<&str> = cx
        .content
        .recipes
        .recipe
        .iter()
        .filter(|r| matches!(r.learn, crate::content_town::Learn::Library) && h.knows(r))
        .map(|r| r.name.as_str())
        .collect();
    lines.push(Line::styled("Patterns read", cx.roles.dim));
    if known.is_empty() {
        lines.push(Line::styled("  none yet", cx.roles.dim));
    }
    for k in known {
        lines.push(Line::styled(format!("  {k}"), cx.roles.plain));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn works(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, game: &Game, sel: Option<&Entry>) {
    let c = cx.content;
    let sel_project = match sel.map(|e| &e.action) {
        Some(Action::Town(TownAct::Donate { project, .. })) => Some(project.as_str()),
        _ => None,
    };
    let done = c
        .projects
        .project
        .iter()
        .filter(|p| game.world.is_done(c, &p.id))
        .count();
    frame.render_widget(
        Meter::heated(
            "Finished",
            frac(
                i64::try_from(done).unwrap_or(0),
                i64::try_from(c.projects.project.len()).unwrap_or(1),
            ),
            format!("{done} of {}", c.projects.project.len()),
            0.4,
            0.88,
        )
        .labelled(9, cx.roles.dim, cx.roles.plain),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let mut y = area.y + 2;
    let mut shown = 0;
    for p in &c.projects.project {
        if game.world.is_done(c, &p.id) || !game.world.is_open(c, &p.id) {
            continue;
        }
        if y + 2 > area.bottom() || shown >= 4 {
            break;
        }
        shown += 1;
        let (pc, ps) = game.world.progress(&p.id);
        let is_sel = sel_project == Some(p.id.as_str());
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                clip(&p.name, usize::from(area.width).saturating_sub(2)),
                sel_style(is_sel).patch(cx.roles.plain),
            )])),
            Rect::new(area.x, y, area.width, 1),
        );
        let [a, _, b] = Layout::horizontal([
            Constraint::Percentage(50),
            Constraint::Length(2),
            Constraint::Fill(1),
        ])
        .areas(Rect::new(area.x, y + 1, area.width, 1));
        frame.render_widget(
            Meter::heated(
                "",
                frac(i64::from(pc), i64::from(p.coin)),
                format!("{pc}/{}c", p.coin),
                0.4,
                0.88,
            ),
            a,
        );
        frame.render_widget(
            Meter::new(
                "",
                frac(i64::from(ps), i64::from(p.salvage)),
                format!("{ps}/{}s", p.salvage),
                STONE_DARK,
                BONE,
            ),
            b,
        );
        y += 3;
    }
    if shown == 0 {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "Nothing is being built right now.",
                cx.roles.dim,
            )),
            Rect::new(area.x, y, area.width, 1),
        );
    }
    let (gc, gs) = game
        .hero
        .given
        .iter()
        .fold((0u32, 0u32), |(a, b), (_, c, s)| (a + c, b + s));
    if area.bottom() > y {
        frame.render_widget(
            Paragraph::new(Line::styled(
                format!("You have given {gc} coin and {gs} salvage this season."),
                cx.roles.dim,
            )),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    }
}

/// The preview for the entry under the cursor: what it is, what it costs, what is left after.
fn detail(cx: &Cx<'_>, game: &Game, e: &Entry, width: usize) -> (String, Vec<Line<'static>>) {
    let c = cx.content;
    let h = &game.hero;
    let text = |t: &str| -> Vec<Line<'static>> {
        wrap_to(t, width, 6)
            .into_iter()
            .map(|l| Line::styled(l, cx.roles.plain))
            .collect()
    };
    let Action::Town(act) = &e.action else {
        return (e.label.clone(), Vec::new());
    };
    match act {
        TownAct::BuyStock(i) => {
            let stock = game.stock(c);
            let Some(s) = stock.get(*i) else {
                return (e.label.clone(), Vec::new());
            };
            let mut lines = Vec::new();
            let title = match &s.wares {
                Wares::Gear(g) => {
                    lines.push(Line::styled(stats_of(cx, &g.base), cx.roles.strong));
                    let need = gear_level(base_band(c, &g.base));
                    lines.push(Line::styled(
                        format!("Needs hero level {need} (you are {}).", h.level),
                        if u32::from(h.level) >= need {
                            cx.roles.plain
                        } else {
                            Style::new().fg(BLOOD)
                        },
                    ));
                    for a in &g.kit.affixes {
                        if let Some(af) = c.affix(a) {
                            lines.push(Line::styled(
                                format!(
                                    "{}: {}",
                                    af.name,
                                    crate::session::effect_summary(c, &af.effects)
                                ),
                                Style::new().fg(DEEP_BRIGHT),
                            ));
                        }
                    }
                    item_name(c, &g.base, &g.kit)
                }
                Wares::Core(id) => {
                    let core = c.core(id);
                    if let Some(k) = core {
                        lines.extend(text(&k.text));
                        lines.push(Line::styled(
                            crate::session::effect_summary(c, &k.effects),
                            Style::new().fg(DEEP_BRIGHT),
                        ));
                    }
                    core.map_or_else(String::new, |k| k.name.clone())
                }
            };
            lines.push(Line::raw(""));
            lines.push(Line::from(money(h.coin, s.price)));
            (title, lines)
        }
        TownAct::Sell(i) | TownAct::Equip(i) => {
            let Some(it) = h.pack.get(*i) else {
                return (e.label.clone(), Vec::new());
            };
            let mut lines = vec![Line::styled(stats_of(cx, &it.base), cx.roles.strong)];
            if matches!(act, TownAct::Sell(_)) {
                let price = game.sell_price(c, it);
                lines.push(Line::styled(
                    format!("Sells for {price}c: Coin {} -> {}", h.coin, h.coin + price),
                    cx.roles.plain,
                ));
            } else {
                lines.push(Line::styled(
                    format!(
                        "Now worn: {}",
                        stats_of(
                            cx,
                            if c.weapon(&it.base).is_some() {
                                &h.weapon
                            } else {
                                &h.armour
                            }
                        )
                    ),
                    cx.roles.dim,
                ));
                lines.push(Line::styled(
                    format!(
                        "Needs hero level {} (you are {}).",
                        gear_level(base_band(c, &it.base)),
                        h.level
                    ),
                    cx.roles.dim,
                ));
            }
            (item_name(c, &it.base, &it.kit), lines)
        }
        TownAct::SellSalvage => (
            "Sell salvage".to_owned(),
            vec![
                Line::styled(format!("Store {} salvage", h.stash), cx.roles.plain),
                Line::styled(e.label.clone(), cx.roles.dim),
            ],
        ),
        TownAct::Repair(w) => {
            let (base, kit) = if *w == Worn::Weapon {
                (&h.weapon, &h.wkit)
            } else {
                (&h.armour, &h.akit)
            };
            let d = game.derived(c);
            let cost = repair_cost(
                kit.wear,
                base_band(c, base),
                game.town_fx(c).forge.max(1),
                d.has_tag("smithcraft"),
            );
            (
                format!("Repair {}", item_name(c, base, kit)),
                vec![
                    Line::styled(format!("Wear {}% -> 0%", kit.wear), cx.roles.plain),
                    Line::from(money(h.coin, cost)),
                ],
            )
        }
        TownAct::Craft(id) => {
            let Some(r) = c.recipe(id) else {
                return (e.label.clone(), Vec::new());
            };
            let d = game.derived(c);
            let (salvage, coin) = Game::craft_cost(r, &d);
            let mut lines = text(&r.text);
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                format!(
                    "Salvage {} -> {}",
                    h.stash,
                    i64::from(h.stash) - i64::from(salvage)
                ),
                Style::new().fg(if h.stash >= salvage { MOSS } else { BLOOD }),
            ));
            lines.push(Line::from(money(h.coin, coin)));
            match game.craft_refusal(c, id) {
                Some(why) => lines.extend(
                    wrap_to(&why, width, 3)
                        .into_iter()
                        .map(|l| Line::styled(l, Style::new().fg(BLOOD))),
                ),
                None => lines.push(Line::styled("Ready: press Enter.", cx.roles.good)),
            }
            (r.name.clone(), lines)
        }
        TownAct::Slot(n, w) => {
            let core = h.cores.get(*n).and_then(|id| c.core(id));
            let mut lines = Vec::new();
            if let Some(k) = core {
                lines.extend(text(&k.text));
                lines.push(Line::styled(
                    crate::session::effect_summary(c, &k.effects),
                    Style::new().fg(DEEP_BRIGHT),
                ));
            }
            lines.push(Line::styled(
                format!("Into your {}. 8c.", w.word()),
                cx.roles.dim,
            ));
            lines.push(Line::styled(
                "A core already there cracks, unless you are a runesmith.",
                cx.roles.dim,
            ));
            (core.map_or(e.label.clone(), |k| k.name.clone()), lines)
        }
        TownAct::Heal => (
            "Be tended".to_owned(),
            vec![
                Line::styled(
                    format!("Takes {} turn(s).", game.heal_turns(c)),
                    cx.roles.plain,
                ),
                Line::from(money(h.coin, game.heal_price(c))),
            ],
        ),
        TownAct::Cure(n) => {
            let w = h
                .wounds
                .get(*n)
                .and_then(|(id, d)| c.wound(id).map(|w| (w, *d)));
            let price = game.cure_price(c, *n).unwrap_or(0);
            (
                w.map_or(e.label.clone(), |(w, _)| format!("Cure {}", w.name)),
                vec![
                    Line::styled("Lifts the wound at once, with no scar.", cx.roles.plain),
                    Line::from(money(h.coin, price)),
                ],
            )
        }
        TownAct::Train => (
            "Drill".to_owned(),
            vec![
                Line::styled(
                    "A quarter of the way to the next level and a one-day boon.",
                    cx.roles.plain,
                ),
                Line::from(money(h.coin, game.train_price())),
            ],
        ),
        TownAct::Donate {
            project,
            coin,
            salvage,
        } => {
            let Some(p) = c.project(project) else {
                return (e.label.clone(), Vec::new());
            };
            let (pc, ps) = game.world.progress(&p.id);
            let mut lines = text(&p.text);
            lines.push(Line::styled(
                format!("Gives: {}", Game::project_effect_text(p.effect)),
                cx.roles.dim,
            ));
            lines.push(Line::raw(""));
            let give = if *coin > 0 {
                format!("Coin {pc} -> {}/{}", (pc + coin).min(p.coin), p.coin)
            } else {
                format!(
                    "Salvage {ps} -> {}/{}",
                    (ps + salvage).min(p.salvage),
                    p.salvage
                )
            };
            lines.push(Line::styled(give, cx.roles.plain));
            (p.name.clone(), lines)
        }
        TownAct::Take(i) => {
            let board = game.board(c);
            let Some(p) = board.get(*i) else {
                return (e.label.clone(), Vec::new());
            };
            let mut lines = text(&p.text);
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                format!(
                    "Level {}+ · {}c · {}xp · {} day(s)",
                    p.depth, p.coin, p.xp, p.days
                ),
                cx.roles.plain,
            ));
            (p.name.clone(), lines)
        }
        TownAct::Court(id) => {
            let Some(comp) = c.companion(id) else {
                return (e.label.clone(), Vec::new());
            };
            let mut lines = text(&comp.text);
            lines.push(Line::raw(""));
            lines.push(Line::from(money(h.coin, comp.gift)));
            lines.push(Line::styled(
                format!("A roll on {} (DC {}).", comp.attr.label(), comp.dc),
                cx.roles.dim,
            ));
            (comp.name.clone(), lines)
        }
        TownAct::Gamble { stake, cheat } => (
            if *cheat {
                "Loaded dice".to_owned()
            } else {
                "Dice".to_owned()
            },
            vec![
                Line::styled(format!("Stake {stake}c."), cx.roles.plain),
                Line::from(money(h.coin, *stake)),
                Line::styled(
                    if *cheat {
                        "If you are caught: a fine, the door and a wound."
                    } else {
                        "A stake you cannot cover goes on the crew's tab."
                    },
                    cx.roles.dim,
                ),
            ],
        ),
        TownAct::Identify => (
            "Read a relic".to_owned(),
            vec![
                Line::styled(format!("You hold {} relic(s).", h.relics), cx.roles.plain),
                Line::from(money(h.coin, 20)),
            ],
        ),
        _ => (e.label.clone(), Vec::new()),
    }
}
