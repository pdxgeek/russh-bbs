//! The shared world: the season's seed, the Tidecourt's prices and the registry of holds.
//! It is changed by sessions (through the framework's compare-and-swap edit) and by the daily
//! tick, and it never holds anything private to one player.
//!
//! Layout (version 3), little-endian: season `u32`, seed `u64`, created `i64`, last tick `i64`,
//! prices (13 x `u16`), net sold (13 x `i32` as `u32`), clan count `u8`, then name, site,
//! renown (`u32`), the clan's posted defence and the world's record of the clan per clan, then
//! the price history: a count `u8` and that many rows of 13 x `u16`, oldest first, then the
//! social state ([`Social`]: raid orders, pacts, the exchange, the post, the feed and the
//! book of past seasons). Version 2 ended after the price history; version 1 also had no score
//! (see [`World::upgrade_v1`] and [`World::upgrade_v2`]).

use russh_bbs_door::DoorRng;

use crate::codec::{Reader, Writer};
use crate::social::legacy::Ending;
use crate::social::{ClanIx, Posted, Record, Social, Wire};
use crate::types::{RESOURCES, Resource};

/// The most holds the registry keeps.
pub const MAX_CLANS: usize = 200;

/// Prices are kept in hundredths of a tally.
pub type Prices = [u16; RESOURCES];

/// How many nights of prices the market remembers (two weeks, one tide cycle).
pub const HISTORY_DAYS: usize = 14;

/// What a clan has done that the season's ending reads (the clan writes it; the world sums it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Badge {
    /// The clan has stood in the Deep.
    pub deep: bool,
    /// Quest chains finished.
    pub quests: u8,
    /// Contracts finished.
    pub contracts: u8,
}

impl Badge {
    /// What a clan has done that the ending reads.
    #[must_use]
    pub fn of(cat: &crate::content::Catalog, clan: &crate::clan::Clan) -> Self {
        Self {
            deep: clan.social.deep,
            quests: u8::try_from(cat.quests_done(|m| clan.has_mark(m))).unwrap_or(u8::MAX),
            contracts: u8::try_from(clan.story.done).unwrap_or(u8::MAX),
        }
    }
}

/// A causeway tile laid by a clan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Column.
    pub x: u8,
    /// Row.
    pub y: u8,
    /// The builder.
    pub owner: ClanIx,
    /// The toll in tallies.
    pub toll: u8,
    /// The day it was laid.
    pub laid: i64,
}

/// The most causeway tiles on the marsh.
pub const SPANS_CAP: usize = 40;

/// A hold in the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClanEntry {
    /// The clan's name.
    pub name: String,
    /// Index into the map's hold sites.
    pub site: u8,
    /// The clan's renown the last time it was saved (`social::renown`): the league table's order.
    pub score: u32,
    /// What the clan shows the marsh: its home guard, defences and a few figures of stock.
    pub posted: Posted,
    /// The world's counters about the clan (standing, betrayals, raid and letter counts).
    pub record: Record,
    /// What the clan has done that the ending reads.
    pub badge: Badge,
}

/// The world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    /// The door's season this world belongs to.
    pub season: u32,
    /// The season's seed: the map and the tides.
    pub seed: u64,
    /// The day the world was made.
    pub created: i64,
    /// The last day the tick ran.
    pub last_tick: i64,
    /// Current prices, hundredths of a tally.
    pub prices: Prices,
    /// Goods sold to the Tidecourt today minus goods bought (the nightly price pressure).
    pub net_sold: [i32; RESOURCES],
    /// The holds.
    pub clans: Vec<ClanEntry>,
    /// The prices at the end of each of the last nights, oldest first, for the market's
    /// sparklines. Display only: no rule reads it.
    pub history: Vec<Prices>,
    /// Raids, pacts, the exchange, letters and the book of past seasons.
    pub social: Social,
    /// Causeway tiles on the marsh.
    pub causeways: Vec<Span>,
    /// Changes quests made to the marsh (bits of [`crate::story::Flag`]).
    pub flags: u8,
    /// How the season before ended (what this one inherited).
    pub legacy: Ending,
}

