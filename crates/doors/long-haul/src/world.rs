//! The shared world: what changes on the frontier and is the same for every captain.
//!
//! Stock levels at every station, news that is running, which systems have been charted and
//! by whom. The map itself is regenerated from the season ([`Frontier`]) and never stored. Two
//! things change it: a captain's trade or chart, sent as a [`Delta`] and applied to the stored
//! world with the framework's compare-and-swap (so concurrent captains each land their change),
//! and the nightly tick ([`World::tick_day`]).

use russh_bbs_door::DoorRng;

use crate::codec::{Reader, Writer};
use crate::content::Content;
use crate::frontier::{Frontier, GEN_VERSION};
use crate::market::{FULL_STOCK_DEPTHS, base_target, depth_at};
use crate::social::{Social, SocialDelta};

/// The world's stored format version.
pub const WORLD_FORMAT: u8 = 2;
/// The bytes before the social section (format byte, generator, season, day, tons).
const HEADER_LEN: usize = 22;
/// The most news events running at once.
pub const MAX_EVENTS: usize = 24;
/// The longest a captain's name stays on a chart.
pub const NAME_MAX: usize = 24;
/// How fast stock returns to its target each night, per-mille of the gap.
pub const RECOVERY_PM: i64 = 160;

/// A running news event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsEvent {
    /// Index into the content's news kinds.
    pub kind: u8,
    /// The station it started at.
    pub origin: u16,
    /// The day it began.
    pub born: i64,
    /// Nights it lasts.
    pub duration: u8,
}

impl NewsEvent {
    /// Nights since it began.
    #[must_use]
    pub const fn age(&self, today: i64) -> i64 {
        today - self.born
    }

    /// Whether it still acts on `today`.
    #[must_use]
    pub const fn active(&self, today: i64) -> bool {
        let age = self.age(today);
        age >= 0 && age < self.duration as i64
    }
}

/// Who charted a system and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Charter {
    /// The day number.
    pub day: i64,
    /// The captain's name, as the board knew it.
    pub name: String,
}

/// A change a captain makes to the shared world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delta {
    /// Tons moved into (positive) or out of (negative) a station's stock.
    Trade {
        /// The station.
        station: u16,
        /// The slot in its goods list.
        slot: u8,
        /// Tons, signed.
        tons: i32,
    },
    /// A system charted for the first time.
    Chart {
        /// The system.
        system: u16,
        /// The day number.
        day: i64,
        /// The captain's name.
        name: String,
    },
    /// A social change: convoys, piracy, outposts, the hall.
    Social(SocialDelta),
}

/// The shared world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    /// The season the frontier was generated for.
    pub season: u32,
    /// The generator version.
    pub gen_version: u8,
    /// The last day the nightly tick applied (or the day the world was made).
    pub day: i64,
    /// Stock in tons, per station and slot.
    pub stock: Vec<Vec<i32>>,
    /// Who charted each system (None: not charted by a captain).
    pub charted: Vec<Option<Charter>>,
    /// News running.
    pub events: Vec<NewsEvent>,
    /// Tons traded by every captain, for the season.
    pub traded_tons: i64,
    /// Convoys, piracy, outposts and records.
    pub social: Social,
}

fn clamp_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

/// The economy a new outpost has, from what its system offers.
fn outpost_economy(content: &Content, frontier: &Frontier, system: u16) -> u8 {
    let want = frontier.system(system).map_or("industrial", |s| {
        if s.belts >= 2 {
            "mining"
        } else if s.planets >= 3 {
            "farming"
        } else if s.richness >= 7 {
            "research"
        } else {
            "industrial"
        }
    });
    content
        .economy_ix(want)
        .and_then(|i| u8::try_from(i).ok())
        .unwrap_or(0)
}

impl World {
    /// A fresh world for `frontier` on `day`: every market near its target. Built from the
    /// season alone, so two sessions that race to make it make the same one.
    #[must_use]
    pub fn fresh(content: &Content, frontier: &Frontier, day: i64) -> Self {
        let mut rng = DoorRng::from_label("long-haul/world", i64::from(frontier.season));
        let stock = frontier
            .stations
            .iter()
            .map(|station| {
                station
                    .goods
                    .iter()
                    .enumerate()
                    .map(|(slot, s)| {
                        let depth = depth_at(&content.goods, frontier, station.id, slot);
                        let target = base_target(depth, s.role);
                        let wobble = i64::from(rng.range(900, 1100));
                        clamp_i32(target * wobble / 1000)
                    })
                    .collect()
            })
            .collect();
        Self {
            season: frontier.season,
            gen_version: GEN_VERSION,
            day,
            stock,
            charted: vec![None; frontier.systems.len()],
            events: Vec::new(),
            traded_tons: 0,
            social: Social::new(day),
        }
    }

