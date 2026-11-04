//! The explored floor as a lit map: fog over what the hero has never seen, a warm light around the
//! hero that shrinks as the torch-turns run down, remembered rooms in cold blue, and the way
//! down. The light is a *picture* of the torch (it changes how rooms are drawn, never what the
//! hero can do), so the rules stay in the engine; the same facts are written out by
//! [`describe`] for the look-around.

use russh_bbs_door::buffer::Buffer;
use russh_bbs_door::layout::Rect;
use russh_bbs_door::style::{Color, Modifier, Style};
use russh_bbs_door::widgets::Widget;

use super::kit::{
    BLOOD, BONE, DEEP, DEEP_BRIGHT, EMBER, EMBER_HOT, Look, MOSS, STONE, STONE_DARK, STONE_DEEP,
    STONE_LIGHT, hash, heat, lerp,
};
use crate::content::{Content, RoomKind};
use crate::dungeon::{self, Dir, GRID_H, GRID_W, Level};
use crate::game::{Game, Place};
use crate::rules::TURNS_PER_DAY;

/// How a room stands with the hero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Sight {
    /// Never seen.
    Fog,
    /// Seen from a visited room, never entered.
    Seen,
    /// Entered (or revealed).
    Known,
}

/// What the map needs to know about one room.
#[derive(Debug, Clone)]
pub(super) struct RoomView {
    pub x: u8,
    pub y: u8,
    pub sight: Sight,
    pub here: bool,
    /// Rooms away from the hero on the grid (Manhattan).
    pub dist: i32,
    pub kind: RoomKind,
    pub exit: bool,
    pub entry: bool,
    pub shaft: bool,
    /// A pack or a guardian still bars it (only known for a known room).
    pub danger: bool,
    pub fallen: bool,
    pub links: [Option<u8>; 4],
    pub name: String,
}

/// The torch: how far the light reaches and how bright it burns, from the turns left.
#[derive(Debug, Clone, Copy)]
pub(super) struct Torch {
    /// Rooms the light reaches from the hero (0: only their own room).
    pub radius: i32,
    /// 0 (a guttering coal) to 1 (a fresh torch).
    pub warmth: f32,
}

impl Torch {
    pub(super) fn from_turns(turns: u32) -> Self {
        let radius = match turns {
            0 => 0,
            1..=5 => 1,
            6..=14 => 2,
            _ => 3,
        };
        Self {
            radius,
            warmth: super::kit::frac(i64::from(turns), i64::from(TURNS_PER_DAY)).max(0.15),
        }
    }
}

/// The rooms of the level the hero is on, as the map shows them.
pub(super) fn room_views(game: &Game, content: &Content, level: &Level, here: u8) -> Vec<RoomView> {
    let depth = level.depth;
    let explored = game
        .explored
        .get(usize::from(depth).saturating_sub(1))
        .copied()
        .unwrap_or(0);
    let known = |i: u8| explored & (1 << i) != 0;
    let seen = |i: u8| {
        known(i)
            || level.rooms.iter().enumerate().any(|(j, r)| {
                known(u8::try_from(j).unwrap_or(0)) && r.links.iter().flatten().any(|n| *n == i)
            })
    };
    let (hx, hy) = level.room(here).map_or((0, 0), |r| (r.x, r.y));
    level
        .rooms
        .iter()
        .enumerate()
        .map(|(at, r)| {
            let id = u8::try_from(at).unwrap_or(0);
            let sight = if known(id) || id == here {
                Sight::Known
            } else if seen(id) {
                Sight::Seen
            } else {
                Sight::Fog
            };
            let dist =
                (i32::from(r.x) - i32::from(hx)).abs() + (i32::from(r.y) - i32::from(hy)).abs();
            let danger = sight == Sight::Known && game.is_hostile(content, level, id);
            let fallen = sight != Sight::Fog
                && game.world.fallen.iter().any(|f| {
                    f.depth == depth
                        && f.room == id
                        && f.rescued_by.is_none()
                        && f.name != game.hero.name
                });
            RoomView {
                x: r.x,
                y: r.y,
                sight,
                here: id == here,
                dist,
                kind: r.kind,
                exit: id == level.exit,
                entry: id == level.entry,
                shaft: id == level.shaft,
                danger,
                fallen,
                links: r.links,
                name: level
                    .template(content, id)
                    .map_or_else(String::new, |t| t.name.clone()),
            }
        })
        .collect()
}

