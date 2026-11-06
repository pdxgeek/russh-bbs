//! Emberdeep's screens, drawn with ratatui into the door's own buffer (D104). `draw` is the one
//! entry point: it picks a layout by the size of the area (a compact one at the board's 74 by 19,
//! more at larger sizes), draws the screen for the session's mode, and in the ASCII tier ends with
//! a pass that swaps the few icon characters the framework does not map for plain letters.
//!
//! **No rules here.** Every screen reads the engine's state (`Game`, `Hero`, `Derived`, the
//! content tables) and draws it; every key still goes through the same `Action`s as before. A
//! window below [`MIN_COLS`] by [`MIN_ROWS`] gets the plain text view, which is also what the
//! look-around reads.

// Drawing code is geometry: short names for a rectangle's parts and a few casts between cell
// counts and fractions are the clearest way to write it.
#![allow(
    clippy::many_single_char_names,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::struct_field_names,
    clippy::struct_excessive_bools,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::explicit_counter_loop
)]

use russh_bbs_door::buffer::Buffer;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Widget};
use russh_bbs_door::{Canvas, DoorEnv, DoorFrame, Tier};

use super::{Mode, Screen, Session};
use crate::content::Content;
use crate::game::{Game, Place};
use crate::rules::{DEPTH_MAX, TURNS_PER_DAY};

mod art;
mod create;
mod delve;
mod fight;
mod hall;
mod kit;
mod map;
mod places;
mod sheet;
mod town;

pub(crate) use delve::closed_options;
pub(crate) use map::describe as describe_map;
pub(crate) use town::PLACES;

use kit::{
    BLOOD, DEEP, DEEP_BRIGHT, EMBER, EMBER_DIM, EMBER_HOT, Look, Meter, Roles, Rule, STONE,
    STONE_DARK, frac, wrap_to,
};

/// How many tabs the hall has.
pub(super) const HALL_TABS: usize = hall::TABS.len();
/// How many tabs the hero sheet has.
pub(super) const SHEET_TABS: usize = sheet::SHEET_TABS.len();

/// How many rows the sheet's `tab` lists (for the cursor).
pub(super) fn sheet_len(s: &Session, game: &Game, tab: usize) -> usize {
    let cx = Cx {
        s,
        game,
        content: &s.content,
        roles: Roles::none(),
        look: Look {
            ascii: true,
            color: false,
            phase: 0,
            w: 80,
            h: 24,
        },
    };
    sheet::sheet_len(&cx, tab)
}

/// The depth the hero is at, if they are below.
pub(super) fn depth_opt(game: &Game) -> Option<u8> {
    match game.depth() {
        0 => None,
        d => Some(d),
    }
}

/// The narrowest window that gets the full screens.
pub(crate) const MIN_COLS: u16 = 64;
/// The shortest window that gets the full screens.
pub(crate) const MIN_ROWS: u16 = 16;

/// The names of the five depth bands (the design's, in order).
pub(super) fn band_name(band: u8) -> &'static str {
    match band {
        1 => "the Upper Works",
        2 => "the Slag Galleries",
        3 => "the Glass Halls",
        4 => "the Hearth of Kings",
        _ => "the Cold Furnace",
    }
}

/// What every screen reads.
pub(super) struct Cx<'a> {
    pub s: &'a Session,
    pub game: &'a Game,
    pub content: &'a Content,
    pub roles: Roles,
    pub look: Look,
}

/// Draw the door. `canvas` is the door's whole area.
pub(super) fn draw(s: &Session, canvas: &mut Canvas<'_>, env: &DoorEnv) {
    canvas.clear();
    let (w, h) = (canvas.width(), canvas.height());
    if w < MIN_COLS || h < MIN_ROWS {
        text_fallback(s, canvas);
        return;
    }
    let ascii = env.tier == Tier::Ascii;
    let mut frame = canvas.frame();
    let roles = Roles::of(&frame);
    let area = frame.area();
    let phase = s.game.as_ref().map_or(0, |g| {
        g.hero.turns.wrapping_mul(7) ^ g.hero.kills ^ u32::try_from(s.log.len()).unwrap_or(0)
    });
    let look = Look {
        ascii,
        color: env.settings.color(),
        phase,
        w,
        h,
    };
    if let Some(buf) = &s.epitaph {
        epitaph_screen(&mut frame, area, buf, roles);
        if ascii {
            frame.render_widget(Asciify, area);
        }
        return;
    }
    match (&s.mode, &s.game) {
        (Mode::Stuck(text), _) => stuck(&mut frame, area, text, roles),
        (Mode::Create(c), _) => create::draw(s, &mut frame, area, c, roles, look),
        (Mode::Play(screen), Some(game)) => {
            let cx = Cx {
                s,
                game,
                content: &s.content,
                roles,
                look,
            };
            match screen {
                Screen::Main => main_screen(&cx, &mut frame, area),
                Screen::Skills => sheet::draw_skills(&cx, &mut frame, area),
                Screen::Sheet => sheet::draw_sheet(&cx, &mut frame, area),
                Screen::Hall => hall::draw(&cx, &mut frame, area),
                Screen::Map => draw_big_map(&cx, &mut frame, area),
                Screen::Spot(spot) => places::draw(&cx, &mut frame, area, *spot),
            }
        }
        (Mode::Play(_), None) => stuck(&mut frame, area, "No hero.", roles),
    }
    if ascii {
        frame.render_widget(Asciify, area);
    }
}

