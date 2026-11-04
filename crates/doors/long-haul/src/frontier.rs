//! The frontier: about 120 star systems, their lanes and stations, generated from the season
//! number alone.
//!
//! Nothing here is stored. The map (positions, lanes, names, what each station makes and
//! needs) is a pure function of the season, so every session and the nightly tick rebuild
//! the same frontier, and only what *changes* (stock, news, who charted what) lives in the
//! shared world. Changing the generator therefore changes the map: bump [`GEN_VERSION`], which
//! the world records, if you do.

use std::collections::HashSet;

use russh_bbs_door::DoorRng;

use crate::content::{Content, GoodClass};

/// The generator's version, written into the world so a map change is noticed.
pub const GEN_VERSION: u8 = 1;
/// Systems per season.
pub const SYSTEM_COUNT: usize = 120;
/// Map width, in spans.
pub const MAP_W: u32 = 1000;
/// Map height, in spans.
pub const MAP_H: u32 = 600;
/// Systems within this many hops of home start charted.
pub const HOME_CHARTED_HOPS: u8 = 2;

/// A star's colour class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Star {
    /// A dim red dwarf.
    Red,
    /// An orange dwarf.
    Orange,
    /// A yellow star.
    Yellow,
    /// A white star.
    White,
    /// A hot blue star.
    Blue,
    /// Two suns.
    Binary,
}

impl Star {
    /// Display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Red => "red dwarf",
            Self::Orange => "orange dwarf",
            Self::Yellow => "yellow star",
            Self::White => "white star",
            Self::Blue => "blue giant",
            Self::Binary => "binary pair",
        }
    }
}

/// A standing danger in a system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hazard {
    /// Clear space.
    None,
    /// Gas that blinds an unaided navigator and costs a day.
    Nebula,
    /// A radiation belt that wears modules.
    Radiation,
    /// Wreckage and rock that scars the hull.
    Debris,
}

impl Hazard {
    /// Display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "clear",
            Self::Nebula => "nebula",
            Self::Radiation => "radiation belt",
            Self::Debris => "debris field",
        }
    }
}

/// What a station does with a good.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// It makes it: well stocked, cheap.
    Produces,
    /// It lacks it: short, dear.
    Needs,
    /// It trades it at a balanced price.
    Neutral,
}

impl Role {
    /// Target stock as a percent of the market's depth.
    #[must_use]
    pub const fn target_pct(self) -> i64 {
        match self {
            Self::Produces => 140,
            Self::Needs => 65,
            Self::Neutral => 100,
        }
    }
}

/// One good a station trades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketSlot {
    /// Index into the content's goods.
    pub good: u8,
    /// What the station does with it.
    pub role: Role,
}

/// A star system.
#[derive(Debug, Clone)]
pub struct System {
    /// Index in the frontier.
    pub id: u16,
    /// Name.
    pub name: String,
    /// Map position, in spans.
    pub x: u16,
    /// Map position, in spans.
    pub y: u16,
    /// Star class.
    pub star: Star,
    /// Planets.
    pub planets: u8,
    /// Asteroid belts.
    pub belts: u8,
    /// A gas giant to skim, if any.
    pub gas_giant: bool,
    /// Standing hazard.
    pub hazard: Hazard,
    /// Unknown to everyone until someone visits.
    pub uncharted: bool,
    /// How much a survey of it is worth, 1 to 10.
    pub richness: u8,
    /// Stations in the system.
    pub stations: Vec<u16>,
}

/// A station.
#[derive(Debug, Clone)]
pub struct Station {
    /// Index in the frontier.
    pub id: u16,
    /// The system it orbits in.
    pub system: u16,
    /// Name.
    pub name: String,
    /// Index into the content's economies.
    pub economy: u8,
    /// Size 1 to 3: deeper markets and cheaper trades.
    pub size: u8,
    /// Sells hulls and modules.
    pub shipyard: bool,
    /// Repairs and overhauls.
    pub repair: bool,
    /// The goods it trades.
    pub goods: Vec<MarketSlot>,
}

/// A drive route between two systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lane {
    /// One end.
    pub a: u16,
    /// The other end.
    pub b: u16,
    /// Length in spans.
    pub dist: u16,
}

