//! The shared world: the season's seed, the Tidecourt's prices and the registry of holds.
//! It is changed by sessions (through the framework's compare-and-swap edit) and by the daily
//! tick, and it never holds anything private to one player.
//!
//! Layout, little-endian: season `u32`, seed `u64`, created `i64`, last tick `i64`, prices
//! (13 x `u16`), net sold (13 x `i32` as `u32`), clan count `u8`, then name and site per clan.

use russh_bbs_door::DoorRng;

use crate::codec::{Reader, Writer};
use crate::types::{RESOURCES, Resource};

/// The most holds the registry keeps.
pub const MAX_CLANS: usize = 200;

/// Prices are kept in hundredths of a tally.
pub type Prices = [u16; RESOURCES];

/// A hold in the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClanEntry {
    /// The clan's name.
    pub name: String,
    /// Index into the map's hold sites.
    pub site: u8,
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
            });
        }
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
        }
        w.finish()
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
                })
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
        assert_eq!(w.clans.len(), 1);
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
}