    /// Whether a system is known to everyone: charted from the start or visited since.
    #[must_use]
    pub fn is_charted(&self, frontier: &Frontier, system: u16) -> bool {
        let Some(sys) = frontier.system(system) else {
            return false;
        };
        !sys.uncharted
            || self
                .charted
                .get(usize::from(system))
                .is_some_and(Option::is_some)
    }

    /// Stock of one slot, in tons.
    #[must_use]
    pub fn stock_of(&self, station: u16, slot: usize) -> i64 {
        self.stock
            .get(usize::from(station))
            .and_then(|row| row.get(slot))
            .map_or(0, |&v| i64::from(v))
    }

    /// Apply a captain's change. Stock stays between zero and the warehouse limit.
    pub fn apply(&mut self, content: &Content, frontier: &Frontier, delta: &Delta) {
        match delta {
            Delta::Trade {
                station,
                slot,
                tons,
            } => {
                let depth = depth_at(&content.goods, frontier, *station, usize::from(*slot));
                let limit = depth * FULL_STOCK_DEPTHS;
                if let Some(cell) = self
                    .stock
                    .get_mut(usize::from(*station))
                    .and_then(|row| row.get_mut(usize::from(*slot)))
                {
                    let next = (i64::from(*cell) + i64::from(*tons)).clamp(0, limit.max(0));
                    self.traded_tons += i64::from(tons.unsigned_abs());
                    *cell = clamp_i32(next);
                }
                let base_len = frontier.stations.len().saturating_sub(self.social.outposts.len());
                if let Some(k) = usize::from(*station).checked_sub(base_len)
                    && let Some(o) = self.social.outposts.get_mut(k)
                {
                    o.traded += i64::from(tons.unsigned_abs());
                }
            }
            Delta::Chart { system, day, name } => {
                if let Some(slot) = self.charted.get_mut(usize::from(*system))
                    && slot.is_none()
                    && frontier.system(*system).is_some_and(|s| s.uncharted)
                {
                    *slot = Some(Charter {
                        day: *day,
                        name: name.chars().take(NAME_MAX).collect(),
                    });
                }
            }
            Delta::Social(d) => self.social.apply(d, self.day),
        }
    }

    /// News that has reached `at` (it travels one hop a night), newest first, with its age and
    /// headline. A captain does not see news that has not yet arrived.
    #[must_use]
    pub fn news_at(
        &self,
        content: &Content,
        frontier: &Frontier,
        at: u16,
        today: i64,
    ) -> Vec<(i64, String)> {
        let hops = frontier.hops_from(at);
        let mut out: Vec<(i64, String)> = self
            .events
            .iter()
            .filter(|e| e.active(today))
            .filter_map(|e| {
                let origin = frontier.station(e.origin)?;
                let reached = i64::from(*hops.get(usize::from(origin.system))?) <= e.age(today);
                let def = content.news.get(usize::from(e.kind))?;
                reached.then(|| {
                    (
                        e.age(today),
                        def.headline.replace("{station}", &origin.name),
                    )
                })
            })
            .collect();
        out.sort_by_key(|(age, _)| *age);
        out
    }

    /// Extra inspection chance at a station from news in its reach, per-mille.
    #[must_use]
    pub fn inspection_bonus(
        &self,
        content: &Content,
        frontier: &Frontier,
        station: u16,
        today: i64,
    ) -> i64 {
        let Some(at) = frontier.station(station) else {
            return 0;
        };
        let mut total = 0i64;
        for event in self.events.iter().filter(|e| e.active(today)) {
            let (Some(def), Some(origin)) = (
                content.news.get(usize::from(event.kind)),
                frontier.station(event.origin),
            ) else {
                continue;
            };
            let hops = frontier.hops_from(origin.system);
            if def.inspection_pm > 0
                && hops
                    .get(usize::from(at.system))
                    .is_some_and(|&h| h <= def.radius)
            {
                total += i64::from(def.inspection_pm);
            }
        }
        total.min(600)
    }

