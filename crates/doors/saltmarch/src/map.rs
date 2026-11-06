//! The marsh: a seasonal map generated from the board's seed.
//!
//! The same seed always gives the same map, so nothing about the terrain is stored: a save
//! keeps the seed and the clan's charted tiles. Generation is layered so each step can be
//! tested alone:
//!
//! 1. two smooth noise fields (height and wetness), drawn from the seeded RNG only;
//! 2. heights are ranked and mapped onto a fixed curve, so every seed has the same share of
//!    mud, water and high ground;
//! 3. the Tidecourt is raised at the south edge, the Deep fills the north rows;
//! 4. hold sites are picked on high ground and a causeway-wide track of firm mud is carved
//!    from each to the Tidecourt, so every hold can be reached on foot at an ordinary low tide;
//! 5. points of interest are placed from templates.

use std::cmp::Reverse;

use russh_bbs_door::DoorRng;
use serde::Deserialize;

use crate::content::PoiTemplate;
use crate::tide::{Access, WET_BAND};

/// The map is `SIZE` by `SIZE` tiles.
pub const SIZE: usize = 64;

/// How far from the Tidecourt a dredged channel is dredged, in tiles.
pub const DREDGE_RADIUS: i32 = 14;

/// The elevation a dredged channel is raised to (the flats' own, so it is dry at a low).
pub const DREDGE_FLOOR: u8 = 36;

/// The northern rows that make up the Deep.
pub const DEEP_ROWS: usize = 6;

/// Where the Tidecourt stands.
pub const TIDECOURT: (u8, u8) = (32, 60);

/// The most hold sites placed.
pub const MAX_SITES: usize = 24;

/// The fewest hold sites a map is allowed to have (the generator raises ground if it must).
pub const MIN_SITES: usize = 12;

/// Hold sites are at least this far from each other (Chebyshev distance).
pub const SITE_SPACING: i32 = 5;

/// The elevation every carved track is raised to, at least: above the lowest low of a neap
/// tide, so it is dry at an ordinary low water.
pub const TRACK_ELEVATION: u8 = 32;

/// What a tile is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TileKind {
    /// Soft grey mud.
    Mudflat,
    /// Dense reed, slow going.
    Reedbed,
    /// Peat bog.
    PeatBog,
    /// Shallow beds that dry to salt.
    SaltPan,
    /// A tidal channel.
    Channel,
    /// High ground.
    Island,
    /// A street of the old coast.
    DrownedTown,
    /// A built causeway (placed by players in later slices; never generated).
    Causeway,
    /// A wreck.
    Wreck,
    /// A deep pool.
    Mere,
    /// The far edge.
    Deep,
}

impl TileKind {
    /// Every kind.
    pub const ALL: [Self; 11] = [
        Self::Mudflat,
        Self::Reedbed,
        Self::PeatBog,
        Self::SaltPan,
        Self::Channel,
        Self::Island,
        Self::DrownedTown,
        Self::Causeway,
        Self::Wreck,
        Self::Mere,
        Self::Deep,
    ];

    /// The display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mudflat => "mudflat",
            Self::Reedbed => "reedbed",
            Self::PeatBog => "peat bog",
            Self::SaltPan => "salt pan",
            Self::Channel => "channel",
            Self::Island => "island",
            Self::DrownedTown => "drowned town",
            Self::Causeway => "causeway",
            Self::Wreck => "wreck",
            Self::Mere => "mere",
            Self::Deep => "the Deep",
        }
    }

    /// One ASCII glyph for the plainest tier.
    #[must_use]
    pub const fn glyph(self) -> char {
        match self {
            Self::Mudflat => '.',
            Self::Reedbed => '"',
            Self::PeatBog => ',',
            Self::SaltPan => ':',
            Self::Channel => '~',
            Self::Island => '^',
            Self::DrownedTown => '#',
            Self::Causeway => '=',
            Self::Wreck => 'x',
            Self::Mere => 'o',
            Self::Deep => '%',
        }
    }

    /// Turns to enter this kind of tile when it is dry (before the water adds to it).
    #[must_use]
    pub const fn base_cost(self) -> u8 {
        match self {
            Self::Reedbed | Self::PeatBog | Self::DrownedTown | Self::Deep => 2,
            _ => 1,
        }
    }
}

