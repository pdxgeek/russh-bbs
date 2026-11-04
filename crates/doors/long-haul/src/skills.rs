//! Skills that grow by doing, and what each level changes.
//!
//! Every skill is a pool of experience points that only its own actions fill. A level is the
//! number of thresholds passed (0 to 10). Each level changes an outcome the engine computes,
//! and the branch tests in `tests/branches.rs` compare level 0 with level 10 for every skill.

/// The five skills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Skill {
    /// Narrows the spread on every trade.
    Trade,
    /// Cuts fuel burn and shortens jumps; lets a captain read a nebula and skim safely.
    Pilot,
    /// Cheaper repairs and slower wear.
    Engineer,
    /// Fewer inspections found.
    Stealth,
    /// Dearer survey data.
    Survey,
}

impl Skill {
    /// Every skill, in save order.
    pub const ALL: [Self; 5] = [
        Self::Trade,
        Self::Pilot,
        Self::Engineer,
        Self::Stealth,
        Self::Survey,
    ];

    /// Position in a skill array.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Trade => 0,
            Self::Pilot => 1,
            Self::Engineer => 2,
            Self::Stealth => 3,
            Self::Survey => 4,
        }
    }

    /// Display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Trade => "Trading",
            Self::Pilot => "Piloting",
            Self::Engineer => "Engineering",
            Self::Stealth => "Stealth",
            Self::Survey => "Surveying",
        }
    }
}

/// Experience needed for level 1, 2, ... 10.
pub const THRESHOLDS: [u32; 10] = [20, 60, 120, 200, 300, 420, 560, 720, 900, 1100];

/// The level a pool of experience reaches.
#[must_use]
pub fn level_of(xp: u32) -> u8 {
    u8::try_from(THRESHOLDS.iter().filter(|&&t| xp >= t).count()).unwrap_or(10)
}

/// The least experience that gives `level`.
#[must_use]
pub fn xp_for(level: u8) -> u32 {
    match level {
        0 => 0,
        n => THRESHOLDS[usize::from(n.min(10) - 1)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_the_thresholds_and_stop_at_ten() {
        assert_eq!(level_of(0), 0);
        assert_eq!(level_of(19), 0);
        assert_eq!(level_of(20), 1);
        assert_eq!(level_of(1099), 9);
        assert_eq!(level_of(u32::MAX), 10);
        for level in 0..=10 {
            assert_eq!(level_of(xp_for(level)), level);
        }
    }
}
