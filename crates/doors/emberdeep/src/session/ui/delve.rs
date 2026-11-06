//! The dungeon view: the lit map beside the room's vignette, with the depth and torch strip above
//! and the dispatches and keys below. Event scenes are the same screen: a room's options are the
//! choices, and the options this build cannot take are shown with the reason, so two heroes can
//! see why their neighbour had a way through.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Clear, Paragraph};

use super::art::Vignette;
use super::kit::{
    BLOOD, BONE, Chip, DEEP_BRIGHT, EMBER, EMBER_HOT, Keys, MOSS, STONE, STONE_LIGHT, clip, inner,
    panel, pips, spans, wrap_to,
};
use super::map::{MapWidget, Torch, legend, room_views};
use super::{Cx, band_name};
use crate::content::{Attr, Content, Need, RoomOption};
use crate::dungeon::{self, Dir};
use crate::game::{Game, OptionWhat};
use crate::rules::{DEPTH_MAX, REST_TURNS};

/// Why a build cannot take an option, from the first condition it fails.
pub(super) fn why_closed(c: &Content, need: &Need) -> String {
    match need {
        Need::Tag { tag } => format!("needs {tag}"),
        Need::NoTag { tag } => format!("only without {tag}"),
        Need::Attr { attr, min } => format!("needs {} {min}", attr.label()),
        Need::Calling { id } => format!(
            "a {} only",
            c.calling(id)
                .map_or(id.as_str(), |x| x.name.as_str())
                .to_lowercase()
        ),
        Need::Trait { id } => format!(
            "needs {}",
            c.trait_or_scar(id).map_or(id.as_str(), |x| x.name.as_str())
        ),
        Need::Node { id } => format!(
            "needs skill {}",
            c.node(id).map_or(id.as_str(), |x| x.name.as_str())
        ),
        Need::Level { min } => format!("needs level {min}"),
        Need::Flag { id } => format!("needs the choice `{id}`"),
        Need::NoFlag { id } => format!("closed by the choice `{id}`"),
    }
}

/// Options of the room this build cannot take, each with the reason.
pub(crate) fn closed_options(
    content: &Content,
    game: &Game,
    depth: u8,
    room: u8,
) -> Vec<(String, String)> {
    let level = dungeon::generate(content, game.season, game.epoch, depth);
    let Some(template) = level.template(content, room) else {
        return Vec::new();
    };
    let d = game.derived(content);
    let event = level.event(content, room);
    let spent = |i: usize, once: bool| {
        let idx = u8::try_from(i).unwrap_or(u8::MAX);
        once && game.today.spent.contains(&(depth, room, idx))
    };
    template
        .option
        .iter()
        .enumerate()
        .filter(|(i, o)| !spent(*i, o.once))
        .chain(
            event
                .into_iter()
                .flat_map(|e| e.option.iter().enumerate())
                .filter(|(i, o)| !spent(100 + *i, o.once)),
        )
        .filter_map(|(_, o): (usize, &RoomOption)| {
            o.need
                .iter()
                .find(|n| !game.hero.meets(&d, n))
                .map(|n| (o.label.clone(), why_closed(content, n)))
        })
        .collect()
}

/// The commands of the dungeon screen, as chips.
pub(super) fn chips(
    cx: &Cx<'_>,
    game: &Game,
    depth: u8,
    room: u8,
    hostile: bool,
    opts: usize,
) -> Vec<Chip> {
    let level = dungeon::generate(cx.content, game.season, game.epoch, depth);
    let mut out = vec![Chip::new("n e s w", "move")];
    if opts > 0 {
        out.push(Chip::new(format!("1-{opts}").replace("1-1", "1"), "act"));
    }
    if room == level.exit && !hostile && depth < DEPTH_MAX {
        out.push(Chip::new(">", "stairs down"));
    }
    if !hostile {
        out.push(Chip::new("r", format!("camp ({REST_TURNS}t)")));
    }
    out.push(Chip::new(
        "u",
        format!("climb ({}t)", game.ascent_cost(cx.content)),
    ));
    out.push(Chip::new("m", "map"));
    out.push(Chip::new("c", "sheet"));
    out.push(Chip::new("Esc", "leave door"));
    out
}

