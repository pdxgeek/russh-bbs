//! Resources, stock, bundles of cost, trades and the places in the hold: the vocabulary the
//! rest of the engine uses.

use std::collections::BTreeMap;

use serde::Deserialize;

/// How many kinds of resource there are.
pub const RESOURCES: usize = 13;

/// A thing the clan can hold. Raw materials come from the marsh and the hold, goods are made
/// at workshops, and relics are found in the drowned towns. (`Fish` is the one raw material
/// the design doc does not list: it is what a hungry hold eats before it has a smokehouse.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Resource {
    /// Raked from the salt pans.
    Salt,
    /// Cut from the bogs; the hold's fuel.
    Peat,
    /// Cut from the reedbeds.
    Reed,
    /// Timber from the old coast.
    Timber,
    /// Iron salvaged from the drowned towns.
    Iron,
    /// Silt-glass, fused in the mud.
    SiltGlass,
    /// Fish from the flats.
    Fish,
    /// Rope.
    Rope,
    /// Preserved food; what the crew eat.
    Food,
    /// Boats.
    Boats,
    /// Tools.
    Tools,
    /// Arms.
    Arms,
    /// A relic of the old world.
    Relic,
}

/// Raw, goods or rare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceClass {
    /// Gathered.
    Raw,
    /// Crafted.
    Good,
    /// Found.
    Rare,
}

impl Resource {
    /// Every resource, in index order.
    pub const ALL: [Self; RESOURCES] = [
        Self::Salt,
        Self::Peat,
        Self::Reed,
        Self::Timber,
        Self::Iron,
        Self::SiltGlass,
        Self::Fish,
        Self::Rope,
        Self::Food,
        Self::Boats,
        Self::Tools,
        Self::Arms,
        Self::Relic,
    ];

    /// The position in [`Resource::ALL`] and in a [`Stock`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The id used in content tables (the same as the serde name).
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Salt => "salt",
            Self::Peat => "peat",
            Self::Reed => "reed",
            Self::Timber => "timber",
            Self::Iron => "iron",
            Self::SiltGlass => "silt-glass",
            Self::Fish => "fish",
            Self::Rope => "rope",
            Self::Food => "food",
            Self::Boats => "boats",
            Self::Tools => "tools",
            Self::Arms => "arms",
            Self::Relic => "relic",
        }
    }

    /// The resource with this content id.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.id() == id)
    }

    /// The display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Salt => "salt",
            Self::Peat => "peat",
            Self::Reed => "reed",
            Self::Timber => "timber",
            Self::Iron => "iron",
            Self::SiltGlass => "silt-glass",
            Self::Fish => "fish",
            Self::Rope => "rope",
            Self::Food => "preserved food",
            Self::Boats => "boats",
            Self::Tools => "tools",
            Self::Arms => "arms",
            Self::Relic => "relics",
        }
    }

    /// Raw, good or rare.
    #[must_use]
    pub const fn class(self) -> ResourceClass {
        match self {
            Self::Salt
            | Self::Peat
            | Self::Reed
            | Self::Timber
            | Self::Iron
            | Self::SiltGlass
            | Self::Fish => ResourceClass::Raw,
            Self::Rope | Self::Food | Self::Boats | Self::Tools | Self::Arms => ResourceClass::Good,
            Self::Relic => ResourceClass::Rare,
        }
    }

    /// The price, in tallies, the Tidecourt starts a season at.
    #[must_use]
    pub const fn base_price(self) -> u16 {
        match self {
            Self::Salt => 4,
            Self::Peat => 3,
            Self::Reed | Self::Fish => 2,
            Self::Timber => 6,
            Self::Iron => 10,
            Self::SiltGlass => 14,
            Self::Rope => 8,
            Self::Food => 5,
            Self::Boats => 60,
            Self::Tools => 20,
            Self::Arms => 30,
            Self::Relic => 120,
        }
    }
}

/// A clan's goods.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stock([u32; RESOURCES]);

impl Stock {
    /// How many of `r`.
    #[must_use]
    pub const fn get(&self, r: Resource) -> u32 {
        self.0[r.index()]
    }

