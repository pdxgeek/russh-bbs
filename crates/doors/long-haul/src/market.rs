//! Prices: the one function that turns a station's stock into a price, and the fills that move
//! stock and credits together.
//!
//! The price of a ton depends only on how much of it the station holds. A station keeps
//! `depth` tons of a good when balanced (scaled by its size); fewer tons than that and the
//! price climbs, more and it falls, always inside [`MULT_MIN`] and [`MULT_MAX`] per-mille of
//! the good's base price. Buying a ton takes it from stock; selling puts it back.
//!
//! **Why nobody can print money here.** The price of the *k*-th ton is a function of *k*
//! alone ([`step_mid`]), the same whether that ton is being bought (stock `k` falls to
//! `k - 1`) or sold (stock `k - 1` rises to `k`). A trader pays `ceil(mid * (1 + spread))` and is paid
//! `floor(mid * (1 - spread))` for the same step, so any sequence of buys and sells at one
//! station, however it is ordered, ends with the player poorer by at least the spread on every
//! ton moved. The tests check that on random sequences. Profit comes only from carrying a good
//! to a station whose stock differs, which is the game.

use crate::frontier::{Frontier, Role};

/// The lowest price multiplier, per-mille of base.
pub const MULT_MIN: i64 = 400;
/// The highest price multiplier, per-mille of base.
pub const MULT_MAX: i64 = 3000;
/// A station refuses goods once its stock passes this many times its depth.
pub const FULL_STOCK_DEPTHS: i64 = 3;
/// The widest spread either side of the middle price, per-mille.
pub const SPREAD_MAX: i64 = 90;
/// The narrowest spread, per-mille: even the best trader pays something.
pub const SPREAD_MIN: i64 = 20;

/// Depth of a station's market in a good, in tons: the good's depth times station size.
#[must_use]
pub fn market_depth(good_depth: u32, size: u8) -> i64 {
    i64::from(good_depth) * i64::from(size.max(1))
}

/// The multiplier (per-mille of base) for the ton at stock level `k`, given the market `depth`.
#[must_use]
pub fn mult_pm(depth: i64, k: i64) -> i64 {
    (1000 * depth.max(1) / k.max(1)).clamp(MULT_MIN, MULT_MAX)
}

/// The middle price of the ton at stock level `k`.
#[must_use]
pub fn step_mid(base: u32, depth: i64, k: i64) -> i64 {
    (i64::from(base) * mult_pm(depth, k) / 1000).max(1)
}

/// The spread per side, per-mille, for a trader of `trade_level` (0 to 10) and a given standing
/// with the station's faction: skill narrows it, a bad name widens it.
#[must_use]
pub fn spread_pm(trade_level: u8, standing: i16) -> i64 {
    let skill = i64::from(trade_level.min(10)) * 3;
    let name = if standing >= 25 {
        8
    } else if standing <= -25 {
        -15
    } else {
        0
    };
    (60 - skill - name).clamp(SPREAD_MIN, SPREAD_MAX)
}

/// What a fill came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fill {
    /// Tons actually moved.
    pub units: u32,
    /// Credits paid or received for all of them.
    pub total: i64,
}

/// Buy up to `qty` tons from a market holding `stock`: stops when the stock runs out.
#[must_use]
pub fn fill_buy(base: u32, depth: i64, stock: i64, qty: u32, spread: i64) -> Fill {
    let mut fill = Fill::default();
    let mut at = stock;
    while fill.units < qty && at >= 1 {
        let mid = step_mid(base, depth, at);
        fill.total += (mid * (1000 + spread) + 999) / 1000;
        fill.units += 1;
        at -= 1;
    }
    fill
}

/// Buy up to `qty` tons, stopping at the stock or when the next ton would cost more than
/// `budget`.
#[must_use]
pub fn fill_buy_within(
    base: u32,
    depth: i64,
    stock: i64,
    qty: u32,
    spread: i64,
    budget: i64,
) -> Fill {
    let mut fill = Fill::default();
    let mut at = stock;
    while fill.units < qty && at >= 1 {
        let price = (step_mid(base, depth, at) * (1000 + spread) + 999) / 1000;
        if fill.total + price > budget {
            break;
        }
        fill.total += price;
        fill.units += 1;
        at -= 1;
    }
    fill
}

/// Sell up to `qty` tons to a market holding `stock`: stops when the warehouse is full.
#[must_use]
pub fn fill_sell(base: u32, depth: i64, stock: i64, qty: u32, spread: i64) -> Fill {
    let mut fill = Fill::default();
    let mut at = stock;
    let cap = depth * FULL_STOCK_DEPTHS;
    while fill.units < qty && at < cap {
        at += 1;
        let mid = step_mid(base, depth, at);
        fill.total += mid * (1000 - spread) / 1000;
        fill.units += 1;
    }
    fill
}

