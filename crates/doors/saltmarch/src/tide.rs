//! The tide: the clock and the map at once.
//!
//! The water follows one continuous curve of highs and lows from the board's seed, so a table
//! for any day, past or future, is a pure function of `(seed, day)` and never needs storing.
//! Highs and lows are `HALF_CYCLE` minutes apart (a tidal day is 24 h 48 m), so the times
//! shift by 48 minutes a day. Their height follows a 14-day spring/neap cycle, and a storm
//! (rare, from the seed and the day) piles the water higher on that day's highs.
//!
//! Levels share one scale with tile elevations (0 to 100, plus surge above that).

use std::fmt;

use russh_bbs_door::DoorRng;

/// Minutes in a day.
pub const MINUTES_PER_DAY: i64 = 1440;

/// Minutes from a high to the next low (and a low to the next high).
pub const HALF_CYCLE: i64 = 372;

/// Mean water level.
pub const MID: i32 = 50;

/// Half the range of a spring tide.
pub const SPRING_AMP: i32 = 45;

/// Half the range of a neap tide.
pub const NEAP_AMP: i32 = 20;

/// Extra height a storm adds to the day's highs.
pub const STORM_SURGE: i32 = 15;

/// Days from one spring tide to the next.
pub const SPRING_CYCLE_DAYS: i64 = 14;

/// How deep the water over a tile may be and still be wadeable ("wet": slow, costs extra).
pub const WET_BAND: i32 = 12;

/// One storm in this many days, on average.
pub const STORM_ONE_IN: u32 = 18;

/// Eased interpolation weights `(1 - cos(pi * i / 16)) / 2` in thousandths, for `i` in 0..=16.
const EASE: [i32; 17] = [
    0, 10, 38, 84, 146, 222, 309, 402, 500, 598, 691, 778, 854, 916, 962, 990, 1000,
];

/// How far the tide is from the extreme of a 14-day cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Big tides: drowned towns stand dry at the lows, floods are deadlier.
    Spring,
    /// In between.
    Middling,
    /// Small tides: safe and dull.
    Neap,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Spring => "spring",
            Self::Middling => "middling",
            Self::Neap => "neap",
        })
    }
}

/// How a tile stands against the water.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Above the water.
    Dry,
    /// Knee-deep: passable, slow.
    Wet,
    /// Under: passable by boat only.
    Drowned,
}

impl Access {
    /// How a tile of `elevation` stands when the water is at `level`.
    #[must_use]
    pub const fn of(elevation: i32, level: i32) -> Self {
        if elevation >= level {
            Self::Dry
        } else if level - elevation <= WET_BAND {
            Self::Wet
        } else {
            Self::Drowned
        }
    }
}

/// A warning band for the strip across the top of a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Warning {
    /// Nothing to worry about.
    Calm,
    /// The water is high.
    High,
    /// The water is over the work sites.
    Flood,
    /// A storm surge.
    Storm,
}

/// A high or a low.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extreme {
    /// Minute of the day, 0 to 1439.
    pub minute: u16,
    /// The water level at the turn.
    pub level: i32,
    /// High water (or low).
    pub high: bool,
}

/// One day's table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayTable {
    /// The day number (days since the Unix epoch).
    pub day: i64,
    /// Spring, middling or neap.
    pub phase: Phase,
    /// Whether a storm surge rides the day's highs.
    pub storm: bool,
    /// The highs and lows that fall in this day, in time order (three or four).
    pub extremes: Vec<Extreme>,
}

/// The tide of one board and season.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tide {
    seed: u64,
    offset: i64,
}