/// The letter a known room is drawn with, and what it stands for.
fn kind_glyph(v: &RoomView, ascii: bool) -> &'static str {
    if v.exit {
        return if ascii { ">" } else { "▼" };
    }
    if v.entry {
        return if ascii { "<" } else { "▲" };
    }
    if v.shaft {
        return "S";
    }
    match v.kind {
        RoomKind::Lair => "L",
        RoomKind::Guardian => "G",
        RoomKind::Vault => "$",
        RoomKind::Forge => "F",
        RoomKind::Flooded => "~",
        RoomKind::Shrine => "+",
        RoomKind::Corridor => {
            if ascii {
                "."
            } else {
                "·"
            }
        }
        RoomKind::Entry | RoomKind::Exit | RoomKind::Shaft => "#",
    }
}

/// The map drawn at one of two scales.
pub(super) struct MapWidget<'a> {
    pub rooms: &'a [RoomView],
    pub torch: Torch,
    pub look: Look,
    /// Rooms drawn as three-row boxes (needs 37 by 15 cells); otherwise one-row `[x]` cells.
    pub big: bool,
}

impl MapWidget<'_> {
    /// Columns and rows the map needs at a scale.
    pub(super) const fn size(big: bool) -> (u16, u16) {
        if big {
            (GRID_W as u16 * 8 - 3, GRID_H as u16 * 4 - 1)
        } else {
            (GRID_W as u16 * 6 - 3, GRID_H as u16 * 2 - 1)
        }
    }

    fn colors(&self, v: &RoomView) -> (Style, Style, Option<Color>) {
        // (frame, glyph, glow)
        let lit = v.dist <= self.torch.radius;
        let w = self.torch.warmth;
        if v.here {
            let glow = lerp(STONE_DEEP, Color::Rgb(86, 36, 16), 0.4 + 0.6 * w);
            return (
                Style::new().fg(heat(0.80 + 0.2 * w)),
                Style::new().fg(EMBER_HOT).add_modifier(Modifier::BOLD),
                Some(glow),
            );
        }
        match v.sight {
            Sight::Fog => (
                Style::new().fg(STONE_DEEP),
                Style::new().fg(STONE_DEEP),
                None,
            ),
            Sight::Seen => {
                if lit {
                    let t = 0.35 * w;
                    (
                        Style::new().fg(heat(t + 0.2)),
                        Style::new().fg(heat(t + 0.25)),
                        None,
                    )
                } else {
                    (Style::new().fg(STONE_DARK), Style::new().fg(DEEP), None)
                }
            }
            Sight::Known => {
                if lit {
                    let falloff = 1.0 - (v.dist as f32 / (self.torch.radius as f32 + 1.5));
                    let t = (0.25 + 0.6 * falloff) * (0.4 + 0.6 * w);
                    let glyph = if v.danger { BLOOD } else { heat(t + 0.15) };
                    (
                        Style::new().fg(heat(t)),
                        Style::new().fg(glyph),
                        Some(lerp(STONE_DEEP, Color::Rgb(70, 30, 14), t.clamp(0.0, 1.0))),
                    )
                } else {
                    let glyph = if v.danger {
                        lerp(BLOOD, STONE_DARK, 0.5)
                    } else {
                        STONE
                    };
                    (Style::new().fg(DEEP), Style::new().fg(glyph), None)
                }
            }
        }
    }

    fn link_style(&self, a: &RoomView, b: &RoomView) -> Style {
        let lit = a.dist.min(b.dist) <= self.torch.radius;
        if lit && a.sight != Sight::Fog && b.sight != Sight::Fog {
            Style::new().fg(heat(0.30 + 0.25 * self.torch.warmth))
        } else {
            Style::new().fg(STONE_DARK)
        }
    }

    fn fog(&self, buf: &mut Buffer, area: Rect) {
        // The whole grid is textured, rooms or not, so the fog never says where a room is.
        for y in 0..area.height {
            for x in 0..area.width {
                let h = hash(u32::from(x), u32::from(y), self.look.phase / 7);
                let g = match h % 23 {
                    0 => Some(if self.look.ascii { "." } else { "░" }),
                    1 => Some(if self.look.ascii { "," } else { "·" }),
                    _ => None,
                };
                if let Some(g) = g {
                    buf.set_string(area.x + x, area.y + y, g, Style::new().fg(STONE_DEEP));
                }
            }
        }
    }
}