/// What sort of place a point of interest is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PoiKind {
    /// A camp of travellers.
    Camp,
    /// A shrine.
    Shrine,
    /// A place to salvage.
    Salvage,
    /// A creature's lair.
    Lair,
    /// A hermit.
    Hermit,
    /// A trader.
    Trader,
    /// A ruin with rooms.
    Ruin,
}

/// How far from the Tidecourt a template wants to stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Band {
    /// Within 18 tiles.
    Near,
    /// 19 to 38 tiles.
    Mid,
    /// Further than 38 tiles (outside the Deep).
    Far,
    /// In the Deep.
    Deep,
}

/// One tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    /// What it is.
    pub kind: TileKind,
    /// How high it stands, 0 to 100 (the tide's scale).
    pub elevation: u8,
}

/// A placed point of interest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Poi {
    /// Column.
    pub x: u8,
    /// Row.
    pub y: u8,
    /// The index of its template in the catalog.
    pub template: u16,
    /// What sort of place it is.
    pub kind: PoiKind,
}

/// A generated map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Map {
    /// The seed it came from.
    pub seed: u64,
    tiles: Vec<Tile>,
    /// The Tidecourt.
    pub tidecourt: (u8, u8),
    /// Places a clan may stake a hold.
    pub sites: Vec<(u8, u8)>,
    /// Placed points of interest.
    pub pois: Vec<Poi>,
    /// Causeways laid by clans, as this player meets them (the world's list, with the toll
    /// this player would pay). Not generated: see [`Map::lay`].
    pub laid: Vec<Laid>,
}

/// A causeway tile laid by a clan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Laid {
    /// Column.
    pub x: u8,
    /// Row.
    pub y: u8,
    /// The builder's clan name.
    pub owner: String,
    /// The toll this player pays to cross it, in tallies (zero for the builder and for a pact
    /// partner who shares causeways).
    pub toll: u8,
    /// Whether the player's own clan laid it.
    pub mine: bool,
}

/// The ground a causeway may be laid on.
#[must_use]
pub const fn can_lay_on(kind: TileKind) -> bool {
    matches!(
        kind,
        TileKind::Mudflat | TileKind::Channel | TileKind::PeatBog | TileKind::SaltPan
    )
}

fn lattice(rng: &mut DoorRng, n: usize) -> Vec<u32> {
    (0..n * n).map(|_| rng.below(256)).collect()
}

/// Smooth value noise at `(x, y)`: a lattice of `n` by `n` points over the map, bilinearly
/// interpolated with a smoothstep. Result 0 to 255.
fn sample(lat: &[u32], n: usize, x: usize, y: usize) -> u32 {
    let cell = SIZE / (n - 1);
    let (gx, gy) = ((x / cell).min(n - 2), (y / cell).min(n - 2));
    let ease = |f: usize| {
        let f = u32::try_from(f * 256 / cell).unwrap_or(0);
        f * f * (3 * 256 - 2 * f) / (256 * 256)
    };
    let (fx, fy) = (ease(x % cell), ease(y % cell));
    let at = |cx: usize, cy: usize| lat[cy * n + cx];
    let top = at(gx, gy) * (256 - fx) + at(gx + 1, gy) * fx;
    let bottom = at(gx, gy + 1) * (256 - fx) + at(gx + 1, gy + 1) * fx;
    (top * (256 - fy) + bottom * fy) >> 16
}

fn field(rng: &mut DoorRng) -> Vec<u32> {
    let octaves = [(5usize, 5u32), (9, 3), (17, 2)];
    let lats: Vec<Vec<u32>> = octaves.iter().map(|(n, _)| lattice(rng, *n)).collect();
    (0..SIZE * SIZE)
        .map(|i| {
            let (x, y) = (i % SIZE, i / SIZE);
            octaves
                .iter()
                .zip(&lats)
                .map(|((n, w), lat)| sample(lat, *n, x, y) * w)
                .sum::<u32>()
                / 10
        })
        .collect()
}