/// The whole map.
#[derive(Debug, Clone)]
pub struct Frontier {
    /// The season it was generated for.
    pub season: u32,
    /// Systems, indexed by id.
    pub systems: Vec<System>,
    /// Stations, indexed by id.
    pub stations: Vec<Station>,
    /// Lanes.
    pub lanes: Vec<Lane>,
    /// Where every captain starts.
    pub home_station: u16,
    adj: Vec<Vec<(u16, u16)>>,
}

fn clamp_u16(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn clamp_u8(value: u32) -> u8 {
    u8::try_from(value).unwrap_or(u8::MAX)
}

fn pick<'a, T>(list: &'a [T], rng: &mut DoorRng) -> Option<&'a T> {
    let len = u32::try_from(list.len()).ok()?;
    list.get(usize::try_from(rng.below(len)).ok()?)
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn weighted<T>(items: &[(T, u32)], rng: &mut DoorRng) -> usize {
    let total: u32 = items.iter().map(|(_, w)| *w).sum();
    let mut roll = rng.below(total.max(1));
    for (i, (_, w)) in items.iter().enumerate() {
        if roll < *w {
            return i;
        }
        roll -= *w;
    }
    items.len().saturating_sub(1)
}

impl Frontier {
    /// Generate the frontier for `season`.
    #[must_use]
    pub fn generate(content: &Content, season: u32) -> Self {
        let mut rng = DoorRng::from_label("long-haul/frontier", i64::from(season));
        let points = place_points(&mut rng);
        let names = system_names(content, &mut rng);
        let lanes = build_lanes(&points);
        let mut systems: Vec<System> = points
            .iter()
            .zip(names)
            .enumerate()
            .map(|(i, (&(x, y), name))| make_system(i, x, y, name, &mut rng))
            .collect();
        let mut adj = vec![Vec::new(); systems.len()];
        for lane in &lanes {
            adj[usize::from(lane.a)].push((lane.b, lane.dist));
            adj[usize::from(lane.b)].push((lane.a, lane.dist));
        }
        let mut stations = place_stations(content, &mut systems, &mut rng);
        ensure_every_economy(content, &mut stations, &mut rng);
        let home_station = pick_home(content, &systems, &stations);
        if let Some(home) = stations.get_mut(usize::from(home_station)) {
            home.size = home.size.max(2);
            home.shipyard = true;
            home.repair = true;
        }
        let home_system = stations[usize::from(home_station)].system;
        let hops = bfs(&adj, home_system);
        settle_home_region(content, &hops, &mut systems, &mut stations, &mut rng);
        ensure_shipyards(content, &mut stations, &mut rng);
        finish_stations(content, &mut stations, &mut rng);
        ensure_goods_made(content, &mut stations, &mut rng);
        for system in &mut systems {
            system.stations.clear();
        }
        for station in &stations {
            systems[usize::from(station.system)]
                .stations
                .push(station.id);
        }
        for (system, &hop) in systems.iter_mut().zip(&hops) {
            if hop <= HOME_CHARTED_HOPS {
                system.uncharted = false;
                if hop <= 1 {
                    system.hazard = Hazard::None;
                }
            } else {
                system.uncharted = rng.chance(28, 100);
            }
        }
        Self {
            season,
            systems,
            stations,
            lanes,
            home_station,
            adj,
        }
    }

    /// The stations a new captain may start at: charted, near home, and not a haven or a
    /// military post, in id order. New captains are spread over them (by name) so a busy board
    /// does not begin with everyone bidding for the same stock.
    #[must_use]
    pub fn start_stations(&self, content: &Content) -> Vec<u16> {
        let home_system = self.station(self.home_station).map_or(0, |s| s.system);
        let hops = self.hops_from(home_system);
        let mut found: Vec<(u8, u16)> = self
            .stations
            .iter()
            .filter(|s| {
                let eco = content.economies[usize::from(s.economy)].id.as_str();
                s.id != self.home_station
                    && hops[usize::from(s.system)] <= 3
                    && !self.systems[usize::from(s.system)].uncharted
                    && eco != "haven"
                    && eco != "military"
            })
            .map(|s| (hops[usize::from(s.system)], s.id))
            .collect();
        found.sort_unstable();
        let mut out = vec![self.home_station];
        out.extend(found.into_iter().take(5).map(|(_, id)| id));
        out
    }

