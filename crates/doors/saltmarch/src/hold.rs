//! The hold: a grid of plots on the high ground, three building tracks and a sluice.

use crate::codec::{Reader, Writer};
use crate::content::{BuildingDef, Catalog};
use crate::types::Track;

/// Columns of plots.
pub const COLS: u8 = 5;

/// Rows of plots.
pub const ROWS: u8 = 4;

/// Plots in the grid.
pub const PLOTS: usize = (COLS as usize) * (ROWS as usize);

/// Plots open at the start; each shelter building clears two more.
pub const BASE_PLOTS: usize = 6;

/// Crew the hold houses with no buildings.
pub const BASE_CREW: usize = 3;

/// The most crew a clan can have, whatever it builds.
pub const MAX_CREW: usize = 12;

/// Work in progress on one track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Construction {
    /// The building.
    pub building: String,
    /// The plot it stands on.
    pub plot: u8,
    /// Turns of labour left.
    pub remaining: u8,
}

/// The hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    /// What stands on each plot (building ids).
    pub plots: Vec<Option<String>>,
    /// One construction per track.
    pub building: [Option<Construction>; 3],
    /// The water level at or above which the sluice floods the approach (set by the clan).
    pub sluice_level: Option<u8>,
}

impl Default for Hold {
    fn default() -> Self {
        Self {
            plots: vec![None; PLOTS],
            building: [None, None, None],
            sluice_level: None,
        }
    }
}

impl Hold {
    /// Finished buildings of one kind.
    #[must_use]
    pub fn count(&self, id: &str) -> usize {
        self.plots.iter().flatten().filter(|b| *b == id).count()
    }

    /// Whether at least one `id` is finished.
    #[must_use]
    pub fn has(&self, id: &str) -> bool {
        self.count(id) > 0
    }

