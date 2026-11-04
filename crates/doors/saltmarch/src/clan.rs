//! The clan: everything one player owns, and the save format that holds it.

use russh_bbs_door::DoorRng;

use crate::codec::{Reader, Writer};
use crate::content::{Catalog, EpitaphCause};
use crate::crew::{BOND_AT, Bond, Crew};
use crate::event::Event;
use crate::hold::{Hold, MAX_CREW};
use crate::map::{Map, SIZE};
use crate::trip::{MARKS_CAP, Trip};
use crate::types::{RESOURCES, Resource, Stock, Trade};

/// Longest clan name kept.
pub const NAME_MAX: usize = 24;

/// Entries the hall keeps per clan.
pub const HALL_CAP: usize = 40;

/// Bytes of the charted-tiles bitset.
const CHART_BYTES: usize = SIZE * SIZE / 8;

/// Game settings the sysop may change (S1 has the defaults only; a settings screen is later).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Tide-turns a clan gets each day.
    pub turns_per_day: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self { turns_per_day: 30 }
    }
}

/// A line in the hall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HallEntry {
    /// The season it was written in.
    pub season: u32,
    /// The text.
    pub text: String,
}

/// Counters for the hall and the balance runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// Shifts worked.
    pub shifts: u32,
    /// Things crafted.
    pub crafted: u32,
    /// Injuries suffered.
    pub injuries: u32,
    /// Tallies earned by selling.
    pub earned: u32,
    /// Bonds formed.
    pub bonds: u32,
    /// Crew lost (starved, left, retired).
    pub lost: u32,
    /// Expeditions set out on.
    pub departures: u32,
    /// Expeditions ended (any way).
    pub trips: u32,
    /// Expeditions that came home with the cargo.
    pub returned: u32,
    /// Expeditions cut off by the day running out.
    pub cut_off: u32,
    /// Expeditions swept by the flood.
    pub swept: u32,
    /// Skirmishes fought.
    pub fights: u32,
    /// Skirmishes won.
    pub fights_won: u32,
    /// Skirmishes lost (the party was routed).
    pub routed: u32,
    /// Parties that never came back.
    pub lost_parties: u32,
    /// Crew killed on expeditions.
    pub lost_on_trips: u32,
    /// Tallies' worth of cargo brought home (at the Tidecourt's opening prices).
    pub loot_value: u32,
}

/// How many expedition counters the save holds after the original six.
const EXPEDITION_COUNTERS: usize = 11;

impl Stats {
    fn expedition_counters(&self) -> [u32; EXPEDITION_COUNTERS] {
        [
            self.departures,
            self.trips,
            self.returned,
            self.cut_off,
            self.swept,
            self.fights,
            self.fights_won,
            self.routed,
            self.lost_parties,
            self.lost_on_trips,
            self.loot_value,
        ]
    }

    fn set_expedition_counters(&mut self, n: [u32; EXPEDITION_COUNTERS]) {
        [
            self.departures,
            self.trips,
            self.returned,
            self.cut_off,
            self.swept,
            self.fights,
            self.fights_won,
            self.routed,
            self.lost_parties,
            self.lost_on_trips,
            self.loot_value,
        ] = n;
    }
}

/// One clan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clan {
    /// The clan's name (checked by the board's name rules before it gets here).
    pub name: String,
    /// The door season it plays.
    pub season: u32,
    /// The seed of the map it plays on.
    pub seed: u64,
    /// Which hold site is theirs (index into the map's sites).
    pub site: u8,
    /// The day it was founded.
    pub founded: i64,
    /// The last day its night was run.
    pub last_day: i64,
    /// Tide-turns spent today.
    pub turns_used: u8,
    /// Tallies.
    pub tallies: u32,
    /// Goods.
    pub stock: Stock,
    /// 0 to 100.
    pub morale: u8,
    /// Nights in a row the larder fell short.
    pub starving: u8,
    /// The crew.
    pub crew: Vec<Crew>,
    /// The id the next crew member gets.
    pub next_id: u16,
    /// Bonds and bonds in the making.
    pub bonds: Vec<Bond>,
    /// The hold.
    pub hold: Hold,
    /// Recipe ids the clan knows.
    pub recipes: Vec<String>,
    /// Today's candidates at the gate.
    pub candidates: Vec<Crew>,
    /// Charted tiles, one bit each.
    pub charted: Vec<u8>,
    /// The hall.
    pub hall: Vec<HallEntry>,
    /// Seasons this clan has played (including this one).
    pub seasons_played: u16,
    /// Counters.
    pub stats: Stats,
    /// What the clan did, remembered: choices made and places done. Scenes read them.
    pub marks: Vec<String>,
    /// The expedition in progress, if the party is out.
    pub trip: Option<Trip>,
}