/// Replace each value by its rank mapped through a piecewise-linear curve of
/// `(percentile, result)` points. Ties break by position, so the result is a pure function of
/// the input order.
fn by_rank(values: &[u32], curve: &[(usize, u8)]) -> Vec<u8> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by_key(|&i| (values[i], i));
    let mut out = vec![0u8; values.len()];
    let total = values.len();
    for (rank, &i) in order.iter().enumerate() {
        let pct = rank * 100 / total;
        let upper = curve
            .iter()
            .position(|(p, _)| *p >= pct)
            .unwrap_or(curve.len() - 1);
        let (p1, v1) = curve[upper];
        let (p0, v0) = if upper == 0 { (0, 0) } else { curve[upper - 1] };
        let span = (p1 - p0).max(1);
        let rise = i32::from(v1) - i32::from(v0);
        let step = i32::try_from((rank * 100 - p0 * total) * 1000 / (span * total)).unwrap_or(0);
        out[i] = u8::try_from((i32::from(v0) + rise * step / 1000).clamp(0, 100)).unwrap_or(100);
    }
    out
}

/// Elevation by percentile: 5% meres, 7% channels, 28% mud, 12% pans, 36% reed and peat, 12%
/// high ground.
const ELEVATION_CURVE: [(usize, u8); 7] = [
    (0, 0),
    (5, 8),
    (12, 18),
    (40, 35),
    (52, 48),
    (88, 62),
    (100, 100),
];

const MOISTURE_CURVE: [(usize, u8); 2] = [(0, 0), (100, 100)];

fn classify(y: usize, elevation: u8, moisture: u8) -> TileKind {
    if y < DEEP_ROWS {
        return TileKind::Deep;
    }
    match elevation {
        0..=7 => TileKind::Mere,
        8..=17 => TileKind::Channel,
        35..=47 if moisture < 50 => TileKind::SaltPan,
        18..=47 => TileKind::Mudflat,
        48..=61 if moisture >= 55 => TileKind::PeatBog,
        48..=61 => TileKind::Reedbed,
        _ => TileKind::Island,
    }
}

