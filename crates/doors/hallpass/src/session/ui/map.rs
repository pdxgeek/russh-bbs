//! The school map: the places drawn as a plan of inky corridors, each place lit by what it is
//! to you now (where you stand, where you can walk, what is shut for the hour, what is locked),
//! with the staff you can see and the patrols you know of. Arrow keys move a cursor from place
//! to place; Enter walks there if the engine allows it (`Action::Go`, the same action the
//! numbered list presses).
//!
//! The grid below is only the *picture*. Which places are open, locked or reachable is asked of
//! the engine (`Game::place_open`, `Game::routes`, `Game::patrol_level`).

use std::collections::HashSet;

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Wrap};

use super::day::{capitalise, people_line};
use super::{Cells, Size, Ui, fit, hue};

/// A direction the map cursor can move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Where a place sits on the plan: (column, row), and the short name it is drawn with.
const GRID: [(&str, u8, u8, &str); 34] = [
    ("room_sciences", 3, 0, "Science"),
    ("room_languages", 4, 0, "Languages"),
    ("dorm", 1, 1, "Dorm"),
    ("art_room", 2, 1, "Art room"),
    ("classroom_corridor", 3, 1, "Classes"),
    ("room_history", 4, 1, "History"),
    ("restricted_stacks", 0, 2, "Stacks"),
    ("common_room", 1, 2, "Common rm"),
    ("quad", 2, 2, "The Quad"),
    ("tuck_shop", 3, 2, "Tuck shop"),
    ("room_maths", 4, 2, "Maths"),
    ("headmistress_office", 0, 3, "Head's"),
    ("library", 1, 3, "Library"),
    ("main_hall", 2, 3, "Hall"),
    ("gym", 3, 3, "Gym"),
    ("organ_loft", 4, 3, "Loft"),
    ("deputy_office", 0, 4, "Deputy"),
    ("admin_corridor", 1, 4, "Admin"),
    ("green_room", 2, 4, "Greenroom"),
    ("chapel", 3, 4, "Chapel"),
    ("grounds", 4, 4, "Grounds"),
    ("car_park", 5, 4, "Car park"),
    ("staff_quarters", 0, 5, "Quarters"),
    ("staffroom", 1, 5, "Staffroom"),
    ("tunnels", 2, 5, "Tunnels"),
    ("boathouse", 3, 5, "Boathouse"),
    ("caretaker_shed", 4, 5, "Shed"),
    ("candle_den", 1, 6, "Candles"),
    ("cellars", 2, 6, "Cellars"),
    ("old_wing", 3, 6, "Old Wing"),
    ("east_wing", 4, 6, "East Wing"),
    ("paper_office", 2, 7, "Paper"),
    ("detention_room", 3, 7, "Detention"),
    ("workshop", 4, 7, "Workshop"),
];

/// The plan is this many columns wide and this many cells tall.
const COLS: usize = 6;
const CELLS_HIGH: usize = 8;

/// How big a cell of the plan is drawn: wider and taller when the window has room.
#[derive(Debug, Clone, Copy)]
struct Geo {
    cw: usize,
    ch: usize,
}

impl Geo {
    const fn width(self) -> usize {
        COLS * self.cw
    }
    const fn height(self) -> usize {
        (CELLS_HIGH - 1) * self.ch + 1
    }
}

fn grid_of(id: &str) -> Option<(usize, usize, &'static str)> {
    GRID.iter()
        .find(|(g, ..)| *g == id)
        .map(|(_, c, r, n)| (usize::from(*c), usize::from(*r), *n))
}

/// Where a place is drawn: (x, y) of its glyph.
fn label_xy(id: &str, geo: Geo) -> Option<(usize, usize)> {
    grid_of(id).map(|(c, r, _)| (c * geo.cw, r * geo.ch))
}