pub(super) fn draw(cx: &Cx<'_>, frame: &mut DoorFrame<'_>, area: Rect, depth: u8, room: u8) {
    let game = cx.game;
    let content = cx.content;
    let level = dungeon::generate(content, game.season, game.epoch, depth);
    let Some(template) = level.template(content, room) else {
        return;
    };
    let views = room_views(game, content, &level, room);
    let torch = Torch::from_turns(game.hero.turns);
    let hostile = game.is_hostile(content, &level, room);
    let options = game.options(content);
    let key_chips = chips(cx, game, depth, room, hostile, options.len().min(9));
    let key_rows = Keys::rows(&key_chips, area.width).min(2);
    let log_h: u16 = if area.height >= 30 {
        7
    } else if area.height >= 22 {
        5
    } else {
        3
    };
    let [head, gauges, middle, log, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(5),
        Constraint::Length(log_h),
        Constraint::Length(key_rows),
    ])
    .areas(area);
    let band = dungeon::band_of(depth);
    super::header(
        cx,
        frame,
        head,
        &format!("Level {depth} · {}", band_name(band)),
    );
    super::gauges(cx, frame, gauges, Some(depth));

    // Columns: the map, the scene, and (wide windows) the hero's card.
    let hero_w: u16 = if cx.look.wide() { 28 } else { 0 };
    let big_fits = middle.height >= 17 && middle.width >= 37 + 2 + 30 + hero_w;
    let (mw, _) = MapWidget::size(big_fits);
    let map_w = (mw + 2).min(middle.width.saturating_sub(30 + hero_w));
    let cols: Vec<Constraint> = if hero_w > 0 {
        vec![
            Constraint::Length(map_w),
            Constraint::Min(30),
            Constraint::Length(hero_w),
        ]
    } else {
        vec![Constraint::Length(map_w), Constraint::Min(30)]
    };
    let parts = Layout::horizontal(cols).split(middle);
    draw_map(cx, frame, parts[0], &views, torch, big_fits, depth);
    draw_scene(cx, frame, parts[1], depth, room, hostile, &options);
    if hero_w > 0 {
        super::sheet::card(cx, frame, parts[2]);
    }
    super::log_panel(cx, frame, log);
    frame.render_widget(
        Keys {
            chips: &key_chips,
            roles: cx.roles,
        },
        keys,
    );
    let _ = template;
}

fn draw_map(
    cx: &Cx<'_>,
    frame: &mut DoorFrame<'_>,
    area: Rect,
    views: &[super::map::RoomView],
    torch: Torch,
    big: bool,
    depth: u8,
) {
    frame.render_widget(Clear, area);
    frame.render_widget(panel(&format!("Floor {depth}"), true), area);
    let body = inner(area);
    let (mw, mh) = MapWidget::size(big);
    if body.width < mw || body.height < mh {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "(window too small for the map; press m)",
                cx.roles.dim,
            )),
            body,
        );
        return;
    }
    let map_area = Rect::new(body.x, body.y, body.width, mh.min(body.height));
    frame.render_widget(
        MapWidget {
            rooms: views,
            torch,
            look: cx.look,
            big,
        },
        map_area,
    );
    let mut y = body.y + mh;
    // The light, as pips and words.
    if y < body.bottom() {
        let radius = u32::try_from(torch.radius + 1).unwrap_or(1);
        let mut line = vec![Span::styled("Light ", cx.roles.dim)];
        line.extend(pips(radius, 4, EMBER_HOT));
        let words = match torch.radius {
            0 => " the torch is out",
            1 => " guttering",
            2 => " steady",
            _ => " bright",
        };
        line.push(Span::styled(words, cx.roles.dim));
        frame.render_widget(
            Paragraph::new(Line::from(line)),
            Rect::new(body.x, y, body.width, 1),
        );
        y += 1;
    }
    // The legend, where there is room.
    let mut row: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for (g, word, color) in legend(&cx.look) {
        let cell = word.len() + 3;
        if used + cell > usize::from(body.width) {
            if y < body.bottom() {
                frame.render_widget(
                    Paragraph::new(Line::from(std::mem::take(&mut row))),
                    Rect::new(body.x, y, body.width, 1),
                );
                y += 1;
            }
            used = 0;
        }
        row.push(Span::styled(g, Style::new().fg(color)));
        row.push(Span::styled(format!(" {word}  "), cx.roles.dim));
        used += cell;
    }
    if !row.is_empty() && y < body.bottom() && body.height > mh + 2 {
        frame.render_widget(
            Paragraph::new(Line::from(row)),
            Rect::new(body.x, y, body.width, 1),
        );
    }
}

