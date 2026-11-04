//! Expeditions: a party leaves the hold with supplies, crosses the marsh with the tide, meets
//! scenes and fights, works the ground for goods, and must get home before the flood. The trip
//! is part of the clan's save (a disconnect mid-scene loses nothing) and every roll comes from
//! the `DoorRng` the caller passes.
//!
//! The clan's clock is the tide-turn: each turn spent moves the water on 48 minutes, so a party
//! out when the flood comes is `turns_used` crossing a rising `Tide::level`. A party caught on
//! ground the water covers is swept (half the cargo, a chance of injury); one still out when the
//! day's turns run out is cut off.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use russh_bbs_door::DoorRng;

use crate::clan::Clan;
use crate::codec::{Reader, Writer};
use crate::combat::{Fight, FightAction, Progress, ROUNDS_PER_TURN, Side, Unit, Verdict};
use crate::content::{Catalog, Cause, EpitaphCause, Learn};
use crate::crew::Crew;
use crate::engine::{ActionError, Env, finish_injury, injure, scale};
use crate::event::Event;
use crate::lore::{Cond, Daypart, Effect, EventDef, Req, Stage, Water, Who};
use crate::map::{PoiKind, SIZE, TileKind};
use crate::tide::{Access, MINUTES_PER_DAY};
use crate::types::{Resource, Stock, Trade};

/// The most crew on one expedition.
pub const MAX_PARTY: usize = 5;

/// Turns a clan needs left in the day to set out.
pub const MIN_DEPART_TURNS: u8 = 4;

/// Turns one ration feeds one crew member for (a day out costs a ration each, and the hold
/// does not feed them that night).
pub const FOOD_TURNS: u32 = 24;

/// Night turns one peat keeps the lantern lit for.
pub const LIGHT_TURNS: u32 = 6;

/// The most boats a party takes.
pub const MAX_BOATS: u8 = 2;

/// Chance (percent) that entering a fresh tile raises a scene.
pub const EVENT_PCT: u32 = 22;

/// The most marks a clan remembers.
pub const MARKS_CAP: usize = 48;

/// The most tiles remembered as visited on one trip.
const VISITED_CAP: usize = 120;

/// Turns a gather takes.
pub const GATHER_TURNS: u8 = 2;

/// Turns a room of a ruin takes.
pub const DELVE_TURNS: u8 = 2;

/// What a unit of a resource weighs in the pack.
#[must_use]
pub const fn weight(r: Resource) -> u32 {
    match r {
        Resource::Timber | Resource::Iron | Resource::Relic => 2,
        Resource::Boats => 6,
        _ => 1,
    }
}

/// A compass direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Up the map.
    North,
    /// Right.
    East,
    /// Down.
    South,
    /// Left.
    West,
}

impl Dir {
    /// Every direction, in the order routes try them.
    pub const ALL: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    /// The word.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::North => "north",
            Self::East => "east",
            Self::South => "south",
            Self::West => "west",
        }
    }

    /// One letter, for keys.
    #[must_use]
    pub const fn letter(self) -> char {
        match self {
            Self::North => 'n',
            Self::East => 'e',
            Self::South => 's',
            Self::West => 'w',
        }
    }

    /// The tile one step this way, if it is on the map.
    #[must_use]
    pub fn step(self, (x, y): (u8, u8)) -> Option<(u8, u8)> {
        let (x, y) = (usize::from(x), usize::from(y));
        let (nx, ny) = match self {
            Self::North => (x, y.checked_sub(1)?),
            Self::East => (x + 1, y),
            Self::South => (x, y + 1),
            Self::West => (x.checked_sub(1)?, y),
        };
        (nx < SIZE && ny < SIZE)
            .then(|| (u8::try_from(nx).unwrap_or(0), u8::try_from(ny).unwrap_or(0)))
    }
}

/// What a party takes with it, paid from the stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Kit {
    /// Preserved food: a ration feeds one for 12 turns.
    pub food: u8,
    /// Rope.
    pub rope: u8,
    /// Peat for the lantern: night travel without light is slower.
    pub peat: u8,
    /// Tools.
    pub tools: u8,
    /// Arms: one reaches a row deeper in a fight.
    pub arms: u8,
    /// Boats: they carry more and let the party enter drowned ground.
    pub boats: u8,
}

impl Kit {
    /// How many of `r` the kit holds.
    #[must_use]
    pub const fn get(&self, r: Resource) -> u32 {
        (match r {
            Resource::Food => self.food,
            Resource::Rope => self.rope,
            Resource::Peat => self.peat,
            Resource::Tools => self.tools,
            Resource::Arms => self.arms,
            Resource::Boats => self.boats,
            _ => 0,
        }) as u32
    }

    fn slot(&mut self, r: Resource) -> Option<&mut u8> {
        Some(match r {
            Resource::Food => &mut self.food,
            Resource::Rope => &mut self.rope,
            Resource::Peat => &mut self.peat,
            Resource::Tools => &mut self.tools,
            Resource::Arms => &mut self.arms,
            Resource::Boats => &mut self.boats,
            _ => return None,
        })
    }

    /// Take up to `n` of `r`; returns how many.
    pub fn take_up_to(&mut self, r: Resource, n: u32) -> u32 {
        let Some(slot) = self.slot(r) else { return 0 };
        let taken = u32::from(*slot).min(n);
        *slot -= u8::try_from(taken).unwrap_or(0);
        taken
    }

    /// The carried supplies' weight (boats carry; they are not carried).
    #[must_use]
    pub const fn weight(&self) -> u32 {
        self.food as u32
            + self.rope as u32
            + self.peat as u32
            + self.tools as u32
            + self.arms as u32
    }

    /// Resource and count pairs.
    #[must_use]
    pub fn pairs(&self) -> [(Resource, u8); 6] {
        [
            (Resource::Food, self.food),
            (Resource::Rope, self.rope),
            (Resource::Peat, self.peat),
            (Resource::Tools, self.tools),
            (Resource::Arms, self.arms),
            (Resource::Boats, self.boats),
        ]
    }
}

/// How a trip ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// Home with the cargo.
    Home,
    /// Out of turns away from home: cut off for the night.
    Stranded,
    /// The water covered the ground: swept.
    Swept,
    /// Beaten in a skirmish and left for dead; survivors crawl home with nothing.
    Routed,
    /// Nobody came back.
    Lost,
    /// The player left the trip open past midnight.
    Overnight,
}

impl Ending {
    /// A word for the report.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Stranded => "cut off",
            Self::Swept => "swept",
            Self::Routed => "routed",
            Self::Lost => "lost",
            Self::Overnight => "benighted",
        }
    }
}

/// A scene waiting for an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    /// The event's id.
    pub event: String,
    /// Indices of the options offered, in order.
    pub offered: Vec<u8>,
}

/// What the trip is waiting on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// The party is free to move.
    Free,
    /// A scene wants an answer.
    Scene(Scene),
    /// A fight is on.
    Fight(Box<Fight>),
}

/// Where the party is in a ruin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ruin {
    /// Room the party has reached (1 based).
    pub room: u8,
    /// Rooms in all.
    pub rooms: u8,
}

/// An expedition in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trip {
    /// Crew ids, in order of precedence.
    pub party: Vec<u16>,
    /// The supplies left.
    pub kit: Kit,
    /// Goods found.
    pub loot: Stock,
    /// Tallies found.
    pub tallies: u32,
    /// Where the party stands.
    pub at: (u8, u8),
    /// Where they mean to go.
    pub target: Option<(u8, u8)>,
    /// Turns spent out.
    pub turns_out: u16,
    /// Of those, turns that fell at night.
    pub night_turns: u16,
    /// What the trip waits on.
    pub phase: Phase,
    /// The ruin the party is in, if any.
    pub ruin: Option<Ruin>,
    /// Tiles entered (a tile raises a scene once per trip).
    pub visited: Vec<(u8, u8)>,
    /// Tiles worked: how many times.
    pub worked: Vec<(u8, u8, u8)>,
    /// Hardship shared, paid out as bond points at the end.
    pub hardship: u8,
    /// Scenes seen.
    pub scenes: u8,
    /// Fights fought.
    pub fights: u8,
    /// The trip's own dice. They live in the save, so a dropped connection cannot re-roll a
    /// choice: the same save and the same key always give the same result (D108).
    pub rng: [u64; 4],
}