fn main_screen(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect) {
    let game = cx.game;
    if game.fight.is_some() {
        fight::draw(cx, frame, area);
        return;
    }
    match &game.place {
        Place::Town => town::draw(cx, frame, area),
        Place::Delve { depth, room } => delve::draw(cx, frame, area, *depth, *room),
        Place::Dead { depth, since, .. } => town::draw_dead(cx, frame, area, *depth, *since),
    }
}

/// The line the hero is typing for the hall.
fn epitaph_screen(frame: &mut DoorFrame<'_>, area: Rect, buf: &str, roles: Roles) {
    frame.render_widget(russh_bbs_door::widgets::Clear, area);
    let block = kit::panel("EMBERDEEP: an epitaph", true);
    frame.render_widget(block, area);
    let b = kit::inner(area);
    let max = crate::social::EPITAPH_MAX;
    let lines: Vec<Line<'static>> = vec![
        Line::styled(
            format!(
                "One line, up to {max} characters. The hall prints it only if you are not found."
            ),
            roles.dim,
        ),
        Line::default(),
        Line::styled(format!("> {buf}_"), roles.strong),
        Line::default(),
        Line::styled(
            format!(
                "{}/{max}   [Enter] save   [Backspace] delete   [Esc] cancel",
                buf.chars().count()
            ),
            roles.dim,
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines).wrap(russh_bbs_door::widgets::Wrap { trim: false }),
        b,
    );
}

/// The sealed or damaged screen.
fn stuck(frame: &mut DoorFrame<'_>, area: Rect, text: &str, roles: Roles) {
    let lines: Vec<Line<'static>> =
        crate::session::wrap(text, usize::from(area.width.saturating_sub(4)))
            .into_iter()
            .map(|l| Line::styled(l, roles.bad))
            .collect();
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(
            area.x + 2,
            area.y + 1,
            area.width.saturating_sub(4),
            area.height.saturating_sub(2),
        ),
    );
    frame.render_widget(
        Paragraph::new(Line::styled("[Esc] Leave", roles.accent)),
        Rect::new(area.x + 2, area.bottom().saturating_sub(1), 20, 1),
    );
}

/// The plain text screen of a small window.
fn text_fallback(s: &Session, canvas: &mut Canvas<'_>) {
    let lines = s.lines();
    let height = usize::from(canvas.height());
    let room_for_log = height.saturating_sub(lines.head.len() + lines.keys.len());
    let skip = lines.log.len().saturating_sub(room_for_log);
    let shown = lines
        .head
        .iter()
        .chain(lines.log.iter().skip(skip))
        .chain(lines.keys.iter());
    for (y, (text, style)) in shown.take(height).enumerate() {
        canvas.text(0, u16::try_from(y).unwrap_or(u16::MAX), text, *style);
    }
}

