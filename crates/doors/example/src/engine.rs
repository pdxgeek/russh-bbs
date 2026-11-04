//! The coin toss rules: state plus an action gives a new state plus events. No I/O, no clock, no
//! randomness except the [`DoorRng`] passed in, so a recorded run replays exactly.

use russh_bbs_door::DoorRng;

/// Tosses allowed per day.
pub const DAILY_TOSSES: u8 = 5;

/// Coins a new player starts with.
pub const START_COINS: u32 = 5;

/// A balance the daily relief tops up to, so nobody is stuck with an empty purse.
pub const RELIEF_FLOOR: u32 = 3;

/// The most coins a purse holds.
pub const MAX_COINS: u32 = 1_000_000;

/// The longest nickname, in characters.
pub const NAME_MAX: usize = 24;

/// A call on the coin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// Heads.
    Heads,
    /// Tails.
    Tails,
}

/// What a toss did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The coin landed on the called side: one coin won.
    Won(Call),
    /// It landed on the other side: one coin lost.
    Lost(Call),
    /// No tosses left today.
    NoTossesLeft,
    /// An empty purse (cannot happen after the daily relief, kept for a corrupt save).
    NoCoins,
}

/// One player's game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// Coins in the purse.
    pub coins: u32,
    /// Tosses left today.
    pub left: u8,
    /// The UTC day number `left` is for.
    pub day: i64,
    /// Wins in a row.
    pub streak: u16,
    /// The best streak ever.
    pub best: u16,
    /// The nickname shown on the screen (already passed through the name rules).
    pub name: String,
}

impl State {
    /// A first visit on `day`, called `name`.
    #[must_use]
    pub fn new(day: i64, name: &str) -> Self {
        Self {
            coins: START_COINS,
            left: DAILY_TOSSES,
            day,
            streak: 0,
            best: 0,
            name: name.chars().take(NAME_MAX).collect(),
        }
    }

    /// Roll the allowance over if `today` is a later day than the saved one: five new tosses
    /// and the relief top-up. A day that is not later (a clock that moved back) changes
    /// nothing, so going back in time cannot refill the allowance.
    pub fn new_day(&mut self, today: i64) {
        if today > self.day {
            self.day = today;
            self.left = DAILY_TOSSES;
            self.coins = self.coins.max(RELIEF_FLOOR);
        }
    }

    /// Call the coin, tossing it with `rng`.
    pub fn toss(&mut self, call: Call, rng: &mut DoorRng) -> Outcome {
        if self.left == 0 {
            return Outcome::NoTossesLeft;
        }
        if self.coins == 0 {
            return Outcome::NoCoins;
        }
        self.left -= 1;
        if rng.chance(1, 2) {
            self.coins = (self.coins + 1).min(MAX_COINS);
            self.streak = self.streak.saturating_add(1);
            self.best = self.best.max(self.streak);
            Outcome::Won(call)
        } else {
            self.coins -= 1;
            self.streak = 0;
            Outcome::Lost(call)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_tosses_a_day_and_then_none() {
        let mut state = State::new(10, "ann");
        let mut rng = DoorRng::from_seed(3);
        for _ in 0..DAILY_TOSSES {
            assert!(matches!(
                state.toss(Call::Heads, &mut rng),
                Outcome::Won(_) | Outcome::Lost(_)
            ));
        }
        assert_eq!(state.toss(Call::Heads, &mut rng), Outcome::NoTossesLeft);
        assert_eq!(state.left, 0);
    }

    #[test]
    fn a_new_day_refills_once_and_going_back_in_time_does_not() {
        let mut state = State::new(10, "ann");
        state.left = 0;
        state.coins = 0;
        state.new_day(10);
        assert_eq!((state.left, state.coins), (0, 0), "same day: nothing");
        state.new_day(9);
        assert_eq!((state.left, state.coins), (0, 0), "earlier day: nothing");
        state.new_day(11);
        assert_eq!((state.left, state.coins, state.day), (DAILY_TOSSES, RELIEF_FLOOR, 11));
        state.left = 0;
        state.new_day(11);
        assert_eq!(state.left, 0, "the same new day is not a second refill");
    }

    #[test]
    fn streaks_count_wins_in_a_row_and_remember_the_best() {
        let mut state = State::new(1, "ann");
        state.coins = 100;
        let mut rng = DoorRng::from_seed(11);
        let mut best = 0u16;
        let mut run = 0u16;
        for day in 1..=40 {
            state.new_day(day);
            for _ in 0..DAILY_TOSSES {
                match state.toss(Call::Tails, &mut rng) {
                    Outcome::Won(_) => run += 1,
                    Outcome::Lost(_) => run = 0,
                    _ => {}
                }
                best = best.max(run);
            }
        }
        assert_eq!(state.best, best);
        assert!(state.best >= 2, "40 days of tosses should show a streak");
    }

    #[test]
    fn the_purse_never_goes_below_zero_or_over_the_cap() {
        let mut state = State::new(1, "ann");
        state.coins = MAX_COINS;
        let mut rng = DoorRng::from_seed(1);
        for _ in 0..DAILY_TOSSES {
            state.toss(Call::Heads, &mut rng);
        }
        assert!(state.coins <= MAX_COINS);
        let mut broke = State::new(1, "ann");
        broke.coins = 0;
        assert_eq!(broke.toss(Call::Heads, &mut rng), Outcome::NoCoins);
    }

    #[test]
    fn the_nickname_is_cut_to_the_limit() {
        assert_eq!(State::new(1, &"x".repeat(100)).name.chars().count(), NAME_MAX);
    }
}
