//! A crew member: trade, attributes, skills that grow with use, traits, injuries and age.

use russh_bbs_door::DoorRng;

use crate::codec::{Reader, Writer};
use crate::content::Catalog;
use crate::types::{Attr, Skill, TRADES, Trade};

/// The most xp a skill holds (level 10).
pub const MAX_XP: u16 = 400;

/// The oldest a crew member gets before they must retire.
pub const RETIRE_AGE: u8 = 60;

/// Bond points two crew need to be bonded.
pub const BOND_AT: u8 = 8;

/// An injury being healed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Injured {
    /// The injury's id in the catalog.
    pub id: String,
    /// Days left.
    pub days: u8,
}

/// One crew member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crew {
    /// Unique within the clan.
    pub id: u16,
    /// Name (generated; never typed by a player in this slice).
    pub name: String,
    /// Trade.
    pub trade: Trade,
    /// Grit, wit and reach, 1 to 10.
    pub attrs: [u8; 3],
    /// Experience in each skill, in [`Trade::ALL`] order.
    pub xp: [u16; TRADES],
    /// Trait ids (one or two at hire; scars can add one).
    pub traits: Vec<String>,
    /// Injuries being healed.
    pub injuries: Vec<Injured>,
    /// Age in years.
    pub age: u8,
    /// The plot this crew member works at overnight.
    pub post: Option<u8>,
    /// Shifts worked, for the hall.
    pub shifts: u16,
}

/// A pair of crew who have been through it together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bond {
    /// The lower id.
    pub a: u16,
    /// The higher id.
    pub b: u16,
    /// Hardship shared; bonded at [`BOND_AT`].
    pub points: u8,
}

impl Bond {
    /// Whether the pair is bonded.
    #[must_use]
    pub const fn bonded(&self) -> bool {
        self.points >= BOND_AT
    }

    /// Whether `id` is one of the pair.
    #[must_use]
    pub const fn has(&self, id: u16) -> bool {
        self.a == id || self.b == id
    }
}

impl Crew {
    /// The level of `skill`: 0 to 10, one level per `4 * level^2` xp.
    #[must_use]
    pub fn level(&self, skill: Skill) -> u8 {
        let xp = u32::from(self.xp[skill.index()]);
        (0..=10u32)
            .rev()
            .find(|l| xp >= 4 * l * l)
            .and_then(|l| u8::try_from(l).ok())
            .unwrap_or(0)
    }

    /// An attribute as it stands today: grit wanes with age past 48.
    #[must_use]
    pub fn attr(&self, a: Attr) -> i32 {
        let base = i32::from(self.attrs[a.index()]);
        if a == Attr::Grit && self.age >= 49 {
            (base - (i32::from(self.age) - 45) / 4).max(1)
        } else {
            base
        }
    }

    /// The sum of one number over this crew member's traits.
    #[must_use]
    pub fn trait_sum(&self, cat: &Catalog, f: impl Fn(&crate::content::TraitDef) -> i32) -> i32 {
        self.traits
            .iter()
            .filter_map(|t| cat.trait_def(t))
            .map(f)
            .sum()
    }

    /// Whether any trait satisfies `f`.
    #[must_use]
    pub fn trait_any(&self, cat: &Catalog, f: impl Fn(&crate::content::TraitDef) -> bool) -> bool {
        self.traits.iter().filter_map(|t| cat.trait_def(t)).any(f)
    }

    /// Percent of their work lost to injury (0 to 100).
    #[must_use]
    pub fn penalty(&self, cat: &Catalog) -> u32 {
        self.injuries
            .iter()
            .filter_map(|i| cat.injury(&i.id))
            .map(|d| d.work_pct)
            .sum::<u32>()
            .min(100)
    }

    /// Whether they can work at all today (an injury of 80% or worse puts them in bed).
    #[must_use]
    pub fn fit(&self, cat: &Catalog) -> bool {
        self.penalty(cat) < 80
    }

    /// How hard they work at `skill`, in percent of a standard hand: a baseline, plus skill and
    /// the trade's attribute, plus a bonus for their own trade, less injury.
    #[must_use]
    pub fn power(&self, cat: &Catalog, skill: Skill) -> i32 {
        let raw = 40
            + 7 * i32::from(self.level(skill))
            + 4 * self.attr(skill.attr())
            + if self.trade == skill { 20 } else { 0 };
        raw * i32::try_from(100 - self.penalty(cat)).unwrap_or(0) / 100
    }