fn exits_line(
    cx: &Cx<'_>,
    game: &Game,
    depth: u8,
    room: u8,
    hostile: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let level = dungeon::generate(cx.content, game.season, game.epoch, depth);
    let explored = game
        .explored
        .get(usize::from(depth) - 1)
        .copied()
        .unwrap_or(0);
    let mut spans_out: Vec<Span<'static>> = Vec::new();
    let mut lines = Vec::new();
    let mut used = 0usize;
    if let Some(r) = level.room(room) {
        for dir in Dir::ALL {
            let Some(next) = r.links[dir.index()] else {
                continue;
            };
            let known = explored & (1 << next) != 0;
            let name = if known {
                level
                    .template(cx.content, next)
                    .map_or("?".to_owned(), |t| t.name.clone())
            } else {
                "unexplored".to_owned()
            };
            let back = if next == game.prev_room && hostile {
                " (back)"
            } else {
                ""
            };
            let text = clip(&format!("{name}{back}"), 18).clone();
            let w = text.chars().count() + 4;
            if used + w > width && used > 0 {
                lines.push(Line::from(std::mem::take(&mut spans_out)));
                used = 0;
            }
            spans_out.push(Span::styled(format!("[{}]", dir.letter()), cx.roles.accent));
            spans_out.push(Span::styled(
                format!(" {text}  "),
                if known { cx.roles.plain } else { cx.roles.dim },
            ));
            used += w;
        }
    }
    if !spans_out.is_empty() {
        lines.push(Line::from(spans_out));
    }
    lines
}

fn roll_text(roll: Option<(Attr, u32)>) -> String {
    roll.map_or(String::new(), |(attr, dc)| {
        format!(" · {} {dc}", attr.label())
    })
}

