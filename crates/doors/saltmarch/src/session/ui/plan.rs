//! The expedition planner: the party, the supplies, the target, and a cost preview that reads
//! the same search the march will use (`trip::Reach`).

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Wrap};

use super::super::trip_ui::Preset;
use super::{Size, Ui, fit, tides};
use crate::tide::Access;
use crate::trip::{MAX_PARTY, Trip};
use crate::types::Resource;

/// What the plan would cost, worked out from the engine.
pub(super) struct Preview {
    pub trip: Trip,
    /// Turns to the target, if it can be reached today.
    pub out: Option<u32>,
    /// The water at the target when the party would arrive, and how the ground stands.
    pub arrival: Option<(i32, Access)>,
    /// The water when the party would be starting back.
    pub turning: Option<i32>,
    /// Turns the clan has left today.
    pub left: u32,
    /// Rations the trip needs and the kit has.
    pub rations: (u32, u32),
    pub load: u32,
    pub capacity: u32,
}

pub(super) fn preview(ui: &Ui<'_>) -> Preview {
    preview_for(ui.s, ui.clan, ui.minute)
}

/// The cost preview of the session's plan.
pub(super) fn preview_for(s: &super::Session, clan: &crate::clan::Clan, minute: i64) -> Preview {
    let trip = s.plan_trip(clan);
    // The session keeps the last search, so drawing and describing the same plan is one
    // search, not one a call (a simulated player asks for the screen after every key).
    let reach = s.reach_of(clan, &trip);
    let out = trip.target.and_then(|t| reach.cost(t));
    let tpd = i64::from(s.settings.turns_per_day.max(1));
    let per_turn = 1440 / tpd;
    let arrival = trip.target.zip(out).and_then(|(t, c)| {
        let level = s.tide.level(minute + i64::from(c) * per_turn);
        let tile = s.map.tile(usize::from(t.0), usize::from(t.1))?;
        Some((level, Access::of(i32::from(tile.elevation), level)))
    });
    let turning = out.map(|c| s.tide.level(minute + i64::from(c) * per_turn));
    let crew = u32::try_from(trip.party.len()).unwrap_or(0);
    // A ration feeds one for 12 turns.
    let there_and_back = out.map_or(0, |c| c * 2);
    let rations = (crew * there_and_back.div_ceil(12), u32::from(trip.kit.food));
    Preview {
        out,
        arrival,
        turning,
        left: u32::from(clan.turns_left(&s.settings)),
        rations,
        load: trip.load(),
        capacity: trip.capacity(clan, &s.cat),
        trip,
    }
}

fn party_panel(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let block = ui.panel(&format!("Party {}/{MAX_PARTY}", ui.s.plan.party.len()));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, e) in ui.entries_like(&["party:"]) {
        let id = e
            .tag
            .strip_prefix("party:")
            .and_then(|n| n.parse::<u16>().ok());
        let Some(c) = id.and_then(|id| ui.clan.crew_by(id)) else {
            continue;
        };
        let on = ui.s.plan.party.contains(&c.id);
        let sel = i == ui.s.cursor;
        let base = if sel { p.selected } else { p.plain };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", Ui::key_of(i)),
                if sel { p.selected } else { p.accent },
            ),
            Span::styled(
                if on { ui.look.g("[■]", "[x]") } else { "[ ]" },
                if sel {
                    p.selected
                } else if on {
                    p.gold()
                } else {
                    p.dim
                },
            ),
            Span::styled(
                format!(" {}", fit(super::short_name(&c.name), 9)),
                if on && !sel { p.salt() } else { base },
            ),
            Span::styled(
                format!(
                    " {}",
                    fit(c.trade.name(), usize::from(inner.width).saturating_sub(18))
                ),
                if sel { p.selected } else { p.peat() },
            ),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::styled("Nobody is fit to go.", p.dim));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn kit_panel(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, pv: &Preview) {
    let p = &ui.pal;
    let block = ui.panel("Supplies");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, e) in ui.entries_like(&["kit:"]) {
        let name = e.tag.strip_prefix("kit:").unwrap_or("");
        let sel = i == ui.s.cursor;
        let preset = match name {
            "light" => Some(Preset::Light),
            "standard" => Some(Preset::Standard),
            _ => Some(Preset::Heavy),
        };
        let current = preset.is_some_and(|pr| {
            super::super::trip_ui::kit_for(ui.clan, ui.s.plan.party.len().max(1), pr)
                == ui.s.plan.kit
        });
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", Ui::key_of(i)),
                if sel { p.selected } else { p.accent },
            ),
            Span::styled(
                format!("{} {name}", if current { ui.look.g("◆", "*") } else { " " }),
                if sel {
                    p.selected
                } else if current {
                    p.gold()
                } else {
                    p.plain
                },
            ),
        ]));
    }
    let tiny = ui.look.size == super::Size::Tiny;
    if !tiny {
        lines.push(Line::raw(""));
    }
    for (r, n) in pv.trip.kit.pairs() {
        if tiny && n == 0 {
            continue;
        }
        let held = ui.clan.stock.get(r);
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "{:<6}",
                    if r == Resource::Food {
                        "food"
                    } else {
                        r.name()
                    }
                ),
                p.dim,
            ),
            Span::styled(format!("{n:>2}"), if n > 0 { p.salt() } else { p.slate() }),
            Span::styled(format!(" of {held}"), p.dim),
        ]));
    }
    let over = pv.load > pv.capacity;
    lines.push(ui.gauge_with(
        "Load",
        pv.load.min(pv.capacity.max(1)),
        pv.capacity.max(1),
        inner.width,
        if over { p.rust() } else { p.sea() },
        format!(" {}/{}", pv.load, pv.capacity),
    ));
    f.render_widget(Paragraph::new(lines), inner);
}