/// The place nearest to `from` in a direction, by the plan's own grid.
pub(crate) fn step_cursor(from: &str, dir: Dir) -> Option<String> {
    let geo = Geo { cw: 12, ch: 2 };
    let (fx, fy) = label_xy(from, geo).map(|(x, y)| (x as i32, y as i32))?;
    let mut best: Option<(i32, &str)> = None;
    for (id, ..) in &GRID {
        if *id == from {
            continue;
        }
        let (x, y) = label_xy(id, geo).map(|(x, y)| (x as i32, y as i32))?;
        // Rows are twice as tall as columns are wide in the eye: weigh them to match.
        let (dx, dy) = ((x - fx) / 2, (y - fy) * 2);
        let (along, across) = match dir {
            Dir::Up => (-dy, dx.abs()),
            Dir::Down => (dy, dx.abs()),
            Dir::Left => (-dx, dy.abs()),
            Dir::Right => (dx, dy.abs()),
        };
        if along <= 0 {
            continue;
        }
        let score = along + across * 2;
        if best.is_none_or(|(b, _)| score < b) {
            best = Some((score, id));
        }
    }
    best.map(|(_, id)| id.to_owned())
}

/// How a place stands to you right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    You,
    Reachable(u8),
    Open,
    ShutNow,
    Locked,
    Keyed,
}

fn state_of(ui: &Ui<'_>, id: &str, routes: &[(String, u8)]) -> State {
    let g = ui.g;
    if id == g.player.place {
        return State::You;
    }
    if let Some((_, hops)) = routes.iter().find(|(p, _)| p == id) {
        return State::Reachable(*hops);
    }
    let Some(spec) = g.content.place(id) else {
        return State::ShutNow;
    };
    let locked = spec.lock.is_some() && !spec.unlock.iter().any(|c| g.holds(c));
    if locked {
        return State::Locked;
    }
    match g.current_slot() {
        Some(slot) if g.place_open(id, g.player.day, slot) => {
            if spec.lock.is_some() {
                State::Keyed
            } else {
                State::Open
            }
        }
        _ => State::ShutNow,
    }
}

/// The places you know people are at: where you can walk, where you stand, and the places a
/// scheme of yours has been scouted.
fn known_places(ui: &Ui<'_>, routes: &[(String, u8)]) -> HashSet<String> {
    let g = ui.g;
    let mut out: HashSet<String> = routes.iter().map(|(p, _)| p.clone()).collect();
    out.insert(g.player.place.clone());
    for s in &g.player.schemes {
        if s.scout > 0
            && let Some(p) = g.scheme_place(s)
        {
            out.insert(p);
        }
    }
    out
}