impl Widget for MapWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (mw, mh) = Self::size(self.big);
        if area.width < mw || area.height < mh {
            return;
        }
        let origin = Rect::new(
            area.x + (area.width - mw) / 2,
            area.y + (area.height - mh) / 2,
            mw,
            mh,
        );
        self.fog(buf, origin);
        let (sx, sy) = if self.big { (8u16, 4u16) } else { (6, 2) };
        let at = |x: u8, y: u8| -> Option<&RoomView> {
            self.rooms.iter().find(|r| r.x == x && r.y == y)
        };
        for v in self.rooms {
            if v.sight == Sight::Fog {
                continue;
            }
            let cx = origin.x + u16::from(v.x) * sx;
            let cy = origin.y + u16::from(v.y) * sy;
            let (frame, glyph, glow) = self.colors(v);
            let g = if v.here {
                "@"
            } else if v.sight == Sight::Seen {
                "?"
            } else {
                kind_glyph(v, self.look.ascii)
            };
            if self.big {
                let (tl, tr, bl, br, h, vt) = if self.look.ascii {
                    ("+", "+", "+", "+", "-", "|")
                } else {
                    ("╭", "╮", "╰", "╯", "─", "│")
                };
                buf.set_string(cx, cy, format!("{tl}{h}{h}{h}{tr}"), frame);
                buf.set_string(cx, cy + 1, vt, frame);
                buf.set_string(cx + 4, cy + 1, vt, frame);
                buf.set_string(cx, cy + 2, format!("{bl}{h}{h}{h}{br}"), frame);
                if let Some(bg) = glow {
                    for dx in 1..4 {
                        if let Some(c) = buf.cell_mut((cx + dx, cy + 1)) {
                            c.set_bg(bg);
                        }
                    }
                }
                let left = if v.fallen {
                    if self.look.ascii { "x" } else { "†" }
                } else {
                    " "
                };
                let right = if v.danger && !matches!(v.kind, RoomKind::Lair | RoomKind::Guardian) {
                    "!"
                } else {
                    " "
                };
                buf.set_string(cx + 1, cy + 1, left, Style::new().fg(BONE));
                buf.set_string(cx + 2, cy + 1, g, glyph);
                buf.set_string(cx + 3, cy + 1, right, Style::new().fg(BLOOD));
            } else {
                let (l, r) = if v.here { ("<", ">") } else { ("[", "]") };
                buf.set_string(cx, cy, l, frame);
                buf.set_string(cx + 2, cy, r, frame);
                buf.set_string(cx + 1, cy, g, glyph);
                if let Some(bg) = glow
                    && let Some(c) = buf.cell_mut((cx + 1, cy))
                {
                    c.set_bg(bg);
                }
                if v.fallen {
                    buf.set_string(
                        cx + 3,
                        cy,
                        if self.look.ascii { "x" } else { "†" },
                        Style::new().fg(BONE),
                    );
                }
            }
            // Links to the east and the south, drawn once, from the room on the west or north.
            for (dir, link) in [
                (Dir::East, v.links[Dir::East.index()]),
                (Dir::South, v.links[Dir::South.index()]),
            ] {
                let Some(n) = link else { continue };
                let Some(other) = at(
                    (i16::from(v.x) + i16::from(dir.delta().0))
                        .try_into()
                        .unwrap_or(0),
                    (i16::from(v.y) + i16::from(dir.delta().1))
                        .try_into()
                        .unwrap_or(0),
                ) else {
                    continue;
                };
                let _ = n;
                if other.sight == Sight::Fog {
                    continue;
                }
                let style = self.link_style(v, other);
                match dir {
                    Dir::East => {
                        let (ox, oy, len) = if self.big {
                            (cx + 5, cy + 1, 3)
                        } else {
                            (cx + 3, cy, 3)
                        };
                        for i in 0..len {
                            buf.set_string(
                                ox + i,
                                oy,
                                if self.look.ascii { "-" } else { "─" },
                                style,
                            );
                        }
                    }
                    Dir::South => {
                        let (ox, oy, len) = if self.big {
                            (cx + 2, cy + 3, 1)
                        } else {
                            (cx + 1, cy + 1, 1)
                        };
                        for i in 0..len {
                            buf.set_string(
                                ox,
                                oy + i,
                                if self.look.ascii { "|" } else { "│" },
                                style,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// The map in words, for the look-around and screen readers.
pub(crate) fn describe(game: &Game, content: &Content) -> Vec<String> {
    let Place::Delve { depth, room } = game.place else {
        return vec!["There is no map in town.".to_owned()];
    };
    let level = dungeon::generate(content, game.season, game.epoch, depth);
    let views = room_views(game, content, &level, room);
    let torch = Torch::from_turns(game.hero.turns);
    let band = dungeon::band_of(depth);
    let known = views.iter().filter(|v| v.sight == Sight::Known).count();
    let mut out = vec![format!(
        "Map of level {depth}, band {band}: {known} of {} rooms explored. The torch reaches {} room(s).",
        views.len(),
        torch.radius
    )];
    if let Some(v) = views.iter().find(|v| v.here) {
        out.push(format!(
            "You stand in {} (column {}, row {} of a {GRID_W} by {GRID_H} grid, north is up).",
            v.name,
            v.x + 1,
            v.y + 1
        ));
        for dir in Dir::ALL {
            if let Some(n) = v.links[dir.index()]
                && let Some(w) = views.get(usize::from(n))
            {
                out.push(format!(
                    "  {}: {}{}",
                    dir.word(),
                    if w.sight == Sight::Known {
                        w.name.as_str()
                    } else {
                        "unexplored"
                    },
                    if w.danger { " (a pack is there)" } else { "" }
                ));
            }
        }
    }
    for v in &views {
        if v.sight != Sight::Known || v.here {
            continue;
        }
        let mut notes = Vec::new();
        if v.exit {
            notes.push("the way down");
        }
        if v.entry {
            notes.push("the way up");
        }
        if v.shaft {
            notes.push("a shaft");
        }
        if v.danger {
            notes.push("a pack bars it");
        }
        if v.fallen {
            notes.push("a fallen hero lies here");
        }
        out.push(format!(
            "  Known: {} at column {}, row {}{}",
            v.name,
            v.x + 1,
            v.y + 1,
            if notes.is_empty() {
                String::new()
            } else {
                format!(" ({})", notes.join(", "))
            }
        ));
    }
    let way_down = views.iter().any(|v| v.exit && v.sight != Sight::Fog);
    out.push(if way_down {
        "The way down has been found.".to_owned()
    } else {
        "The way down has not been found yet.".to_owned()
    });
    out
}

/// The legend under the map, as `(glyph, meaning)` pairs.
pub(super) fn legend(look: &Look) -> Vec<(&'static str, &'static str, Color)> {
    vec![
        ("@", "you", EMBER_HOT),
        (glyph_of(look, "▼", ">"), "way down", EMBER),
        (glyph_of(look, "▲", "<"), "way up", DEEP_BRIGHT),
        ("L", "lair", BLOOD),
        ("G", "guardian", BLOOD),
        ("S", "shaft", MOSS),
        ("$", "vault", STONE_LIGHT),
        ("F", "forge", EMBER),
        ("~", "flooded", DEEP_BRIGHT),
        ("+", "shrine", BONE),
        ("?", "unvisited", DEEP),
    ]
}

fn glyph_of(look: &Look, uni: &'static str, ascii: &'static str) -> &'static str {
    if look.ascii { ascii } else { uni }
}