fn target_panel(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let block = ui.panel("Target");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let trip = ui.s.plan_trip(ui.clan);
    let reach = ui.s.reach_of(ui.clan, &trip);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, e) in ui.entries_like(&["target:"]) {
        let (x, y) = e
            .tag
            .strip_prefix("target:")
            .and_then(|t| t.split_once(','))
            .and_then(|(a, b)| Some((a.parse::<u8>().ok()?, b.parse::<u8>().ok()?)))
            .unwrap_or((0, 0));
        let name =
            ui.s.map
                .poi_at(x, y)
                .and_then(|poi| ui.s.cat.pois.get(usize::from(poi.template)))
                .map_or("a place", |t| t.name.as_str());
        let chosen = ui.s.plan.target == Some((x, y));
        let cost = reach.cost((x, y));
        let on = i == ui.s.cursor;
        let mark = if chosen { ui.look.g("◆", "*") } else { " " };
        let text = format!(
            " {} {}{} {}",
            Ui::key_of(i),
            mark,
            fit(name, usize::from(inner.width).saturating_sub(13)),
            cost.map_or("far".to_owned(), |c| format!("{c}t"))
        );
        let style: Style = if on {
            p.selected
        } else if chosen {
            p.gold()
        } else if cost.is_none() {
            p.dim
        } else {
            p.plain
        };
        lines.push(Line::styled(text, style));
    }
    if lines.is_empty() {
        lines.push(Line::styled(
            "No place is charted yet. Survey the marsh from the hold first.",
            p.dim,
        ));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn cost_panel(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, pv: &Preview) {
    let p = &ui.pal;
    let block = ui.panel("Cost preview");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines: Vec<Line<'static>> = Vec::new();
    match (pv.trip.target, pv.out) {
        (None, _) => lines.push(Line::styled(
            "No target: the party will go out and look about, with the turns the day has left.",
            p.dim,
        )),
        (Some(t), None) => lines.push(Line::styled(
            format!(
                "{},{} cannot be reached today: the water or the day gets in the way.",
                t.0, t.1
            ),
            p.rust(),
        )),
        (Some(_), Some(out)) => {
            let there_back = out * 2;
            lines.push(ui.gauge_with(
                "Turns",
                there_back.min(pv.left.max(1)),
                pv.left.max(1),
                inner.width,
                if there_back + 4 > pv.left {
                    p.rust()
                } else {
                    p.sea()
                },
                format!(" {out} out, {out} back, {} left", pv.left),
            ));
            if let (Some((level, access)), Some(turning)) = (pv.arrival, pv.turning) {
                let word = match access {
                    Access::Dry => ("dry", p.moss()),
                    Access::Wet => ("knee-deep", p.gold()),
                    Access::Drowned => ("under water", p.rust()),
                };
                lines.push(Line::from(vec![
                    Span::styled("Arrival: water ", p.dim),
                    Span::styled(format!("{level}"), p.plain),
                    Span::styled(", the ground is ", p.dim),
                    Span::styled(word.0, word.1),
                    Span::styled(format!(". Heading back at water {turning}."), p.dim),
                ]));
            }
            let margin = i64::from(pv.left) - i64::from(there_back);
            let (verdict, style) = if margin >= 8 {
                ("A comfortable margin.", p.moss())
            } else if margin >= 2 {
                ("A thin margin: the water will not wait.", p.gold())
            } else if margin >= 0 {
                (
                    "No margin at all. Anything that costs a turn costs the party.",
                    p.rust(),
                )
            } else {
                ("Too far for one day.", p.rust())
            };
            lines.push(Line::styled(verdict, style));
        }
    }
    let (need, have) = pv.rations;
    if need > 0 {
        lines.push(Line::styled(
            format!(
                "Rations: about {need} needed, {have} packed{}.",
                if have < need { " (short)" } else { "" }
            ),
            if have < need { p.rust() } else { p.dim },
        ));
    }
    if let Some((i, e)) = ui.entry("depart") {
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", Ui::key_of(i)),
                if i == ui.s.cursor {
                    p.selected
                } else {
                    p.accent
                },
            ),
            Span::styled(
                fit(&e.text, usize::from(inner.width).saturating_sub(4)),
                p.salt(),
            ),
        ]));
    } else {
        lines.push(Line::styled(
            "Pick at least one crew member to set out.",
            p.dim,
        ));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let pv = preview(ui);
    let roomy = ui.look.size >= Size::Roomy;
    let bottom_h = match ui.look.size {
        Size::Tiny => 5,
        Size::Compact => 7,
        _ => 8,
    };
    let [top, bottom] =
        Layout::vertical([Constraint::Min(8), Constraint::Length(bottom_h)]).areas(area);
    let [a, b, c] = Layout::horizontal([
        Constraint::Percentage(36),
        Constraint::Percentage(27),
        Constraint::Percentage(37),
    ])
    .areas(top);
    party_panel(ui, f, a);
    kit_panel(ui, f, b, &pv);
    target_panel(ui, f, c);
    if roomy {
        let [cost, chart] =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Min(30)]).areas(bottom);
        cost_panel(ui, f, cost, &pv);
        tides::day_chart(ui, f, chart, "The day's water");
    } else {
        cost_panel(ui, f, bottom, &pv);
    }
    let _ = Resource::Food;
}