impl World {
    /// A fresh world for a season.
    #[must_use]
    pub fn new(season: u32, seed: u64, day: i64) -> Self {
        let mut prices = [0u16; RESOURCES];
        for r in Resource::ALL {
            prices[r.index()] = r.base_price() * 100;
        }
        Self {
            season,
            seed,
            created: day,
            last_tick: day,
            prices,
            net_sold: [0; RESOURCES],
            clans: Vec::new(),
            history: Vec::new(),
            social: Social::default(),
            causeways: Vec::new(),
            flags: 0,
            legacy: Ending::Quiet,
        }
    }

    /// Put what clans and quests have done onto a freshly generated map, as the clan named
    /// `me` meets it: the toll on a span is zero for its builder and for a pact partner whose
    /// pact shares causeways.
    pub fn overlay(
        &self,
        map: &mut crate::map::Map,
        cat: &crate::content::Catalog,
        me: Option<&str>,
    ) {
        map.apply_flags(self.flags, &cat.pois);
        let me_ix = me.and_then(|n| self.ix(n));
        for s in &self.causeways {
            let Some(owner) = self.clans.get(usize::from(s.owner)) else {
                continue;
            };
            let mine = me_ix == Some(s.owner);
            let shared = me_ix.is_some_and(|m| {
                self.pact_between(m, s.owner)
                    .is_some_and(|p| p.terms & crate::social::pact::CAUSEWAY != 0)
            });
            map.lay(crate::map::Laid {
                x: s.x,
                y: s.y,
                owner: owner.name.clone(),
                toll: if mine || shared { 0 } else { s.toll },
                mine,
            });
        }
    }

    /// Record a sale (`qty` > 0) or a purchase (`qty` < 0) for tonight's price update.
    pub fn trade(&mut self, what: Resource, qty: i32) {
        let slot = &mut self.net_sold[what.index()];
        *slot = slot.saturating_add(qty);
    }

    /// Register a hold (once per name), founded when the world was made.
    pub fn register(&mut self, name: &str, site: u8) {
        self.register_founded(name, site, self.created);
    }

    /// Register a hold (once per name) founded on `founded`.
    pub fn register_founded(&mut self, name: &str, site: u8, founded: i64) {
        if self.clans.len() < MAX_CLANS && !self.clans.iter().any(|c| c.name == name) {
            self.clans.push(ClanEntry {
                name: name.to_owned(),
                site,
                score: 0,
                posted: Posted {
                    seen: founded,
                    founded,
                    ..Posted::default()
                },
                record: Record::default(),
                badge: Badge::default(),
            });
        }
    }

    /// The registry index of a clan by name.
    #[must_use]
    pub fn ix(&self, name: &str) -> Option<ClanIx> {
        self.clans
            .iter()
            .position(|c| c.name == name)
            .and_then(|i| ClanIx::try_from(i).ok())
    }

    /// Count tallies traded on the exchange against a clan (for the hall's records).
    pub fn note_traded(&mut self, clan: ClanIx, value: u32) {
        if let Some(c) = self.clans.get_mut(usize::from(clan)) {
            c.record.traded = c.record.traded.saturating_add(value);
        }
    }

    /// Record a clan's score for the league table. `false` when nothing changed (so a session
    /// does not write the world for nothing) or the clan is not registered.
    pub fn set_score(&mut self, name: &str, score: u32) -> bool {
        match self.clans.iter_mut().find(|c| c.name == name) {
            Some(c) if c.score != score => {
                c.score = score;
                true
            }
            _ => false,
        }
    }