impl Clan {
    /// Found a clan on `site` of the map.
    #[must_use]
    pub fn found(
        cat: &Catalog,
        map: &Map,
        name: &str,
        season: u32,
        site: u8,
        day: i64,
        rng: &mut DoorRng,
    ) -> (Self, Vec<Event>) {
        let mut clan = Self {
            name: name.chars().take(NAME_MAX).collect(),
            season,
            seed: map.seed,
            site,
            founded: day,
            last_day: day,
            turns_used: 0,
            tallies: 40,
            stock: Stock::default(),
            morale: 60,
            starving: 0,
            crew: Vec::new(),
            next_id: 1,
            bonds: Vec::new(),
            hold: Hold::default(),
            recipes: cat
                .recipes
                .iter()
                .filter(|r| r.learn == crate::content::Learn::Start)
                .map(|r| r.id.clone())
                .collect(),
            candidates: Vec::new(),
            charted: vec![0; CHART_BYTES],
            hall: Vec::new(),
            seasons_played: 1,
            stats: Stats::default(),
            marks: Vec::new(),
            trip: None,
        };
        for (r, n) in [
            (Resource::Timber, 12),
            (Resource::Reed, 10),
            (Resource::Peat, 8),
            (Resource::Food, 20),
            (Resource::Tools, 1),
        ] {
            clan.stock.add(r, n);
        }
        for trade in [Some(Trade::Wader), Some(Trade::Raker), None] {
            let mut member = Crew::generate(cat, rng, 0, trade);
            member.id = clan.take_id();
            clan.crew.push(member);
        }
        // The hold starts with a fish weir and a peat stack, worked by the wader and the raker,
        // so a clan that never comes back still eats for a while and keeps its fires.
        for (plot, (id, member)) in [("fish-weir", 0usize), ("peat-stack", 1)]
            .into_iter()
            .enumerate()
        {
            clan.hold.plots[plot] = Some(id.to_owned());
            clan.crew[member].post = Some(u8::try_from(plot).unwrap_or(0));
        }
        if let Some(&(x, y)) = map.sites.get(usize::from(site)) {
            clan.chart_around(usize::from(x), usize::from(y), 3);
        }
        clan.refresh_candidates(cat, rng);
        let events = vec![Event::Founded {
            name: clan.name.clone(),
        }];
        (clan, events)
    }

    /// Start the next season: the hall and the count of seasons carry over, nothing else does.
    #[must_use]
    pub fn next_season(
        &self,
        cat: &Catalog,
        map: &Map,
        season: u32,
        site: u8,
        day: i64,
        rng: &mut DoorRng,
    ) -> Self {
        let (mut fresh, _) = Self::found(cat, map, &self.name, season, site, day, rng);
        fresh.hall.clone_from(&self.hall);
        fresh.seasons_played = self.seasons_played.saturating_add(1);
        fresh
    }

    /// Which of the map's hold sites a clan of this name takes: a hash of the name, so two
    /// sessions agree without asking each other. Clans may share a site.
    #[must_use]
    pub fn site_for(name: &str, sites: usize) -> u8 {
        let mut hash: u32 = 2_166_136_261;
        for b in name.bytes() {
            hash = (hash ^ u32::from(b)).wrapping_mul(16_777_619);
        }
        u8::try_from(hash % u32::try_from(sites.max(1)).unwrap_or(1)).unwrap_or(0)
    }

    /// The id for the next crew member.
    pub fn take_id(&mut self) -> u16 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    /// Tide-turns left today.
    #[must_use]
    pub fn turns_left(&self, settings: &Settings) -> u8 {
        settings.turns_per_day.saturating_sub(self.turns_used)
    }