/// What a player can do on a trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TripAction {
    /// One step.
    Step(Dir),
    /// March along the quickest safe route to a tile (or the set target) until something stops
    /// the party.
    March {
        /// The tile; `None` for the set target.
        to: Option<(u8, u8)>,
    },
    /// Work the ground here for goods.
    Gather,
    /// Wait where you are.
    Camp {
        /// Turns, 1 to 10.
        turns: u8,
    },
    /// Go on into the next room of a ruin.
    Delve,
    /// Answer a scene with the n-th option offered.
    Choose(u8),
    /// Act in a fight.
    Fight(FightAction),
    /// Fight it out on the standard policy.
    Quick,
    /// Drop goods from the pack to travel lighter.
    Jettison {
        /// What.
        what: Resource,
        /// How many.
        qty: u32,
    },
}

impl Trip {
    /// How many of `r` the party carries, kit and loot together.
    #[must_use]
    pub fn carried(&self, r: Resource) -> u32 {
        self.loot.get(r) + self.kit.get(r)
    }

    /// The weight carried.
    #[must_use]
    pub fn load(&self) -> u32 {
        self.kit.weight()
            + Resource::ALL
                .iter()
                .map(|r| self.loot.get(*r) * weight(*r))
                .sum::<u32>()
    }

    /// What the party can carry without being slowed.
    #[must_use]
    pub fn capacity(&self, clan: &Clan, cat: &Catalog) -> u32 {
        capacity_of(clan, cat, &self.party, self.kit.boats)
    }

    /// Slowed by the load.
    #[must_use]
    pub fn overloaded(&self, clan: &Clan, cat: &Catalog) -> bool {
        self.load() > self.capacity(clan, cat)
    }

    /// Rations the party has eaten so far.
    #[must_use]
    pub fn rations_due(&self) -> u32 {
        (u32::try_from(self.party.len()).unwrap_or(1) * u32::from(self.turns_out))
            .div_ceil(FOOD_TURNS)
    }

    /// Out of food.
    #[must_use]
    pub fn hungry(&self) -> bool {
        self.rations_due() > u32::from(self.kit.food)
    }

    /// Whether the lantern has peat for one more night turn.
    #[must_use]
    pub fn lit(&self) -> bool {
        (u32::from(self.night_turns) + 1).div_ceil(LIGHT_TURNS) <= u32::from(self.kit.peat)
    }

    /// Write to a save.
    pub fn write(&self, w: &mut Writer) {
        w.count(self.party.len());
        for p in &self.party {
            w.u16(*p);
        }
        for (_, n) in self.kit.pairs() {
            w.u8(n);
        }
        for n in self.loot.counts() {
            w.u32(*n);
        }
        w.u32(self.tallies);
        w.u8(self.at.0);
        w.u8(self.at.1);
        match self.target {
            Some((x, y)) => {
                w.bool(true);
                w.u8(x);
                w.u8(y);
            }
            None => w.bool(false),
        }
        w.u16(self.turns_out);
        w.u16(self.night_turns);
        match &self.phase {
            Phase::Free => w.u8(0),
            Phase::Scene(s) => {
                w.u8(1);
                w.str(&s.event);
                w.count(s.offered.len());
                for o in &s.offered {
                    w.u8(*o);
                }
            }
            Phase::Fight(f) => {
                w.u8(2);
                f.write(w);
            }
        }
        match self.ruin {
            Some(r) => {
                w.bool(true);
                w.u8(r.room);
                w.u8(r.rooms);
            }
            None => w.bool(false),
        }
        w.count(self.visited.len());
        for (x, y) in &self.visited {
            w.u8(*x);
            w.u8(*y);
        }
        w.count(self.worked.len());
        for (x, y, n) in &self.worked {
            w.u8(*x);
            w.u8(*y);
            w.u8(*n);
        }
        w.u8(self.hardship);
        w.u8(self.scenes);
        w.u8(self.fights);
        for word in self.rng {
            w.u64(word);
        }
    }

    /// Read from a save.
    pub fn read(r: &mut Reader<'_>) -> Option<Self> {
        let party = (0..r.u8()?).map(|_| r.u16()).collect::<Option<Vec<_>>>()?;
        let kit = Kit {
            food: r.u8()?,
            rope: r.u8()?,
            peat: r.u8()?,
            tools: r.u8()?,
            arms: r.u8()?,
            boats: r.u8()?,
        };
        let mut counts = [0u32; crate::types::RESOURCES];
        for n in &mut counts {
            *n = r.u32()?;
        }
        let tallies = r.u32()?;
        let at = (r.u8()?, r.u8()?);
        let target = if r.bool()? {
            Some((r.u8()?, r.u8()?))
        } else {
            None
        };
        let turns_out = r.u16()?;
        let night_turns = r.u16()?;
        let phase = match r.u8()? {
            0 => Phase::Free,
            1 => Phase::Scene(Scene {
                event: r.str()?,
                offered: (0..r.u8()?).map(|_| r.u8()).collect::<Option<Vec<_>>>()?,
            }),
            2 => Phase::Fight(Box::new(Fight::read(r)?)),
            _ => return None,
        };
        let ruin = if r.bool()? {
            Some(Ruin {
                room: r.u8()?,
                rooms: r.u8()?,
            })
        } else {
            None
        };
        let visited = (0..r.u8()?)
            .map(|_| Some((r.u8()?, r.u8()?)))
            .collect::<Option<Vec<_>>>()?;
        let worked = (0..r.u8()?)
            .map(|_| Some((r.u8()?, r.u8()?, r.u8()?)))
            .collect::<Option<Vec<_>>>()?;
        let (hardship, scenes, fights) = (r.u8()?, r.u8()?, r.u8()?);
        let dice = [r.u64()?, r.u64()?, r.u64()?, r.u64()?];
        let on_map = |(x, y): (u8, u8)| usize::from(x) < SIZE && usize::from(y) < SIZE;
        (!party.is_empty() && party.len() <= MAX_PARTY && on_map(at)).then_some(Self {
            party,
            kit,
            loot: Stock::from_counts(counts),
            tallies,
            at,
            target,
            turns_out,
            night_turns,
            phase,
            ruin,
            visited,
            worked,
            hardship,
            scenes,
            fights,
            rng: dice,
        })
    }
}

fn capacity_of(clan: &Clan, cat: &Catalog, party: &[u16], boats: u8) -> u32 {
    let hands: i32 = party
        .iter()
        .filter_map(|id| clan.crew_by(*id))
        .map(|c| {
            let pen = i32::try_from(100 - c.penalty(cat)).unwrap_or(0);
            (8 + c.attr(crate::types::Attr::Grit)) * pen / 100
        })
        .sum();
    u32::try_from(hands).unwrap_or(0) + 12 * u32::from(boats)
}

/// The tile of the clan's hold.
#[must_use]
pub fn home(env: &Env<'_>, clan: &Clan) -> (u8, u8) {
    env.map
        .sites
        .get(usize::from(clan.site))
        .copied()
        .unwrap_or(env.map.tidecourt)
}

fn turn_len(env: &Env<'_>) -> i64 {
    MINUTES_PER_DAY / i64::from(env.settings.turns_per_day.max(1))
}

/// How the party moves right now: fixed for the length of one plan.
#[derive(Debug, Clone, Copy)]
struct Gait {
    discount: u8,
    overloaded: bool,
    lit: bool,
    boat: bool,
}

impl Gait {
    fn of(env: &Env<'_>, clan: &Clan, trip: &Trip) -> Self {
        let discount = u8::from(
            trip.party
                .iter()
                .filter_map(|id| clan.crew_by(*id))
                .any(|c| {
                    c.fit(env.cat)
                        && (c.trait_any(env.cat, |t| t.wet_pct > 0) || c.level(Trade::Wader) >= 2)
                }),
        );
        Self {
            discount,
            overloaded: trip.overloaded(clan, env.cat),
            lit: trip.lit(),
            boat: trip.kit.boats > 0,
        }
    }
}