    /// Neighbouring systems as `(system, lane length)`.
    #[must_use]
    pub fn neighbours(&self, system: u16) -> &[(u16, u16)] {
        self.adj.get(usize::from(system)).map_or(&[], Vec::as_slice)
    }

    /// The length of the lane between two systems, if there is one.
    #[must_use]
    pub fn lane(&self, a: u16, b: u16) -> Option<u16> {
        self.neighbours(a)
            .iter()
            .find(|(n, _)| *n == b)
            .map(|(_, d)| *d)
    }

    /// Hops from `system` to every system (255 where unreachable).
    #[must_use]
    pub fn hops_from(&self, system: u16) -> Vec<u8> {
        bfs(&self.adj, system)
    }

    /// A system by id.
    #[must_use]
    pub fn system(&self, id: u16) -> Option<&System> {
        self.systems.get(usize::from(id))
    }

    /// A station by id.
    #[must_use]
    pub fn station(&self, id: u16) -> Option<&Station> {
        self.stations.get(usize::from(id))
    }

    /// The system a station is in.
    #[must_use]
    pub fn system_of(&self, station: u16) -> Option<&System> {
        self.station(station).and_then(|s| self.system(s.system))
    }

    /// Straight-line distance between two systems, in spans.
    #[must_use]
    pub fn distance(&self, a: u16, b: u16) -> u32 {
        match (self.system(a), self.system(b)) {
            (Some(a), Some(b)) => isqrt_dist(
                i64::from(a.x) - i64::from(b.x),
                i64::from(a.y) - i64::from(b.y),
            ),
            _ => 0,
        }
    }
}

fn isqrt_dist(dx: i64, dy: i64) -> u32 {
    let d2 = u64::try_from(dx * dx + dy * dy).unwrap_or(0);
    u32::try_from(d2.isqrt()).unwrap_or(u32::MAX)
}

fn bfs(adj: &[Vec<(u16, u16)>], from: u16) -> Vec<u8> {
    let mut hops = vec![255u8; adj.len()];
    let Some(start) = hops.get_mut(usize::from(from)) else {
        return hops;
    };
    *start = 0;
    let mut queue = std::collections::VecDeque::from([from]);
    while let Some(at) = queue.pop_front() {
        let here = hops[usize::from(at)];
        for &(next, _) in &adj[usize::from(at)] {
            if hops[usize::from(next)] == 255 {
                hops[usize::from(next)] = here.saturating_add(1).min(254);
                queue.push_back(next);
            }
        }
    }
    hops
}

fn place_points(rng: &mut DoorRng) -> Vec<(u16, u16)> {
    let mut min_d2: i64 = 42 * 42;
    loop {
        let mut points: Vec<(u16, u16)> = Vec::with_capacity(SYSTEM_COUNT);
        let mut attempts = 0u32;
        while points.len() < SYSTEM_COUNT && attempts < 40_000 {
            attempts += 1;
            let (x, y) = (rng.below(MAP_W), rng.below(MAP_H));
            let clear = points.iter().all(|&(px, py)| {
                let (dx, dy) = (i64::from(px) - i64::from(x), i64::from(py) - i64::from(y));
                dx * dx + dy * dy >= min_d2
            });
            if clear {
                points.push((clamp_u16(x), clamp_u16(y)));
            }
        }
        if points.len() == SYSTEM_COUNT {
            return points;
        }
        min_d2 = min_d2 * 81 / 100;
    }
}

fn system_names(content: &Content, rng: &mut DoorRng) -> Vec<String> {
    let mut used: HashSet<String> = HashSet::new();
    let mut out = Vec::with_capacity(SYSTEM_COUNT);
    for _ in 0..SYSTEM_COUNT {
        let mut name = String::new();
        for _ in 0..40 {
            let a = pick(&content.names.system_prefix, rng)
                .cloned()
                .unwrap_or_default();
            let b = pick(&content.names.system_suffix, rng)
                .cloned()
                .unwrap_or_default();
            name = format!("{a} {b}");
            if !used.contains(&name) {
                break;
            }
        }
        let mut unique = name.clone();
        let mut n = 2u32;
        while used.contains(&unique) {
            unique = format!("{name} {n}");
            n += 1;
        }
        used.insert(unique.clone());
        out.push(unique);
    }
    out
}

