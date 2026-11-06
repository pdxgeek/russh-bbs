//! The seasonal shared dungeon. A level is a pure function of `(season, epoch, depth)`: the same
//! seed gives every player the same rooms, and what changes between days (traps, packs) is a pure
//! function of the day as well. Nothing here is stored; only the *changes* heroes make (opened
//! shortcuts, slain guardians) live in the shared world.

use russh_bbs_door::DoorRng;

use crate::content::{Content, Monster, RoomKind, RoomTemplate, Trap};
use crate::rules::{BAND_LEVELS, DEPTH_MAX};

/// Grid width and height of a level.
pub(crate) const GRID_W: u8 = 5;
pub(crate) const GRID_H: u8 = 4;

/// A compass direction. Order is the order of [`Room::links`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dir {
    North,
    East,
    South,
    West,
}

impl Dir {
    pub(crate) const ALL: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::North => 0,
            Self::East => 1,
            Self::South => 2,
            Self::West => 3,
        }
    }

    pub(crate) const fn delta(self) -> (i8, i8) {
        match self {
            Self::North => (0, -1),
            Self::East => (1, 0),
            Self::South => (0, 1),
            Self::West => (-1, 0),
        }
    }

    pub(crate) const fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::East => Self::West,
            Self::South => Self::North,
            Self::West => Self::East,
        }
    }

    pub(crate) const fn word(self) -> &'static str {
        match self {
            Self::North => "north",
            Self::East => "east",
            Self::South => "south",
            Self::West => "west",
        }
    }

    pub(crate) const fn letter(self) -> char {
        match self {
            Self::North => 'n',
            Self::East => 'e',
            Self::South => 's',
            Self::West => 'w',
        }
    }
}

/// One room of a level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Room {
    pub(crate) x: u8,
    pub(crate) y: u8,
    /// Index into the room templates.
    pub(crate) template: usize,
    pub(crate) kind: RoomKind,
    /// The room reached by going north, east, south, west.
    pub(crate) links: [Option<u8>; 4],
    /// Index into the dungeon events: a scene laid over an ordinary room (M4-E4).
    pub(crate) event: Option<usize>,
}

/// A generated level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Level {
    pub(crate) depth: u8,
    pub(crate) rooms: Vec<Room>,
    pub(crate) entry: u8,
    pub(crate) exit: u8,
    pub(crate) shaft: u8,
}

/// The depth band (1 to 5) of a level.
pub(crate) const fn band_of(depth: u8) -> u8 {
    (depth.saturating_sub(1)) / BAND_LEVELS + 1
}

/// Whether a level's way down is held by a guardian.
pub(crate) const fn has_guardian(depth: u8) -> bool {
    depth > 0 && depth <= DEPTH_MAX && depth.is_multiple_of(BAND_LEVELS)
}

/// A seeded generator for a named purpose.
pub(crate) fn rng_for(purpose: &str, parts: &[i64]) -> DoorRng {
    let mut label = String::from("emberdeep/");
    label.push_str(purpose);
    for part in parts {
        label.push(':');
        label.push_str(&part.to_string());
    }
    DoorRng::from_label(&label, 0)
}

fn pick_template(
    content: &Content,
    kind: RoomKind,
    band: u8,
    used: &[usize],
    rng: &mut DoorRng,
) -> usize {
    let all: Vec<usize> = content
        .rooms
        .room
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == kind && r.bands.contains(&u32::from(band)))
        .map(|(i, _)| i)
        .collect();
    let fresh: Vec<usize> = all.iter().copied().filter(|i| !used.contains(i)).collect();
    let pool = if fresh.is_empty() { &all } else { &fresh };
    let at = below(rng, pool.len());
    pool.get(at).copied().unwrap_or(0)
}

/// A random number below `n`, as an index.
fn below(rng: &mut DoorRng, n: usize) -> usize {
    usize::try_from(rng.below(u32::try_from(n).unwrap_or(1))).unwrap_or(0)
}

type Links = Vec<[Option<u8>; 4]>;