    /// A crew member by id.
    #[must_use]
    pub fn crew_by(&self, id: u16) -> Option<&Crew> {
        self.crew.iter().find(|c| c.id == id)
    }

    /// A crew member by id, mutably.
    pub fn crew_mut(&mut self, id: u16) -> Option<&mut Crew> {
        self.crew.iter_mut().find(|c| c.id == id)
    }

    /// Whether the clan holds all of `bundle`.
    #[must_use]
    pub fn can_pay(&self, bundle: &crate::types::Bundle) -> bool {
        self.tallies >= bundle.tallies && bundle.items.iter().all(|(r, n)| self.stock.get(*r) >= *n)
    }

    /// Take `bundle` from the clan; returns `false` and changes nothing if it cannot.
    pub fn pay(&mut self, bundle: &crate::types::Bundle) -> bool {
        if !self.can_pay(bundle) {
            return false;
        }
        self.tallies -= bundle.tallies;
        for (r, n) in &bundle.items {
            self.stock.take_up_to(*r, *n);
        }
        true
    }

    /// Mark a tile as charted; returns whether it was new.
    pub fn chart(&mut self, x: usize, y: usize) -> bool {
        if x >= SIZE || y >= SIZE {
            return false;
        }
        let i = y * SIZE + x;
        let (byte, bit) = (i / 8, 1u8 << (i % 8));
        let new = self.charted[byte] & bit == 0;
        self.charted[byte] |= bit;
        new
    }

    /// Whether a tile is charted.
    #[must_use]
    pub fn is_charted(&self, x: usize, y: usize) -> bool {
        x < SIZE && y < SIZE && {
            let i = y * SIZE + x;
            self.charted[i / 8] & (1 << (i % 8)) != 0
        }
    }

    /// Chart a square of tiles around a point; returns how many were new.
    pub fn chart_around(&mut self, cx: usize, cy: usize, radius: usize) -> u32 {
        let mut new = 0;
        for y in cy.saturating_sub(radius)..=(cy + radius).min(SIZE - 1) {
            for x in cx.saturating_sub(radius)..=(cx + radius).min(SIZE - 1) {
                new += u32::from(self.chart(x, y));
            }
        }
        new
    }

    /// How many tiles are charted.
    #[must_use]
    pub fn charted_count(&self) -> u32 {
        self.charted.iter().map(|b| b.count_ones()).sum()
    }

    /// Draw a fresh set of candidates at the gate. The gatehouse brings more of them, and a
    /// bigger hold brings better ones.
    pub fn refresh_candidates(&mut self, cat: &Catalog, rng: &mut DoorRng) {
        let extra = self.hold.total(cat, |b| u32::from(b.recruit));
        let count = 2 + usize::try_from(extra).unwrap_or(0);
        let quality = u8::try_from((self.hold.built(cat).count() / 4).min(3)).unwrap_or(3);
        self.candidates = (0..count)
            .map(|_| {
                let mut c = Crew::generate(cat, rng, quality, None);
                c.id = 0;
                c
            })
            .collect();
    }

    /// Add a bond point between two crew; returns an event when the pair become bonded.
    pub fn share_hardship(&mut self, a: u16, b: u16, points: u8) -> Option<(u16, u16)> {
        let (lo, hi) = (a.min(b), a.max(b));
        if lo == hi {
            return None;
        }
        let idx = self.bonds.iter().position(|x| x.a == lo && x.b == hi);
        let bond = if let Some(i) = idx {
            &mut self.bonds[i]
        } else {
            self.bonds.push(Bond {
                a: lo,
                b: hi,
                points: 0,
            });
            let last = self.bonds.len() - 1;
            &mut self.bonds[last]
        };
        let was = bond.bonded();
        bond.points = bond.points.saturating_add(points).min(BOND_AT + 8);
        (!was && bond.bonded()).then_some((lo, hi))
    }

    /// The bonded partners of `id`.
    #[must_use]
    pub fn partners(&self, id: u16) -> Vec<u16> {
        self.bonds
            .iter()
            .filter(|b| b.bonded() && b.has(id))
            .map(|b| if b.a == id { b.b } else { b.a })
            .collect()
    }