    /// Stock a station aims for in a slot tonight, news included.
    #[must_use]
    pub fn target(
        &self,
        content: &Content,
        frontier: &Frontier,
        reach: &[(usize, Vec<u8>)],
        station: u16,
        slot: usize,
        today: i64,
    ) -> i64 {
        let Some(st) = frontier.station(station) else {
            return 0;
        };
        let Some(ms) = st.goods.get(slot) else {
            return 0;
        };
        let depth = depth_at(&content.goods, frontier, station, slot);
        let mut target = base_target(depth, ms.role);
        let economy_id = content
            .economies
            .get(usize::from(st.economy))
            .map_or("", |e| e.id.as_str());
        for (event_ix, hops) in reach {
            let Some(event) = self.events.get(*event_ix).filter(|e| e.active(today)) else {
                continue;
            };
            let Some(def) = content.news.get(usize::from(event.kind)) else {
                continue;
            };
            let hop = hops.get(usize::from(st.system)).copied().unwrap_or(255);
            if hop > def.radius {
                continue;
            }
            let good_id = content
                .goods
                .get(usize::from(ms.good))
                .map_or("", |g| g.id.as_str());
            for fx in def.effects.iter().filter(|fx| fx.good == good_id) {
                if !fx.only.is_empty() && !fx.only.iter().any(|o| o == economy_id) {
                    continue;
                }
                let span = i64::from(def.radius) + 1;
                let falloff = span - i64::from(hop);
                let factor = 1000 + (i64::from(fx.target_pm) - 1000) * falloff / span;
                target = target * factor / 1000;
            }
        }
        target
    }