impl Tide {
    /// The tide for a board seed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut rng = DoorRng::from_label("saltmarch-tide", i64::from_ne_bytes(seed.to_ne_bytes()));
        let offset = i64::from(rng.below(u32::try_from(2 * HALF_CYCLE).unwrap_or(744)));
        Self { seed, offset }
    }

    /// The half-range of the tides on `day`: a triangle wave between neap and spring.
    #[must_use]
    pub fn amplitude(day: i64) -> i32 {
        let d = day.rem_euclid(SPRING_CYCLE_DAYS);
        let from_neap = i32::try_from((d - SPRING_CYCLE_DAYS / 2).abs()).unwrap_or(0);
        let half = i32::try_from(SPRING_CYCLE_DAYS / 2).unwrap_or(7);
        NEAP_AMP + (SPRING_AMP - NEAP_AMP) * from_neap / half
    }

    /// The phase on `day`.
    #[must_use]
    pub fn phase(day: i64) -> Phase {
        match Self::amplitude(day) {
            a if a >= 40 => Phase::Spring,
            a if a <= 26 => Phase::Neap,
            _ => Phase::Middling,
        }
    }

    /// Whether a storm surge rides `day`'s highs.
    #[must_use]
    pub fn storm(&self, day: i64) -> bool {
        DoorRng::from_label(&format!("saltmarch-storm-{}", self.seed), day).chance(1, STORM_ONE_IN)
    }

    fn time_of(&self, k: i64) -> i64 {
        self.offset + k * HALF_CYCLE
    }

    fn level_of(&self, k: i64) -> i32 {
        let day = self.time_of(k).div_euclid(MINUTES_PER_DAY);
        let amp = Self::amplitude(day);
        if k.rem_euclid(2) == 0 {
            let surge = if self.storm(day) { STORM_SURGE } else { 0 };
            MID + amp + surge
        } else {
            MID - amp
        }
    }

    /// The water level at `minute` (minutes since the epoch: `day * 1440 + minute of day`).
    #[must_use]
    pub fn level(&self, minute: i64) -> i32 {
        let k = (minute - self.offset).div_euclid(HALF_CYCLE);
        let from = self.level_of(k);
        let to = self.level_of(k + 1);
        let into = (minute - self.time_of(k)) * 16;
        let step = usize::try_from(into / HALF_CYCLE).unwrap_or(0).min(15);
        let rest = i32::try_from(into % HALF_CYCLE).unwrap_or(0);
        let span = i32::try_from(HALF_CYCLE).unwrap_or(372);
        let (lo, hi) = (EASE[step], EASE[step + 1]);
        let weight = lo + (hi - lo) * rest / span;
        from + (to - from) * weight / 1000
    }

    /// The minute-of-epoch of `turn` out of `turns_per_day` on `day`: the tide-turn is the
    /// unit of the clan's day, and each one spent moves the water on.
    #[must_use]
    pub fn turn_minute(day: i64, turn: u8, turns_per_day: u8) -> i64 {
        let per = i64::from(turns_per_day.max(1));
        day * MINUTES_PER_DAY + i64::from(turn) * MINUTES_PER_DAY / per
    }

    /// The table for `day`.
    #[must_use]
    pub fn table(&self, day: i64) -> DayTable {
        let start = day * MINUTES_PER_DAY;
        let end = start + MINUTES_PER_DAY;
        let first = (start - self.offset).div_euclid(HALF_CYCLE);
        let extremes = (first..=first + 5)
            .filter(|k| (start..end).contains(&self.time_of(*k)))
            .map(|k| Extreme {
                minute: u16::try_from(self.time_of(k) - start).unwrap_or(0),
                level: self.level_of(k),
                high: k.rem_euclid(2) == 0,
            })
            .collect();
        DayTable {
            day,
            phase: Self::phase(day),
            storm: self.storm(day),
            extremes,
        }
    }

    /// Minutes until the next high or low after `minute`, and whether it is a high.
    #[must_use]
    pub fn next_turn(&self, minute: i64) -> (i64, bool) {
        let k = (minute - self.offset).div_euclid(HALF_CYCLE) + 1;
        (self.time_of(k) - minute, k.rem_euclid(2) == 0)
    }

    /// The warning band for `level` on `day`.
    #[must_use]
    pub fn warning(&self, level: i32, day: i64) -> Warning {
        if self.storm(day) && level >= HOLD_FLOOD_LEVEL + 10 {
            Warning::Storm
        } else if level >= HOLD_FLOOD_LEVEL {
            Warning::Flood
        } else if level >= MID + 10 {
            Warning::High
        } else {
            Warning::Calm
        }
    }
}

