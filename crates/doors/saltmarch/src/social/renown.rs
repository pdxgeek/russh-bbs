//! Renown: what the league ranks. A mix of hold size, crew, expedition deeds, pacts kept and
//! raids won, less betrayals. Raiding alone cannot top it: the raids part is capped at 18, and
//! a clan that does nothing else has no hold to speak of.

use super::pact::State;
use crate::clan::Clan;
use crate::content::Catalog;
use crate::types::Trade;
use crate::world::World;

/// The most renown raids can add.
pub const RAIDS_MAX: u32 = 18;
/// The most renown pacts can add.
pub const PACTS_MAX: u32 = 24;
/// Renown lost per pact broken.
pub const BETRAYAL: u32 = 15;

/// Where a clan's renown comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Renown {
    /// Buildings finished.
    pub hold: u32,
    /// Crew, their skill and their bonds.
    pub crew: u32,
    /// Expeditions: cargo brought home and fights won.
    pub deeds: u32,
    /// Pacts kept, by the week.
    pub pacts: u32,
    /// Raids won.
    pub raids: u32,
    /// Wealth, a little.
    pub wealth: u32,
    /// Betrayals on record.
    pub penalty: u32,
}

impl Renown {
    /// The whole.
    #[must_use]
    pub const fn total(&self) -> u32 {
        (self.hold + self.crew + self.deeds + self.pacts + self.raids + self.wealth)
            .saturating_sub(self.penalty)
    }
}

/// A clan's renown today, reading the world for what only the world knows (pacts, betrayals).
#[must_use]
pub fn of(cat: &Catalog, clan: &Clan, world: &World, today: i64) -> Renown {
    let buildings = u32::try_from(clan.hold.built(cat).count()).unwrap_or(0);
    let skill: u32 = clan
        .crew
        .iter()
        .map(|c| {
            Trade::ALL
                .iter()
                .map(|t| u32::from(c.level(*t)))
                .sum::<u32>()
        })
        .sum();
    let bonds = u32::try_from(clan.bonds.iter().filter(|b| b.bonded()).count()).unwrap_or(0);
    let crew = u32::try_from(clan.crew.len()).unwrap_or(0);
    let me = world.ix(&clan.name);
    let pacts: u32 = me
        .map(|m| {
            world
                .social
                .pacts
                .iter()
                .filter(|p| p.state == State::Active && p.has(m))
                .map(|p| u32::try_from(((today - p.since) / 7).clamp(0, 4)).unwrap_or(0) * 3)
                .sum()
        })
        .unwrap_or(0);
    let betrayals = me
        .and_then(|m| world.clans.get(usize::from(m)))
        .map_or(0, |c| u32::from(c.record.betrayals));
    Renown {
        hold: buildings * 5,
        crew: skill + bonds * 3 + crew * 2,
        deeds: (clan.stats.loot_value / 40).min(30) + clan.stats.fights_won.min(15),
        pacts: pacts.min(PACTS_MAX),
        raids: (u32::from(clan.social.raids_won).min(6) * 3).min(RAIDS_MAX),
        wealth: (clan.tallies / 50).min(20),
        penalty: betrayals * BETRAYAL,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::clan::tests::fixture;

    #[test]
    fn raiding_alone_cannot_top_the_league() {
        let (cat, _, mut raider) = fixture();
        let (_, _, mut builder) = fixture();
        builder.name = "Builder".into();
        // A raider who has won two hundred raids but built nothing...
        raider.social.raids_won = 200;
        // ...against a builder with a modest hold and no raids at all.
        builder.hold.plots[2] = Some("smokehouse".into());
        builder.hold.plots[3] = Some("palisade".into());
        builder.hold.plots[4] = Some("watch-tower".into());
        builder.hold.plots[5] = Some("smithy".into());
        let w = World::new(1, 1, 0);
        let r = of(&cat, &raider, &w, 100);
        let b = of(&cat, &builder, &w, 100);
        assert_eq!(r.raids, RAIDS_MAX);
        assert!(b.total() > r.total(), "{} vs {}", b.total(), r.total());
    }

    #[test]
    fn pacts_kept_add_by_the_week_and_a_betrayal_costs() {
        let (cat, _, clan) = fixture();
        let mut w = World::new(1, 1, 0);
        w.register("Brindle", 0);
        w.register("Cove", 0);
        for c in &mut w.clans {
            c.posted.buildings = 4;
        }
        let base = of(&cat, &clan, &w, 100).total();
        let id = w.propose(0, 1, super::super::pact::TRUCE, 100).unwrap();
        w.accept(1, id, 100).unwrap();
        assert_eq!(of(&cat, &clan, &w, 100).pacts, 0);
        assert_eq!(of(&cat, &clan, &w, 114).pacts, 6);
        assert_eq!(of(&cat, &clan, &w, 400).pacts, 12, "four weeks at most");
        assert!(of(&cat, &clan, &w, 114).total() > base);
        w.break_pact(0, id, 115).unwrap();
        let after = of(&cat, &clan, &w, 115);
        assert_eq!(after.penalty, BETRAYAL);
        assert_eq!(after.pacts, 0);
    }
}
