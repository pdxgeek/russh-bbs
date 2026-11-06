//! How a season ends and what the next one inherits.
//!
//! A season has several endings, chosen from what the board did (the world's own counters and
//! what each clan posted). The ending is written into the hall's book with the season, and the
//! next season's world carries it as a **legacy** with a defined, small consequence for every
//! clan that starts in it. A clan also carries one **heirloom** of its own deeds.

use super::pact::State;
use crate::clan::Clan;
use crate::content::Catalog;
use crate::map::Map;
use crate::story::{Faction, Heirloom};
use crate::types::Resource;
use crate::world::{Prices, World};

/// Days of new-clan protection when the last season ended in a long peace.
pub const PEACE_NEW_CLAN_DAYS: i64 = 14;

/// How the season ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ending {
    /// Nothing outweighed the water.
    #[default]
    Quiet,
    /// The Deep was opened: clans stood in it and quests were finished.
    DeepOpened,
    /// A season of raids.
    RedTide,
    /// A season of broken oaths.
    BrokenOaths,
    /// A season of trade.
    TradeBoom,
    /// A season of pacts.
    LongPeace,
    /// A season of causeways.
    Roads,
    /// A season of honest work: contracts and chains finished.
    WorkYear,
}

/// What a season's play adds up to, for choosing its ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    /// Raids won, all clans.
    pub raids: u32,
    /// Pacts broken.
    pub broken: u32,
    /// Tallies traded on the exchange.
    pub traded: u32,
    /// Pacts in force at the end.
    pub pacts: u32,
    /// Clans that stood in the Deep.
    pub deep: u32,
    /// Quest chains finished.
    pub quests: u32,
    /// Contracts finished.
    pub contracts: u32,
    /// Causeway tiles on the marsh.
    pub laid: u32,
}

impl Tally {
    /// Count a world.
    #[must_use]
    pub fn of(w: &World) -> Self {
        let sum = |f: &dyn Fn(&crate::world::ClanEntry) -> u32| w.clans.iter().map(f).sum::<u32>();
        Self {
            raids: sum(&|c| u32::from(c.record.raids_won)),
            broken: sum(&|c| u32::from(c.record.betrayals)),
            traded: sum(&|c| c.record.traded),
            pacts: u32::try_from(
                w.social
                    .pacts
                    .iter()
                    .filter(|p| p.state == State::Active)
                    .count(),
            )
            .unwrap_or(0),
            deep: sum(&|c| u32::from(c.badge.deep)),
            quests: sum(&|c| u32::from(c.badge.quests)),
            contracts: sum(&|c| u32::from(c.badge.contracts)),
            laid: u32::try_from(w.causeways.len()).unwrap_or(0),
        }
    }
}

impl Ending {
    /// Every ending.
    pub const ALL: [Self; 8] = [
        Self::Quiet,
        Self::DeepOpened,
        Self::RedTide,
        Self::BrokenOaths,
        Self::TradeBoom,
        Self::LongPeace,
        Self::Roads,
        Self::WorkYear,
    ];

    /// The save code.
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// From a save code (unknown codes read as a quiet season).
    #[must_use]
    pub fn from_code(c: u8) -> Self {
        Self::ALL.get(usize::from(c)).copied().unwrap_or_default()
    }

    /// The ending a season's numbers add up to, in order of how much they mattered.
    #[must_use]
    pub fn of(t: &Tally) -> Self {
        if t.deep >= 2 || (t.deep >= 1 && t.quests >= 3) {
            Self::DeepOpened
        } else if t.broken >= 3 {
            Self::BrokenOaths
        } else if t.raids >= 5 && t.raids > t.pacts * 2 {
            Self::RedTide
        } else if t.traded >= 400 {
            Self::TradeBoom
        } else if t.pacts >= 2 {
            Self::LongPeace
        } else if t.laid >= 6 {
            Self::Roads
        } else if t.contracts >= 12 || t.quests >= 4 {
            Self::WorkYear
        } else {
            Self::Quiet
        }
    }

