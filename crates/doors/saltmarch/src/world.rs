//! The shared world: the season's seed, the Tidecourt's prices and the registry of holds.
//! It is changed by sessions (through the framework's compare-and-swap edit) and by the daily
//! tick, and it never holds anything private to one player.
//!
//! Layout (version 2), little-endian: season `u32`, seed `u64`, created `i64`, last tick `i64`,
//! prices (13 x `u16`), net sold (13 x `i32` as `u32`), clan count `u8`, then name, site and
//! score (`u32`) per clan, then the price history: a count `u8` and that many rows of 13 x
//! `u16`, oldest first. Version 1 had no score and no history (see [`World::upgrade_v1`]).

use russh_bbs_door::DoorRng;

use crate::codec::{Reader, Writer};
use crate::types::{RESOURCES, Resource};

/// The most holds the registry keeps.
pub const MAX_CLANS: usize = 200;

/// Prices are kept in hundredths of a tally.
pub type Prices = [u16; RESOURCES];

/// How many nights of prices the market remembers (two weeks, one tide cycle).
pub const HISTORY_DAYS: usize = 14;

/// A hold in the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClanEntry {
    /// The clan's name.
    pub name: String,
    /// Index into the map's hold sites.
    pub site: u8,
    /// The clan's score the last time it was saved (`Clan::score`): the league table's order.
    pub score: u32,
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
        }
    }

    /// Record a sale (`qty` > 0) or a purchase (`qty` < 0) for tonight's price update.
    pub fn trade(&mut self, what: Resource, qty: i32) {
        let slot = &mut self.net_sold[what.index()];
        *slot = slot.saturating_add(qty);
    }

    /// Register a hold (once per name).
    pub fn register(&mut self, name: &str, site: u8) {
        if self.clans.len() < MAX_CLANS && !self.clans.iter().any(|c| c.name == name) {
            self.clans.push(ClanEntry {
                name: name.to_owned(),
                site,
                score: 0,
            });
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

    /// Encode.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
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
        }
        w.count(self.history.len());
        for row in &self.history {
            for p in row {
                w.u16(*p);
            }
        }
        w.finish()
    }

    /// Version 1 bytes brought to version 2: no scores yet and no history.
    #[must_use]
    pub fn upgrade_v1(bytes: &[u8]) -> Option<Vec<u8>> {
        let mut r = Reader::new(bytes);
        let (season, seed, created, last_tick) = (r.u32()?, r.u64()?, r.i64()?, r.i64()?);
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
                Some(ClanEntry {
                    name: r.str()?,
                    site: r.u8()?,
                    score: 0,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        r.done().then(|| {
            Self {
                season,
                seed,
                created,
                last_tick,
                prices,
                net_sold,
                clans,
                history: Vec::new(),
            }
            .encode()
        })
    }

    /// Decode, or `None` for bytes that are not a world (an empty blob is "no world yet").
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
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
                Some(ClanEntry {
                    name: r.str()?,
                    site: r.u8()?,
                    score: r.u32()?,
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
        r.done().then_some(Self {
            season,
            seed,
            created,
            last_tick,
            prices,
            net_sold,
            clans,
            history,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
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
        assert_eq!(World::decode(&v1), None, "version 1 is not version 2");
        let up = World::decode(&World::upgrade_v1(&v1).unwrap()).unwrap();
        assert_eq!((up.season, up.seed, up.last_tick), (2, 77, 20_003));
        assert_eq!(up.clans[0].name, "Brindle");
        assert_eq!(up.clans[0].score, 0);
        assert_eq!(up.history, [] as [[u16; 13]; 0]);
        assert_eq!(World::upgrade_v1(&v1[..9]), None);
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