/// The level at which water reaches the work sites at the foot of the hold. Above this, shifts
/// are dangerous and the fearful will not go.
pub const HOLD_FLOOD_LEVEL: i32 = 70;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn amplitude_swings_between_neap_and_spring_over_fourteen_days() {
        let amps: Vec<i32> = (0..14).map(Tide::amplitude).collect();
        assert_eq!(amps[0], SPRING_AMP);
        assert_eq!(amps[7], NEAP_AMP);
        assert_eq!(Tide::amplitude(14), SPRING_AMP);
        assert_eq!(Tide::amplitude(-7), NEAP_AMP);
        assert_eq!(Tide::phase(0), Phase::Spring);
        assert_eq!(Tide::phase(7), Phase::Neap);
        assert_eq!(Tide::phase(3), Phase::Middling);
    }

    #[test]
    fn a_day_has_three_or_four_turns_that_alternate_and_shift_each_day() {
        let tide = Tide::new(5);
        let mut firsts = Vec::new();
        for day in 20_000..20_030 {
            let t = tide.table(day);
            assert!((3..=4).contains(&t.extremes.len()), "{:?}", t.extremes);
            assert!(t.extremes.windows(2).all(|w| w[0].high != w[1].high));
            assert!(t.extremes.windows(2).all(|w| w[0].minute < w[1].minute));
            firsts.push(t.extremes[0].minute);
        }
        assert!(firsts.windows(2).any(|w| w[0] != w[1]), "times shift");
    }

    #[test]
    fn the_level_hits_the_listed_extremes_and_stays_between_them() {
        let tide = Tide::new(77);
        for day in 20_000..20_040 {
            let table = tide.table(day);
            for e in &table.extremes {
                let at = day * MINUTES_PER_DAY + i64::from(e.minute);
                assert_eq!(tide.level(at), e.level);
                let later = tide.level(at + 30);
                assert!(if e.high {
                    later <= e.level
                } else {
                    later >= e.level
                });
            }
        }
        for minute in 28_000_000..28_003_000 {
            let l = tide.level(minute);
            assert!(
                (MID - SPRING_AMP..=MID + SPRING_AMP + STORM_SURGE).contains(&l),
                "{l}"
            );
        }
    }

    #[test]
    fn spring_tides_are_bigger_than_neaps_and_storms_raise_the_highs() {
        let tide = Tide::new(1);
        let range = |day: i64| {
            let t = tide.table(day);
            let hi = t.extremes.iter().filter(|e| e.high).map(|e| e.level).max();
            let lo = t.extremes.iter().filter(|e| !e.high).map(|e| e.level).min();
            (hi.unwrap(), lo.unwrap())
        };
        // Day 14 * k is a spring day; 14 * k + 7 a neap day.
        let (spring_hi, spring_lo) = range(20_006);
        let (neap_hi, neap_lo) = range(20_006 + 7);
        assert!(spring_hi - spring_lo > neap_hi - neap_lo + 20);
        let stormy = (20_000..21_000).find(|d| tide.storm(*d)).unwrap();
        let calm = (20_000..21_000)
            .find(|d| !tide.storm(*d) && Tide::amplitude(*d) == Tide::amplitude(stormy))
            .unwrap();
        let top = |d: i64| {
            tide.table(d)
                .extremes
                .iter()
                .map(|e| e.level)
                .max()
                .unwrap()
        };
        assert_eq!(top(stormy), top(calm) + STORM_SURGE);
    }

    #[test]
    fn storms_are_rare_and_boards_differ() {
        let a = Tide::new(1);
        let b = Tide::new(2);
        let storms = (0..2_000).filter(|d| a.storm(*d)).count();
        assert!((60..200).contains(&storms), "{storms}");
        assert!((0..60).any(|d| a.table(d) != b.table(d)));
        assert_eq!(a.table(33), Tide::new(1).table(33));
    }

    #[test]
    fn access_has_three_bands_and_the_warning_follows_the_water() {
        assert_eq!(Access::of(40, 30), Access::Dry);
        assert_eq!(Access::of(40, 40), Access::Dry);
        assert_eq!(Access::of(40, 45), Access::Wet);
        assert_eq!(Access::of(40, 40 + WET_BAND), Access::Wet);
        assert_eq!(Access::of(40, 40 + WET_BAND + 1), Access::Drowned);
        let tide = Tide::new(3);
        let day = (0..1000).find(|d| !tide.storm(*d)).unwrap();
        assert_eq!(tide.warning(30, day), Warning::Calm);
        assert_eq!(tide.warning(65, day), Warning::High);
        assert_eq!(tide.warning(HOLD_FLOOD_LEVEL, day), Warning::Flood);
        let stormy = (0..1000).find(|d| tide.storm(*d)).unwrap();
        assert_eq!(tide.warning(90, stormy), Warning::Storm);
    }

    #[test]
    fn spending_turns_moves_the_water_on_and_the_next_turn_is_counted() {
        let tide = Tide::new(9);
        let day = 20_001;
        let m0 = Tide::turn_minute(day, 0, 30);
        let m1 = Tide::turn_minute(day, 15, 30);
        assert_eq!(m0, day * MINUTES_PER_DAY);
        assert_eq!(m1 - m0, 720);
        assert_eq!(Tide::turn_minute(day, 30, 30), (day + 1) * MINUTES_PER_DAY);
        let (wait, high) = tide.next_turn(m0);
        assert!((1..=HALF_CYCLE).contains(&wait));
        let at = tide.level(m0 + wait);
        assert_eq!(high, at >= MID);
    }
}