    /// The title in the hall's book.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Quiet => "A Quiet Season",
            Self::DeepOpened => "The Deep Opened",
            Self::RedTide => "The Red Tide",
            Self::BrokenOaths => "The Broken Oaths",
            Self::TradeBoom => "The Market Year",
            Self::LongPeace => "The Long Peace",
            Self::Roads => "The Year of Roads",
            Self::WorkYear => "The Working Year",
        }
    }

    /// How the season is remembered.
    #[must_use]
    pub const fn line(self) -> &'static str {
        match self {
            Self::Quiet => "It was a quiet season: the water did most of the talking.",
            Self::DeepOpened => {
                "It was the season the Deep opened: lamps went north and some of them came back."
            }
            Self::RedTide => {
                "It was a season of raids: the gates were never quiet, and the reed-folk learned to sleep light."
            }
            Self::BrokenOaths => {
                "It was a season of broken oaths: the Tidecourt kept a longer list of liars than of friends."
            }
            Self::TradeBoom => {
                "It was a season of trade: the Tidecourt's boards were never empty, and the lamps burned late."
            }
            Self::LongPeace => {
                "It was a season of pacts: lamps were lit between the holds, and the water found fewer open doors."
            }
            Self::Roads => {
                "It was a season of roads: the flats were stitched with timber, and every span had its keeper."
            }
            Self::WorkYear => {
                "It was a working season: the boards were cleared, the old debts paid, and the guilds kept their ledgers straight."
            }
        }
    }

    /// What the next season inherits, in a line for the screens.
    #[must_use]
    pub const fn legacy_text(self) -> &'static str {
        match self {
            Self::Quiet => "Nothing carries over: the marsh starts clean.",
            Self::DeepOpened => "Every hold starts with the gate of the Deep charted.",
            Self::RedTide => "Every hold starts with four more arms in the racks.",
            Self::BrokenOaths => "The Tidecourt starts the season distrustful of every clan.",
            Self::TradeBoom => "Prices at the Tidecourt start a tenth higher.",
            Self::LongPeace => "New clans are left alone for fourteen days, not seven.",
            Self::Roads => "Every hold starts with six more rope and a spare set of tools.",
            Self::WorkYear => "Every clan starts a little better known to the three factions.",
        }
    }
}

/// The days a new clan is safe from raids in a world that inherited `legacy`.
#[must_use]
pub const fn new_clan_days(legacy: Ending) -> i64 {
    match legacy {
        Ending::LongPeace => PEACE_NEW_CLAN_DAYS,
        _ => super::caps::NEW_CLAN_DAYS,
    }
}

/// The Tidecourt's prices at the start of a season that inherited `legacy`.
pub fn adjust_prices(legacy: Ending, prices: &mut Prices) {
    if legacy == Ending::TradeBoom {
        for r in Resource::ALL {
            let base = u32::from(r.base_price()) * 100;
            let p = u32::from(prices[r.index()]) * 110 / 100;
            prices[r.index()] = u16::try_from(p.min(base * 2)).unwrap_or(u16::MAX);
        }
    }
}

/// What a clan starts a season with because of the last season's ending and its heirloom.
pub fn start(
    clan: &mut Clan,
    cat: &Catalog,
    map: &Map,
    legacy: Ending,
    heirloom: Option<Heirloom>,
) {
    match legacy {
        Ending::Quiet | Ending::LongPeace | Ending::TradeBoom => {}
        Ending::DeepOpened => chart_gate(clan, map),
        Ending::RedTide => clan.stock.add(Resource::Arms, 4),
        Ending::BrokenOaths => {
            clan.story.rep[Faction::Court.index()] = -12;
        }
        Ending::Roads => {
            clan.stock.add(Resource::Rope, 6);
            clan.stock.add(Resource::Tools, 1);
        }
        Ending::WorkYear => {
            for f in Faction::ALL {
                clan.story.add_rep(f, 10);
            }
        }
    }
    if let Some(h) = heirloom {
        clan.story.heirloom = h.code();
        match h {
            Heirloom::SaltCrown => clan.tallies = clan.tallies.saturating_add(25),
            Heirloom::DeepLantern => {
                clan.stock.add(Resource::Peat, 6);
                chart_gate(clan, map);
            }
            Heirloom::OathRing => clan.morale = clan.morale.saturating_add(12).min(100),
            Heirloom::RaidersHorn => clan.stock.add(Resource::Arms, 3),
            Heirloom::CourtSeal => {
                let c = &mut clan.story.rep[Faction::Court.index()];
                *c = (*c).max(15);
            }
            Heirloom::ReedKnot => {
                let c = &mut clan.story.rep[Faction::Reed.index()];
                *c = (*c).max(15);
                if let Some(r) = cat
                    .recipes
                    .iter()
                    .find(|r| r.learn == crate::content::Learn::Trade)
                    && !clan.recipes.contains(&r.id)
                {
                    clan.recipes.push(r.id.clone());
                }
            }
        }
    }
}