fn build_lanes(points: &[(u16, u16)]) -> Vec<Lane> {
    let n = points.len();
    let dist = |a: usize, b: usize| {
        isqrt_dist(
            i64::from(points[a].0) - i64::from(points[b].0),
            i64::from(points[a].1) - i64::from(points[b].1),
        )
        .max(1)
    };
    let mut pairs: Vec<(u32, usize, usize)> = Vec::with_capacity(n * n / 2);
    for a in 0..n {
        for b in (a + 1)..n {
            pairs.push((dist(a, b), a, b));
        }
    }
    pairs.sort_unstable();
    let mut parent: Vec<usize> = (0..n).collect();
    let mut chosen: Vec<(usize, usize)> = Vec::new();
    for &(_, a, b) in &pairs {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            parent[ra] = rb;
            chosen.push((a, b));
        }
    }
    let mut degree = vec![0usize; n];
    for &(a, b) in &chosen {
        degree[a] += 1;
        degree[b] += 1;
    }
    let mut have: HashSet<(usize, usize)> = chosen.iter().copied().collect();
    for a in 0..n {
        let mut near: Vec<(u32, usize)> = (0..n)
            .filter(|&b| b != a)
            .map(|b| (dist(a, b), b))
            .collect();
        near.sort_unstable();
        for &(d, b) in near.iter().take(2) {
            let key = (a.min(b), a.max(b));
            if d <= 110 && degree[a] < 5 && degree[b] < 5 && have.insert(key) {
                chosen.push(key);
                degree[a] += 1;
                degree[b] += 1;
            }
        }
    }
    chosen.sort_unstable();
    chosen
        .into_iter()
        .map(|(a, b)| Lane {
            a: clamp_u16(u32::try_from(a).unwrap_or(0)),
            b: clamp_u16(u32::try_from(b).unwrap_or(0)),
            dist: clamp_u16(dist(a, b)),
        })
        .collect()
}

fn make_system(i: usize, x: u16, y: u16, name: String, rng: &mut DoorRng) -> System {
    let star = match rng.below(100) {
        0..=29 => Star::Red,
        30..=49 => Star::Orange,
        50..=74 => Star::Yellow,
        75..=87 => Star::White,
        88..=94 => Star::Blue,
        _ => Star::Binary,
    };
    let hazard = match rng.below(100) {
        0..=61 => Hazard::None,
        62..=73 => Hazard::Nebula,
        74..=83 => Hazard::Radiation,
        _ => Hazard::Debris,
    };
    let planets = clamp_u8(rng.below(7));
    let belts = clamp_u8(rng.below(4));
    let gas_giant = rng.chance(30, 100);
    let mut richness = 1 + rng.below(7) + u32::from(belts > 1) + u32::from(hazard != Hazard::None);
    richness = richness.min(10);
    System {
        id: clamp_u16(u32::try_from(i).unwrap_or(0)),
        name,
        x,
        y,
        star,
        planets,
        belts,
        gas_giant,
        hazard,
        uncharted: false,
        richness: clamp_u8(richness),
        stations: Vec::new(),
    }
}

fn pick_economy(content: &Content, rng: &mut DoorRng) -> u8 {
    let weights: Vec<(usize, u32)> = content
        .economies
        .iter()
        .enumerate()
        .map(|(i, e)| (i, e.weight))
        .collect();
    clamp_u8(u32::try_from(weighted(&weights, rng)).unwrap_or(0))
}

fn new_station(content: &Content, system: u16, rng: &mut DoorRng) -> Station {
    let economy = pick_economy(content, rng);
    let size = match rng.below(100) {
        0..=49 => 1,
        50..=84 => 2,
        _ => 3,
    };
    let is_haven = content.economies[usize::from(economy)].id == "haven";
    let shipyard = !is_haven && (size == 3 || (size == 2 && rng.chance(45, 100)));
    let repair = rng.chance(if is_haven { 40 } else { 65 }, 100);
    Station {
        id: 0,
        system,
        name: String::new(),
        economy,
        size,
        shipyard,
        repair,
        goods: Vec::new(),
    }
}

fn place_stations(content: &Content, systems: &mut [System], rng: &mut DoorRng) -> Vec<Station> {
    let mut stations = Vec::new();
    for system in systems.iter_mut() {
        let count = match rng.below(100) {
            0..=34 => 0,
            35..=79 => 1,
            80..=94 => 2,
            _ => 3,
        };
        let sid = system.id;
        for _ in 0..count {
            stations.push(new_station(content, sid, rng));
        }
    }
    for (i, station) in stations.iter_mut().enumerate() {
        station.id = clamp_u16(u32::try_from(i).unwrap_or(0));
    }
    stations
}