    /// Add `n` of `r` (saturating).
    pub fn add(&mut self, r: Resource, n: u32) {
        let slot = &mut self.0[r.index()];
        *slot = slot.saturating_add(n);
    }

    /// Take up to `n` of `r`; returns how many were taken.
    pub fn take_up_to(&mut self, r: Resource, n: u32) -> u32 {
        let slot = &mut self.0[r.index()];
        let taken = (*slot).min(n);
        *slot -= taken;
        taken
    }

    /// The raw counts, in [`Resource::ALL`] order.
    #[must_use]
    pub const fn counts(&self) -> &[u32; RESOURCES] {
        &self.0
    }

    /// Build from raw counts.
    #[must_use]
    pub const fn from_counts(counts: [u32; RESOURCES]) -> Self {
        Self(counts)
    }
}

/// A price or a yield: some resources and some tallies. In a content table it is written as
/// `{ timber = 6, reed = 4, tallies = 10 }`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(try_from = "BTreeMap<String, u32>")]
pub struct Bundle {
    /// Resources.
    pub items: Vec<(Resource, u32)>,
    /// Tallies.
    pub tallies: u32,
}

impl TryFrom<BTreeMap<String, u32>> for Bundle {
    type Error = String;

    fn try_from(map: BTreeMap<String, u32>) -> Result<Self, String> {
        let mut bundle = Self::default();
        for (key, n) in map {
            if key == "tallies" {
                bundle.tallies = n;
            } else if let Some(r) = Resource::from_id(&key) {
                bundle.items.push((r, n));
            } else {
                return Err(format!("`{key}` is not a resource"));
            }
        }
        bundle.items.sort_by_key(|(r, _)| r.index());
        Ok(bundle)
    }
}

impl Bundle {
    /// Whether the bundle is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tallies == 0 && self.items.iter().all(|(_, n)| *n == 0)
    }

    /// The amount of `r` in the bundle.
    #[must_use]
    pub fn of(&self, r: Resource) -> u32 {
        self.items
            .iter()
            .filter(|(x, _)| *x == r)
            .map(|(_, n)| *n)
            .sum()
    }

    /// `Nx name` pieces for display.
    #[must_use]
    pub fn describe(&self) -> String {
        let mut parts: Vec<String> = self
            .items
            .iter()
            .filter(|(_, n)| *n > 0)
            .map(|(r, n)| format!("{n} {}", r.name()))
            .collect();
        if self.tallies > 0 {
            parts.push(format!("{} tallies", self.tallies));
        }
        if parts.is_empty() {
            "nothing".to_owned()
        } else {
            parts.join(", ")
        }
    }
}

/// The three building tracks of the hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Track {
    /// Huts, longhouse, infirmary: more crew, healing.
    Shelter,
    /// Salt pans, peat stacks, smithy, boatyard: production and crafting.
    Work,
    /// Palisade, tower, sluice, mudworks: raid resistance.
    Defence,
}

impl Track {
    /// Every track.
    pub const ALL: [Self; 3] = [Self::Shelter, Self::Work, Self::Defence];

    /// Position in [`Track::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Shelter => "shelter",
            Self::Work => "work",
            Self::Defence => "defence",
        }
    }

    /// The trades whose crew speed up building on this track.
    #[must_use]
    pub const fn builders(self) -> [Trade; 2] {
        match self {
            Self::Shelter => [Trade::Wader, Trade::Boatwright],
            Self::Work => [Trade::Smith, Trade::Boatwright],
            Self::Defence => [Trade::Fighter, Trade::Smith],
        }
    }
}

/// A crew member's trade. A skill is named for the trade that uses it ([`Skill`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trade {
    /// Knows the flats; wades where others swim.
    Wader,
    /// Rakes salt and cuts peat.
    Raker,
    /// Salvages the drowned towns.
    Salvager,
    /// Fights.
    Fighter,
    /// Mends the injured.
    Healer,
    /// Builds and mends boats.
    Boatwright,
    /// Works iron.
    Smith,
    /// Reads omens in the reeds.
    ReedWitch,
}