fn glyph(ui: &Ui<'_>, st: State) -> &'static str {
    match st {
        State::You => ui.look.g("◉", "@"),
        State::Reachable(_) => ui.look.g("●", "o"),
        State::Open => ui.look.g("○", "."),
        State::ShutNow => ui.look.g("∙", "-"),
        State::Locked => ui.look.g("⊘", "X"),
        State::Keyed => ui.look.g("◈", "*"),
    }
}

fn style_of(ui: &Ui<'_>, st: State) -> Style {
    let p = &ui.pal;
    match st {
        State::You => p.lamp(),
        State::Reachable(_) => p.ink().add_modifier(russh_bbs_door::style::Modifier::BOLD),
        State::Open => p.chalk_soft(),
        State::ShutNow => p.slate(),
        State::Locked => p.red(),
        State::Keyed => p.moss(),
    }
}

/// Put `text` into the plan with one style from (x, y).
fn put(canvas: &mut [Vec<(String, Style)>], x: usize, y: usize, text: &str, style: Style) {
    for (i, ch) in text.chars().enumerate() {
        if let Some(cell) = canvas.get_mut(y).and_then(|row| row.get_mut(x + i)) {
            *cell = (ch.to_string(), style);
        }
    }
}

/// The corridors: a line from each place to the neighbours it opens on, where they sit next
/// to each other on the plan. (Two long ways, Admin to Classes and Quad to Grounds, are in the
/// text under the plan instead.)
fn corridors(ui: &Ui<'_>, canvas: &mut [Vec<(String, Style)>], geo: Geo) {
    let p = &ui.pal;
    let line = if ui.look.color {
        p.fg_or(hue::INK_DEEP, p.dim)
    } else {
        p.slate()
    };
    for spec in &ui.g.content.places {
        let Some((c1, r1, n1)) = grid_of(&spec.id) else {
            continue;
        };
        for exit in &spec.exits {
            let Some((c2, r2, _)) = grid_of(exit) else {
                continue;
            };
            let (dc, dr) = (c2 as i32 - c1 as i32, r2 as i32 - r1 as i32);
            // Draw each pair once: from the left cell, or the upper one.
            if dc < 0 || (dc == 0 && dr < 0) {
                continue;
            }
            let (x1, y1) = (c1 * geo.cw, r1 * geo.ch);
            let (x2, y2) = (c2 * geo.cw, r2 * geo.ch);
            match (dc, dr) {
                (1, 0) => {
                    let from = x1 + 1 + n1.chars().count() + 1;
                    for x in from..x2 {
                        put(canvas, x, y1, ui.look.g("─", "-"), line);
                    }
                }
                (0, 1) => {
                    for k in 1..geo.ch {
                        put(canvas, x1 + 1, y1 + k, ui.look.g("│", "|"), line);
                    }
                }
                (1, 1) => {
                    for k in 1..geo.ch {
                        put(canvas, x2 + k - geo.ch, y1 + k, ui.look.g("╲", "\\"), line);
                    }
                }
                (1, -1) => {
                    for k in 1..geo.ch {
                        put(canvas, x2 - k, y2 + k, ui.look.g("╱", "/"), line);
                    }
                }
                _ => {}
            }
        }
    }
}

/// The whole plan as cells.
fn plan(ui: &Ui<'_>, cursor: &str, routes: &[(String, u8)], geo: Geo) -> Vec<Vec<(String, Style)>> {
    let blank = (" ".to_owned(), Style::new());
    let mut canvas = vec![vec![blank; geo.width()]; geo.height()];
    corridors(ui, &mut canvas, geo);
    let known = known_places(ui, routes);
    for (id, c, r, name) in &GRID {
        let st = state_of(ui, id, routes);
        let mut style = style_of(ui, st);
        let x = usize::from(*c) * geo.cw;
        let y = usize::from(*r) * geo.ch;
        let mut text = format!("{} {}", glyph(ui, st), name);
        let watched = known.contains(*id)
            && ui
                .g
                .current_slot()
                .is_some_and(|s| ui.g.witnesses_at(id, ui.g.player.day, s) > 0);
        if watched {
            text.push_str(ui.look.g("†", "!"));
        }
        if *id == cursor {
            style = ui.pal.bar_on();
            text = format!(" {text} ");
            put(&mut canvas, x.saturating_sub(1), y, &text, style);
        } else {
            put(&mut canvas, x, y, &text, style);
        }
    }
    canvas
}

/// The place panel for the place under the cursor.
fn detail(ui: &Ui<'_>, id: &str, routes: &[(String, u8)], width: u16) -> Vec<Line<'static>> {
    let g = ui.g;
    let pal = &ui.pal;
    let Some(spec) = g.content.place(id) else {
        return Vec::new();
    };
    let st = state_of(ui, id, routes);
    let mut lines = Vec::new();
    let (word, wstyle) = match st {
        State::You => ("you are here".to_owned(), pal.lamp()),
        State::Reachable(1) => ("1 corridor away".to_owned(), pal.ink()),
        State::Reachable(n) => (format!("{n} corridors away"), pal.ink()),
        State::Open => (
            "open, out of reach this period".to_owned(),
            pal.chalk_soft(),
        ),
        State::ShutNow => ("shut at this hour".to_owned(), pal.slate()),
        State::Locked => ("LOCKED".to_owned(), pal.red()),
        State::Keyed => ("locked, but you can get in".to_owned(), pal.moss()),
    };
    lines.push(Line::from(vec![
        Span::styled(format!("{} ", glyph(ui, st)), style_of(ui, st)),
        Span::styled(spec.name.clone(), pal.chalk()),
        Span::styled(format!("  {} wing", capitalise(&spec.wing)), pal.ink()),
    ]));
    lines.push(Line::styled(word, wstyle));
    let hours = spec.hours.join(" ");
    lines.push(Line::from(vec![
        Span::styled("hours ", pal.slate()),
        Span::styled(hours, pal.chalk_soft()),
    ]));
    if st == State::Locked {
        // Say what kind of key it wants only where a scheme of yours has looked at it.
        let scouted = g
            .player
            .schemes
            .iter()
            .any(|s| s.scout > 0 && g.scheme_place(s).as_deref() == Some(id));
        if scouted {
            let ways: Vec<String> = spec
                .unlock
                .iter()
                .map(crate::cond::Cond::describe)
                .collect();
            lines.push(Line::styled(
                format!("any of: {}", ways.join("; ")),
                pal.red(),
            ));
        } else {
            lines.push(Line::styled("scout it to learn what opens it", pal.slate()));
        }
    }
    let mut doors: Vec<String> = spec.exits.clone();
    doors.extend(
        g.content
            .places
            .iter()
            .filter(|p| p.exits.iter().any(|e| e == id))
            .map(|p| p.id.clone()),
    );
    doors.sort();
    doors.dedup();
    let names: Vec<String> = doors
        .iter()
        .filter_map(|d| g.content.place(d).map(|p| p.name.clone()))
        .collect();
    lines.push(Line::from(vec![
        Span::styled("doors ", pal.slate()),
        Span::styled(names.join(", "), pal.chalk_soft()),
    ]));
    let known = known_places(ui, routes);
    if known.contains(id) {
        let people = people_line(ui, id, false);
        let mut spans = vec![Span::styled("here ", pal.slate())];
        if people.is_empty() {
            spans.push(Span::styled("nobody you can see", pal.slate()));
        } else {
            spans.extend(people);
        }
        lines.push(Line::from(spans));
        let patrol = g.patrol_level(id);
        if patrol > 0 {
            lines.push(Line::styled(
                format!(
                    "{} patrol level {patrol}: prefects keep coming back here",
                    ui.look.g("†", "!")
                ),
                pal.red(),
            ));
        }
    } else {
        lines.push(Line::styled("who is there: out of sight", pal.slate()));
    }
    let _ = width;
    lines
}

/// The place under the cursor: the cursor, or where you stand if it has not been moved.
pub(super) fn selected(ui: &Ui<'_>) -> String {
    selected_place(ui.s)
}

/// The same, from the session.
pub(super) fn selected_place(s: &crate::session::Session) -> String {
    if grid_of(&s.map_cursor).is_some() {
        s.map_cursor.clone()
    } else {
        s.game.player.place.clone()
    }
}

/// The legend, short enough for the plan's bottom border.
fn legend(ui: &Ui<'_>) -> Line<'static> {
    let p = &ui.pal;
    let item = |st: State, word: &str| {
        vec![
            Span::styled(glyph(ui, st), style_of(ui, st)),
            Span::styled(format!("{word} "), p.slate()),
        ]
    };
    let mut spans = vec![Span::styled(" ", p.slate())];
    spans.extend(item(State::You, "you"));
    spans.extend(item(State::Reachable(1), "reach"));
    spans.extend(item(State::Open, "open"));
    spans.extend(item(State::ShutNow, "shut"));
    spans.extend(item(State::Locked, "locked"));
    spans.push(Span::styled(ui.look.g("†", "!"), p.red()));
    spans.push(Span::styled("staff ", p.slate()));
    Line::from(spans)
}

/// Who is where, as far as you know: the characters in places you can see.
fn whereabouts(ui: &Ui<'_>, routes: &[(String, u8)], height: usize) -> Vec<Line<'static>> {
    let g = ui.g;
    let pal = &ui.pal;
    let Some(slot) = g.current_slot() else {
        return vec![Line::styled("Lights out: nobody is about.", pal.slate())];
    };
    let known = known_places(ui, routes);
    let mut lines = Vec::new();
    for n in &g.content.npcs {
        let place = g.npc_place(n, g.player.day, slot);
        if !known.contains(place) {
            continue;
        }
        let name = g.content.place(place).map_or("?", |p| p.name.as_str());
        let Some(who) = super::day::who(ui, &n.id, true) else {
            continue;
        };
        lines.push(Line::from(vec![
            who,
            Span::styled(format!("  {name}"), pal.slate()),
        ]));
        if lines.len() >= height {
            break;
        }
    }
    if lines.is_empty() {
        lines.push(Line::styled("Nobody you can see.", pal.slate()));
    }
    lines
}

/// The map screen.
pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let routes = ui.g.routes();
    let cursor = selected(ui);
    let wide = matches!(ui.look.size, Size::Roomy | Size::Grand) && area.width >= 100;
    let cw = if area.width >= 132 { 14 } else { 12 };
    let (map_area, side) = if wide {
        let [m, s] = Layout::horizontal([
            Constraint::Length(u16::try_from(COLS * cw + 2).unwrap_or(74)),
            Constraint::Min(24),
        ])
        .areas(area);
        (m, s)
    } else {
        let detail_h = if ui.look.size == Size::Tiny { 5 } else { 6 };
        let [m, d] =
            Layout::vertical([Constraint::Min(5), Constraint::Length(detail_h)]).areas(area);
        (m, d)
    };
    let block = ui.panel("Greywater Academy").title_bottom(legend(ui));
    let inner = block.inner(map_area);
    f.render_widget(block, map_area);
    let geo = Geo {
        cw,
        ch: [4, 3, 2]
            .into_iter()
            .find(|ch| usize::from(inner.height) > ch * (CELLS_HIGH - 1))
            .unwrap_or(2),
    };
    let canvas = plan(ui, &cursor, &routes, geo);
    // Pan so the cursor shows.
    let (cx, cy) = label_xy(&cursor, geo).unwrap_or((0, 0));
    let (vw, vh) = (usize::from(inner.width), usize::from(inner.height));
    let ox = (cx + 6)
        .saturating_sub(vw / 2)
        .min(geo.width().saturating_sub(vw));
    let oy = (cy + 1)
        .saturating_sub(vh / 2)
        .min(geo.height().saturating_sub(vh));
    let rows: Vec<Vec<(String, Style)>> = canvas
        .into_iter()
        .skip(oy)
        .take(vh)
        .map(|row| row.into_iter().skip(ox).take(vw).collect())
        .collect();
    f.render_widget(Cells { rows }, inner);
    if wide {
        let [top, bottom] =
            Layout::vertical([Constraint::Length(11), Constraint::Min(4)]).areas(side);
        let inner = ui.framed(f, top, "The place");
        let lines = detail(ui, &cursor, &routes, inner.width);
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
        let inner = ui.framed(f, bottom, "Who is where");
        let mut lines = whereabouts(ui, &routes, usize::from(inner.height).saturating_sub(2));
        lines.push(Line::styled(
            "Also: Admin to Classes, Quad to Grounds.",
            ui.pal.slate(),
        ));
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    } else {
        let inner = ui.framed(f, side, "");
        let mut lines = detail(ui, &cursor, &routes, inner.width);
        lines.truncate(usize::from(inner.height));
        f.render_widget(Paragraph::new(lines), inner);
    }
    let _ = fit;
}