/// What entering `(x, y)` costs at `minute`, or `None` if the ground is drowned then or by the
/// time the party would get there.
fn cost_with(env: &Env<'_>, gait: Gait, (x, y): (u8, u8), minute: i64) -> Option<u8> {
    let level = env.tide.level(minute);
    let mut cost = env.map.step_cost(
        usize::from(x),
        usize::from(y),
        level,
        gait.boat,
        gait.discount,
    )?;
    if gait.overloaded {
        cost += 1;
    }
    if Daypart::of(minute) == Daypart::Night && !gait.lit {
        cost += 1;
    }
    let arrive = minute + i64::from(cost) * turn_len(env);
    let tile = env.map.tile(usize::from(x), usize::from(y))?;
    if !gait.boat
        && Access::of(i32::from(tile.elevation), env.tide.level(arrive)) == Access::Drowned
    {
        return None;
    }
    Some(cost)
}

fn move_cost(env: &Env<'_>, clan: &Clan, trip: &Trip, to: (u8, u8), minute: i64) -> Option<u8> {
    cost_with(env, Gait::of(env, clan, trip), to, minute)
}

/// See [`Reach::key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachKey((u8, u8), u8, [u8; 4]);

/// Every tile the party can reach in the turns left today, by the quickest route that stays
/// above the water as it will be on arrival at each tile.
#[derive(Debug, Clone)]
pub struct Reach {
    from: (u8, u8),
    dist: Vec<u32>,
    prev: Vec<Option<(u8, u8)>>,
}

impl Reach {
    /// Search outward from where the party stands.
    #[must_use]
    pub fn new(env: &Env<'_>, clan: &Clan, trip: &Trip) -> Self {
        let left = u32::from(clan.turns_left(&env.settings));
        let base = env.minute(clan);
        let len = turn_len(env);
        let gait = Gait::of(env, clan, trip);
        let idx = |(x, y): (u8, u8)| usize::from(y) * SIZE + usize::from(x);
        let mut dist = vec![u32::MAX; SIZE * SIZE];
        let mut prev: Vec<Option<(u8, u8)>> = vec![None; SIZE * SIZE];
        let mut heap = BinaryHeap::new();
        dist[idx(trip.at)] = 0;
        heap.push(Reverse((0u32, trip.at.1, trip.at.0)));
        while let Some(Reverse((d, y, x))) = heap.pop() {
            if d > dist[idx((x, y))] {
                continue;
            }
            let minute = base + i64::from(d) * len;
            for dir in Dir::ALL {
                let Some(n) = dir.step((x, y)) else { continue };
                let Some(cost) = cost_with(env, gait, n, minute) else {
                    continue;
                };
                let nd = d + u32::from(cost);
                if nd <= left && nd < dist[idx(n)] {
                    dist[idx(n)] = nd;
                    prev[idx(n)] = Some((x, y));
                    heap.push(Reverse((nd, n.1, n.0)));
                }
            }
        }
        Self {
            from: trip.at,
            dist,
            prev,
        }
    }

    /// What a search depends on: when this is unchanged, so is the answer.
    #[must_use]
    pub fn key(env: &Env<'_>, clan: &Clan, trip: &Trip) -> ReachKey {
        let g = Gait::of(env, clan, trip);
        ReachKey(
            trip.at,
            clan.turns_used,
            [
                g.discount,
                u8::from(g.overloaded),
                u8::from(g.lit),
                u8::from(g.boat),
            ],
        )
    }

    /// The turns to reach `to`, if it can be reached.
    #[must_use]
    pub fn cost(&self, to: (u8, u8)) -> Option<u32> {
        let d = *self
            .dist
            .get(usize::from(to.1) * SIZE + usize::from(to.0))?;
        (d != u32::MAX).then_some(d)
    }

    /// The tiles to enter, in order, and the turns they cost.
    #[must_use]
    pub fn path_to(&self, to: (u8, u8)) -> Option<(Vec<(u8, u8)>, u32)> {
        let total = self.cost(to)?;
        let idx = |(x, y): (u8, u8)| usize::from(y) * SIZE + usize::from(x);
        let mut path = Vec::new();
        let mut at = to;
        while at != self.from {
            path.push(at);
            at = self.prev[idx(at)]?;
        }
        path.reverse();
        Some((path, total))
    }
}

/// The quickest safe route to `to`: the tiles to enter in order and the turns they cost. `None`
/// if there is none that fits in the turns left today.
#[must_use]
pub fn route(
    env: &Env<'_>,
    clan: &Clan,
    trip: &Trip,
    to: (u8, u8),
) -> Option<(Vec<(u8, u8)>, u32)> {
    if trip.at == to {
        return Some((Vec::new(), 0));
    }
    Reach::new(env, clan, trip).path_to(to)
}

/// Turns until the water covers the party's tile (they are swept then), looking `ahead` turns:
/// `None` with a boat, or if it stays above the water that long.
#[must_use]
pub fn turns_until_drowned(env: &Env<'_>, clan: &Clan, trip: &Trip, ahead: u8) -> Option<u8> {
    if trip.kit.boats > 0 {
        return None;
    }
    let tile = env
        .map
        .tile(usize::from(trip.at.0), usize::from(trip.at.1))?;
    let base = env.minute(clan);
    (0..=ahead).find(|t| {
        let level = env.tide.level(base + i64::from(*t) * turn_len(env));
        Access::of(i32::from(tile.elevation), level) == Access::Drowned
    })
}

/// The highest water level the party's tile stands above (they are swept past it), or `None`
/// with a boat.
#[must_use]
pub fn safe_level(env: &Env<'_>, trip: &Trip) -> Option<i32> {
    if trip.kit.boats > 0 {
        return None;
    }
    let tile = env
        .map
        .tile(usize::from(trip.at.0), usize::from(trip.at.1))?;
    Some(i32::from(tile.elevation) + crate::tide::WET_BAND)
}

// ------------------------------------------------------------------ conditions and scenes

struct View<'a> {
    cat: &'a Catalog,
    clan: &'a Clan,
    trip: &'a Trip,
}

impl View<'_> {
    fn members(&self) -> impl Iterator<Item = &Crew> {
        self.trip
            .party
            .iter()
            .filter_map(|id| self.clan.crew_by(*id))
            .filter(|c| c.fit(self.cat))
    }

    fn holds(&self, cond: &Cond) -> bool {
        let yes = match &cond.req {
            Req::Trade(t, lvl) => self.members().any(|c| c.trade == *t && c.level(*t) >= *lvl),
            Req::Skill(t, lvl) => self.members().any(|c| c.level(*t) >= *lvl),
            Req::Trait(id) => self.members().any(|c| c.traits.iter().any(|x| x == id)),
            Req::Scar => self.members().any(|c| c.trait_any(self.cat, |t| t.scar)),
            Req::Bond => {
                let ids: Vec<u16> = self.members().map(|c| c.id).collect();
                self.clan.bonded_pairs_in(&ids) > 0
            }
            Req::Mark(m) => self.clan.has_mark(m),
            Req::Carry(r, n) => self.trip.carried(*r) >= *n,
            Req::Size(n) => self.trip.party.len() >= usize::from(*n),
            Req::Boat => self.trip.kit.boats > 0,
        };
        yes != cond.not
    }

    fn all(&self, conds: &[Cond]) -> bool {
        conds.iter().all(|c| self.holds(c))
    }

    /// The options of `e` this party is offered.
    fn offered(&self, e: &EventDef) -> Vec<u8> {
        e.options
            .iter()
            .enumerate()
            .filter(|(_, o)| self.all(&o.needs))
            .map(|(i, _)| u8::try_from(i).unwrap_or(0))
            .collect()
    }

    fn best_level(&self, skill: Trade) -> u8 {
        self.members().map(|c| c.level(skill)).max().unwrap_or(0)
    }
}

/// Where a scene is drawn for.
#[derive(Debug, Clone, Copy)]
enum Place {
    Tile(TileKind),
    Poi(PoiKind),
    Camp,
}

/// The options a scene would offer this party (used by the screen and the branch tests).
#[must_use]
pub fn offered_options(cat: &Catalog, clan: &Clan, trip: &Trip, event: &EventDef) -> Vec<u8> {
    View { cat, clan, trip }.offered(event)
}