fn draw_scene(
    cx: &Cx<'_>,
    frame: &mut DoorFrame<'_>,
    area: Rect,
    depth: u8,
    room: u8,
    hostile: bool,
    options: &[crate::game::OptionView],
) {
    let game = cx.game;
    let content = cx.content;
    let level = dungeon::generate(content, game.season, game.epoch, depth);
    let Some(template) = level.template(content, room) else {
        return;
    };
    frame.render_widget(Clear, area);
    let lit = hostile;
    frame.render_widget(panel(&template.name, !lit), area);
    let body = inner(area);
    if body.width < 12 || body.height < 3 {
        return;
    }
    let w = usize::from(body.width);
    let mut feats: Vec<Line<'static>> = Vec::new();
    if let Some((_, shortcut)) = game
        .ways_down(content)
        .into_iter()
        .find(|(d, _)| *d == depth)
    {
        feats.push(Line::styled(
            clip(
                &format!("A {} runs from here to the surface.", shortcut.label()),
                w,
            ),
            Style::new().fg(MOSS),
        ));
    }
    if hostile {
        feats.push(Line::styled(
            clip("A pack bars the way out of this room.", w),
            Style::new().fg(BLOOD).add_modifier(Modifier::BOLD),
        ));
    }
    for f in &game.world.fallen {
        if f.depth == depth && f.room == room && f.rescued_by.is_none() && f.name != game.hero.name
        {
            feats.push(Line::styled(
                clip(
                    &format!("{} lies here, dead. They can be carried home.", f.name),
                    w,
                ),
                Style::new().fg(BONE),
            ));
        }
    }
    let mut opt_lines: Vec<Line<'static>> = Vec::new();
    for (i, o) in options.iter().enumerate().take(9) {
        let mark = match o.what {
            OptionWhat::Event(t)
                if level
                    .event(content, room)
                    .is_some_and(|e| !e.option[t].need.is_empty()) =>
            {
                if cx.look.ascii {
                    "*"
                } else {
                    "◆"
                }
            }
            OptionWhat::Template(t) if !template.option[t].need.is_empty() => {
                if cx.look.ascii {
                    "*"
                } else {
                    "◆"
                }
            }
            _ => " ",
        };
        let tail = format!("{}t{}", o.cost, roll_text(o.roll));
        let label = clip(&o.label, w.saturating_sub(tail.chars().count() + 9));
        let gap = w.saturating_sub(label.chars().count() + tail.chars().count() + 8);
        opt_lines.push(Line::from(vec![
            Span::styled(format!("[{}]", i + 1), cx.roles.accent),
            Span::styled(format!(" {mark} "), Style::new().fg(EMBER)),
            Span::styled(label, cx.roles.plain),
            Span::raw(" ".repeat(gap)),
            Span::styled(tail, cx.roles.dim),
        ]));
    }
    let closed = closed_options(cx.content, game, depth, room);
    let mut closed_lines: Vec<Line<'static>> = Vec::new();
    for (label, why) in closed.iter().take(3) {
        closed_lines.push(Line::from(vec![
            Span::styled(if cx.look.ascii { " x " } else { " ✗ " }, cx.roles.dim),
            Span::styled(clip(label, 20), cx.roles.dim),
            Span::styled(
                clip(&format!(" ({why})"), w.saturating_sub(24)),
                Style::new().fg(STONE),
            ),
        ]));
    }
    let exits = exits_line(cx, game, depth, room, hostile, w);

    // Spend the height: options and exits first, then the picture and the words.
    let mut room_left = usize::from(body.height);
    let take = |n: usize, room_left: &mut usize| -> usize {
        let t = n.min(*room_left);
        *room_left -= t;
        t
    };
    let n_opts = take(opt_lines.len(), &mut room_left);
    let n_exits = take(exits.len(), &mut room_left);
    let n_feats = take(feats.len(), &mut room_left);
    let n_closed = take(closed_lines.len(), &mut room_left);
    let art_rows = if cx.look.tall() && room_left > 9 {
        3
    } else {
        0
    };
    let art_rows = take(art_rows, &mut room_left);
    let gaps = usize::from(n_opts > 0) + usize::from(n_exits > 0 && n_opts > 0);
    let gaps = take(gaps, &mut room_left);
    let full = match level.event(content, room) {
        Some(e) => format!("{} {}", template.text, e.text),
        None => template.text.clone(),
    };
    let desc = wrap_to(&full, w, room_left.max(1));
    let n_desc = take(desc.len(), &mut room_left);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut y = body.y;
    if art_rows > 0 {
        let rows = u16::try_from(art_rows).unwrap_or(0);
        frame.render_widget(
            Vignette {
                kind: template.kind,
                look: cx.look,
                warmth: Torch::from_turns(game.hero.turns).warmth,
                seed: u32::from(room) * 31 + u32::from(depth),
            },
            Rect::new(body.x, y, body.width, rows),
        );
        y += rows;
    }
    for l in desc.iter().take(n_desc) {
        lines.push(Line::styled(l.clone(), cx.roles.plain));
    }
    lines.extend(feats.into_iter().take(n_feats));
    if gaps > 0 {
        lines.push(Line::raw(""));
    }
    lines.extend(opt_lines.into_iter().take(n_opts));
    lines.extend(closed_lines.into_iter().take(n_closed));
    if gaps > 1 || (gaps > 0 && n_exits > 0 && n_opts == 0) {
        lines.push(Line::raw(""));
    }
    lines.extend(exits.into_iter().take(n_exits));
    let rest = Rect::new(body.x, y, body.width, body.bottom().saturating_sub(y));
    frame.render_widget(Paragraph::new(lines), rest);
    let _ = (STONE_LIGHT, DEEP_BRIGHT, spans(&[]));
}