fn chebyshev(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

fn band_of(x: usize, y: usize) -> Band {
    if y < DEEP_ROWS {
        return Band::Deep;
    }
    let d = chebyshev(
        (i32::try_from(x).unwrap_or(0), i32::try_from(y).unwrap_or(0)),
        (i32::from(TIDECOURT.0), i32::from(TIDECOURT.1)),
    );
    match d {
        ..=18 => Band::Near,
        19..=38 => Band::Mid,
        _ => Band::Far,
    }
}

impl Map {
    /// Generate the map for `seed`, placing points of interest from `templates`.
    #[must_use]
    pub fn generate(seed: u64, templates: &[PoiTemplate]) -> Self {
        let mut rng = DoorRng::from_label("saltmarch-map", i64::from_ne_bytes(seed.to_ne_bytes()));
        let height = field(&mut rng);
        let wet = field(&mut rng);
        let mut elevation = by_rank(&height, &ELEVATION_CURVE);
        let moisture = by_rank(&wet, &MOISTURE_CURVE);
        // The Tidecourt stands on a raised patch.
        let (tx, ty) = (usize::from(TIDECOURT.0), usize::from(TIDECOURT.1));
        for y in ty - 1..=ty + 1 {
            for x in tx - 1..=tx + 1 {
                elevation[y * SIZE + x] = 80;
            }
        }
        // The Deep is low: it is dry only at the lowest springs.
        for e in elevation.iter_mut().take(DEEP_ROWS * SIZE) {
            *e = (*e).min(38);
        }
        let kinds = |elevation: &[u8]| -> Vec<TileKind> {
            (0..SIZE * SIZE)
                .map(|i| classify(i / SIZE, elevation[i], moisture[i]))
                .collect()
        };
        let first = kinds(&elevation);
        let sites = Self::pick_sites(&first, &mut elevation, &mut rng);
        for &(sx, sy) in &sites {
            Self::carve(&mut elevation, (usize::from(sx), usize::from(sy)), &mut rng);
        }
        let after = kinds(&elevation);
        let tiles = (0..SIZE * SIZE)
            .map(|i| Tile {
                kind: after[i],
                elevation: elevation[i],
            })
            .collect();
        let mut map = Self {
            seed,
            tiles,
            tidecourt: TIDECOURT,
            sites,
            pois: Vec::new(),
            laid: Vec::new(),
        };
        map.place_pois(templates, &mut rng);
        map
    }

    /// Hold sites: high ground away from the Tidecourt and the Deep, spaced apart. If the
    /// noise left too little high ground, ground is raised at random spots until there are
    /// [`MIN_SITES`].
    fn pick_sites(kinds: &[TileKind], elevation: &mut [u8], rng: &mut DoorRng) -> Vec<(u8, u8)> {
        let ok_row = |y: usize| (DEEP_ROWS + 8..SIZE - 4).contains(&y);
        let far_from_court = |x: usize, y: usize| {
            chebyshev(
                (i32::try_from(x).unwrap_or(0), i32::try_from(y).unwrap_or(0)),
                (i32::from(TIDECOURT.0), i32::from(TIDECOURT.1)),
            ) >= 6
        };
        let mut candidates: Vec<(usize, usize)> = (0..SIZE * SIZE)
            .filter(|&i| {
                kinds[i] == TileKind::Island
                    && ok_row(i / SIZE)
                    && far_from_court(i % SIZE, i / SIZE)
            })
            .map(|i| (i % SIZE, i / SIZE))
            .collect();
        // Fisher-Yates shuffle from the seeded RNG.
        for i in (1..candidates.len()).rev() {
            let j = usize::try_from(rng.below(u32::try_from(i + 1).unwrap_or(1))).unwrap_or(0);
            candidates.swap(i, j);
        }
        let mut sites: Vec<(usize, usize)> = Vec::new();
        let spaced = |sites: &[(usize, usize)], x: usize, y: usize| {
            sites.iter().all(|&(sx, sy)| {
                chebyshev(
                    (i32::try_from(x).unwrap_or(0), i32::try_from(y).unwrap_or(0)),
                    (
                        i32::try_from(sx).unwrap_or(0),
                        i32::try_from(sy).unwrap_or(0),
                    ),
                ) >= SITE_SPACING
            })
        };
        for (x, y) in candidates {
            if sites.len() >= MAX_SITES {
                break;
            }
            if spaced(&sites, x, y) {
                sites.push((x, y));
            }
        }
        let mut guard = 0;
        while sites.len() < MIN_SITES && guard < 10_000 {
            guard += 1;
            let x = usize::try_from(rng.range(2, 61)).unwrap_or(2);
            let y = usize::try_from(rng.range(
                u32::try_from(DEEP_ROWS + 8).unwrap_or(14),
                u32::try_from(SIZE - 5).unwrap_or(58),
            ))
            .unwrap_or(14);
            if far_from_court(x, y) && spaced(&sites, x, y) {
                elevation[y * SIZE + x] = 75;
                sites.push((x, y));
            }
        }
        sites.sort_unstable_by_key(|&(x, y)| (y, x));
        sites
            .into_iter()
            .map(|(x, y)| (u8::try_from(x).unwrap_or(0), u8::try_from(y).unwrap_or(0)))
            .collect()
    }

    /// Raise a walkable track from `from` to the Tidecourt, one orthogonal step at a time, so
    /// every step of it is dry at an ordinary low water.
    fn carve(elevation: &mut [u8], from: (usize, usize), rng: &mut DoorRng) {
        let (mut x, mut y) = from;
        let target = (usize::from(TIDECOURT.0), usize::from(TIDECOURT.1));
        while (x, y) != target {
            let (dx, dy) = (target.0.abs_diff(x), target.1.abs_diff(y));
            let along_x = dy == 0
                || (dx != 0
                    && rng.chance(
                        u32::try_from(dx).unwrap_or(1),
                        u32::try_from(dx + dy).unwrap_or(1),
                    ));
            if along_x {
                x = if target.0 > x { x + 1 } else { x - 1 };
            } else {
                y = if target.1 > y { y + 1 } else { y - 1 };
            }
            let floor = TRACK_ELEVATION + u8::try_from(rng.below(6)).unwrap_or(0);
            let e = &mut elevation[y * SIZE + x];
            *e = (*e).max(floor);
        }
    }

    fn place_pois(&mut self, templates: &[PoiTemplate], rng: &mut DoorRng) {
        let mut taken: Vec<(i32, i32)> = self
            .sites
            .iter()
            .map(|&(x, y)| (i32::from(x), i32::from(y)))
            .collect();
        taken.push((i32::from(TIDECOURT.0), i32::from(TIDECOURT.1)));
        for (index, template) in templates.iter().enumerate() {
            for _ in 0..template.count {
                for _ in 0..200 {
                    let x =
                        usize::try_from(rng.below(u32::try_from(SIZE).unwrap_or(64))).unwrap_or(0);
                    let y =
                        usize::try_from(rng.below(u32::try_from(SIZE).unwrap_or(64))).unwrap_or(0);
                    let pos = (i32::try_from(x).unwrap_or(0), i32::try_from(y).unwrap_or(0));
                    let tile = self.tiles[y * SIZE + x];
                    let fits = band_of(x, y) == template.band
                        && template.tiles.contains(&tile.kind)
                        && taken.iter().all(|t| chebyshev(*t, pos) >= 3)
                        && (x, y) != (usize::from(TIDECOURT.0), usize::from(TIDECOURT.1));
                    if !fits {
                        continue;
                    }
                    if let Some(kind) = template.paints {
                        // Drowned towns stand dry only at spring lows; wrecks lie a little higher.
                        let (lo, hi) = if kind == TileKind::DrownedTown {
                            (14, 28)
                        } else {
                            (20, 34)
                        };
                        self.tiles[y * SIZE + x] = Tile {
                            kind,
                            elevation: tile.elevation.clamp(lo, hi),
                        };
                    }
                    taken.push(pos);
                    self.pois.push(Poi {
                        x: u8::try_from(x).unwrap_or(0),
                        y: u8::try_from(y).unwrap_or(0),
                        template: u16::try_from(index).unwrap_or(0),
                        kind: template.kind,
                    });
                    break;
                }
            }
        }
    }

    /// The tile at `(x, y)`, or `None` off the map.
    #[must_use]
    pub fn tile(&self, x: usize, y: usize) -> Option<Tile> {
        (x < SIZE && y < SIZE).then(|| self.tiles[y * SIZE + x])
    }

    /// Lay a causeway tile: the ground becomes a causeway that the water never covers. Returns
    /// `false` (and changes nothing) if the ground cannot take one, or a place stands there.
    pub fn lay(&mut self, laid: Laid) -> bool {
        let (x, y) = (usize::from(laid.x), usize::from(laid.y));
        let Some(tile) = self.tile(x, y) else {
            return false;
        };
        if !can_lay_on(tile.kind) || self.poi_at(laid.x, laid.y).is_some() {
            return false;
        }
        self.tiles[y * SIZE + x] = Tile {
            kind: TileKind::Causeway,
            elevation: 100,
        };
        self.laid.push(laid);
        true
    }

    /// The causeway laid at `(x, y)`.
    #[must_use]
    pub fn laid_at(&self, x: u8, y: u8) -> Option<&Laid> {
        self.laid.iter().find(|l| l.x == x && l.y == y)
    }

    /// The Deep tile nearest the Tidecourt: the gate of the Deep.
    #[must_use]
    pub fn deep_gate(&self) -> Option<(u8, u8)> {
        let (tx, ty) = (i32::from(self.tidecourt.0), i32::from(self.tidecourt.1));
        self.iter()
            .filter(|(_, t)| t.kind == TileKind::Deep)
            .min_by_key(|((x, y), _)| {
                let (dx, dy) = (
                    i32::try_from(*x).unwrap_or(0) - tx,
                    i32::try_from(*y).unwrap_or(0) - ty,
                );
                (dx * dx + dy * dy, *y, *x)
            })
            .map(|((x, y), _)| (u8::try_from(x).unwrap_or(0), u8::try_from(y).unwrap_or(0)))
    }

    /// Apply what quests have done to the marsh for the whole board: `flags` is the world's
    /// bit set of [`crate::story::Flag`]. `templates` names the points of interest.
    pub fn apply_flags(&mut self, flags: u8, templates: &[PoiTemplate]) {
        use crate::story::Flag;
        if flags & Flag::Dredge.bit() != 0 {
            let (tx, ty) = (i32::from(self.tidecourt.0), i32::from(self.tidecourt.1));
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let (dx, dy) = (
                        i32::try_from(x).unwrap_or(0) - tx,
                        i32::try_from(y).unwrap_or(0) - ty,
                    );
                    let near = dx * dx + dy * dy <= DREDGE_RADIUS * DREDGE_RADIUS;
                    let tile = self.tiles[y * SIZE + x];
                    if near && tile.kind == TileKind::Channel {
                        self.tiles[y * SIZE + x] = Tile {
                            kind: TileKind::Mudflat,
                            elevation: tile.elevation.max(DREDGE_FLOOR),
                        };
                    }
                }
            }
        }
        for (flag, template) in [
            (Flag::ClearEels, "eel-lair"),
            (Flag::ClearHags, "bog-hag-lair"),
        ] {
            if flags & flag.bit() != 0 {
                self.pois.retain(|p| {
                    templates
                        .get(usize::from(p.template))
                        .is_none_or(|t| t.id != template)
                });
            }
        }
    }

    /// The point of interest at `(x, y)`.
    #[must_use]
    pub fn poi_at(&self, x: u8, y: u8) -> Option<&Poi> {
        self.pois.iter().find(|p| p.x == x && p.y == y)
    }

    /// How many tiles of each kind there are, in [`TileKind::ALL`] order.
    #[must_use]
    pub fn census(&self) -> [usize; 11] {
        let mut out = [0; 11];
        for t in &self.tiles {
            let i = TileKind::ALL.iter().position(|k| *k == t.kind).unwrap_or(0);
            out[i] += 1;
        }
        out
    }

    /// Every tile with its coordinates.
    pub fn iter(&self) -> impl Iterator<Item = ((usize, usize), Tile)> + '_ {
        self.tiles
            .iter()
            .enumerate()
            .map(|(i, t)| ((i % SIZE, i / SIZE), *t))
    }

    /// Whether the tile at `(x, y)` can be entered at water `level`, and what it costs in
    /// turns: `None` when it is drowned and the party has no boat. Wet tiles cost one more
    /// turn, and a deep drowned tile costs two more by boat. `wet_discount` is the
    /// reduction a sea-born or wader party earns on wet tiles (never below the dry cost).
    #[must_use]
    pub fn step_cost(
        &self,
        x: usize,
        y: usize,
        level: i32,
        boat: bool,
        wet_discount: u8,
    ) -> Option<u8> {
        let tile = self.tile(x, y)?;
        let base = tile.kind.base_cost();
        match Access::of(i32::from(tile.elevation), level) {
            Access::Dry => Some(base),
            Access::Wet => Some((base + 1).saturating_sub(wet_discount).max(base)),
            Access::Drowned => boat.then_some(base + 1),
        }
    }

    /// Every tile reachable on foot from `from` at water `level` (drowned tiles block),
    /// as a list of coordinates. Used for connectivity checks and, later, by expeditions.
    #[must_use]
    pub fn reachable_on_foot(&self, from: (u8, u8), level: i32) -> Vec<(usize, usize)> {
        let mut seen = vec![false; SIZE * SIZE];
        let mut stack = vec![(usize::from(from.0), usize::from(from.1))];
        let mut out = Vec::new();
        while let Some((x, y)) = stack.pop() {
            if seen[y * SIZE + x] {
                continue;
            }
            seen[y * SIZE + x] = true;
            out.push((x, y));
            let around = [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ];
            for (nx, ny) in around {
                if let Some(t) = self.tile(nx, ny)
                    && !seen[ny * SIZE + nx]
                    && Access::of(i32::from(t.elevation), level) != Access::Drowned
                {
                    stack.push((nx, ny));
                }
            }
        }
        out.sort_by_key(|&(x, y)| Reverse((y, x)));
        out
    }
}

/// The widest water a wader crosses: shared with the tide so both agree.
pub const WADEABLE: i32 = WET_BAND;