    /// Add experience to `skill` (adjusted by their learning traits). Returns the new level if
    /// it went up.
    pub fn gain(&mut self, cat: &Catalog, skill: Skill, amount: u16) -> Option<u8> {
        let before = self.level(skill);
        let learn = self.trait_sum(cat, |t| t.learn_pct);
        let scaled = (i32::from(amount) * (100 + learn) / 100).max(1);
        let slot = &mut self.xp[skill.index()];
        *slot = (*slot)
            .saturating_add(u16::try_from(scaled).unwrap_or(u16::MAX))
            .min(MAX_XP);
        let after = self.level(skill);
        (after > before).then_some(after)
    }

    /// What a gate candidate asks for, in tallies: more for better attributes and skill, less
    /// for a bad trait.
    #[must_use]
    pub fn price(&self, cat: &Catalog) -> u32 {
        let attrs: i32 = self.attrs.iter().map(|a| i32::from(*a)).sum();
        let base = 4 + attrs + 3 * i32::from(self.level(self.trade));
        let adjust = self.trait_sum(cat, |t| t.hire_pct);
        u32::try_from((base * (100 + adjust) / 100).max(3)).unwrap_or(3)
    }

    /// A new crew member. `quality` (0 to 3) is how good the hold's gate is at drawing people;
    /// `trade` is fixed or drawn at random. The id is set by the clan.
    #[must_use]
    pub fn generate(cat: &Catalog, rng: &mut DoorRng, quality: u8, trade: Option<Trade>) -> Self {
        let trade = trade.unwrap_or_else(|| {
            Trade::ALL[usize::try_from(rng.below(u32::try_from(TRADES).unwrap_or(8))).unwrap_or(0)]
        });
        let pick = |list: &[String], rng: &mut DoorRng| -> String {
            let n = u32::try_from(list.len()).unwrap_or(1);
            list.get(usize::try_from(rng.below(n)).unwrap_or(0))
                .cloned()
                .unwrap_or_default()
        };
        let name = format!(
            "{} {}",
            pick(&cat.names.first, rng),
            pick(&cat.names.by, rng)
        );
        let mut attrs = [0u8; 3];
        for a in &mut attrs {
            *a = u8::try_from(rng.range(3, 6) + u32::from(quality))
                .unwrap_or(6)
                .min(10);
        }
        let lead = trade.attr().index();
        attrs[lead] = (attrs[lead] + 2).min(10);
        let mut xp = [0u16; TRADES];
        xp[trade.index()] = u16::try_from(4 + rng.below(12) + 4 * u32::from(quality)).unwrap_or(4);
        let hireable: Vec<&str> = cat
            .traits
            .iter()
            .filter(|t| !t.scar)
            .map(|t| t.id.as_str())
            .collect();
        let mut traits = Vec::new();
        let count = if rng.chance(2, 5) { 2 } else { 1 };
        while traits.len() < count && !hireable.is_empty() {
            let t =
                hireable[usize::try_from(rng.below(u32::try_from(hireable.len()).unwrap_or(1)))
                    .unwrap_or(0)];
            if !traits.iter().any(|x: &String| x == t) {
                traits.push(t.to_owned());
            }
        }
        Self {
            id: 0,
            name,
            trade,
            attrs,
            xp,
            traits,
            injuries: Vec::new(),
            age: u8::try_from(rng.range(17, 47)).unwrap_or(30),
            post: None,
            shifts: 0,
        }
    }

    /// Write to a save.
    pub fn write(&self, w: &mut Writer) {
        w.u16(self.id);
        w.str(&self.name);
        w.u8(u8::try_from(self.trade.index()).unwrap_or(0));
        for a in self.attrs {
            w.u8(a);
        }
        for x in self.xp {
            w.u16(x);
        }
        w.count(self.traits.len());
        for t in &self.traits {
            w.str(t);
        }
        w.count(self.injuries.len());
        for i in &self.injuries {
            w.str(&i.id);
            w.u8(i.days);
        }
        w.u8(self.age);
        w.u8(self.post.unwrap_or(u8::MAX));
        w.u16(self.shifts);
    }