fn ensure_every_economy(content: &Content, stations: &mut [Station], rng: &mut DoorRng) {
    for want in 0..content.economies.len() {
        let want = clamp_u8(u32::try_from(want).unwrap_or(0));
        if stations.iter().any(|s| s.economy == want) {
            continue;
        }
        let mut counts = vec![0usize; content.economies.len()];
        for s in stations.iter() {
            counts[usize::from(s.economy)] += 1;
        }
        let candidates: Vec<usize> = stations
            .iter()
            .enumerate()
            .filter(|(_, s)| counts[usize::from(s.economy)] > 2)
            .map(|(i, _)| i)
            .collect();
        if let Some(&i) = pick(&candidates, rng) {
            stations[i].economy = want;
            if content.economies[usize::from(want)].id == "haven" {
                stations[i].shipyard = false;
            }
        }
    }
}

fn pick_home(content: &Content, systems: &[System], stations: &[Station]) -> u16 {
    let target = (150i64, 300i64);
    let homely = |s: &Station| {
        let id = content.economies[usize::from(s.economy)].id.as_str();
        id == "farming" || id == "industrial"
    };
    stations
        .iter()
        .filter(|s| homely(s))
        .min_by_key(|s| {
            let sys = &systems[usize::from(s.system)];
            let (dx, dy) = (i64::from(sys.x) - target.0, i64::from(sys.y) - target.1);
            (dx * dx + dy * dy, s.id)
        })
        .or_else(|| stations.first())
        .map_or(0, |s| s.id)
}

/// Make sure a new captain has somewhere to trade within a few hops of home.
fn settle_home_region(
    content: &Content,
    hops: &[u8],
    systems: &mut [System],
    stations: &mut Vec<Station>,
    rng: &mut DoorRng,
) {
    // Stations a new captain could start at or trade with: near home, not a haven or a post.
    let near = |stations: &[Station]| {
        stations
            .iter()
            .filter(|s| {
                let eco = content.economies[usize::from(s.economy)].id.as_str();
                hops[usize::from(s.system)] <= HOME_CHARTED_HOPS
                    && eco != "haven"
                    && eco != "military"
            })
            .count()
    };
    let mut order: Vec<usize> = (0..systems.len())
        .filter(|&i| hops[i] <= HOME_CHARTED_HOPS)
        .collect();
    order.sort_by_key(|&i| (hops[i], i));
    for pass in 0..6usize {
        for &i in &order {
            if near(stations) >= 6 {
                return;
            }
            if stations
                .iter()
                .filter(|s| usize::from(s.system) == i)
                .count()
                <= pass
            {
                let mut station = new_station(content, systems[i].id, rng);
                station.id = clamp_u16(u32::try_from(stations.len()).unwrap_or(0));
                stations.push(station);
            }
        }
    }
}

fn ensure_shipyards(content: &Content, stations: &mut [Station], rng: &mut DoorRng) {
    let haven = content
        .economy_ix("haven")
        .map_or(u8::MAX, |i| clamp_u8(u32::try_from(i).unwrap_or(0)));
    let want = 8usize;
    let mut have = stations.iter().filter(|s| s.shipyard).count();
    let mut open: Vec<usize> = stations
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.shipyard && s.size >= 2 && s.economy != haven)
        .map(|(i, _)| i)
        .collect();
    while have < want && !open.is_empty() {
        let at = usize::try_from(rng.below(u32::try_from(open.len()).unwrap_or(1))).unwrap_or(0);
        let i = open.swap_remove(at);
        stations[i].shipyard = true;
        have += 1;
    }
}

fn distinct(from: &[u8], count: usize, rng: &mut DoorRng) -> Vec<u8> {
    let mut left: Vec<u8> = from.to_vec();
    let mut out = Vec::new();
    while out.len() < count && !left.is_empty() {
        let at = usize::try_from(rng.below(u32::try_from(left.len()).unwrap_or(1))).unwrap_or(0);
        out.push(left.swap_remove(at));
    }
    out
}