/// The title row: the ember mark, the screen, a rule that cools from ember to deep blue, and the
/// hero on the right.
pub(super) fn header(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, title: &str) {
    let mark = if cx.look.ascii { "*" } else { "◆" };
    let left = format!("{mark} EMBERDEEP · {title} ");
    let right = format!(
        " {} L{} {} ",
        cx.game.hero.name,
        cx.game.hero.level,
        cx.content
            .calling(&cx.game.hero.calling)
            .map_or("hero", |c| c.name.as_str())
    );
    let lw = u16::try_from(left.chars().count()).unwrap_or(u16::MAX);
    let rw = u16::try_from(right.chars().count()).unwrap_or(u16::MAX);
    if lw + rw + 4 > area.width {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                kit::clip(&left, usize::from(area.width)),
                Style::new().fg(EMBER).add_modifier(Modifier::BOLD),
            ))),
            area,
        );
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            left,
            Style::new().fg(EMBER).add_modifier(Modifier::BOLD),
        ))),
        Rect::new(area.x, area.y, lw, 1),
    );
    frame.render_widget(
        Rule {
            glyph: if cx.look.ascii { "-" } else { "━" },
            from: EMBER_DIM,
            to: DEEP,
        },
        Rect::new(area.x + lw, area.y, area.width - lw - rw, 1),
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(right, cx.roles.dim))),
        Rect::new(area.right() - rw, area.y, rw, 1),
    );
}

/// The two-row strip of gauges: depth and torch, then health and fatigue (or, in town, the day's
/// turns and the coin).
pub(super) fn gauges(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, depth: Option<u8>) {
    let h = &cx.game.hero;
    let d = cx.game.derived(cx.content);
    let [left, _, right] = Layout::horizontal([
        Constraint::Percentage(50),
        Constraint::Length(2),
        Constraint::Fill(1),
    ])
    .areas(area);
    let rows = |r: Rect| -> [Rect; 2] {
        [
            Rect::new(r.x, r.y, r.width, 1),
            Rect::new(r.x, r.y + 1, r.width, r.height.saturating_sub(1).min(1)),
        ]
    };
    let [l1, l2] = rows(left);
    let [r1, r2] = rows(right);
    let label = cx.roles.dim;
    let text = cx.roles.plain;
    // Depth.
    match depth {
        Some(depth) => frame.render_widget(
            Meter::new(
                "Depth",
                frac(i64::from(depth), i64::from(DEPTH_MAX)),
                format!("{depth}/{DEPTH_MAX}"),
                DEEP,
                DEEP_BRIGHT,
            )
            .labelled(7, label, text),
            l1,
        ),
        None => frame.render_widget(
            Meter::new(
                "Deepest",
                frac(i64::from(h.best_depth), i64::from(DEPTH_MAX)),
                format!("{}/{DEPTH_MAX}", h.best_depth),
                DEEP,
                DEEP_BRIGHT,
            )
            .labelled(8, label, text),
            l1,
        ),
    }
    // The torch: the day's turns, burning down.
    let tf = frac(i64::from(h.turns), i64::from(TURNS_PER_DAY));
    frame.render_widget(
        Meter::heated(
            "Torch",
            tf,
            format!("{}/{TURNS_PER_DAY} turns", h.turns),
            0.25,
            0.35 + 0.65 * tf,
        )
        .labelled(7, label, text),
        r1,
    );
    let hp = h.hp.max(0);
    let hpf = frac(i64::from(hp), i64::from(d.max_hp));
    frame.render_widget(
        Meter::vital("Health", hpf, format!("{hp}/{}", d.max_hp)).labelled(7, label, text),
        l2,
    );
    let xp_lo = crate::rules::xp_for_level(h.level);
    let xp_hi = crate::rules::xp_for_level(h.level.saturating_add(1));
    let tired = h.fatigue >= 75;
    if depth.is_some() {
        let ff = frac(i64::from(h.fatigue), 100);
        frame.render_widget(
            Meter::new(
                "Fatigue",
                ff,
                if tired {
                    format!("{}% weary", h.fatigue)
                } else {
                    format!("{}%", h.fatigue)
                },
                STONE_DARK,
                if tired { BLOOD } else { STONE },
            )
            .labelled(8, label, text),
            r2,
        );
    } else {
        frame.render_widget(
            Meter::heated(
                "Level up",
                frac(
                    i64::from(h.xp.saturating_sub(xp_lo)),
                    i64::from(xp_hi.saturating_sub(xp_lo)),
                ),
                format!("{}/{}xp", h.xp, xp_hi),
                0.4,
                0.88,
            )
            .labelled(9, label, text),
            r2,
        );
    }
}