    /// Finished buildings with their definitions.
    pub fn built<'a>(&'a self, cat: &'a Catalog) -> impl Iterator<Item = (usize, &'a BuildingDef)> {
        self.plots
            .iter()
            .enumerate()
            .filter_map(|(i, b)| Some((i, cat.building(b.as_deref()?)?)))
    }

    /// Sum a number over finished buildings.
    #[must_use]
    pub fn total(&self, cat: &Catalog, f: impl Fn(&BuildingDef) -> u32) -> u32 {
        self.built(cat).map(|(_, b)| f(b)).sum()
    }

    /// Plots open to build on: [`BASE_PLOTS`] plus two for each finished shelter building.
    #[must_use]
    pub fn unlocked(&self, cat: &Catalog) -> usize {
        let shelters = self
            .built(cat)
            .filter(|(_, b)| b.track == Track::Shelter)
            .count();
        (BASE_PLOTS + 2 * shelters).min(PLOTS)
    }

    /// How many crew the hold houses.
    #[must_use]
    pub fn crew_cap(&self, cat: &Catalog) -> usize {
        (BASE_CREW + usize::try_from(self.total(cat, |b| u32::from(b.crew_cap))).unwrap_or(0))
            .min(MAX_CREW)
    }

    /// Extra healing steps a night.
    #[must_use]
    pub fn heal_steps(&self, cat: &Catalog) -> u8 {
        u8::try_from(self.total(cat, |b| u32::from(b.heal_steps))).unwrap_or(u8::MAX)
    }

    /// Peat burned a night by the buildings.
    #[must_use]
    pub fn upkeep_peat(&self, cat: &Catalog) -> u32 {
        self.total(cat, |b| u32::from(b.upkeep_peat))
    }

    /// Morale the buildings give each night.
    #[must_use]
    pub fn morale(&self, cat: &Catalog) -> i32 {
        self.built(cat).map(|(_, b)| i32::from(b.morale)).sum()
    }

    /// The raiders' view of the hold at water `level`: the walls and ditches, plus the flood
    /// the sluice makes when the water is at or above its set level.
    #[must_use]
    pub fn defence(&self, cat: &Catalog, level: i32) -> u32 {
        let flooded = self.sluice_level.is_some_and(|set| level >= i32::from(set))
            && self.built(cat).any(|(_, b)| b.is_sluice());
        self.total(cat, |b| {
            u32::from(b.defence)
                + if flooded {
                    u32::from(b.flood_defence)
                } else {
                    0
                }
        })
    }

    /// Write to a save.
    pub fn write(&self, w: &mut Writer) {
        for p in &self.plots {
            w.str(p.as_deref().unwrap_or(""));
        }
        for c in &self.building {
            match c {
                Some(c) => {
                    w.u8(1);
                    w.str(&c.building);
                    w.u8(c.plot);
                    w.u8(c.remaining);
                }
                None => w.u8(0),
            }
        }
        w.u8(self.sluice_level.unwrap_or(u8::MAX));
    }

    /// Read from a save.
    pub fn read(r: &mut Reader<'_>) -> Option<Self> {
        let mut plots = Vec::with_capacity(PLOTS);
        for _ in 0..PLOTS {
            let s = r.str()?;
            plots.push((!s.is_empty()).then_some(s));
        }
        let mut building = [None, None, None];
        for slot in &mut building {
            *slot = match r.u8()? {
                0 => None,
                1 => Some(Construction {
                    building: r.str()?,
                    plot: r.u8()?,
                    remaining: r.u8()?,
                }),
                _ => return None,
            };
        }
        let sluice_level = match r.u8()? {
            u8::MAX => None,
            l => Some(l),
        };
        Some(Self {
            plots,
            building,
            sluice_level,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn hold_with(ids: &[&str]) -> Hold {
        let mut h = Hold::default();
        for (i, id) in ids.iter().enumerate() {
            h.plots[i] = Some((*id).to_owned());
        }
        h
    }

    #[test]
    fn capacity_and_plots_grow_with_shelter_and_stop_at_the_caps() {
        let cat = Catalog::embedded().unwrap();
        let empty = Hold::default();
        assert_eq!(empty.crew_cap(&cat), BASE_CREW);
        assert_eq!(empty.unlocked(&cat), BASE_PLOTS);
        let h = hold_with(&["hut", "hut", "hut", "longhouse"]);
        assert_eq!(h.crew_cap(&cat), 12);
        assert_eq!(h.unlocked(&cat), BASE_PLOTS + 8);
        let work = hold_with(&["salt-pan"]);
        assert_eq!(
            work.unlocked(&cat),
            BASE_PLOTS,
            "work buildings open no plots"
        );
    }

    #[test]
    fn the_sluice_only_helps_when_it_is_built_set_and_the_water_is_up() {
        let cat = Catalog::embedded().unwrap();
        let mut h = hold_with(&["palisade", "sluice"]);
        assert_eq!(h.defence(&cat, 90), 4, "built but not set");
        h.sluice_level = Some(60);
        assert_eq!(h.defence(&cat, 59), 4, "water below the set level");
        assert_eq!(
            h.defence(&cat, 60),
            12,
            "water at the set level floods the approach"
        );
        let mut no_sluice = hold_with(&["palisade"]);
        no_sluice.sluice_level = Some(10);
        assert_eq!(
            no_sluice.defence(&cat, 100),
            3,
            "a set level without a sluice does nothing"
        );
    }

    #[test]
    fn upkeep_morale_and_healing_add_up() {
        let cat = Catalog::embedded().unwrap();
        let h = hold_with(&["longhouse", "infirmary", "watchfire", "smokehouse"]);
        assert_eq!(h.upkeep_peat(&cat), 4);
        assert_eq!(h.morale(&cat), 3);
        assert_eq!(h.heal_steps(&cat), 1);
    }

    #[test]
    fn a_hold_round_trips_with_a_construction_and_a_sluice_level() {
        let mut h = hold_with(&["hut", "salt-pan"]);
        h.building[1] = Some(Construction {
            building: "smithy".into(),
            plot: 3,
            remaining: 4,
        });
        h.sluice_level = Some(55);
        let mut w = Writer::new();
        h.write(&mut w);
        let bytes = w.finish();
        assert_eq!(Hold::read(&mut Reader::new(&bytes)), Some(h));
    }
}