/// The scenes that fit this place, moment, party and history, with their weights.
fn pool(env: &Env<'_>, clan: &Clan, trip: &Trip, place: Place) -> Vec<(usize, u32)> {
    let view = View {
        cat: env.cat,
        clan,
        trip,
    };
    let minute = env.minute(clan);
    let level = env.level(clan);
    let (water, part, stage) = (
        Water::of(level),
        Daypart::of(minute),
        Stage::of(env.day - clan.founded),
    );
    let mut pool: Vec<(usize, u32)> = Vec::new();
    for (i, e) in env.cat.events.iter().enumerate() {
        let fits = match place {
            Place::Tile(kind) => {
                e.poi.is_empty() && !e.camp && (e.tiles.is_empty() || e.tiles.contains(&kind))
            }
            Place::Poi(kind) => e.poi.contains(&kind),
            Place::Camp => e.camp,
        };
        if !fits
            || (!e.water.is_empty() && !e.water.contains(&water))
            || (!e.day.is_empty() && !e.day.contains(&part))
            || (!e.stage.is_empty() && !e.stage.contains(&stage))
            || (e.rare && clan.has_mark(&format!("seen:{}", e.id)))
            || !view.all(&e.needs)
        {
            continue;
        }
        let boost = if e.draws.iter().any(|c| view.holds(c)) {
            3
        } else {
            1
        };
        pool.push((i, e.weight * boost));
    }
    pool
}

fn draw(env: &Env<'_>, clan: &Clan, trip: &Trip, place: Place, rng: &mut DoorRng) -> Option<Scene> {
    let pool = pool(env, clan, trip, place);
    let total: u32 = pool.iter().map(|(_, w)| *w).sum();
    if total == 0 {
        return None;
    }
    let view = View {
        cat: env.cat,
        clan,
        trip,
    };
    let mut pick = rng.below(total);
    for (i, w) in pool {
        if pick < w {
            let e = &env.cat.events[i];
            return Some(Scene {
                event: e.id.clone(),
                offered: view.offered(e),
            });
        }
        pick -= w;
    }
    None
}

fn raise(clan: &mut Clan, trip: &mut Trip, cat: &Catalog, scene: Scene, events: &mut Vec<Event>) {
    if let Some(e) = cat.event(&scene.event) {
        if e.rare {
            clan.add_mark(&format!("seen:{}", e.id));
        }
        events.push(Event::Scene {
            title: e.title.clone(),
            text: e.text.clone(),
        });
    }
    trip.scenes = trip.scenes.saturating_add(1);
    trip.phase = Phase::Scene(scene);
}

// ------------------------------------------------------------------ time and cargo

fn spend(env: &Env<'_>, clan: &mut Clan, trip: &mut Trip, turns: u8) {
    for i in 0..turns {
        let at = clan.turns_used.saturating_add(i);
        let minute = crate::tide::Tide::turn_minute(env.day, at, env.settings.turns_per_day);
        if Daypart::of(minute) == Daypart::Night {
            trip.night_turns = trip.night_turns.saturating_add(1);
        }
    }
    trip.turns_out = trip.turns_out.saturating_add(u16::from(turns));
    clan.turns_used = clan.turns_used.saturating_add(turns);
}

fn add_loot(
    env: &Env<'_>,
    clan: &Clan,
    trip: &mut Trip,
    what: Resource,
    n: u32,
    events: &mut Vec<Event>,
) {
    if n == 0 {
        return;
    }
    let hard = trip.capacity(clan, env.cat) * 3 / 2;
    let room = hard.saturating_sub(trip.load());
    let take = n.min(room / weight(what));
    trip.loot.add(what, take);
    if take > 0 {
        events.push(Event::Found { what, qty: take });
    }
    if take < n {
        events.push(Event::LeftBehind {
            what,
            qty: n - take,
        });
    }
}

fn lose_goods(trip: &mut Trip, what: Resource, n: u32, events: &mut Vec<Event>) {
    let from_loot = trip.loot.take_up_to(what, n);
    let from_kit = trip.kit.take_up_to(what, n - from_loot);
    let qty = from_loot + from_kit;
    if qty > 0 {
        events.push(Event::Dropped { what, qty });
    }
}

fn value_of(trip: &Trip) -> u32 {
    trip.tallies
        + Resource::ALL
            .iter()
            .map(|r| trip.loot.get(*r) * u32::from(r.base_price()))
            .sum::<u32>()
}

// ------------------------------------------------------------------ effects

fn pick_who(clan: &Clan, trip: &Trip, who: Who, rng: &mut DoorRng) -> Vec<u16> {
    let party = &trip.party;
    match who {
        Who::All => party.clone(),
        Who::Lead => party.first().copied().into_iter().collect(),
        Who::Any => {
            let n = u32::try_from(party.len()).unwrap_or(1);
            party
                .get(usize::try_from(rng.below(n)).unwrap_or(0))
                .copied()
                .into_iter()
                .collect()
        }
        Who::Trade(t) => party
            .iter()
            .copied()
            .find(|id| clan.crew_by(*id).is_some_and(|c| c.trade == t))
            .into_iter()
            .collect(),
    }
}

fn kill(
    env: &Env<'_>,
    clan: &mut Clan,
    trip: &mut Trip,
    id: u16,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) {
    let Some(name) = clan.crew_by(id).map(|c| c.name.clone()) else {
        return;
    };
    clan.remove_crew(env.cat, id, Some(EpitaphCause::Lost), rng, events);
    trip.party.retain(|p| *p != id);
    clan.stats.lost_on_trips += 1;
    events.push(Event::Died { name });
}