/// Grow a random spanning tree on the grid with a few loops: room positions and links.
fn grow_layout(rng: &mut DoorRng) -> (Vec<(u8, u8)>, Links) {
    let target = 10 + below(rng, 4);
    let start = (
        u8::try_from(below(rng, usize::from(GRID_W))).unwrap_or(0),
        u8::try_from(below(rng, usize::from(GRID_H))).unwrap_or(0),
    );
    let mut cells = vec![start];
    let mut links: Links = vec![[None; 4]];
    let step = |at: (u8, u8), dir: Dir| -> Option<(u8, u8)> {
        let (dx, dy) = dir.delta();
        let x = u8::try_from(i16::from(at.0) + i16::from(dx)).ok()?;
        let y = u8::try_from(i16::from(at.1) + i16::from(dy)).ok()?;
        (x < GRID_W && y < GRID_H).then_some((x, y))
    };
    let mut guard = 0;
    while cells.len() < target && guard < 2_000 {
        guard += 1;
        let from = below(rng, cells.len());
        let dir = Dir::ALL[below(rng, 4)];
        let Some(to) = step(cells[from], dir) else {
            continue;
        };
        if cells.contains(&to) {
            continue;
        }
        let idx = cells.len();
        cells.push(to);
        links.push([None; 4]);
        links[from][dir.index()] = Some(u8::try_from(idx).unwrap_or(0));
        links[idx][dir.opposite().index()] = Some(u8::try_from(from).unwrap_or(0));
    }
    // A couple of loops, so a level is not only a tree.
    for _ in 0..3 {
        let from = below(rng, cells.len());
        let dir = Dir::ALL[below(rng, 4)];
        let found = step(cells[from], dir).and_then(|to| cells.iter().position(|c| *c == to));
        if let Some(to) = found
            && links[from][dir.index()].is_none()
        {
            links[from][dir.index()] = Some(u8::try_from(to).unwrap_or(0));
            links[to][dir.opposite().index()] = Some(u8::try_from(from).unwrap_or(0));
        }
    }
    (cells, links)
}

/// The kind of every room: the entry, the way down, the shaft, two or three lairs, a shrine, and
/// a mix for the rest.
fn assign_kinds(
    rng: &mut DoorRng,
    n: usize,
    exit: usize,
    shaft: usize,
    depth: u8,
) -> Vec<RoomKind> {
    const FILL: [RoomKind; 8] = [
        RoomKind::Corridor,
        RoomKind::Corridor,
        RoomKind::Corridor,
        RoomKind::Vault,
        RoomKind::Vault,
        RoomKind::Forge,
        RoomKind::Flooded,
        RoomKind::Shrine,
    ];
    let mut kinds: Vec<Option<RoomKind>> = vec![None; n];
    kinds[0] = Some(RoomKind::Entry);
    kinds[exit] = Some(if has_guardian(depth) {
        RoomKind::Guardian
    } else {
        RoomKind::Exit
    });
    kinds[shaft] = Some(RoomKind::Shaft);
    let mut rest: Vec<usize> = (0..n).filter(|&i| kinds[i].is_none()).collect();
    // Shuffle (Fisher-Yates) so lairs and the shrine land somewhere different each level.
    for i in (1..rest.len()).rev() {
        let j = below(rng, i + 1);
        rest.swap(i, j);
    }
    let lairs = 2 + below(rng, 2);
    for (k, &i) in rest.iter().enumerate() {
        kinds[i] = Some(match k.cmp(&lairs) {
            std::cmp::Ordering::Less => RoomKind::Lair,
            std::cmp::Ordering::Equal => RoomKind::Shrine,
            std::cmp::Ordering::Greater => FILL[below(rng, FILL.len())],
        });
    }
    kinds
        .into_iter()
        .map(|k| k.unwrap_or(RoomKind::Corridor))
        .collect()
}

/// Generate a level. Deterministic.
pub(crate) fn generate(content: &Content, season: u32, epoch: u16, depth: u8) -> Level {
    let mut rng = rng_for(
        "level",
        &[i64::from(season), i64::from(epoch), i64::from(depth)],
    );
    let band = band_of(depth);
    let (cells, links) = grow_layout(&mut rng);
    let n = cells.len();
    let dist = distances(&links, 0);
    let mut exit = 0usize;
    let mut far = 0u32;
    for (i, &d) in dist.iter().enumerate() {
        if d > far {
            far = d;
            exit = i;
        }
    }
    // The shaft sits about two thirds of the way out, never on the entry or the exit.
    let want = (far * 2).div_ceil(3);
    let shaft = (1..n)
        .filter(|&i| i != exit)
        .min_by_key(|&i| (dist[i].abs_diff(want), i))
        .unwrap_or(1);
    let kinds = assign_kinds(&mut rng, n, exit, shaft, depth);
    let mut used: Vec<usize> = Vec::new();
    let mut rooms = Vec::with_capacity(n);
    for (i, kind) in kinds.into_iter().enumerate() {
        let template = pick_template(content, kind, band, &used, &mut rng);
        used.push(template);
        rooms.push(Room {
            x: cells[i].0,
            y: cells[i].1,
            template,
            kind,
            links: links[i],
            event: None,
        });
    }
    lay_events(content, season, epoch, depth, band, &mut rooms);
    Level {
        depth,
        rooms,
        entry: 0,
        exit: u8::try_from(exit).unwrap_or(0),
        shaft: u8::try_from(shaft).unwrap_or(0),
    }
}

/// Percent of ordinary rooms that carry a dungeon event.
const EVENT_PCT: u32 = 40;