/// A skill grows with use. There is one per trade, and any crew member may learn any of them.
pub type Skill = Trade;

/// How many trades (and skills) there are.
pub const TRADES: usize = 8;

impl Trade {
    /// Every trade, in index order.
    pub const ALL: [Self; TRADES] = [
        Self::Wader,
        Self::Raker,
        Self::Salvager,
        Self::Fighter,
        Self::Healer,
        Self::Boatwright,
        Self::Smith,
        Self::ReedWitch,
    ];

    /// Position in [`Trade::ALL`] and in a crew member's skills.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The content id.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Wader => "wader",
            Self::Raker => "raker",
            Self::Salvager => "salvager",
            Self::Fighter => "fighter",
            Self::Healer => "healer",
            Self::Boatwright => "boatwright",
            Self::Smith => "smith",
            Self::ReedWitch => "reed-witch",
        }
    }

    /// The display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Wader => "wader",
            Self::Raker => "raker",
            Self::Salvager => "salvager",
            Self::Fighter => "fighter",
            Self::Healer => "healer",
            Self::Boatwright => "boatwright",
            Self::Smith => "smith",
            Self::ReedWitch => "reed-witch",
        }
    }

    /// The attribute this trade leans on.
    #[must_use]
    pub const fn attr(self) -> Attr {
        match self {
            Self::Wader | Self::Raker | Self::Fighter | Self::Smith => Attr::Grit,
            Self::Healer | Self::ReedWitch => Attr::Wit,
            Self::Salvager | Self::Boatwright => Attr::Reach,
        }
    }
}

/// A crew member's three attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attr {
    /// Endurance and force.
    Grit,
    /// Judgement and learning.
    Wit,
    /// Range, balance and skill with the hands.
    Reach,
}

impl Attr {
    /// Every attribute.
    pub const ALL: [Self; 3] = [Self::Grit, Self::Wit, Self::Reach];

    /// Position in a crew member's attributes.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Grit => "grit",
            Self::Wit => "wit",
            Self::Reach => "reach",
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn ids_match_the_serde_names_and_indexes_match_position() {
        #[derive(Deserialize)]
        struct R {
            r: Resource,
        }
        #[derive(Deserialize)]
        struct T {
            t: Trade,
        }
        for (i, r) in Resource::ALL.into_iter().enumerate() {
            assert_eq!(r.index(), i);
            assert_eq!(Resource::from_id(r.id()), Some(r));
            let w: R = toml::from_str(&format!("r = \"{}\"", r.id())).unwrap();
            assert_eq!(w.r, r);
        }
        for (i, t) in Trade::ALL.into_iter().enumerate() {
            assert_eq!(t.index(), i);
            let w: T = toml::from_str(&format!("t = \"{}\"", t.id())).unwrap();
            assert_eq!(w.t, t);
        }
    }

    #[test]
    fn bundles_parse_resources_and_tallies_and_refuse_unknown_keys() {
        #[derive(Deserialize)]
        struct W {
            c: Bundle,
        }
        let w: W = toml::from_str("c = { reed = 4, timber = 6, tallies = 10 }").unwrap();
        assert_eq!(w.c.of(Resource::Timber), 6);
        assert_eq!(w.c.tallies, 10);
        assert_eq!(w.c.describe(), "4 reed, 6 timber, 10 tallies");
        assert!(toml::from_str::<W>("c = { gold = 1 }").is_err());
        assert!(Bundle::default().is_empty());
    }

    #[test]
    fn stock_saturates_and_takes_what_exists() {
        let mut s = Stock::default();
        s.add(Resource::Salt, u32::MAX);
        s.add(Resource::Salt, 5);
        assert_eq!(s.get(Resource::Salt), u32::MAX);
        let mut t = Stock::default();
        t.add(Resource::Peat, 3);
        assert_eq!(t.take_up_to(Resource::Peat, 10), 3);
        assert_eq!(t.get(Resource::Peat), 0);
    }
}