#[allow(clippy::too_many_lines)]
fn run_effects(
    env: &Env<'_>,
    clan: &mut Clan,
    trip: &mut Trip,
    fx: &[Effect],
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Option<Ending> {
    let cat = env.cat;
    for e in fx {
        match e {
            Effect::Gain(r, lo, hi) => {
                let n = rng.range(*lo, *hi);
                add_loot(env, clan, trip, *r, n, events);
            }
            Effect::Lose(r, n) => lose_goods(trip, *r, *n, events),
            Effect::Tallies(lo, hi) => {
                let n = rng.range(*lo, *hi);
                trip.tallies = trip.tallies.saturating_add(n);
                events.push(Event::FoundTallies { qty: n });
            }
            Effect::Pay(n) => {
                trip.tallies = trip.tallies.saturating_sub(*n);
            }
            Effect::Hurt(who, cause) => {
                let hungry = trip.hungry();
                for id in pick_who(clan, trip, *who, rng) {
                    let Some(c) = clan.crew_by(id) else { continue };
                    let soften = c.trait_sum(cat, |t| t.accident_pct);
                    let pct = (100 + soften + if hungry { 25 } else { 0 }).clamp(15, 100);
                    if rng.chance(u32::try_from(pct).unwrap_or(15), 100) {
                        injure(cat, clan, id, *cause, rng, events);
                    }
                }
            }
            Effect::Die(who) => {
                for id in pick_who(clan, trip, *who, rng) {
                    kill(env, clan, trip, id, rng, events);
                }
                if trip.party.is_empty() {
                    return Some(Ending::Lost);
                }
            }
            Effect::Mark(m) => clan.add_mark(m),
            Effect::Unmark(m) => clan.remove_mark(m),
            Effect::Turns(n) => spend(env, clan, trip, *n),
            Effect::Bond(n) => trip.hardship = trip.hardship.saturating_add(*n).min(24),
            Effect::Xp(skill, n) => {
                let best = trip
                    .party
                    .iter()
                    .filter_map(|id| clan.crew_by(*id))
                    .max_by_key(|c| (c.level(*skill), Reverse(c.id)))
                    .map(|c| c.id);
                if let Some(id) = best
                    && let Some(c) = clan.crew_mut(id)
                    && let Some(level) = c.gain(cat, *skill, *n)
                {
                    events.push(Event::Trained {
                        name: c.name.clone(),
                        skill: *skill,
                        level,
                    });
                }
            }
            Effect::Morale(d) => {
                clan.morale = u8::try_from((i32::from(clan.morale) + i32::from(*d)).clamp(0, 100))
                    .unwrap_or(0);
            }
            Effect::Recipe(id) => {
                if !clan.recipes.contains(id)
                    && let Some(def) = cat.recipe(id)
                    && def.learn == Learn::Ruin
                {
                    clan.recipes.push(id.clone());
                    events.push(Event::Learned {
                        recipe: def.name.clone(),
                    });
                }
            }
            Effect::Chart(r) => {
                let tiles = clan.chart_around(
                    usize::from(trip.at.0),
                    usize::from(trip.at.1),
                    usize::from(*r),
                );
                if tiles > 0 {
                    events.push(Event::Surveyed { tiles });
                }
            }
            Effect::Heal(who, days) => {
                for id in pick_who(clan, trip, *who, rng) {
                    let Some(c) = clan.crew_mut(id) else { continue };
                    let mut done = Vec::new();
                    for i in &mut c.injuries {
                        i.days = i.days.saturating_sub(*days);
                        if i.days == 0 {
                            done.push(i.id.clone());
                        }
                    }
                    for inj in done {
                        finish_injury(cat, clan, id, &inj, rng, events);
                    }
                }
            }
            Effect::Fight(group) => {
                if let Some(end) = begin_fight(env, clan, trip, group, rng, events) {
                    return Some(end);
                }
            }
        }
    }
    None
}

// ------------------------------------------------------------------ fights

fn water_limit(env: &Env<'_>, clan: &Clan, trip: &Trip) -> u8 {
    if trip.kit.boats > 0 {
        return 0;
    }
    let Some(tile) = env.map.tile(usize::from(trip.at.0), usize::from(trip.at.1)) else {
        return 0;
    };
    let base = env.minute(clan);
    for t in 0..=12i64 {
        let level = env.tide.level(base + t * turn_len(env));
        if Access::of(i32::from(tile.elevation), level) == Access::Drowned {
            return u8::try_from((t * i64::from(ROUNDS_PER_TURN)).max(1)).unwrap_or(u8::MAX);
        }
    }
    0
}

fn begin_fight(
    env: &Env<'_>,
    clan: &mut Clan,
    trip: &mut Trip,
    group: &str,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Option<Ending> {
    let cat = env.cat;
    let level = env.level(clan);
    let wet = env
        .map
        .tile(usize::from(trip.at.0), usize::from(trip.at.1))
        .is_some_and(|t| i32::from(t.elevation) < level);
    let party: Vec<Unit> = trip
        .party
        .iter()
        .filter_map(|id| clan.crew_by(*id))
        .enumerate()
        .map(|(i, c)| Unit::from_crew(cat, c, i < usize::from(trip.kit.arms)))
        .collect();
    let flee_mod = if trip.overloaded(clan, cat) { -20 } else { 0 };
    let limit = water_limit(env, clan, trip);
    let mut fight = Fight::begin(cat, party, group, wet, level, limit, flee_mod);
    if let Some(g) = cat.group(group) {
        events.push(Event::Told(g.intro.clone()));
    }
    trip.fights = trip.fights.saturating_add(1);
    clan.stats.fights += 1;
    let mut lines = Vec::new();
    let progress = fight.advance(rng, &mut lines);
    events.extend(lines.into_iter().map(Event::Fight));
    match progress {
        Progress::Turn(_) => {
            trip.phase = Phase::Fight(Box::new(fight));
            None
        }
        Progress::Over(v) => finish_fight(env, clan, trip, &fight, v, rng, events),
    }
}

/// Run a fight forward by one decision (or all the way, for quick-resolve) and close it if it
/// ended. Returns the trip's ending if the fight ended it.
fn drive_fight(
    env: &Env<'_>,
    clan: &mut Clan,
    trip: &mut Trip,
    action: Option<FightAction>,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Result<Option<Ending>, ActionError> {
    let Phase::Fight(mut fight) = std::mem::replace(&mut trip.phase, Phase::Free) else {
        return Err(ActionError::NotNow("There is no fight.".to_owned()));
    };
    let mut lines = Vec::new();
    let progress = if let Some(a) = action {
        match fight.act(a, rng, &mut lines) {
            Ok(p) => p,
            Err(msg) => {
                trip.phase = Phase::Fight(fight);
                return Err(ActionError::Cannot(msg));
            }
        }
    } else {
        let mut p = fight.advance(rng, &mut lines);
        for _ in 0..200 {
            let Progress::Turn(idx) = p else { break };
            let a = fight.auto(idx);
            p = fight
                .act(a, rng, &mut lines)
                .or_else(|_| fight.act(FightAction::Brace, rng, &mut lines))
                .unwrap_or(Progress::Over(Verdict::Stalemate));
        }
        p
    };
    events.extend(lines.into_iter().map(Event::Fight));
    match progress {
        Progress::Turn(_) => {
            trip.phase = Phase::Fight(fight);
            Ok(None)
        }
        Progress::Over(v) => Ok(finish_fight(env, clan, trip, &fight, v, rng, events)),
    }
}

#[allow(clippy::too_many_lines)]
fn finish_fight(
    env: &Env<'_>,
    clan: &mut Clan,
    trip: &mut Trip,
    fight: &Fight,
    verdict: Verdict,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Option<Ending> {
    let cat = env.cat;
    trip.phase = Phase::Free;
    events.push(Event::Skirmish { verdict });
    let rounds = fight.round.max(1);
    let fight_turns = rounds.div_ceil(ROUNDS_PER_TURN);
    let party: Vec<(u16, i16, i16)> = fight
        .units
        .iter()
        .filter(|u| u.side == Side::Party)
        .map(|u| (u.crew, u.hp, u.max_hp))
        .collect();
    let healer_up = fight
        .units
        .iter()
        .any(|u| u.side == Side::Party && u.alive() && u.trade == Some(Trade::Healer));
    match verdict {
        Verdict::Won => {
            clan.stats.fights_won += 1;
            let mut taken = 0u32;
            for u in fight
                .units
                .iter()
                .filter(|u| u.side == Side::Foe && u.downed())
            {
                taken += 1;
                if let Some(def) = cat.foe(&u.foe) {
                    for (r, n) in &def.drops.items {
                        add_loot(env, clan, trip, *r, *n, events);
                    }
                    if def.drops.tallies > 0 {
                        trip.tallies = trip.tallies.saturating_add(def.drops.tallies);
                        events.push(Event::FoundTallies {
                            qty: def.drops.tallies,
                        });
                    }
                }
            }
            for &(id, hp, max) in &party {
                if let Some(c) = clan.crew_mut(id) {
                    c.gain(cat, Trade::Fighter, 2);
                }
                let hurt_bad = hp <= 0 || (hp < max * 3 / 10 && rng.chance(3, 10));
                if hurt_bad {
                    injure(cat, clan, id, Cause::Fight, rng, events);
                }
            }
            trip.hardship = trip
                .hardship
                .saturating_add(u8::try_from(taken.min(8)).unwrap_or(1))
                .min(24);
            if let Some(poi) = env.map.poi_at(trip.at.0, trip.at.1)
                && poi.kind == PoiKind::Lair
            {
                clan.add_mark(&format!("done:{},{}", trip.at.0, trip.at.1));
            }
            spend(env, clan, trip, fight_turns);
            None
        }
        Verdict::Fled | Verdict::Washed => {
            for (r, left) in Resource::ALL.map(|r| (r, trip.loot.get(r))) {
                trip.loot.take_up_to(r, left.div_ceil(4));
            }
            trip.tallies -= trip.tallies.div_ceil(4);
            for &(id, hp, _) in &party {
                if hp <= 0 {
                    injure(cat, clan, id, Cause::Fight, rng, events);
                    let chance = if healer_up { 5 } else { 15 };
                    if rng.chance(chance, 100) {
                        kill(env, clan, trip, id, rng, events);
                    }
                }
            }
            spend(env, clan, trip, 1 + fight_turns);
            trip.party.is_empty().then_some(Ending::Lost)
        }
        Verdict::Stalemate => {
            for &(id, hp, _) in &party {
                if hp <= 0 {
                    injure(cat, clan, id, Cause::Fight, rng, events);
                }
            }
            spend(env, clan, trip, 2 + fight_turns);
            None
        }
        Verdict::Lost => {
            clan.stats.routed += 1;
            for &(id, _, _) in &party {
                let sturdy = clan
                    .crew_by(id)
                    .map_or(0, |c| c.trait_sum(cat, |t| t.heal_pct));
                let chance = u32::try_from(35 - sturdy / 2).unwrap_or(10);
                if rng.chance(chance, 100) {
                    kill(env, clan, trip, id, rng, events);
                } else {
                    injure(cat, clan, id, Cause::Fight, rng, events);
                }
            }
            Some(if trip.party.is_empty() {
                Ending::Lost
            } else {
                Ending::Routed
            })
        }
    }
}

// ------------------------------------------------------------------ endings

fn halve(trip: &mut Trip) {
    for r in Resource::ALL {
        let n = trip.loot.get(r);
        trip.loot.take_up_to(r, n.div_ceil(2));
    }
    trip.tallies /= 2;
}

fn bond_party(clan: &mut Clan, trip: &Trip, points: u8, events: &mut Vec<Event>) {
    if points == 0 {
        return;
    }
    for (i, &a) in trip.party.iter().enumerate() {
        for &b in &trip.party[i + 1..] {
            if let Some((x, y)) = clan.share_hardship(a, b, points) {
                clan.stats.bonds += 1;
                let name = |id| clan.crew_by(id).map(|c| c.name.clone()).unwrap_or_default();
                events.push(Event::Bonded {
                    a: name(x),
                    b: name(y),
                });
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn conclude(
    env: &Env<'_>,
    clan: &mut Clan,
    mut trip: Trip,
    ending: Ending,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) {
    let cat = env.cat;
    trip.phase = Phase::Free;
    let due = trip.rations_due();
    trip.kit.take_up_to(Resource::Food, due);
    let burned = u32::from(trip.night_turns).div_ceil(LIGHT_TURNS);
    trip.kit.take_up_to(Resource::Peat, burned);
    let turns = trip.turns_out;
    clan.stats.trips += 1;
    match ending {
        Ending::Home => {
            clan.stats.returned += 1;
            let value = value_of(&trip);
            for (r, n) in trip.kit.pairs() {
                clan.stock.add(r, u32::from(n));
            }
            for r in Resource::ALL {
                clan.stock.add(r, trip.loot.get(r));
            }
            clan.tallies = clan.tallies.saturating_add(trip.tallies);
            clan.stats.loot_value = clan.stats.loot_value.saturating_add(value);
            let xp = 1 + (turns / 8).min(3);
            for &id in &trip.party {
                if let Some(c) = clan.crew_mut(id) {
                    let skill = c.trade;
                    c.gain(cat, skill, xp);
                }
            }
            let points = if turns >= 6 { 1 + trip.hardship } else { 0 };
            bond_party(clan, &trip, points, events);
            events.push(Event::TripEnded {
                ending,
                value,
                turns,
            });
        }
        Ending::Stranded | Ending::Overnight | Ending::Swept => {
            if ending == Ending::Swept {
                clan.stats.swept += 1;
                // The water keeps a boat.
                trip.kit.take_up_to(Resource::Boats, 1);
            } else {
                clan.stats.cut_off += 1;
            }
            halve(&mut trip);
            let value = value_of(&trip);
            for (r, n) in trip.kit.pairs() {
                clan.stock.add(r, u32::from(n));
            }
            for r in Resource::ALL {
                clan.stock.add(r, trip.loot.get(r));
            }
            clan.tallies = clan.tallies.saturating_add(trip.tallies);
            clan.stats.loot_value = clan.stats.loot_value.saturating_add(value);
            let (cause, pct) = match ending {
                Ending::Swept => (Cause::Water, 30),
                _ => (Cause::Cold, 25),
            };
            for id in trip.party.clone() {
                let soften = clan
                    .crew_by(id)
                    .map_or(0, |c| c.trait_sum(cat, |t| t.accident_pct));
                let p = (pct + soften / 4).clamp(10, 95);
                if rng.chance(u32::try_from(p).unwrap_or(40), 100) {
                    injure(cat, clan, id, cause, rng, events);
                }
            }
            bond_party(clan, &trip, 3 + trip.hardship.min(6), events);
            clan.morale = clan.morale.saturating_sub(3);
            let tpd = env.settings.turns_per_day;
            clan.turns_used = if ending == Ending::Swept {
                tpd.min(clan.turns_used.saturating_add(4))
            } else {
                tpd
            };
            events.push(Event::TripEnded {
                ending,
                value,
                turns,
            });
        }
        Ending::Routed => {
            for (r, n) in trip.kit.pairs() {
                if r == Resource::Boats {
                    clan.stock.add(r, u32::from(n));
                }
            }
            let tpd = env.settings.turns_per_day;
            clan.turns_used = tpd.min(clan.turns_used.saturating_add(3));
            clan.morale = clan.morale.saturating_sub(6);
            events.push(Event::TripEnded {
                ending,
                value: 0,
                turns,
            });
        }
        Ending::Lost => {
            clan.stats.lost_parties += 1;
            clan.morale = clan.morale.saturating_sub(10);
            events.push(Event::TripEnded {
                ending,
                value: 0,
                turns,
            });
        }
    }
    clan.trip = None;
}

/// A trip left open past midnight ends in the dark: the party makes camp, and finds its way
/// home in the morning with what it can carry.
pub fn overnight(env: &Env<'_>, clan: &mut Clan, rng: &mut DoorRng, events: &mut Vec<Event>) {
    if let Some(trip) = clan.trip.take() {
        conclude(env, clan, trip, Ending::Overnight, rng, events);
    }
}

fn after_action(env: &Env<'_>, clan: &Clan, trip: &Trip) -> Option<Ending> {
    if trip.at == home(env, clan) {
        return None;
    }
    if let Some(safe) = safe_level(env, trip)
        && env.level(clan) > safe
    {
        return Some(Ending::Swept);
    }
    (trip.phase == Phase::Free && clan.turns_left(&env.settings) == 0).then_some(Ending::Stranded)
}

// ------------------------------------------------------------------ actions

/// Set out. Pays the kit from the stores; nothing is spent if the plan is refused.
///
/// # Errors
/// [`ActionError`] says why.
pub fn depart(
    env: &Env<'_>,
    clan: &mut Clan,
    party: &[u16],
    kit: Kit,
    target: Option<(u8, u8)>,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    let cat = env.cat;
    if clan.trip.is_some() {
        return Err(ActionError::OnTrip);
    }
    let have = clan.turns_left(&env.settings);
    if have < MIN_DEPART_TURNS {
        return Err(ActionError::NoTurns {
            need: MIN_DEPART_TURNS,
            have,
        });
    }
    let distinct = party
        .iter()
        .enumerate()
        .all(|(i, c)| !party[..i].contains(c));
    if party.is_empty() || party.len() > MAX_PARTY || !distinct {
        return Err(ActionError::BadParty);
    }
    let level = env.level(clan);
    for &id in party {
        let c = clan.crew_by(id).ok_or(ActionError::NoCrew(id))?;
        if !c.fit(cat) {
            return Err(ActionError::Unfit(c.name.clone()));
        }
        if level >= crate::tide::HOLD_FLOOD_LEVEL && c.trait_any(cat, |t| t.refuses_high) {
            return Err(ActionError::Refuses(c.name.clone()));
        }
    }
    if kit.boats > MAX_BOATS {
        return Err(ActionError::BadKit(format!("at most {MAX_BOATS} boats")));
    }
    for (r, n) in kit.pairs() {
        if clan.stock.get(r) < u32::from(n) {
            return Err(ActionError::CantAfford(format!("{n} {}", r.name())));
        }
    }
    if let Some((x, y)) = target
        && (usize::from(x) >= SIZE || usize::from(y) >= SIZE)
    {
        return Err(ActionError::BadKit("that tile is off the map".to_owned()));
    }
    let cap = capacity_of(clan, cat, party, kit.boats);
    if kit.weight() > cap * 3 / 2 {
        return Err(ActionError::Overweight);
    }
    for (r, n) in kit.pairs() {
        clan.stock.take_up_to(r, u32::from(n));
    }
    clan.stats.departures += 1;
    let at = home(env, clan);
    clan.trip = Some(Trip {
        party: party.to_vec(),
        kit,
        loot: Stock::default(),
        tallies: 0,
        at,
        target,
        turns_out: 0,
        night_turns: 0,
        phase: Phase::Free,
        ruin: None,
        visited: vec![at],
        worked: Vec::new(),
        hardship: 0,
        scenes: 0,
        fights: 0,
        // The one thing the session's seed does: start the trip's dice. Nothing has been
        // rolled yet, so there is nothing to gain by opening the door again.
        rng: DoorRng::from_seed(rng.next_u64()).state(),
    });
    events.push(Event::Departed {
        crew: u8::try_from(party.len()).unwrap_or(1),
    });
    Ok(())
}

/// What stopped a step or a march.
enum Stop {
    /// Nothing; carry on.
    Go,
    /// Something wants the player (a scene, a fight, a place).
    Halt,
    /// The trip is over.
    End(Ending),
}

fn must_be_free(trip: &Trip) -> Result<(), ActionError> {
    match trip.phase {
        Phase::Free => Ok(()),
        Phase::Scene(_) => Err(ActionError::NotNow("Answer the scene first.".to_owned())),
        Phase::Fight(_) => Err(ActionError::NotNow("Finish the fight first.".to_owned())),
    }
}

/// The clan that has stood in the Deep is remembered for it (renown, the hall).
fn remember_ground(clan: &mut Clan, kind: TileKind) {
    if kind == TileKind::Deep {
        clan.social.deep = true;
    }
}

fn enter(
    env: &Env<'_>,
    clan: &mut Clan,
    trip: &mut Trip,
    to: (u8, u8),
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Result<Stop, ActionError> {
    let minute = env.minute(clan);
    let cost = move_cost(env, clan, trip, to, minute).ok_or_else(|| {
        ActionError::Blocked("The water will have that ground before you do.".to_owned())
    })?;
    let have = clan.turns_left(&env.settings);
    if cost > have {
        return Err(ActionError::NoTurns { need: cost, have });
    }
    spend(env, clan, trip, cost);
    trip.at = to;
    let new = !trip.visited.contains(&to);
    if new {
        trip.visited.push(to);
        if trip.visited.len() > VISITED_CAP {
            trip.visited.remove(0);
        }
    }
    clan.chart_around(usize::from(to.0), usize::from(to.1), 1);
    if let Some(t) = env.map.tile(usize::from(to.0), usize::from(to.1)) {
        remember_ground(clan, t.kind);
    }
    trip.ruin = None;
    if to == home(env, clan) {
        return Ok(Stop::End(Ending::Home));
    }
    if let Some(safe) = safe_level(env, trip)
        && env.level(clan) > safe
    {
        return Ok(Stop::End(Ending::Swept));
    }
    let Some(tile) = env.map.tile(usize::from(to.0), usize::from(to.1)) else {
        return Ok(Stop::Go);
    };
    if let Some(poi) = env.map.poi_at(to.0, to.1) {
        let name = env
            .cat
            .pois
            .get(usize::from(poi.template))
            .map_or("a place", |t| t.name.as_str());
        events.push(Event::Arrived {
            place: name.to_owned(),
        });
        if trip.target == Some(to) {
            trip.target = None;
        }
        let done = clan.has_mark(&format!("done:{},{}", to.0, to.1));
        if done {
            events.push(Event::Told(
                "There is nothing left here to take.".to_owned(),
            ));
        } else {
            if poi.kind == PoiKind::Ruin {
                let rooms = env
                    .cat
                    .pois
                    .get(usize::from(poi.template))
                    .map_or(1, |t| t.rooms);
                trip.ruin = Some(Ruin { room: 1, rooms });
            }
            if let Some(scene) = draw(env, clan, trip, Place::Poi(poi.kind), rng) {
                raise(clan, trip, env.cat, scene, events);
            }
        }
        return Ok(Stop::Halt);
    }
    if new {
        let band = match tile.kind {
            TileKind::Deep => 12,
            _ if to.1 < 30 => 5,
            _ => 0,
        };
        if rng.chance(EVENT_PCT + band, 100)
            && let Some(scene) = draw(env, clan, trip, Place::Tile(tile.kind), rng)
        {
            raise(clan, trip, env.cat, scene, events);
            return Ok(Stop::Halt);
        }
    }
    if trip.target == Some(to) {
        trip.target = None;
        return Ok(Stop::Halt);
    }
    Ok(Stop::Go)
}

fn march(
    env: &Env<'_>,
    clan: &mut Clan,
    trip: &mut Trip,
    to: (u8, u8),
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Result<Option<Ending>, ActionError> {
    if to == trip.at {
        return Ok((to == home(env, clan)).then_some(Ending::Home));
    }
    let (path, _) = route(env, clan, trip, to).ok_or(ActionError::TooFar)?;
    trip.target = Some(to);
    let mut first = true;
    for tile in path {
        match enter(env, clan, trip, tile, rng, events) {
            Ok(Stop::Go) => {}
            Ok(Stop::Halt) => return Ok(None),
            Ok(Stop::End(e)) => return Ok(Some(e)),
            Err(e) if first => return Err(e),
            Err(e) => {
                events.push(Event::Told(format!("The march stops: {e}")));
                return Ok(None);
            }
        }
        first = false;
    }
    Ok(None)
}

#[allow(clippy::too_many_lines)]
fn gather(
    env: &Env<'_>,
    clan: &mut Clan,
    trip: &mut Trip,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    let cat = env.cat;
    let tile = env
        .map
        .tile(usize::from(trip.at.0), usize::from(trip.at.1))
        .ok_or(ActionError::NothingToGather)?;
    let def = cat.gather(tile.kind).ok_or(ActionError::NothingToGather)?;
    if trip.at == home(env, clan) {
        return Err(ActionError::Cannot(
            "The hold's own ground is worked with a shift, not an expedition.".to_owned(),
        ));
    }
    if matches!(tile.kind, TileKind::Channel | TileKind::Mere) && trip.kit.boats == 0 {
        return Err(ActionError::Cannot(
            "You need a boat to work open water.".to_owned(),
        ));
    }
    let have = clan.turns_left(&env.settings);
    if have < GATHER_TURNS {
        return Err(ActionError::NoTurns {
            need: GATHER_TURNS,
            have,
        });
    }
    let times = trip
        .worked
        .iter()
        .find(|(x, y, _)| (*x, *y) == trip.at)
        .map_or(0, |(_, _, n)| *n);
    let factor = match times {
        0 => 100,
        1 => 50,
        _ => {
            return Err(ActionError::Cannot(
                "This ground is worked out for today.".to_owned(),
            ));
        }
    };
    let level = env.level(clan);
    let water_up = level >= crate::engine::WATER_UP;
    let hungry = trip.hungry();
    let mut totals = [0u32; crate::types::RESOURCES];
    let mut pocketed: Vec<(String, Resource, u32)> = Vec::new();
    let mut best_level = 0u8;
    let party = trip.party.clone();
    for &id in &party {
        let Some(c) = clan.crew_by(id) else { continue };
        if !c.fit(cat) {
            continue;
        }
        best_level = best_level.max(c.level(def.trade));
        let modifier = c.trait_sum(cat, |t| t.yield_pct + if water_up { t.wet_pct } else { 0 });
        let mut power = c.power(cat, def.trade) * (100 + modifier).max(0) / 100;
        power = power * factor / 100;
        if hungry {
            power = power * 3 / 4;
        }
        let luck = u32::try_from(c.trait_sum(cat, |t| i32::try_from(t.luck_pct).unwrap_or(0)))
            .unwrap_or(0);
        let pocket = c.trait_sum(cat, |t| i32::try_from(t.pocket_pct).unwrap_or(0));
        let lucky = rng.chance(luck, 100);
        if lucky {
            events.push(Event::Lucky {
                name: c.name.clone(),
            });
        }
        for &(res, n) in &def.yields.items {
            let mut got = scale(n, power, rng);
            if lucky {
                got *= 2;
            }
            let kept = scale(got, pocket, rng).min(got);
            totals[res.index()] += got - kept;
            if kept > 0 {
                pocketed.push((c.name.clone(), res, kept));
            }
        }
    }
    for r in Resource::ALL {
        let n = totals[r.index()];
        if n > 0 {
            add_loot(env, clan, trip, r, n, events);
        }
    }
    for (name, what, qty) in pocketed {
        events.push(Event::Pocketed { name, what, qty });
    }
    if let Some(find) = def.find {
        let pct = def.find_pct + 2 * u32::from(best_level);
        if rng.chance(pct, 100) {
            add_loot(env, clan, trip, find, 1, events);
        }
    }
    // Accidents: the water, hunger, steady and clumsy hands all move the odds.
    let cause = if level >= crate::tide::HOLD_FLOOD_LEVEL {
        Cause::Water
    } else {
        Cause::Hurt
    };
    for &id in &party {
        let Some(c) = clan.crew_by(id) else { continue };
        let mut pct = i32::try_from(def.hazard).unwrap_or(0);
        if water_up {
            pct = pct * 3 / 2;
        }
        if hungry {
            pct = pct * 3 / 2;
        }
        pct = pct * (100 + c.trait_sum(cat, |t| t.accident_pct)).max(0) / 100;
        if rng.chance(u32::try_from(pct).unwrap_or(0), 100) {
            injure(cat, clan, id, cause, rng, events);
        }
    }
    for &id in &party {
        if let Some(c) = clan.crew_mut(id) {
            c.gain(cat, def.trade, 2);
        }
    }
    match trip.worked.iter_mut().find(|(x, y, _)| (*x, *y) == trip.at) {
        Some((_, _, n)) => *n += 1,
        None => trip.worked.push((trip.at.0, trip.at.1, 1)),
    }
    trip.worked.truncate(40);
    trip.hardship = trip.hardship.saturating_add(u8::from(water_up)).min(24);
    spend(env, clan, trip, GATHER_TURNS);
    Ok(())
}

fn camp(
    env: &Env<'_>,
    clan: &mut Clan,
    trip: &mut Trip,
    turns: u8,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    if turns == 0 || turns > 10 {
        return Err(ActionError::BadAmount);
    }
    let have = clan.turns_left(&env.settings);
    if have < turns {
        return Err(ActionError::NoTurns { need: turns, have });
    }
    for _ in 0..turns {
        spend(env, clan, trip, 1);
        let night = Daypart::of(env.minute(clan)) == Daypart::Night;
        if trip.at != home(env, clan)
            && rng.chance(if night { 14 } else { 6 }, 100)
            && let Some(scene) = draw(env, clan, trip, Place::Camp, rng)
        {
            raise(clan, trip, env.cat, scene, events);
            break;
        }
        // The water may come up while you sit; stop and let the check below judge it.
        if safe_level(env, trip).is_some_and(|s| env.level(clan) > s) {
            break;
        }
    }
    events.push(Event::Camped {
        turns,
        level: env.level(clan),
    });
    Ok(())
}

fn delve(
    env: &Env<'_>,
    clan: &mut Clan,
    trip: &mut Trip,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    let ruin = trip.ruin.ok_or(ActionError::NoRuin)?;
    let have = clan.turns_left(&env.settings);
    if have < DELVE_TURNS {
        return Err(ActionError::NoTurns {
            need: DELVE_TURNS,
            have,
        });
    }
    if ruin.room >= ruin.rooms {
        return Err(ActionError::Cannot("That was the last room.".to_owned()));
    }
    spend(env, clan, trip, DELVE_TURNS);
    trip.ruin = Some(Ruin {
        room: ruin.room + 1,
        rooms: ruin.rooms,
    });
    if ruin.room + 1 >= ruin.rooms {
        clan.add_mark(&format!("done:{},{}", trip.at.0, trip.at.1));
    }
    events.push(Event::Told(format!(
        "Room {} of {}.",
        ruin.room + 1,
        ruin.rooms
    )));
    if let Some(scene) = draw(env, clan, trip, Place::Poi(PoiKind::Ruin), rng) {
        raise(clan, trip, env.cat, scene, events);
    }
    Ok(())
}

/// The chance (percent) an option goes badly for this party.
#[must_use]
pub fn fail_chance(
    cat: &Catalog,
    clan: &Clan,
    trip: &Trip,
    event: &str,
    option: usize,
) -> Option<u32> {
    let o = cat.event(event)?.options.get(option)?;
    if o.risk == 0 {
        return Some(0);
    }
    let view = View { cat, clan, trip };
    let level = o.skill.map_or(0, |s| i32::from(view.best_level(s)));
    let luck: i32 = view
        .members()
        .map(|c| c.trait_sum(cat, |t| i32::try_from(t.luck_pct).unwrap_or(0)))
        .sum();
    let accident: i32 = view
        .members()
        .map(|c| c.trait_sum(cat, |t| t.accident_pct))
        .sum();
    let p = i32::from(o.risk) - 8 * level - luck / 2 + accident / 10;
    u32::try_from(p.clamp(3, 95)).ok()
}

fn choose(
    env: &Env<'_>,
    clan: &mut Clan,
    trip: &mut Trip,
    n: u8,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Result<Option<Ending>, ActionError> {
    let Phase::Scene(scene) = &trip.phase else {
        return Err(ActionError::NotNow("There is no scene.".to_owned()));
    };
    let event = scene.event.clone();
    let def = env.cat.event(&event);
    let picked = scene.offered.get(usize::from(n)).copied();
    // A scene saved before the tables were edited may name a scene that is gone, or options
    // that are: the moment passes, and the party can go on.
    if def.is_none() || (scene.offered.is_empty() && n == 0) {
        trip.phase = Phase::Free;
        events.push(Event::Told("The moment passes.".to_owned()));
        return Ok(None);
    }
    let index = usize::from(picked.ok_or(ActionError::BadChoice(n))?);
    let option = def
        .and_then(|d| d.options.get(index))
        .ok_or(ActionError::BadChoice(n))?;
    let failed = fail_chance(env.cat, clan, trip, &event, index)
        .is_some_and(|p| p > 0 && rng.chance(p, 100));
    trip.phase = Phase::Free;
    let outcome = match (&option.lose, failed) {
        (Some(lose), true) => lose,
        _ => &option.win,
    };
    events.push(Event::Told(outcome.text.clone()));
    // A skill used well is a skill learned, win or lose.
    if let Some(skill) = option.skill
        && let Some(best) = trip
            .party
            .iter()
            .filter_map(|id| clan.crew_by(*id))
            .max_by_key(|c| (c.level(skill), Reverse(c.id)))
            .map(|c| c.id)
        && let Some(c) = clan.crew_mut(best)
    {
        c.gain(env.cat, skill, 1);
    }
    Ok(run_effects(env, clan, trip, &outcome.fx, rng, events))
}

/// Do one thing on the trip. A refused action changes nothing.
///
/// # Errors
/// [`ActionError`] says why.
pub fn apply(env: &Env<'_>, clan: &mut Clan, act: TripAction) -> Result<Vec<Event>, ActionError> {
    let mut trip = clan.trip.take().ok_or(ActionError::NoTrip)?;
    let before = (clan.clone(), trip.clone());
    let mut events = Vec::new();
    // The dice come from the save, not the session, and go back into it: a refused action
    // rolls nothing, and a key whose result was lost rolls the same again.
    let mut dice = DoorRng::from_state(trip.rng);
    let result = run(env, clan, &mut trip, act, &mut dice, &mut events);
    trip.rng = dice.state();
    match result {
        Err(e) => {
            // Restore exactly: a refusal changes nothing.
            *clan = before.0;
            clan.trip = Some(before.1);
            Err(e)
        }
        Ok(end) => {
            let end = end.or_else(|| after_action(env, clan, &trip));
            match end {
                Some(e) => conclude(env, clan, trip, e, &mut dice, &mut events),
                None => clan.trip = Some(trip),
            }
            Ok(events)
        }
    }
}

fn run(
    env: &Env<'_>,
    clan: &mut Clan,
    trip: &mut Trip,
    act: TripAction,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Result<Option<Ending>, ActionError> {
    match act {
        TripAction::Step(dir) => {
            must_be_free(trip)?;
            let to = dir
                .step(trip.at)
                .ok_or_else(|| ActionError::Blocked("That is the edge of the map.".to_owned()))?;
            Ok(match enter(env, clan, trip, to, rng, events)? {
                Stop::End(e) => Some(e),
                Stop::Go | Stop::Halt => None,
            })
        }
        TripAction::March { to } => {
            must_be_free(trip)?;
            let to = to
                .or(trip.target)
                .ok_or_else(|| ActionError::Cannot("There is nowhere set to go.".to_owned()))?;
            march(env, clan, trip, to, rng, events)
        }
        TripAction::Gather => {
            must_be_free(trip)?;
            gather(env, clan, trip, rng, events)?;
            Ok(None)
        }
        TripAction::Camp { turns } => {
            must_be_free(trip)?;
            camp(env, clan, trip, turns, rng, events)?;
            Ok(None)
        }
        TripAction::Delve => {
            must_be_free(trip)?;
            delve(env, clan, trip, rng, events)?;
            Ok(None)
        }
        TripAction::Choose(n) => choose(env, clan, trip, n, rng, events),
        TripAction::Fight(a) => drive_fight(env, clan, trip, Some(a), rng, events),
        TripAction::Quick => drive_fight(env, clan, trip, None, rng, events),
        TripAction::Jettison { what, qty } => {
            must_be_free(trip)?;
            if qty == 0 || what == Resource::Boats {
                return Err(ActionError::BadAmount);
            }
            if trip.carried(what) == 0 {
                return Err(ActionError::Cannot("You carry none of that.".to_owned()));
            }
            lose_goods(trip, what, qty, events);
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests;