fn chart_gate(clan: &mut Clan, map: &Map) {
    if let Some((x, y)) = map.deep_gate() {
        clan.chart_around(usize::from(x), usize::from(y), 4);
    }
}

/// The heirloom a clan earned this season, if any: the first of [`Heirloom::ALL`] it
/// deserves. `old` is the world of the season that ended.
#[must_use]
pub fn heirloom_for(clan: &Clan, old: Option<&World>, today: i64) -> Option<Heirloom> {
    let rank_one = old.is_some_and(|w| {
        w.clans.len() >= 2 && w.standings().first().is_some_and(|c| c.name == clan.name)
    });
    let long_pact = old.is_some_and(|w| {
        w.ix(&clan.name).is_some_and(|me| {
            w.social
                .pacts
                .iter()
                .any(|p| p.state == State::Active && p.has(me) && today - p.since >= 14)
        })
    });
    Heirloom::ALL.into_iter().find(|h| match h {
        Heirloom::SaltCrown => rank_one,
        Heirloom::DeepLantern => clan.social.deep,
        Heirloom::CourtSeal => clan.story.tier(Faction::Court) >= 4,
        Heirloom::ReedKnot => clan.story.tier(Faction::Reed) >= 4,
        Heirloom::OathRing => long_pact,
        Heirloom::RaidersHorn => clan.social.raids_won >= 3,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::clan::tests::fixture;

    #[test]
    fn the_numbers_choose_the_ending_in_order_of_weight() {
        let t = Tally::default();
        assert_eq!(Ending::of(&t), Ending::Quiet);
        assert_eq!(
            Ending::of(&Tally { deep: 2, ..t }),
            Ending::DeepOpened,
            "two clans in the Deep open it"
        );
        assert_eq!(
            Ending::of(&Tally {
                deep: 1,
                quests: 3,
                ..t
            }),
            Ending::DeepOpened
        );
        assert_eq!(Ending::of(&Tally { deep: 1, ..t }), Ending::Quiet);
        assert_eq!(Ending::of(&Tally { broken: 3, raids: 9, ..t }), Ending::BrokenOaths);
        assert_eq!(Ending::of(&Tally { raids: 6, ..t }), Ending::RedTide);
        assert_eq!(
            Ending::of(&Tally {
                raids: 6,
                pacts: 3,
                ..t
            }),
            Ending::LongPeace
        );
        assert_eq!(Ending::of(&Tally { traded: 500, ..t }), Ending::TradeBoom);
        assert_eq!(Ending::of(&Tally { laid: 6, ..t }), Ending::Roads);
        assert_eq!(Ending::of(&Tally { contracts: 12, ..t }), Ending::WorkYear);
        assert_eq!(Ending::of(&Tally { quests: 4, ..t }), Ending::WorkYear);
    }

    #[test]
    fn every_ending_has_a_code_a_name_and_a_distinct_line() {
        let mut lines: Vec<&str> = Ending::ALL.iter().map(|e| e.line()).collect();
        lines.sort_unstable();
        lines.dedup();
        assert_eq!(lines.len(), Ending::ALL.len());
        for e in Ending::ALL {
            assert_eq!(Ending::from_code(e.code()), e);
            assert!(!e.name().is_empty() && !e.legacy_text().is_empty());
        }
        assert_eq!(Ending::from_code(200), Ending::Quiet);
    }

    #[test]
    fn each_legacy_changes_the_next_season_and_the_quiet_one_does_not() {
        let (cat, map, base) = fixture();
        let start_of = |legacy: Ending| {
            let mut c = base.clone();
            start(&mut c, &cat, &map, legacy, None);
            c
        };
        let quiet = start_of(Ending::Quiet);
        assert_eq!(quiet, base);
        for e in [
            Ending::DeepOpened,
            Ending::RedTide,
            Ending::BrokenOaths,
            Ending::Roads,
            Ending::WorkYear,
        ] {
            assert_ne!(start_of(e), base, "{e:?} must leave a mark");
        }
        assert_eq!(
            start_of(Ending::RedTide).stock.get(Resource::Arms),
            base.stock.get(Resource::Arms) + 4
        );
        assert!(start_of(Ending::BrokenOaths).story.tier(Faction::Court) == 1);
        assert_eq!(start_of(Ending::BrokenOaths).story.rep[Faction::Court.index()], -12);
        let gate = map.deep_gate().unwrap();
        assert!(!base.is_charted(usize::from(gate.0), usize::from(gate.1)));
        assert!(start_of(Ending::DeepOpened).is_charted(usize::from(gate.0), usize::from(gate.1)));
        let wy = start_of(Ending::WorkYear);
        assert!(Faction::ALL.iter().all(|f| wy.story.rep[f.index()] == 10));
        // The peace and the boom change the world, not the clan.
        assert_eq!(new_clan_days(Ending::LongPeace), 14);
        assert_eq!(new_clan_days(Ending::Quiet), 7);
        let mut p = World::new(1, 1, 0).prices;
        let before = p[Resource::Iron.index()];
        adjust_prices(Ending::TradeBoom, &mut p);
        assert_eq!(u32::from(p[Resource::Iron.index()]), u32::from(before) * 110 / 100);
        let mut q = World::new(1, 1, 0).prices;
        adjust_prices(Ending::Quiet, &mut q);
        assert_eq!(q, World::new(1, 1, 0).prices);
    }

    #[test]
    fn the_heirloom_is_earned_by_a_deed_and_never_stacks() {
        let (cat, map, mut clan) = fixture();
        clan.name = "Ash".into();
        assert_eq!(heirloom_for(&clan, None, 100), None);
        clan.social.raids_won = 3;
        assert_eq!(heirloom_for(&clan, None, 100), Some(Heirloom::RaidersHorn));
        clan.social.deep = true;
        assert_eq!(
            heirloom_for(&clan, None, 100),
            Some(Heirloom::DeepLantern),
            "the first earned in order is the one kept"
        );
        clan.story.rep[Faction::Reed.index()] = 80;
        assert_eq!(heirloom_for(&clan, None, 100), Some(Heirloom::DeepLantern));
        clan.story.rep[Faction::Court.index()] = 80;
        assert_eq!(heirloom_for(&clan, None, 100), Some(Heirloom::DeepLantern));
        clan.social.deep = false;
        assert_eq!(heirloom_for(&clan, None, 100), Some(Heirloom::CourtSeal));
        // Each heirloom does what it says.
        for h in Heirloom::ALL {
            let (_, _, fresh) = fixture();
            let mut c = fresh.clone();
            start(&mut c, &cat, &map, Ending::Quiet, Some(h));
            assert_eq!(c.story.heirloom(), Some(h));
            assert_ne!(c, fresh, "{h:?} must do something");
        }
        // A crown for the league's first.
        let mut w = World::new(2, 1, 0);
        w.register("Ash", 0);
        w.register("Bo", 1);
        w.set_score("Ash", 50);
        w.set_score("Bo", 10);
        clan.social.deep = false;
        clan.social.raids_won = 0;
        clan.story.rep = [0; 3];
        assert_eq!(heirloom_for(&clan, Some(&w), 100), Some(Heirloom::SaltCrown));
        clan.name = "Bo".into();
        assert_eq!(heirloom_for(&clan, Some(&w), 100), None);
    }
}
