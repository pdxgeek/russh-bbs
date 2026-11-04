//! Changes to the shared school, and the daily tick.
//!
//! A [`WorldDelta`] is a small pure edit of the stored [`World`]. The board may run a world edit
//! more than once (it retries on a conflict), so applying the same delta to the stored bytes
//! always gives a valid world, and a big scheme's effect is never applied twice.

use russh_bbs_door::DoorRng;

use crate::content::Content;
use crate::state::{Active, World, YEAR_CAP, YearEntry, map_get, map_set, to_u8};

/// One edit of the shared world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldDelta {
    /// A big scheme took effect.
    Effect {
        /// The effect id.
        id: String,
        /// The player's handle.
        by: String,
        /// The day.
        day: i64,
    },
    /// A place drew attention.
    Patrol {
        /// The place.
        place: String,
        /// Heat added.
        amount: u8,
    },
    /// A line for the paper.
    Headline {
        /// The day.
        day: i64,
        /// The words (template text with the name filled in).
        text: String,
    },
    /// A player's legend for the hall.
    Hall {
        /// The handle.
        name: String,
        /// Their legend.
        legend: u32,
    },
}

impl WorldDelta {
    /// Apply to a world. Idempotent for effects: one that is already in force is not stacked.
    pub fn apply(&self, world: &mut World, content: &Content) {
        match self {
            Self::Effect { id, by, day } => {
                let Some(effect) = content.effect(id) else {
                    return;
                };
                if world.has(id) {
                    return;
                }
                let until = (effect.days > 0).then_some(day + effect.days);
                world.active.push(Active {
                    id: id.clone(),
                    since: *day,
                    until,
                    by: by.clone(),
                });
                world.changes = world.changes.saturating_add(1);
            }
            Self::Patrol { place, amount } => {
                let now = i32::from(map_get(&world.patrol, place).copied().unwrap_or(0))
                    + i32::from(*amount);
                map_set(&mut world.patrol, place, to_u8(now.min(30)));
            }
            Self::Headline { day, text } => world.headline(*day, text.clone()),
            Self::Hall { name, legend } => world.hall_update(name, *legend),
        }
    }

    /// Apply to stored bytes (or a new world started on `today`) and return the new bytes.
    #[must_use]
    pub fn apply_to_bytes(&self, bytes: Option<&[u8]>, today: i64, content: &Content) -> Vec<u8> {
        let mut world = World::from_bytes(bytes, today);
        self.apply(&mut world, content);
        world.to_bytes()
    }
}

/// Apply one day to the world: the term clock, expiring effects, patrols easing, a new term when
/// the old one is over. A pure function of the world, the day and the tick's RNG.
pub fn tick_day(world: &mut World, content: &Content, day: i64, _rng: &mut DoorRng) {
    if day <= world.last_day {
        return;
    }
    world.last_day = day;
    world.active.retain(|a| a.until.is_none_or(|end| end > day));
    for (_, heat) in &mut world.patrol {
        *heat = heat.saturating_sub(1);
    }
    world.patrol.retain(|(_, heat)| *heat > 0);
    if world.term_day(day) >= content.rules.term_days {
        if let Some(best) = world.hall.first() {
            world.yearbook.push(YearEntry {
                term: world.term,
                name: best.name.clone(),
                legend: best.legend,
                changes: world.changes,
            });
            if world.yearbook.len() > YEAR_CAP {
                world.yearbook.remove(0);
            }
        }
        world.term = world.term.saturating_add(1);
        world.term_start = day;
        world.active.clear();
        world.patrol.clear();
        world.hall.clear();
        world.changes = 0;
        world.headline(day, "A new term begins at Greywater Academy.".to_owned());
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn an_effect_is_applied_once_and_expires_and_a_term_rolls_over() {
        let content = Content::embedded().unwrap();
        let mut world = World::new(100);
        let delta = WorldDelta::Effect {
            id: "tuck_shop_closed".into(),
            by: "ann".into(),
            day: 103,
        };
        delta.apply(&mut world, &content);
        delta.apply(&mut world, &content);
        assert_eq!(world.active.len(), 1, "a retried edit does not stack");
        assert_eq!(world.changes, 1);
        let mut rng = DoorRng::from_seed(1);
        tick_day(&mut world, &content, 109, &mut rng);
        assert!(world.has("tuck_shop_closed"));
        tick_day(&mut world, &content, 110, &mut rng);
        assert!(
            !world.has("tuck_shop_closed"),
            "seven days, then it reopens"
        );
        world.hall_update("ann", 40);
        tick_day(
            &mut world,
            &content,
            100 + content.rules.term_days,
            &mut rng,
        );
        assert_eq!(
            (world.term, world.term_start),
            (2, 100 + content.rules.term_days)
        );
        assert_eq!(world.yearbook[0].name, "ann");
        assert_eq!(world.hall, Vec::new());
        // Ticking an old day changes nothing.
        let before = world.clone();
        tick_day(&mut world, &content, 50, &mut rng);
        assert_eq!(world, before);
    }
}