    /// How many bonded pairs lie wholly inside `ids`.
    #[must_use]
    pub fn bonded_pairs_in(&self, ids: &[u16]) -> usize {
        self.bonds
            .iter()
            .filter(|b| b.bonded() && ids.contains(&b.a) && ids.contains(&b.b))
            .count()
    }

    /// Remove a crew member. An epitaph goes in the hall for the ends that earn one; a bonded
    /// partner grieves (morale falls) and the pair's bond is gone either way.
    pub fn remove_crew(
        &mut self,
        cat: &Catalog,
        id: u16,
        cause: Option<EpitaphCause>,
        rng: &mut DoorRng,
        events: &mut Vec<Event>,
    ) {
        let Some(at) = self.crew.iter().position(|c| c.id == id) else {
            return;
        };
        let gone = self.crew.remove(at);
        for p in self.partners(id) {
            if let Some(partner) = self.crew_by(p) {
                events.push(Event::Grief {
                    name: partner.name.clone(),
                });
            }
            self.morale = self.morale.saturating_sub(10);
        }
        self.bonds.retain(|b| !b.has(id));
        self.stats.lost += 1;
        if let Some(cause) = cause {
            let lines: Vec<_> = cat.epitaphs.iter().filter(|e| e.cause == cause).collect();
            if !lines.is_empty() {
                let n = u32::try_from(lines.len()).unwrap_or(1);
                let line = lines[usize::try_from(rng.below(n)).unwrap_or(0)];
                let text = line
                    .text
                    .replace("{name}", &gone.name)
                    .replace("{trade}", gone.trade.name());
                self.hall.push(HallEntry {
                    season: self.season,
                    text,
                });
                if self.hall.len() > HALL_CAP {
                    self.hall.remove(0);
                }
            }
        }
    }

    /// Whether the clan remembers `mark`.
    #[must_use]
    pub fn has_mark(&self, mark: &str) -> bool {
        self.marks.iter().any(|m| m == mark)
    }

    /// Remember something. The oldest memory goes when there are too many.
    pub fn add_mark(&mut self, mark: &str) {
        if self.has_mark(mark) {
            return;
        }
        self.marks.push(mark.to_owned());
        if self.marks.len() > MARKS_CAP {
            self.marks.remove(0);
        }
    }

    /// Forget something.
    pub fn remove_mark(&mut self, mark: &str) {
        self.marks.retain(|m| m != mark);
    }

    /// A score for the balance runs and the hall (the league's renown is a later slice).
    #[must_use]
    pub fn score(&self, cat: &Catalog) -> u32 {
        let buildings = u32::try_from(self.hold.built(cat).count()).unwrap_or(0);
        let skill: u32 = self
            .crew
            .iter()
            .map(|c| {
                Trade::ALL
                    .iter()
                    .map(|t| u32::from(c.level(*t)))
                    .sum::<u32>()
            })
            .sum();
        let bonds = u32::try_from(self.bonds.iter().filter(|b| b.bonded()).count()).unwrap_or(0);
        let crew = u32::try_from(self.crew.len()).unwrap_or(0);
        buildings * 5 + skill + bonds * 3 + crew * 2 + self.tallies / 25
    }