    /// Apply one night: expire and spawn news, then move every stock toward its target with a
    /// little noise. A pure function of the world, the day and the RNG the tick was given.
    pub fn tick_day(
        &mut self,
        content: &Content,
        frontier: &Frontier,
        day: i64,
        rng: &mut DoorRng,
    ) {
        self.day = day;
        self.events.retain(|e| e.active(day));
        self.spawn_news(content, frontier, day, rng);
        let base = frontier;
        let extended = self.extended(content, base);
        let frontier = &extended;
        self.sync_rows(content, frontier);
        let reach: Vec<(usize, Vec<u8>)> = self
            .events
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                frontier
                    .station(e.origin)
                    .map(|o| (i, frontier.hops_from(o.system)))
            })
            .collect();
        for station in &frontier.stations {
            for (slot, ms) in station.goods.iter().enumerate() {
                let depth = depth_at(&content.goods, frontier, station.id, slot);
                let target = self.target(content, frontier, &reach, station.id, slot, day);
                let now = self.stock_of(station.id, slot);
                let gap = target - now;
                let mut next = now + gap * RECOVERY_PM / 1000;
                if gap != 0 && next == now && rng.chance(1, 2) {
                    next += gap.signum();
                }
                let vol = content
                    .goods
                    .get(usize::from(ms.good))
                    .map_or(0, |g| i64::from(g.volatility));
                let amp = depth * vol / 1000;
                if amp > 0 {
                    let span = u32::try_from(amp * 2).unwrap_or(u32::MAX);
                    next += i64::from(rng.below(span.saturating_add(1))) - amp;
                }
                let limit = depth * FULL_STOCK_DEPTHS;
                if let Some(cell) = self
                    .stock
                    .get_mut(usize::from(station.id))
                    .and_then(|row| row.get_mut(slot))
                {
                    *cell = clamp_i32(next.clamp(0, limit.max(0)));
                }
            }
        }
        self.social_night(content, base, day, rng);
    }

    /// The map as players have made it: the generated frontier plus every outpost.
    #[must_use]
    pub fn extended(&self, content: &Content, base: &Frontier) -> Frontier {
        base.with_outposts(content, &self.social.outposts)
    }

    /// Make sure every station of `frontier` has a row of stock (new outposts start at target).
    fn sync_rows(&mut self, content: &Content, frontier: &Frontier) {
        for station in frontier.stations.iter().skip(self.stock.len()) {
            let row = station
                .goods
                .iter()
                .enumerate()
                .map(|(slot, s)| {
                    let depth = depth_at(&content.goods, frontier, station.id, slot);
                    clamp_i32(base_target(depth, s.role))
                })
                .collect();
            self.stock.push(row);
        }
    }

    /// The part of the night that belongs to players: raids, convoys, outposts.
    fn social_night(&mut self, content: &Content, base: &Frontier, day: i64, rng: &mut DoorRng) {
        self.social
            .tick(day, content.social.convoy_event.len(), rng);
        if self.social.season_day(day) >= crate::social::OUTPOST_MIN_DAY {
            let suffixes = content.social.outpost_suffix.clone();
            let before = self.social.outposts.len();
            let mut k = 0usize;
            self.social.found_outposts(
                day,
                |system| outpost_economy(content, base, system),
                |lead, _| {
                    k += 1;
                    let suffix = suffixes
                        .get((usize::from(lead.len() as u8) + before + k) % suffixes.len().max(1))
                        .cloned()
                        .unwrap_or_default();
                    let lead = lead.split_whitespace().next().unwrap_or("Free");
                    format!("{lead} {suffix}")
                },
            );
        }
        self.social.grow_outposts();
        let extended = self.extended(content, base);
        self.sync_rows(content, &extended);
    }

    fn spawn_news(&mut self, content: &Content, frontier: &Frontier, day: i64, rng: &mut DoorRng) {
        let count = u32::from(rng.chance(7, 10)) + u32::from(rng.chance(1, 4));
        for _ in 0..count {
            if self.events.len() >= MAX_EVENTS {
                return;
            }
            let total: u32 = content.news.iter().map(|n| n.weight).sum();
            let mut roll = rng.below(total.max(1));
            let mut kind = 0usize;
            for (i, def) in content.news.iter().enumerate() {
                if roll < def.weight {
                    kind = i;
                    break;
                }
                roll -= def.weight;
            }
            let Some(def) = content.news.get(kind) else {
                return;
            };
            let candidates: Vec<u16> = frontier
                .stations
                .iter()
                .filter(|s| {
                    def.origin.is_empty()
                        || content
                            .economies
                            .get(usize::from(s.economy))
                            .is_some_and(|e| def.origin.contains(&e.id))
                })
                .map(|s| s.id)
                .collect();
            let Some(len) = u32::try_from(candidates.len()).ok().filter(|&n| n > 0) else {
                continue;
            };
            let origin = candidates[usize::try_from(rng.below(len)).unwrap_or(0)];
            self.events.push(NewsEvent {
                kind: u8::try_from(kind).unwrap_or(0),
                origin,
                born: day,
                duration: def.duration,
            });
        }
    }

    /// The season written at the head of a stored world, without reading the rest.
    #[must_use]
    pub fn peek_season(bytes: &[u8]) -> Option<u32> {
        let mut r = Reader::new(bytes);
        (r.u8()? == WORLD_FORMAT).then_some(())?;
        r.u8()?;
        r.u32()
    }

    /// A format-1 world as a format-2 world with an empty social section (the framework's
    /// migration from world version 1).
    #[must_use]
    pub fn migrate_v1(bytes: &[u8]) -> Option<Vec<u8>> {
        if bytes.first() != Some(&1) || bytes.len() < HEADER_LEN {
            return None;
        }
        let mut r = Reader::new(bytes);
        r.u8()?;
        r.u8()?;
        r.u32()?;
        let day = r.i64()?;
        let mut out = bytes[..HEADER_LEN].to_vec();
        out[0] = 2;
        out.extend_from_slice(&Social::empty_bytes(day));
        out.extend_from_slice(&bytes[HEADER_LEN..]);
        Some(out)
    }

    /// The social section of a stored world, without reading the stock (used to carry the hall
    /// into the next season's world).
    #[must_use]
    pub fn peek_social(bytes: &[u8], content: &Content) -> Option<(u32, Social)> {
        let mut r = Reader::new(bytes);
        (r.u8()? == WORLD_FORMAT).then_some(())?;
        r.u8()?;
        let season = r.u32()?;
        r.i64()?;
        r.i64()?;
        let social = Social::read(&mut r, usize::from(u16::MAX), content.economies.len())?;
        Some((season, social))
    }

    /// A fresh world for a new season that keeps the hall of the one before it.
    #[must_use]
    pub fn successor(
        content: &Content,
        frontier: &Frontier,
        day: i64,
        old: Option<&[u8]>,
    ) -> Self {
        let mut world = Self::fresh(content, frontier, day);
        if let Some((season, old_social)) = old.and_then(|b| Self::peek_social(b, content))
            && season < frontier.season
        {
            world.social.past = old_social.closed_into(season);
        }
        world
    }

    /// Replay a session's `deltas` on the stored bytes (or on a fresh world, when there are none
    /// or they belong to another season), and return the new bytes. Used by the closure the
    /// session hands the board, which may run more than once on a conflict.
    #[must_use]
    pub fn apply_stored(
        old: Option<&[u8]>,
        content: &Content,
        base: &Frontier,
        today: i64,
        deltas: &[Delta],
    ) -> Vec<u8> {
        let mut world = old
            .and_then(|bytes| Self::decode(bytes, content, base))
            .filter(|w| w.season == base.season && w.gen_version == GEN_VERSION)
            .unwrap_or_else(|| Self::successor(content, base, today, old));
        let extended = world.extended(content, base);
        for delta in deltas {
            world.apply(content, &extended, delta);
        }
        world.encode()
    }

    /// The stored form.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u8(WORLD_FORMAT);
        w.u8(self.gen_version);
        w.u32(self.season);
        w.i64(self.day);
        w.i64(self.traded_tons);
        self.social.write(&mut w);
        w.len(self.stock.len());
        for row in &self.stock {
            w.u8(u8::try_from(row.len()).unwrap_or(u8::MAX));
            for &cell in row {
                w.i32(cell);
            }
        }
        w.len(self.charted.len());
        for c in &self.charted {
            match c {
                None => w.u8(0),
                Some(c) => {
                    w.u8(1);
                    w.i64(c.day);
                    w.text(&c.name);
                }
            }
        }
        w.len(self.events.len());
        for e in &self.events {
            w.u8(e.kind);
            w.u16(e.origin);
            w.i64(e.born);
            w.u8(e.duration);
        }
        w.finish()
    }

    /// Read a stored world, refusing anything that does not fit `frontier` (a different shape
    /// means a different map, and its stock would be meaningless).
    #[must_use]
    pub fn decode(bytes: &[u8], content: &Content, frontier: &Frontier) -> Option<Self> {
        let mut r = Reader::new(bytes);
        if r.u8()? != WORLD_FORMAT {
            return None;
        }
        let gen_version = r.u8()?;
        let season = r.u32()?;
        let day = r.i64()?;
        let traded_tons = r.i64()?;
        let social = Social::read(&mut r, frontier.systems.len(), content.economies.len())?;
        let extended = frontier.with_outposts(content, &social.outposts);
        let frontier = &extended;
        let n_stations = r.len(frontier.stations.len(), 1)?;
        if n_stations != frontier.stations.len() {
            return None;
        }
        let mut stock = Vec::with_capacity(n_stations);
        for station in &frontier.stations {
            let n = usize::from(r.u8()?);
            if n != station.goods.len() {
                return None;
            }
            let mut row = Vec::with_capacity(n);
            for _ in 0..n {
                row.push(r.i32()?);
            }
            stock.push(row);
        }
        let n_systems = r.len(frontier.systems.len(), 1)?;
        if n_systems != frontier.systems.len() {
            return None;
        }
        let mut charted = Vec::with_capacity(n_systems);
        for _ in 0..n_systems {
            charted.push(match r.u8()? {
                0 => None,
                1 => Some(Charter {
                    day: r.i64()?,
                    name: r.text()?,
                }),
                _ => return None,
            });
        }
        let n_events = r.len(MAX_EVENTS, 12)?;
        let mut events = Vec::with_capacity(n_events);
        for _ in 0..n_events {
            events.push(NewsEvent {
                kind: r.u8()?,
                origin: r.u16()?,
                born: r.i64()?,
                duration: r.u8()?,
            });
        }
        if !r.done()
            || events
                .iter()
                .any(|e| usize::from(e.origin) >= frontier.stations.len())
        {
            return None;
        }
        Some(Self {
            season,
            gen_version,
            day,
            stock,
            charted,
            events,
            traded_tons,
            social,
        })
    }
}