fn finish_stations(content: &Content, stations: &mut [Station], rng: &mut DoorRng) {
    let ixs = |ids: &[String]| -> Vec<u8> {
        ids.iter()
            .filter_map(|id| content.good_ix(id))
            .map(|i| clamp_u8(u32::try_from(i).unwrap_or(0)))
            .collect()
    };
    let legal: Vec<u8> = content
        .goods
        .iter()
        .enumerate()
        .filter(|(_, g)| g.class == GoodClass::Legal)
        .map(|(i, _)| clamp_u8(u32::try_from(i).unwrap_or(0)))
        .collect();
    let contraband: Vec<u8> = content
        .goods
        .iter()
        .enumerate()
        .filter(|(_, g)| g.class == GoodClass::Contraband)
        .map(|(i, _)| clamp_u8(u32::try_from(i).unwrap_or(0)))
        .collect();
    let mut used_names: HashSet<String> = HashSet::new();
    for station in stations.iter_mut() {
        let eco = &content.economies[usize::from(station.economy)];
        let (made, lacks) = (ixs(&eco.produces), ixs(&eco.needs));
        let n_make = if station.size == 3 { 4 } else { 3 };
        let mut slots: Vec<MarketSlot> = distinct(&made, n_make, rng)
            .into_iter()
            .map(|good| MarketSlot {
                good,
                role: Role::Produces,
            })
            .collect();
        slots.extend(distinct(&lacks, 3, rng).into_iter().map(|good| MarketSlot {
            good,
            role: Role::Needs,
        }));
        let others: Vec<u8> = legal
            .iter()
            .copied()
            .filter(|g| !made.contains(g) && !lacks.contains(g))
            .collect();
        let n_other = 4 + usize::try_from(rng.below(3)).unwrap_or(0);
        slots.extend(
            distinct(&others, n_other, rng)
                .into_iter()
                .map(|good| MarketSlot {
                    good,
                    role: Role::Neutral,
                }),
        );
        if eco.id != "haven"
            && eco.id != "military"
            && rng.chance(35, 100)
            && let Some(&good) = pick(&contraband, rng)
        {
            slots.push(MarketSlot {
                good,
                role: Role::Needs,
            });
        }
        slots.sort_by_key(|s| s.good);
        station.goods = slots;
        let noun = pick(content.names.nouns(&eco.id), rng)
            .cloned()
            .unwrap_or_default();
        let mut name = String::new();
        for _ in 0..30 {
            let adjective = pick(&content.names.station_adjective, rng)
                .cloned()
                .unwrap_or_default();
            name = format!("{adjective} {noun}");
            if !used_names.contains(&name) {
                break;
            }
        }
        let mut unique = name.clone();
        let mut n = 2u32;
        while used_names.contains(&unique) {
            unique = format!("{name} {n}");
            n += 1;
        }
        used_names.insert(unique.clone());
        station.name = unique;
    }
}

/// Every legal good is made by at least one station, so no good is missing from the whole
/// frontier. A station that would have to give up another good to make it does so, and the pass
/// repeats until nothing is missing (a few passes at most).
fn ensure_goods_made(content: &Content, stations: &mut [Station], rng: &mut DoorRng) {
    for _ in 0..8 {
        let mut changed = false;
        for (gi, good) in content.goods.iter().enumerate() {
            let gix = clamp_u8(u32::try_from(gi).unwrap_or(0));
            let made = stations.iter().any(|s| {
                s.goods
                    .iter()
                    .any(|m| m.good == gix && m.role == Role::Produces)
            });
            if made || good.class != GoodClass::Legal {
                continue;
            }
            let candidates: Vec<usize> = stations
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    content.economies[usize::from(s.economy)]
                        .produces
                        .contains(&good.id)
                })
                .map(|(i, _)| i)
                .collect();
            let Some(&at) = pick(&candidates, rng) else {
                continue;
            };
            let slots: Vec<usize> = stations[at]
                .goods
                .iter()
                .enumerate()
                .filter(|(_, m)| m.role == Role::Produces)
                .map(|(i, _)| i)
                .collect();
            let Some(&slot) = pick(&slots, rng) else {
                continue;
            };
            stations[at].goods[slot].good = gix;
            stations[at].goods.sort_by_key(|m| m.good);
            changed = true;
        }
        if !changed {
            break;
        }
    }
}