    /// The league table: the holds from the highest score to the lowest, ties by name.
    #[must_use]
    pub fn standings(&self) -> Vec<&ClanEntry> {
        let mut out: Vec<&ClanEntry> = self.clans.iter().collect();
        out.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.name.cmp(&b.name)));
        out
    }

    /// The last nights' prices of `what`, oldest first, ending with today's.
    #[must_use]
    pub fn price_trail(&self, what: Resource) -> Vec<u16> {
        let mut out: Vec<u16> = self.history.iter().map(|row| row[what.index()]).collect();
        // The history already ends with today's price once a night has passed.
        if out.last() != Some(&self.prices[what.index()]) {
            out.push(self.prices[what.index()]);
        }
        out
    }

    /// One night at the market: prices drift back toward their base, are pushed down by what
    /// was sold and up by what was bought, and move a little at random.
    pub fn end_day(&mut self, day: i64, rng: &mut DoorRng) {
        for r in Resource::ALL {
            let i = r.index();
            let base = i32::from(r.base_price()) * 100;
            let now = i32::from(self.prices[i]);
            let pull = (base - now) / 6;
            let pressure = (self.net_sold[i] / 5).clamp(-8, 8);
            let push = -base * pressure / 100;
            let noise = base * (i32::try_from(rng.below(5)).unwrap_or(2) - 2) / 100;
            let next = (now + pull + push + noise).clamp(base / 2, base * 2);
            self.prices[i] = u16::try_from(next).unwrap_or(u16::MAX);
            self.net_sold[i] = 0;
        }
        self.last_tick = day;
        self.history.push(self.prices);
        let extra = self.history.len().saturating_sub(HISTORY_DAYS);
        self.history.drain(..extra);
    }

    /// What `qty` of `what` fetches from the Tidecourt: 90% of the price, rounded down.
    #[must_use]
    pub fn sell_value(prices: &Prices, what: Resource, qty: u32) -> u32 {
        u32::try_from(u64::from(prices[what.index()]) * u64::from(qty) * 90 / 10_000)
            .unwrap_or(u32::MAX)
    }

    /// What `qty` of `what` costs at the Tidecourt: 115% of the price, rounded up.
    #[must_use]
    pub fn buy_cost(prices: &Prices, what: Resource, qty: u32) -> u32 {
        u32::try_from((u64::from(prices[what.index()]) * u64::from(qty) * 115).div_ceil(10_000))
            .unwrap_or(u32::MAX)
    }

    /// Encode as the current version.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = self.encode_v3();
        let mut w = Writer::new();
        w.count(self.causeways.len());
        for c in &self.causeways {
            w.u8(c.x);
            w.u8(c.y);
            w.u8(c.owner);
            w.u8(c.toll);
            w.i64(c.laid);
        }
        w.u8(self.flags);
        w.u8(self.legacy.code());
        for c in &self.clans {
            w.bool(c.badge.deep);
            w.u8(c.badge.quests);
            w.u8(c.badge.contracts);
        }
        for b in &self.social.book {
            w.u8(b.code);
        }
        bytes.extend(w.finish());
        bytes
    }

    /// The version 3 layout (everything but the version 4 tail).
    fn encode_v3(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u32(self.season);
        w.u64(self.seed);
        w.i64(self.created);
        w.i64(self.last_tick);
        for p in self.prices {
            w.u16(p);
        }
        for n in self.net_sold {
            w.u32(u32::from_ne_bytes(n.to_ne_bytes()));
        }
        w.count(self.clans.len());
        for c in &self.clans {
            w.str(&c.name);
            w.u8(c.site);
            w.u32(c.score);
            c.posted.write(&mut w);
            c.record.write(&mut w);
        }
        w.count(self.history.len());
        for row in &self.history {
            for p in row {
                w.u16(*p);
            }
        }
        self.social.write(&mut w);
        w.finish()
    }

    /// Version 3 bytes brought to version 4: nothing laid, no changes, a quiet legacy.
    #[must_use]
    pub fn upgrade_v3(bytes: &[u8]) -> Option<Vec<u8>> {
        Self::decode_layout(bytes, 3).map(|w| w.encode())
    }

    /// Version 2 bytes brought to version 3: nothing is posted yet and the social state is
    /// empty. (A clan posts itself the next time it saves.)
    #[must_use]
    pub fn upgrade_v2(bytes: &[u8]) -> Option<Vec<u8>> {
        Self::decode_v2(bytes).map(|w| w.encode_v3())
    }

    /// Version 1 bytes brought to version 2: no scores yet and no history.
    #[must_use]
    pub fn upgrade_v1(bytes: &[u8]) -> Option<Vec<u8>> {
        let mut r = Reader::new(bytes);
        let mut w = Writer::new();
        w.u32(r.u32()?);
        w.u64(r.u64()?);
        w.i64(r.i64()?);
        w.i64(r.i64()?);
        for _ in 0..RESOURCES {
            w.u16(r.u16()?);
        }
        for _ in 0..RESOURCES {
            w.u32(r.u32()?);
        }
        let clans = r.u8()?;
        w.u8(clans);
        for _ in 0..clans {
            w.str(&r.str()?);
            w.u8(r.u8()?);
            w.u32(0);
        }
        w.u8(0);
        r.done().then(|| w.finish())
    }

    /// Decode, or `None` for bytes that are not a world (an empty blob is "no world yet").
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        Self::decode_layout(bytes, 4)
    }

    /// Decode the version 2 layout (for the migration).
    fn decode_v2(bytes: &[u8]) -> Option<Self> {
        Self::decode_layout(bytes, 2)
    }

    fn decode_layout(bytes: &[u8], version: u8) -> Option<Self> {
        let mut r = Reader::new(bytes);
        let season = r.u32()?;
        let seed = r.u64()?;
        let created = r.i64()?;
        let last_tick = r.i64()?;
        let mut prices = [0u16; RESOURCES];
        for p in &mut prices {
            *p = r.u16()?;
        }
        let mut net_sold = [0i32; RESOURCES];
        for n in &mut net_sold {
            *n = i32::from_ne_bytes(r.u32()?.to_ne_bytes());
        }
        let clans = (0..r.u8()?)
            .map(|_| {
                let name = r.str()?;
                let site = r.u8()?;
                let score = r.u32()?;
                let (posted, record) = if version >= 3 {
                    (Posted::read(&mut r)?, Record::read(&mut r)?)
                } else {
                    (
                        Posted {
                            seen: last_tick,
                            founded: created,
                            ..Posted::default()
                        },
                        Record::default(),
                    )
                };
                Some(ClanEntry {
                    name,
                    site,
                    score,
                    posted,
                    record,
                    badge: Badge::default(),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let history = (0..r.u8()?)
            .map(|_| {
                let mut row = [0u16; RESOURCES];
                for p in &mut row {
                    *p = r.u16()?;
                }
                Some(row)
            })
            .collect::<Option<Vec<_>>>()?;
        let social = if version >= 3 {
            Social::read(&mut r)?
        } else {
            Social::default()
        };
        let mut w = Self {
            season,
            seed,
            created,
            last_tick,
            prices,
            net_sold,
            clans,
            history,
            social,
            causeways: Vec::new(),
            flags: 0,
            legacy: Ending::Quiet,
        };
        if version >= 4 {
            w.causeways = (0..r.u8()?)
                .map(|_| {
                    Some(Span {
                        x: r.u8()?,
                        y: r.u8()?,
                        owner: r.u8()?,
                        toll: r.u8()?,
                        laid: r.i64()?,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            w.flags = r.u8()?;
            w.legacy = Ending::from_code(r.u8()?);
            for c in &mut w.clans {
                c.badge = Badge {
                    deep: r.bool()?,
                    quests: r.u8()?,
                    contracts: r.u8()?,
                };
            }
            for b in &mut w.social.book {
                b.code = r.u8()?;
            }
        }
        r.done().then_some(w)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::assert_is_empty)]
    use super::*;

    #[test]
    fn a_world_round_trips() {
        let mut w = World::new(3, 99, 20_000);
        w.register("Brindle", 4);
        w.register("Brindle", 5);
        w.trade(Resource::Salt, -7);
        w.set_score("Brindle", 41);
        w.end_day(20_001, &mut DoorRng::from_seed(3));
        assert_eq!(w.clans.len(), 1);
        assert_eq!(w.history.len(), 1);
        assert_eq!(World::decode(&w.encode()), Some(w.clone()));
        assert_eq!(World::decode(&[]), None);
        assert_eq!(World::decode(&w.encode()[..10]), None);
    }

    #[test]
    fn selling_pushes_the_price_down_buying_up_and_it_always_drifts_home() {
        let mut w = World::new(1, 1, 0);
        w.trade(Resource::Salt, 400);
        w.trade(Resource::Iron, -400);
        let mut rng = DoorRng::from_seed(1);
        w.end_day(1, &mut rng);
        assert!(w.prices[Resource::Salt.index()] < 400);
        assert!(w.prices[Resource::Iron.index()] > 1000);
        assert_eq!(w.net_sold, [0; RESOURCES]);
        for day in 2..60 {
            w.end_day(day, &mut rng);
        }
        for r in Resource::ALL {
            let base = i32::from(r.base_price()) * 100;
            let now = i32::from(w.prices[r.index()]);
            assert!((base / 2..=base * 2).contains(&now), "{r:?} {now}");
            assert!(
                (now - base).abs() <= base / 4 + 5,
                "{r:?} drifted home: {now} vs {base}"
            );
        }
    }

    #[test]
    fn the_market_takes_a_cut_both_ways() {
        let w = World::new(1, 1, 0);
        let sell = World::sell_value(&w.prices, Resource::Iron, 10);
        let buy = World::buy_cost(&w.prices, Resource::Iron, 10);
        assert_eq!((sell, buy), (90, 115));
        assert_eq!(World::sell_value(&w.prices, Resource::Salt, 1), 3);
        assert_eq!(World::buy_cost(&w.prices, Resource::Salt, 1), 5);
    }

    #[test]
    fn a_version_1_world_upgrades_with_no_history_and_scores_of_zero() {
        // The version 1 bytes, written by hand: the old layout had no score and no history.
        let mut w = Writer::new();
        w.u32(2);
        w.u64(77);
        w.i64(20_000);
        w.i64(20_003);
        for r in Resource::ALL {
            w.u16(r.base_price() * 100);
        }
        for _ in 0..RESOURCES {
            w.u32(0);
        }
        w.count(1);
        w.str("Brindle");
        w.u8(4);
        let v1 = w.finish();
        assert_eq!(World::decode(&v1), None, "version 1 is not the current one");
        let v2 = World::upgrade_v1(&v1).unwrap();
        assert_eq!(World::decode(&v2), None, "version 2 is not the current one");
        let up =
            World::decode(&World::upgrade_v3(&World::upgrade_v2(&v2).unwrap()).unwrap()).unwrap();
        assert_eq!((up.season, up.seed, up.last_tick), (2, 77, 20_003));
        assert_eq!(up.clans[0].name, "Brindle");
        assert_eq!(up.clans[0].score, 0);
        assert_eq!(up.history, [] as [[u16; 13]; 0]);
        assert_eq!(World::upgrade_v1(&v1[..9]), None);
    }

    #[test]
    fn a_version_2_world_upgrades_keeping_scores_and_history_and_starting_the_social_state_empty() {
        // The version 2 bytes, written by hand: a score and a price history, nothing social.
        let mut w = Writer::new();
        w.u32(3);
        w.u64(99);
        w.i64(20_000);
        w.i64(20_005);
        for r in Resource::ALL {
            w.u16(r.base_price() * 100);
        }
        for _ in 0..RESOURCES {
            w.u32(0);
        }
        w.count(2);
        for (name, site, score) in [("Brindle", 4u8, 41u32), ("Cove", 7, 12)] {
            w.str(name);
            w.u8(site);
            w.u32(score);
        }
        w.count(2);
        for day in 0..2u16 {
            for r in Resource::ALL {
                w.u16(r.base_price() * 100 + day);
            }
        }
        let v2 = w.finish();
        assert_eq!(World::decode(&v2), None, "version 2 is not the current one");
        let up =
            World::decode(&World::upgrade_v3(&World::upgrade_v2(&v2).unwrap()).unwrap()).unwrap();
        assert_eq!((up.season, up.seed, up.last_tick), (3, 99, 20_005));
        assert_eq!(up.clans.len(), 2);
        assert_eq!(
            (up.clans[0].name.as_str(), up.clans[0].score),
            ("Brindle", 41)
        );
        assert_eq!(up.clans[1].site, 7);
        assert_eq!(up.history.len(), 2);
        assert_eq!(up.history[1][0], 400 + 1);
        // Nothing is posted and the social state is empty; a clan posts itself on its next save.
        assert_eq!(up.clans[0].record, Record::default());
        assert!(up.clans[0].posted.guard.is_empty());
        assert_eq!(up.social, Social::default());
        // The upgraded world re-encodes to itself.
        assert_eq!(World::decode(&up.encode()), Some(up));
        assert_eq!(World::upgrade_v2(&v2[..20]), None);
    }

    #[test]
    fn the_league_is_ordered_by_score_and_the_history_keeps_two_weeks() {
        let mut w = World::new(1, 1, 0);
        for (n, s) in [("Ash", 5), ("Brindle", 40), ("Cove", 40), ("Dun", 0)] {
            w.register(n, 0);
            w.set_score(n, s);
        }
        assert!(!w.set_score("Dun", 0), "no change is reported as no change");
        assert!(!w.set_score("Nobody", 3));
        let names: Vec<&str> = w.standings().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Brindle", "Cove", "Ash", "Dun"]);
        let mut rng = DoorRng::from_seed(9);
        for day in 1..=30 {
            w.end_day(day, &mut rng);
        }
        assert_eq!(w.history.len(), HISTORY_DAYS);
        let trail = w.price_trail(Resource::Salt);
        assert_eq!(trail.len(), HISTORY_DAYS);
        assert_eq!(
            trail.last().copied(),
            Some(w.prices[Resource::Salt.index()])
        );
    }
}
