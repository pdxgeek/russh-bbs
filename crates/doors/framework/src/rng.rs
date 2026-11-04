//! The door's seeded random numbers.
//!
//! A door never touches the operating system's randomness: it is given a [`DoorRng`] whose seed
//! the board chose, so a recorded run replays exactly (`replay`). The generator is
//! `xoshiro256++` seeded through `SplitMix64`, written out here rather than taken from a crate so
//! its output can never change under a door's saved games or recorded logs: the first outputs
//! for a seed are pinned by a test.
//!
//! **Not for secrets.** A seed the player could guess lets them predict the game. The board
//! seeds a live session from the operating system's randomness; only tests and replays use
//! fixed seeds. A door that must not be save-scummed keeps [`DoorRng::state`] in its save.

/// A small deterministic generator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoorRng {
    s: [u64; 4],
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl DoorRng {
    /// A generator for `seed`. Equal seeds give equal sequences.
    #[must_use]
    pub fn from_seed(seed: u64) -> Self {
        let mut sm = seed;
        Self {
            s: [
                splitmix(&mut sm),
                splitmix(&mut sm),
                splitmix(&mut sm),
                splitmix(&mut sm),
            ],
        }
    }

    /// A generator for a name and a number, mixed with a fixed hash (FNV-1a). Used for the daily
    /// tick, which must give the same numbers for the same door and day on any process.
    #[must_use]
    pub fn from_label(label: &str, number: i64) -> Self {
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
        for byte in label.bytes().chain(number.to_le_bytes()) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
        Self::from_seed(hash)
    }

    /// The internal state, to keep in a save so a reload continues the same sequence.
    #[must_use]
    pub const fn state(&self) -> [u64; 4] {
        self.s
    }

    /// A generator continuing from a saved [`DoorRng::state`]. An all-zero state (which the
    /// algorithm cannot leave) is replaced by seed 0's.
    #[must_use]
    pub fn from_state(state: [u64; 4]) -> Self {
        if state == [0; 4] {
            Self::from_seed(0)
        } else {
            Self { s: state }
        }
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[0]
            .wrapping_add(self.s[3])
            .rotate_left(23)
            .wrapping_add(self.s[0]);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// A number in `0..bound` with no modulo bias. `bound` of zero gives zero.
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        let bound = u64::from(bound);
        // Rejection sampling over the top of the 64-bit range.
        let zone = u64::MAX - (u64::MAX % bound);
        loop {
            let value = self.next_u64();
            if value < zone {
                return u32::try_from(value % bound).unwrap_or(0);
            }
        }
    }

    /// A number in `low..=high`. If `high < low` it is `low`.
    pub fn range(&mut self, low: u32, high: u32) -> u32 {
        if high <= low {
            return low;
        }
        // `high - low + 1` can overflow only for the full u32 range; use u64 for the span.
        let span = u64::from(high) - u64::from(low) + 1;
        let roll = if span > u64::from(u32::MAX) {
            self.next_u64() % span
        } else {
            u64::from(self.below(u32::try_from(span).unwrap_or(u32::MAX)))
        };
        low + u32::try_from(roll).unwrap_or(0)
    }

    /// `true` with probability `numerator / denominator` (never when the denominator is zero).
    pub fn chance(&mut self, numerator: u32, denominator: u32) -> bool {
        denominator != 0 && self.below(denominator) < numerator
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_numbers_and_the_first_outputs_never_change() {
        let mut a = DoorRng::from_seed(42);
        let mut b = DoorRng::from_seed(42);
        let first: Vec<u64> = (0..4).map(|_| a.next_u64()).collect();
        assert_eq!(first, (0..4).map(|_| b.next_u64()).collect::<Vec<_>>());
        // Pinned: a door's saved games and recorded logs depend on these exact values.
        assert_eq!(
            first,
            [
                15_021_278_609_987_233_951,
                5_881_210_131_331_364_753,
                18_149_643_915_985_481_100,
                12_933_668_939_759_105_464
            ]
        );
        assert_ne!(DoorRng::from_seed(43).next_u64(), first[0]);
    }

    #[test]
    fn labels_and_numbers_pick_distinct_but_repeatable_sequences() {
        let a = DoorRng::from_label("coin-toss", 20_000).next_u64();
        assert_eq!(a, DoorRng::from_label("coin-toss", 20_000).next_u64());
        assert_ne!(a, DoorRng::from_label("coin-toss", 20_001).next_u64());
        assert_ne!(a, DoorRng::from_label("other", 20_000).next_u64());
    }

    #[test]
    fn below_range_and_chance_stay_in_bounds_and_cover_the_range() {
        let mut rng = DoorRng::from_seed(7);
        let mut seen = [false; 6];
        for _ in 0..500 {
            let roll = rng.below(6);
            assert!(roll < 6);
            seen[roll as usize] = true;
            let r = rng.range(3, 5);
            assert!((3..=5).contains(&r));
        }
        assert!(seen.iter().all(|&s| s), "every face came up");
        assert_eq!(rng.below(0), 0);
        assert_eq!(rng.range(9, 2), 9);
        let _ = rng.range(0, u32::MAX);
        assert!(!rng.chance(1, 0));
        assert!(rng.chance(5, 5));
        let heads = (0..2000).filter(|_| rng.chance(1, 2)).count();
        assert!((800..1200).contains(&heads), "{heads}");
    }

    #[test]
    fn a_saved_state_continues_the_same_sequence() {
        let mut a = DoorRng::from_seed(1);
        a.next_u64();
        let mut b = DoorRng::from_state(a.state());
        assert_eq!(a.next_u64(), b.next_u64());
        assert_eq!(DoorRng::from_state([0; 4]), DoorRng::from_seed(0));
    }
}