    /// Read from a save.
    pub fn read(r: &mut Reader<'_>) -> Option<Self> {
        let id = r.u16()?;
        let name = r.str()?;
        let trade = *Trade::ALL.get(usize::from(r.u8()?))?;
        let mut attrs = [0u8; 3];
        for a in &mut attrs {
            *a = r.u8()?;
        }
        let mut xp = [0u16; TRADES];
        for x in &mut xp {
            *x = r.u16()?;
        }
        let traits = (0..r.u8()?).map(|_| r.str()).collect::<Option<Vec<_>>>()?;
        let injuries = (0..r.u8()?)
            .map(|_| {
                Some(Injured {
                    id: r.str()?,
                    days: r.u8()?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let age = r.u8()?;
        let post = match r.u8()? {
            u8::MAX => None,
            p => Some(p),
        };
        Some(Self {
            id,
            name,
            trade,
            attrs,
            xp,
            traits,
            injuries,
            age,
            post,
            shifts: r.u16()?,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn cat() -> Catalog {
        Catalog::embedded().unwrap()
    }

    fn crew(trade: Trade) -> Crew {
        let mut rng = DoorRng::from_seed(3);
        let mut c = Crew::generate(&cat(), &mut rng, 0, Some(trade));
        c.id = 1;
        c.traits.clear();
        c
    }

    #[test]
    fn a_generated_crew_member_is_valid_and_repeatable() {
        let cat = cat();
        for seed in 0..200 {
            let a = Crew::generate(&cat, &mut DoorRng::from_seed(seed), 1, None);
            assert_eq!(
                a,
                Crew::generate(&cat, &mut DoorRng::from_seed(seed), 1, None)
            );
            assert!(a.attrs.iter().all(|x| (1..=10).contains(x)));
            assert!((1..=2).contains(&a.traits.len()));
            assert!(a.traits.iter().all(|t| !cat.trait_def(t).unwrap().scar));
            assert!((17..=47).contains(&a.age));
            assert!(a.level(a.trade) >= 1);
            assert!(a.attrs[a.trade.attr().index()] >= 5);
            assert!(!a.name.is_empty() && a.name.contains(' '));
        }
    }

    #[test]
    fn levels_follow_xp_and_use_grows_them_faster_with_a_learning_trait() {
        let cat = cat();
        let mut c = crew(Trade::Smith);
        c.xp = [0; TRADES];
        assert_eq!(c.level(Trade::Smith), 0);
        c.xp[Trade::Smith.index()] = 4;
        assert_eq!(c.level(Trade::Smith), 1);
        c.xp[Trade::Smith.index()] = 36;
        assert_eq!(c.level(Trade::Smith), 3);
        c.xp[Trade::Smith.index()] = MAX_XP;
        assert_eq!(c.level(Trade::Smith), 10);
        let mut plain = crew(Trade::Smith);
        plain.xp = [0; TRADES];
        let mut quick = plain.clone();
        quick.traits.push("quick-study".into());
        for _ in 0..10 {
            plain.gain(&cat, Trade::Smith, 2);
            quick.gain(&cat, Trade::Smith, 2);
        }
        assert_eq!(plain.xp[Trade::Smith.index()], 20);
        assert_eq!(quick.xp[Trade::Smith.index()], 30);
        assert_eq!(plain.gain(&cat, Trade::Smith, 1), None);
        let mut c2 = crew(Trade::Smith);
        c2.xp[Trade::Smith.index()] = 3;
        assert_eq!(c2.gain(&cat, Trade::Smith, 1), Some(1));
    }

    #[test]
    fn power_rises_with_skill_trade_and_attribute_and_falls_with_injury_and_age() {
        let cat = cat();
        let mut c = crew(Trade::Raker);
        c.attrs = [6, 5, 5];
        c.xp = [0; TRADES];
        let own = c.power(&cat, Trade::Raker);
        assert!(own > c.power(&cat, Trade::Smith), "own trade beats another");
        c.xp[Trade::Raker.index()] = 36;
        assert!(c.power(&cat, Trade::Raker) > own, "skill raises it");
        let before = c.power(&cat, Trade::Raker);
        c.injuries.push(Injured {
            id: "gash".into(),
            days: 5,
        });
        assert!(c.power(&cat, Trade::Raker) < before, "injury lowers it");
        assert!(c.fit(&cat));
        c.injuries.push(Injured {
            id: "broken-leg".into(),
            days: 14,
        });
        assert!(!c.fit(&cat), "90% penalty is bedridden");
        c.injuries.clear();
        c.age = 57;
        assert_eq!(c.attr(Attr::Grit), 6 - 3);
        assert_eq!(c.attr(Attr::Wit), 5);
    }

    #[test]
    fn price_reflects_ability_and_traits() {
        let cat = cat();
        let mut c = crew(Trade::Raker);
        let base = c.price(&cat);
        c.traits.push("quick-study".into());
        assert!(c.price(&cat) > base);
        let mut d = crew(Trade::Raker);
        d.traits.push("clumsy".into());
        assert!(d.price(&cat) < base);
    }

    #[test]
    fn a_crew_member_round_trips() {
        let mut c = crew(Trade::Healer);
        c.traits = vec!["steady".into(), "limp".into()];
        c.injuries.push(Injured {
            id: "fever".into(),
            days: 3,
        });
        c.post = Some(4);
        let mut w = Writer::new();
        c.write(&mut w);
        let bytes = w.finish();
        assert_eq!(Crew::read(&mut Reader::new(&bytes)), Some(c));
        assert_eq!(
            Crew::read(&mut Reader::new(&bytes[..bytes.len() - 1])),
            None
        );
    }
}