/// Lay events over ordinary rooms. A separate generator keeps the rooms themselves unchanged.
fn lay_events(content: &Content, season: u32, epoch: u16, depth: u8, band: u8, rooms: &mut [Room]) {
    let mut rng = rng_for(
        "events",
        &[i64::from(season), i64::from(epoch), i64::from(depth)],
    );
    let mut used: Vec<usize> = Vec::new();
    for room in rooms.iter_mut() {
        let eligible = matches!(
            room.kind,
            RoomKind::Corridor
                | RoomKind::Vault
                | RoomKind::Forge
                | RoomKind::Flooded
                | RoomKind::Shrine
        );
        if !eligible || rng.below(100) >= EVENT_PCT {
            continue;
        }
        let pool: Vec<usize> = content
            .events
            .event
            .iter()
            .enumerate()
            .filter(|(i, e)| {
                e.kinds.contains(&room.kind)
                    && e.bands.contains(&u32::from(band))
                    && !used.contains(i)
            })
            .map(|(i, _)| i)
            .collect();
        if pool.is_empty() {
            continue;
        }
        let pick = pool[below(&mut rng, pool.len())];
        used.push(pick);
        room.event = Some(pick);
    }
}

/// Steps from `from` to every room (unreachable: `u32::MAX`).
pub(crate) fn distances(links: &[[Option<u8>; 4]], from: usize) -> Vec<u32> {
    let mut dist = vec![u32::MAX; links.len()];
    dist[from] = 0;
    let mut queue = std::collections::VecDeque::from([from]);
    while let Some(at) = queue.pop_front() {
        for next in links[at].iter().flatten() {
            let next = usize::from(*next);
            if dist[next] == u32::MAX {
                dist[next] = dist[at] + 1;
                queue.push_back(next);
            }
        }
    }
    dist
}

impl Level {
    pub(crate) fn room(&self, at: u8) -> Option<&Room> {
        self.rooms.get(usize::from(at))
    }

    pub(crate) fn template<'a>(&self, content: &'a Content, at: u8) -> Option<&'a RoomTemplate> {
        self.room(at)
            .and_then(|r| content.rooms.room.get(r.template))
    }

    /// The dungeon event laid over a room, if any.
    pub(crate) fn event<'a>(
        &self,
        content: &'a Content,
        at: u8,
    ) -> Option<&'a crate::content_story::EventDef> {
        self.room(at)
            .and_then(|r| r.event)
            .and_then(|i| content.events.event.get(i))
    }

    /// Room indexes within a bitmask-friendly range (levels have at most 20 rooms).
    pub(crate) fn all_mask(&self) -> u32 {
        (0..self.rooms.len()).fold(0u32, |m, i| m | (1 << i))
    }
}

/// The trap in a room on a day, if any.
pub(crate) fn trap_at<'a>(
    content: &'a Content,
    level: &Level,
    season: u32,
    epoch: u16,
    room: u8,
    day: i64,
) -> Option<&'a Trap> {
    let template = level.template(content, room)?;
    if template.trap_pct == 0 {
        return None;
    }
    let mut rng = rng_for(
        "trap",
        &[
            i64::from(season),
            i64::from(epoch),
            i64::from(level.depth),
            i64::from(room),
            day,
        ],
    );
    if rng.below(100) >= template.trap_pct {
        return None;
    }
    let band = u32::from(band_of(level.depth));
    let pool: Vec<&Trap> = content
        .bestiary
        .trap
        .iter()
        .filter(|t| t.band == band)
        .collect();
    let at = below(&mut rng, pool.len());
    pool.get(at).copied()
}

/// The monsters in a room on a day: a lair's pack, a guardian and its company, or one stray.
pub(crate) fn pack_at<'a>(
    content: &'a Content,
    level: &Level,
    season: u32,
    epoch: u16,
    room: u8,
    day: i64,
) -> Vec<&'a Monster> {
    let band = u32::from(band_of(level.depth));
    let kind = level.room(room).map_or(RoomKind::Corridor, |r| r.kind);
    if kind == RoomKind::Guardian {
        let Some(guardian) = content.guardian(band) else {
            return Vec::new();
        };
        let mut pack = vec![guardian];
        pack.extend(guardian.adds.iter().filter_map(|id| content.monster(id)));
        return pack;
    }
    let mut rng = rng_for(
        "pack",
        &[
            i64::from(season),
            i64::from(epoch),
            i64::from(level.depth),
            i64::from(room),
            day,
        ],
    );
    let ordinary: Vec<&Monster> = content
        .bestiary
        .monster
        .iter()
        .filter(|m| !m.guardian && m.band == band)
        .collect();
    if ordinary.is_empty() {
        return Vec::new();
    }
    let count = if kind == RoomKind::Lair {
        1 + below(&mut rng, 2) + usize::from(level.depth >= 8 && rng.chance(1, 2))
    } else {
        1
    };
    (0..count)
        .filter_map(|_| {
            let at = below(&mut rng, ordinary.len());
            ordinary.get(at).copied()
        })
        .collect()
}