    /// Encode as a save.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.str(&self.name);
        w.u32(self.season);
        w.u64(self.seed);
        w.u8(self.site);
        w.i64(self.founded);
        w.i64(self.last_day);
        w.u8(self.turns_used);
        w.u32(self.tallies);
        for n in self.stock.counts() {
            w.u32(*n);
        }
        w.u8(self.morale);
        w.u8(self.starving);
        w.u16(self.next_id);
        w.count(self.crew.len());
        for c in &self.crew {
            c.write(&mut w);
        }
        w.count(self.bonds.len());
        for b in &self.bonds {
            w.u16(b.a);
            w.u16(b.b);
            w.u8(b.points);
        }
        self.hold.write(&mut w);
        w.count(self.recipes.len());
        for r in &self.recipes {
            w.str(r);
        }
        w.count(self.candidates.len());
        for c in &self.candidates {
            c.write(&mut w);
        }
        w.raw(&self.charted);
        w.count(self.hall.len());
        for h in &self.hall {
            w.u32(h.season);
            w.str(&h.text);
        }
        w.u16(self.seasons_played);
        let s = &self.stats;
        for n in [s.shifts, s.crafted, s.injuries, s.earned, s.bonds, s.lost] {
            w.u32(n);
        }
        // Version 2: the expedition counters, the clan's marks and the trip in progress.
        for n in s.expedition_counters() {
            w.u32(n);
        }
        w.count(self.marks.len());
        for m in &self.marks {
            w.str(m);
        }
        match &self.trip {
            Some(t) => {
                w.bool(true);
                t.write(&mut w);
            }
            None => w.bool(false),
        }
        w.finish()
    }

    /// Bring a version 1 save to version 2: the new fields start empty.
    #[must_use]
    pub fn upgrade_v1(bytes: &[u8]) -> Vec<u8> {
        let mut out = bytes.to_vec();
        out.extend(std::iter::repeat_n(0u8, EXPEDITION_COUNTERS * 4));
        out.push(0); // no marks
        out.push(0); // no trip
        out
    }

    /// Decode a save, or `None` if the bytes are not one this version wrote.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader::new(bytes);
        let name = r.str()?;
        let season = r.u32()?;
        let seed = r.u64()?;
        let site = r.u8()?;
        let founded = r.i64()?;
        let last_day = r.i64()?;
        let turns_used = r.u8()?;
        let tallies = r.u32()?;
        let mut counts = [0u32; RESOURCES];
        for n in &mut counts {
            *n = r.u32()?;
        }
        let morale = r.u8()?;
        let starving = r.u8()?;
        let next_id = r.u16()?;
        let crew = (0..r.u8()?)
            .map(|_| Crew::read(&mut r))
            .collect::<Option<Vec<_>>>()?;
        let bonds = (0..r.u8()?)
            .map(|_| {
                Some(Bond {
                    a: r.u16()?,
                    b: r.u16()?,
                    points: r.u8()?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let hold = Hold::read(&mut r)?;
        let recipes = (0..r.u8()?).map(|_| r.str()).collect::<Option<Vec<_>>>()?;
        let candidates = (0..r.u8()?)
            .map(|_| Crew::read(&mut r))
            .collect::<Option<Vec<_>>>()?;
        let charted = r.take(CHART_BYTES)?.to_vec();
        let hall = (0..r.u8()?)
            .map(|_| {
                Some(HallEntry {
                    season: r.u32()?,
                    text: r.str()?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let seasons_played = r.u16()?;
        let mut stats = Stats {
            shifts: r.u32()?,
            crafted: r.u32()?,
            injuries: r.u32()?,
            earned: r.u32()?,
            bonds: r.u32()?,
            lost: r.u32()?,
            ..Stats::default()
        };
        let mut extra = [0u32; EXPEDITION_COUNTERS];
        for n in &mut extra {
            *n = r.u32()?;
        }
        stats.set_expedition_counters(extra);
        let marks = (0..r.u8()?).map(|_| r.str()).collect::<Option<Vec<_>>>()?;
        let trip = if r.bool()? {
            Some(Trip::read(&mut r)?)
        } else {
            None
        };
        (r.done() && crew.len() <= MAX_CREW && morale <= 100).then_some(Self {
            name,
            season,
            seed,
            site,
            founded,
            last_day,
            turns_used,
            tallies,
            stock: Stock::from_counts(counts),
            morale,
            starving,
            crew,
            next_id,
            bonds,
            hold,
            recipes,
            candidates,
            charted,
            hall,
            seasons_played,
            stats,
            marks,
            trip,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    pub(crate) fn fixture() -> (Catalog, Map, Clan) {
        let cat = Catalog::embedded().unwrap();
        let map = Map::generate(7, &cat.pois);
        let (clan, _) = Clan::found(
            &cat,
            &map,
            "Brindle",
            1,
            2,
            20_000,
            &mut DoorRng::from_seed(1),
        );
        (cat, map, clan)
    }

    #[test]
    fn a_founded_clan_is_playable_and_a_save_round_trips() {
        let (cat, _, clan) = fixture();
        assert_eq!(clan.crew.len(), 3);
        assert_eq!(clan.crew[0].trade, Trade::Wader);
        assert_eq!(clan.crew[1].trade, Trade::Raker);
        assert!(
            !clan.recipes.is_empty()
                || cat
                    .recipes
                    .iter()
                    .all(|r| r.learn != crate::content::Learn::Start)
        );
        assert!((2..=5).contains(&clan.candidates.len()));
        assert!(clan.charted_count() > 20);
        let bytes = clan.encode();
        assert!(bytes.len() < 6_000, "{}", bytes.len());
        assert_eq!(Clan::decode(&bytes), Some(clan));
    }

    #[test]
    fn decode_refuses_damaged_saves() {
        let (_, _, clan) = fixture();
        let bytes = clan.encode();
        assert_eq!(Clan::decode(&bytes[..bytes.len() - 1]), None);
        let mut extra = bytes.clone();
        extra.push(0);
        assert_eq!(Clan::decode(&extra), None);
        assert_eq!(Clan::decode(&[]), None);
    }

    #[test]
    fn charting_counts_new_tiles_once_and_stays_on_the_map() {
        let (_, _, mut clan) = fixture();
        let before = clan.charted_count();
        assert_eq!(clan.chart_around(0, 0, 2), 9);
        assert_eq!(clan.chart_around(0, 0, 2), 0);
        assert_eq!(clan.charted_count(), before + 9);
        assert!(!clan.chart(64, 0));
        assert!(clan.is_charted(2, 2) && !clan.is_charted(63, 63));
    }

    #[test]
    fn hardship_bonds_a_pair_once_and_a_death_costs_morale_and_leaves_an_epitaph() {
        let (cat, _, mut clan) = fixture();
        let (a, b) = (clan.crew[0].id, clan.crew[1].id);
        assert_eq!(clan.share_hardship(a, a, 3), None);
        assert_eq!(clan.share_hardship(b, a, 5), None);
        assert_eq!(clan.share_hardship(a, b, 3), Some((a, b)));
        assert_eq!(clan.share_hardship(a, b, 3), None, "already bonded");
        assert_eq!(clan.partners(a), vec![b]);
        assert_eq!(clan.bonded_pairs_in(&[a, b]), 1);
        assert_eq!(clan.bonded_pairs_in(&[a]), 0);
        let morale = clan.morale;
        let mut events = Vec::new();
        clan.remove_crew(
            &cat,
            a,
            Some(EpitaphCause::Lost),
            &mut DoorRng::from_seed(2),
            &mut events,
        );
        assert_eq!(clan.morale, morale - 10);
        assert_eq!(clan.bonds.len(), 0);
        assert!(matches!(events.as_slice(), [Event::Grief { .. }]));
        assert_eq!(clan.hall.len(), 1);
        assert!(clan.hall[0].text.contains("Lost") || !clan.hall[0].text.contains("{name}"));
        // Leaving without an epitaph writes nothing in the hall.
        clan.remove_crew(
            &cat,
            clan.crew[0].id,
            None,
            &mut DoorRng::from_seed(2),
            &mut events,
        );
        assert_eq!(clan.hall.len(), 1);
    }

    #[test]
    fn paying_is_all_or_nothing() {
        let (_, _, mut clan) = fixture();
        let bundle = crate::types::Bundle {
            items: vec![(Resource::Timber, 5)],
            tallies: 10,
        };
        assert!(clan.pay(&bundle));
        assert_eq!((clan.tallies, clan.stock.get(Resource::Timber)), (30, 7));
        let too_much = crate::types::Bundle {
            items: vec![(Resource::Timber, 5), (Resource::Iron, 1)],
            tallies: 1,
        };
        assert!(!clan.pay(&too_much));
        assert_eq!((clan.tallies, clan.stock.get(Resource::Timber)), (30, 7));
    }

    #[test]
    fn a_new_season_keeps_the_hall_and_nothing_else() {
        let (cat, map, mut clan) = fixture();
        clan.hall.push(HallEntry {
            season: 1,
            text: "Old Tam. Remembered.".into(),
        });
        clan.tallies = 900;
        let next = clan.next_season(&cat, &map, 2, 3, 20_060, &mut DoorRng::from_seed(5));
        assert_eq!((next.season, next.seasons_played, next.tallies), (2, 2, 40));
        assert_eq!(next.hall, clan.hall);
        assert_eq!(next.site, 3);
    }
}
