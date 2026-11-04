//! The shared world: how many tosses the whole board has made, today and yesterday, and how many
//! days the board has been running. Sessions add to today's count (through the framework's
//! compare-and-swap world edit) and the daily tick rolls today into yesterday.
//!
//! Layout, little-endian: days `u32`, today `u64`, yesterday `u64`.

/// The world as the tick and the sessions see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct World {
    /// Days the tick has run.
    pub days: u32,
    /// Tosses so far today.
    pub today: u64,
    /// Tosses yesterday.
    pub yesterday: u64,
}

impl World {
    /// Encode.
    #[must_use]
    pub fn encode(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(20);
        out.extend_from_slice(&self.days.to_le_bytes());
        out.extend_from_slice(&self.today.to_le_bytes());
        out.extend_from_slice(&self.yesterday.to_le_bytes());
        out
    }

    /// Decode, or `None` for bytes that are not a world.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let days = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?);
        let today = u64::from_le_bytes(bytes.get(4..12)?.try_into().ok()?);
        let yesterday = u64::from_le_bytes(bytes.get(12..20)?.try_into().ok()?);
        (bytes.len() == 20).then_some(Self {
            days,
            today,
            yesterday,
        })
    }

    /// A day passes: today becomes yesterday.
    pub fn end_day(&mut self) {
        self.days = self.days.saturating_add(1);
        self.yesterday = self.today;
        self.today = 0;
    }

    /// One more toss today.
    pub fn count_toss(&mut self) {
        self.today = self.today.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_world_round_trips_and_a_day_rolls_over() {
        let mut world = World {
            days: 2,
            today: 7,
            yesterday: 1,
        };
        assert_eq!(World::decode(&world.encode()), Some(world));
        world.end_day();
        assert_eq!(
            world,
            World {
                days: 3,
                today: 0,
                yesterday: 7
            }
        );
        world.count_toss();
        assert_eq!(world.today, 1);
        assert_eq!(World::decode(&[0; 19]), None);
        assert_eq!(World::decode(&[0; 21]), None);
    }
}