/// The stock a station aims for in a slot before news: depth times the role's percentage.
#[must_use]
pub fn base_target(depth: i64, role: Role) -> i64 {
    depth * role.target_pct() / 100
}

/// A fuel unit's price at a station of the given economy fuel scale.
#[must_use]
pub fn fuel_price(fuel_pm: u32) -> i64 {
    (8 * i64::from(fuel_pm) / 1000).max(1)
}

/// How far (in whole percent of base) a price sits from base, for the heat overlay and the
/// route planner: positive is dear.
#[must_use]
pub fn heat_pct(mid: i64, base: u32) -> i64 {
    (mid - i64::from(base)) * 100 / i64::from(base).max(1)
}

/// The depth of `good` at `station`, as the frontier and content describe it.
#[must_use]
pub fn depth_at(
    content_goods: &[crate::content::Good],
    frontier: &Frontier,
    station: u16,
    slot: usize,
) -> i64 {
    frontier
        .station(station)
        .and_then(|st| st.goods.get(slot).map(|s| (st.size, s.good)))
        .and_then(|(size, good)| {
            content_goods
                .get(usize::from(good))
                .map(|g| market_depth(g.depth, size))
        })
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn the_multiplier_is_bounded_and_falls_as_stock_rises() {
        let depth = 100;
        let mut last = i64::MAX;
        for k in 0..2_000 {
            let m = mult_pm(depth, k);
            assert!((MULT_MIN..=MULT_MAX).contains(&m));
            assert!(m <= last, "price must not rise with stock");
            last = m;
        }
        assert_eq!(mult_pm(depth, 100), 1000);
        assert_eq!(mult_pm(depth, 0), MULT_MAX);
        assert_eq!(mult_pm(depth, 10_000), MULT_MIN);
    }

    #[test]
    fn skill_narrows_the_spread_and_a_bad_name_widens_it() {
        assert!(spread_pm(10, 0) < spread_pm(0, 0));
        assert!(spread_pm(0, 30) < spread_pm(0, 0));
        assert!(spread_pm(0, -30) > spread_pm(0, 0));
        assert_eq!(spread_pm(10, 100), SPREAD_MIN.max(60 - 30 - 8));
        assert!(spread_pm(0, -100) <= SPREAD_MAX);
    }

    #[test]
    fn buying_stops_at_the_stock_and_selling_at_a_full_warehouse() {
        let buy = fill_buy(100, 50, 7, 20, 40);
        assert_eq!(buy.units, 7);
        let sell = fill_sell(100, 50, 148, 20, 40);
        assert_eq!(
            sell.units, 2,
            "stock may rise to three depths and no further"
        );
        assert_eq!(fill_sell(100, 50, 150, 5, 40), Fill::default());
        assert_eq!(fill_buy(100, 50, 0, 5, 40), Fill::default());
    }

    proptest! {
        /// A round trip at one station always loses money, and no sequence of buys and sells
        /// ever ends ahead of where it started.
        #[test]
        fn no_sequence_of_trades_at_one_station_makes_money(
            base in 5u32..2_000,
            depth in 1i64..2_000,
            stock in 0i64..6_000,
            spread in SPREAD_MIN..=SPREAD_MAX,
            ops in proptest::collection::vec((any::<bool>(), 1u32..60), 1..30),
        ) {
            let mut stock_now = stock;
            let mut held: i64 = 0;
            let mut cash: i64 = 0;
            for (buy, qty) in ops {
                if buy {
                    let fill = fill_buy(base, depth, stock_now, qty, spread);
                    stock_now -= i64::from(fill.units);
                    held += i64::from(fill.units);
                    cash -= fill.total;
                } else {
                    let qty = qty.min(u32::try_from(held).unwrap_or(0));
                    let fill = fill_sell(base, depth, stock_now, qty, spread);
                    stock_now += i64::from(fill.units);
                    held -= i64::from(fill.units);
                    cash += fill.total;
                }
                prop_assert!(stock_now >= 0);
            }
            // Sell what is left back at the station, as far as it will take it.
            let rest = fill_sell(base, depth, stock_now, u32::try_from(held).unwrap_or(0), spread);
            cash += rest.total;
            prop_assert!(cash <= 0, "ended {cash} credits ahead");
        }

        #[test]
        fn every_unit_price_is_inside_the_band(base in 1u32..5_000, depth in 1i64..3_000, k in 0i64..20_000) {
            let mid = step_mid(base, depth, k);
            let lo = (i64::from(base) * MULT_MIN / 1000).max(1);
            let hi = i64::from(base) * MULT_MAX / 1000;
            prop_assert!(mid >= lo && mid <= hi.max(1));
        }
    }
}