/// The dispatches: the last lines of the log, newest brightest, under a rule.
pub(super) fn log_panel(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect) {
    if area.height == 0 {
        return;
    }
    let rule = Rect::new(area.x, area.y, area.width, 1);
    frame.render_widget(
        Rule {
            glyph: if cx.look.ascii { "-" } else { "─" },
            from: STONE_DARK,
            to: DEEP,
        },
        rule,
    );
    let body = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
    let mut lines: Vec<(crate::game::Tone, String)> = Vec::new();
    for (tone, text) in &cx.s.log {
        for l in wrap_to(text, usize::from(body.width).saturating_sub(2), 6) {
            lines.push((*tone, l));
        }
    }
    let keep = usize::from(body.height);
    let skip = lines.len().saturating_sub(keep);
    let shown: Vec<Line<'static>> = lines
        .iter()
        .skip(skip)
        .enumerate()
        .map(|(i, (tone, text))| {
            let newest = skip + i + 1 == lines.len();
            let mut style = cx.roles.tone(*tone);
            if !newest {
                style = style.add_modifier(Modifier::DIM);
            }
            let bullet = if newest {
                if cx.look.ascii { "> " } else { "▸ " }
            } else {
                "  "
            };
            Line::from(vec![
                Span::styled(bullet, Style::new().fg(EMBER)),
                Span::styled(text.clone(), style),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(shown), body);
}

/// The whole map, for the map screen.
fn draw_big_map(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect) {
    let game = cx.game;
    let Place::Delve { depth, room } = game.place else {
        header(cx, frame, Rect::new(area.x, area.y, area.width, 1), "Map");
        frame.render_widget(
            Paragraph::new(Line::styled("There is no map in town.", cx.roles.dim)),
            Rect::new(area.x + 2, area.y + 2, area.width.saturating_sub(4), 1),
        );
        return;
    };
    let level = crate::dungeon::generate(cx.content, game.season, game.epoch, depth);
    let views = map::room_views(game, cx.content, &level, room);
    let torch = map::Torch::from_turns(game.hero.turns);
    let [head, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .areas(area);
    header(
        cx,
        frame,
        head,
        &format!(
            "Map of level {depth} · {}",
            band_name(crate::dungeon::band_of(depth))
        ),
    );
    let big = body.height >= 17 && body.width >= 37 + 2 + 26;
    let (mw, mh) = map::MapWidget::size(big);
    let [left, right] = Layout::horizontal([
        Constraint::Length((mw + 2).min(body.width.saturating_sub(24))),
        Constraint::Min(22),
    ])
    .areas(body);
    frame.render_widget(kit::panel("The floor as your torch showed it", true), left);
    let inner = kit::inner(left);
    if inner.width >= mw && inner.height >= mh {
        frame.render_widget(
            map::MapWidget {
                rooms: &views,
                torch,
                look: cx.look,
                big,
            },
            Rect::new(inner.x, inner.y, inner.width, mh),
        );
    }
    // The legend and the state of the light, down the right side.
    frame.render_widget(kit::panel("Key", false), right);
    let rb = kit::inner(right);
    let known = views
        .iter()
        .filter(|v| v.sight == map::Sight::Known)
        .count();
    let mut lines: Vec<Line<'static>> = vec![
        Line::styled(
            format!("{known} of {} rooms explored", views.len()),
            cx.roles.strong,
        ),
        Line::from({
            let mut v = vec![Span::styled("Light ", cx.roles.dim)];
            v.extend(kit::pips(
                u32::try_from(torch.radius + 1).unwrap_or(1),
                4,
                EMBER_HOT,
            ));
            v
        }),
        Line::raw(""),
    ];
    for (g, word, color) in map::legend(&cx.look) {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{g} "),
                Style::new().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(word.to_owned(), cx.roles.dim),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), rb);
    frame.render_widget(
        kit::Keys {
            chips: &[kit::Chip::new("Esc", "back")],
            roles: cx.roles,
        },
        foot,
    );
}

/// Swaps the icon characters this crate draws that the framework's ASCII tier does not map for
/// plain letters, so the ASCII tier stays pure ASCII whatever a screen drew.
struct Asciify;

impl Widget for Asciify {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let Some(cell) = buf.cell_mut((x, y)) else {
                    continue;
                };
                let mapped = match cell.symbol() {
                    "…" => "~",
                    "†" => "x",
                    "✗" => "x",
                    "◉" => "o",
                    "✦" => "*",
                    "≈" => "~",
                    "◢" | "◥" => "/",
                    "◣" | "◤" => "\\",
                    "·" | "‧" => ".",
                    "»" => ">",
                    "—" | "–" => "-",
                    "↕" => "^",
                    "◆" | "●" => "*",
                    "○" => "o",
                    "▸" | "▶" => ">",
                    "▲" => "^",
                    "▼" => "v",
                    "■" | "▪" => "#",
                    _ => continue,
                };
                cell.set_symbol(mapped);
            }
        }
    }
}
