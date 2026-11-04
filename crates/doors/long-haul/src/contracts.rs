//! Jobs: delivery, passenger runs, bounties, escorts and surveys, posted by five factions.
//!
//! A station's **board** is a pure function of the season, the station and the three-day epoch,
//! so every captain sees the same offers and they cannot be re-rolled by reconnecting. Each
//! faction with an office at the station posts its own kind of work, and **standing decides what
//! is offered**: a faction at -40 or worse posts nothing for the captain; the better-paid and
//! riskier jobs (bounties, escorts, premium contracts) need standing to be shown at all. Taking
//! a contract is a promise: deliver by the due day or lose standing, goods and the deposit of
//! trust.

use russh_bbs_door::DoorRng;
use russh_bbs_door::content::Table;
use serde::Deserialize;

use crate::codec::{Reader, Writer};
use crate::content::{Content, Faction, GoodClass};
use crate::crew::{GoalKind, epoch_of};
use crate::engine::Env;
use crate::events::{Event, Refusal};
use crate::frontier::Frontier;
use crate::player::{Cause, Player};
use crate::skills::Skill;

/// Most contracts a captain can hold.
pub const MAX_CONTRACTS: usize = 4;
/// Offers per office per epoch.
pub const OFFERS_PER_OFFICE: usize = 3;
/// Standing at which a faction's best work (and a better rate) opens.
pub const PREMIUM_STANDING: i16 = 25;
/// At or below this standing a faction's office posts nothing.
pub const OFFICE_SHUT: i16 = -40;

/// A kind of job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Haul goods the client loads.
    Delivery,
    /// Carry passengers (they have their own events).
    Passenger,
    /// Wreck a named kind of ship.
    Bounty,
    /// See a client's freighter safely to a station.
    Escort,
    /// Survey a system and bring the data home.
    Survey,
}

impl Kind {
    /// Every kind.
    pub const ALL: [Self; 5] = [
        Self::Delivery,
        Self::Passenger,
        Self::Bounty,
        Self::Escort,
        Self::Survey,
    ];

    /// Save position.
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::Delivery => 0,
            Self::Passenger => 1,
            Self::Bounty => 2,
            Self::Escort => 3,
            Self::Survey => 4,
        }
    }

    /// The kind at a position.
    #[must_use]
    pub fn from_index(i: u8) -> Option<Self> {
        Self::ALL.get(usize::from(i)).copied()
    }

    /// Display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Delivery => "Delivery",
            Self::Passenger => "Passengers",
            Self::Bounty => "Bounty",
            Self::Escort => "Escort",
            Self::Survey => "Survey",
        }
    }
}

/// A passenger kind: who they are and what they like.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PassengerDef {
    /// Stable id (events refer to it).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Who sends them.
    pub faction: Faction,
    /// How many travel together (low, high).
    pub count: [u8; 2],
    /// A line about them.
    pub blurb: String,
}

/// A line a client says when posting a job.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineDef {
    /// Which kind of job.
    pub kind: Kind,
    /// Who says it.
    pub faction: Faction,
    /// The words. `{to}` is the destination.
    pub text: String,
}

/// The job tables.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobTable {
    /// Passenger kinds.
    pub passenger: Vec<PassengerDef>,
    /// Client lines.
    pub line: Vec<LineDef>,
}

impl Table for JobTable {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        crate::content::duplicates(
            self.passenger.iter().map(|d| d.id.as_str()),
            "passenger",
            &mut p,
        );
        if self.passenger.len() < 5 || self.passenger.len() > 24 {
            p.push("between 5 and 24 passenger kinds".into());
        }
        for d in &self.passenger {
            if !crate::content::text_ok(&d.name, 28)
                || !crate::content::text_ok(&d.blurb, 100)
                || d.count[0] == 0
                || d.count[0] > d.count[1]
                || d.count[1] > 6
            {
                p.push(format!(
                    "passenger `{}`: name <= 28, blurb <= 100, count 1 to 6",
                    d.id
                ));
            }
        }
        for l in &self.line {
            if !crate::content::text_ok(&l.text, 120) {
                p.push("a client line must be 1 to 120 plain characters".into());
            }
        }
        for faction in Faction::ALL {
            for kind in Kind::ALL {
                if offers_of(faction, kind)
                    && !self
                        .line
                        .iter()
                        .any(|l| l.faction == faction && l.kind == kind)
                {
                    p.push(format!(
                        "no client line for {} {}",
                        faction.name(),
                        kind.name()
                    ));
                }
            }
        }
        p
    }
}

/// Whether a faction posts a kind of job at all.
#[must_use]
pub const fn offers_of(faction: Faction, kind: Kind) -> bool {
    matches!(
        (faction, kind),
        (
            Faction::League,
            Kind::Delivery | Kind::Escort | Kind::Bounty | Kind::Passenger
        ) | (
            Faction::Miners,
            Kind::Delivery | Kind::Escort | Kind::Passenger
        ) | (
            Faction::Free,
            Kind::Delivery | Kind::Passenger | Kind::Bounty
        ) | (Faction::Guild, Kind::Survey | Kind::Delivery)
            | (Faction::Church, Kind::Passenger | Kind::Delivery)
    )
}

/// The standing a kind of job needs before it is shown.
const fn min_standing(kind: Kind) -> i16 {
    match kind {
        Kind::Delivery | Kind::Passenger => -39,
        Kind::Survey => 0,
        Kind::Escort => 10,
        Kind::Bounty => 15,
    }
}

/// An accepted (or offered) contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    /// The kind.
    pub kind: Kind,
    /// Who posted it.
    pub faction: Faction,
    /// The station it was taken at.
    pub from: u16,
    /// The station it must be brought to.
    pub to: u16,
    /// Delivery: the good.
    pub good: u8,
    /// Delivery: tons carried.
    pub tons: u16,
    /// Passenger: the passenger kind. Bounty: the enemy type.
    pub pax: u8,
    /// Passengers aboard.
    pub count: u8,
    /// Survey: the system.
    pub target: u16,
    /// Credits on completion.
    pub reward: u32,
    /// The last real day to finish.
    pub due: i64,
    /// Bounty and survey: 1 when the work is done. Escort: the ward's hull percent.
    pub progress: u8,
    /// Passengers' contentment, 0 to 10.
    pub mood: u8,
}

impl Contract {
    /// Write to a save.
    pub fn write(&self, w: &mut Writer) {
        w.u8(self.kind.index());
        w.u8(u8::try_from(self.faction.index()).unwrap_or(0));
        w.u16(self.from);
        w.u16(self.to);
        w.u8(self.good);
        w.u16(self.tons);
        w.u8(self.pax);
        w.u8(self.count);
        w.u16(self.target);
        w.u32(self.reward);
        w.i64(self.due);
        w.u8(self.progress);
        w.u8(self.mood);
    }

    /// Read from a save, or `None` if anything is out of range.
    pub fn read(r: &mut Reader<'_>, content: &Content, frontier: &Frontier) -> Option<Self> {
        let kind = Kind::from_index(r.u8()?)?;
        let faction = *Faction::ALL.get(usize::from(r.u8()?))?;
        let from = r.u16()?;
        let to = r.u16()?;
        let good = r.u8()?;
        let tons = r.u16()?;
        let pax = r.u8()?;
        let count = r.u8()?;
        let target = r.u16()?;
        let reward = r.u32()?;
        let due = r.i64()?;
        let progress = r.u8()?;
        let mood = r.u8()?.min(10);
        if frontier.station(from).is_none()
            || frontier.station(to).is_none()
            || frontier.system(target).is_none()
            || usize::from(good) >= content.goods.len()
            || (kind == Kind::Passenger && usize::from(pax) >= content.jobs.passenger.len())
            || (kind == Kind::Bounty && usize::from(pax) >= content.enemies.enemy.len())
        {
            return None;
        }
        Some(Self {
            kind,
            faction,
            from,
            to,
            good,
            tons,
            pax,
            count,
            target,
            reward,
            due,
            progress,
            mood,
        })
    }
}

/// A job on a board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The contract as it would be taken.
    pub contract: Contract,
    /// The key that marks it taken.
    pub key: u64,
    /// Standing the captain lacks to see the details (`None` if open to them).
    pub locked: Option<i16>,
    /// The client's words.
    pub line: String,
}

fn offer_key(station: u16, epoch: i64, faction: Faction, slot: usize) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in b"co"
        .iter()
        .copied()
        .chain(station.to_le_bytes())
        .chain(epoch.to_le_bytes())
        .chain([
            u8::try_from(faction.index()).unwrap_or(0),
            u8::try_from(slot).unwrap_or(0),
        ])
    {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// The factions with an office at a station: its owner, the Guild at research stations, and a
/// Quiet Church chapel at about a third of them.
#[must_use]
pub fn offices(content: &Content, frontier: &Frontier, station: u16) -> Vec<Faction> {
    let Some(st) = frontier.station(station) else {
        return Vec::new();
    };
    let eco = &content.economies[usize::from(st.economy)];
    let mut out = vec![eco.faction];
    if eco.id == "research" {
        out.push(Faction::Guild);
    }
    if station.is_multiple_of(3) {
        out.push(Faction::Church);
    }
    out
}

fn weights(faction: Faction) -> Vec<(Kind, u32)> {
    let table: &[(Kind, u32)] = match faction {
        Faction::League => &[
            (Kind::Delivery, 3),
            (Kind::Escort, 2),
            (Kind::Bounty, 2),
            (Kind::Passenger, 1),
        ],
        Faction::Miners => &[(Kind::Delivery, 4), (Kind::Escort, 2), (Kind::Passenger, 1)],
        Faction::Free => &[(Kind::Delivery, 3), (Kind::Passenger, 2), (Kind::Bounty, 1)],
        Faction::Guild => &[(Kind::Survey, 4), (Kind::Delivery, 1)],
        Faction::Church => &[(Kind::Passenger, 3), (Kind::Delivery, 1)],
    };
    table.to_vec()
}

/// The board at a station today for a captain with these standings.
#[must_use]
pub fn board(
    content: &Content,
    frontier: &Frontier,
    station: u16,
    today: i64,
    standing: &[i16; crate::content::FACTIONS],
) -> Vec<Offer> {
    let Some(st) = frontier.station(station) else {
        return Vec::new();
    };
    let epoch = epoch_of(today);
    let hops = frontier.hops_from(st.system);
    let mut out = Vec::new();
    for faction in offices(content, frontier, station) {
        let rep = standing[faction.index()];
        if rep <= OFFICE_SHUT {
            continue;
        }
        let count = if faction == Faction::Church || faction == Faction::Guild {
            OFFERS_PER_OFFICE - 1
        } else {
            OFFERS_PER_OFFICE
        };
        for slot in 0..count {
            let mut rng = DoorRng::from_label(
                &format!(
                    "lh-board-{}-{station}-{}-{slot}",
                    frontier.season,
                    faction.index()
                ),
                epoch,
            );
            if let Some(offer) = make_offer(
                content, frontier, station, today, faction, slot, rep, &hops, &mut rng,
            ) {
                out.push(offer);
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn make_offer(
    content: &Content,
    frontier: &Frontier,
    station: u16,
    today: i64,
    faction: Faction,
    slot: usize,
    rep: i16,
    hops: &[u8],
    rng: &mut DoorRng,
) -> Option<Offer> {
    let st = frontier.station(station)?;
    // Pick a kind by the faction's weights.
    let table = weights(faction);
    let total: u32 = table.iter().map(|(_, w)| w).sum();
    let mut roll = rng.below(total);
    let mut kind = table[0].0;
    for (k, w) in &table {
        if roll < *w {
            kind = *k;
            break;
        }
        roll -= w;
    }
    // A destination 1 to 4 hops away: a station in a system someone has charted.
    let near: Vec<u16> = frontier
        .stations
        .iter()
        .filter(|s| {
            let h = hops.get(usize::from(s.system)).copied().unwrap_or(u8::MAX);
            s.id != station
                && (1..=4).contains(&h)
                && !frontier.systems[usize::from(s.system)].uncharted
        })
        .map(|s| s.id)
        .collect();
    if near.is_empty() {
        return None;
    }
    let to = near[rng.below(u32::try_from(near.len()).unwrap_or(1)) as usize];
    let to_sys = frontier.station(to)?.system;
    let spans = frontier.distance(st.system, to_sys);
    let route_hops = u32::from(hops.get(usize::from(to_sys)).copied().unwrap_or(1)).max(1);
    let due = today + 3 + i64::from(route_hops) * 2;
    let premium = rep >= PREMIUM_STANDING;
    let mut c = Contract {
        kind,
        faction,
        from: station,
        to,
        good: 0,
        tons: 0,
        pax: 0,
        count: 0,
        target: to_sys,
        reward: 0,
        due,
        progress: 0,
        mood: 7,
    };
    let spans32 = spans.max(10);
    match kind {
        Kind::Delivery => {
            let contraband = faction == Faction::Free;
            let goods: Vec<usize> = content
                .goods
                .iter()
                .enumerate()
                .filter(|(_, g)| (g.class == GoodClass::Contraband) == contraband)
                .map(|(i, _)| i)
                .collect();
            let gi = goods[rng.below(u32::try_from(goods.len()).unwrap_or(1)) as usize];
            let tons = u16::try_from(rng.range(6, 22)).unwrap_or(10);
            c.good = u8::try_from(gi).unwrap_or(0);
            c.tons = tons;
            let base = i64::from(content.goods[gi].base);
            c.reward = u32::try_from(
                200 + i64::from(spans32) * i64::from(tons) * 2 / 5
                    + base * i64::from(tons) / 8
                    + if contraband { 350 } else { 0 },
            )
            .unwrap_or(u32::MAX);
        }
        Kind::Passenger => {
            let opts: Vec<usize> = content
                .jobs
                .passenger
                .iter()
                .enumerate()
                .filter(|(_, d)| d.faction == faction)
                .map(|(i, _)| i)
                .collect();
            let opts = if opts.is_empty() {
                (0..content.jobs.passenger.len()).collect()
            } else {
                opts
            };
            let pi = opts[rng.below(u32::try_from(opts.len()).unwrap_or(1)) as usize];
            let def = &content.jobs.passenger[pi];
            c.pax = u8::try_from(pi).unwrap_or(0);
            c.count = u8::try_from(rng.range(u32::from(def.count[0]), u32::from(def.count[1])))
                .unwrap_or(1);
            c.reward = 150 + spans32 * u32::from(c.count) * 5;
        }
        Kind::Bounty => {
            let targets: Vec<usize> = content
                .enemies
                .enemy
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    // The League pays for pirates; the Free Captains pay for cutters.
                    (faction == Faction::League && e.faction == Faction::Free)
                        || (faction == Faction::Free && e.faction == Faction::League)
                })
                .map(|(i, _)| i)
                .collect();
            if targets.is_empty() {
                return None;
            }
            let ti = targets[rng.below(u32::try_from(targets.len()).unwrap_or(1)) as usize];
            c.pax = u8::try_from(ti).unwrap_or(0);
            c.to = station;
            c.target = st.system;
            c.due = today + 8;
            c.reward = 450 + u32::from(content.enemies.enemy[ti].hull) * 9;
        }
        Kind::Escort => {
            c.progress = 100;
            c.reward = 350 + spans32 * 14;
        }
        Kind::Survey => {
            // An unscanned system a few hops out; it need not be uncharted, a deep scan counts.
            let cands: Vec<u16> = frontier
                .systems
                .iter()
                .filter(|s| {
                    let h = hops.get(usize::from(s.id)).copied().unwrap_or(u8::MAX);
                    (2..=4).contains(&h) && s.richness >= 3
                })
                .map(|s| s.id)
                .collect();
            if cands.is_empty() {
                return None;
            }
            let target = cands[rng.below(u32::try_from(cands.len()).unwrap_or(1)) as usize];
            c.target = target;
            c.to = station;
            c.due = today + 5 + i64::from(hops.get(usize::from(target)).copied().unwrap_or(2)) * 2;
            c.reward = 500 + u32::from(frontier.systems[usize::from(target)].richness) * 110;
        }
    }
    // Jobs carry no purchase risk, so they pay a little under what a trader clears.
    c.reward = c.reward * 80 / 100;
    if premium {
        c.reward = c.reward * 135 / 100;
    }
    let need = min_standing(kind);
    let locked = (rep < need).then_some(need);
    let lines: Vec<&LineDef> = content
        .jobs
        .line
        .iter()
        .filter(|l| l.faction == faction && l.kind == kind)
        .collect();
    let line = lines
        .get(rng.below(u32::try_from(lines.len().max(1)).unwrap_or(1)) as usize)
        .map(|l| l.text.clone())
        .unwrap_or_default();
    Some(Offer {
        key: offer_key(station, epoch_of(today), faction, slot),
        contract: c,
        locked,
        line,
    })
}

impl Player {
    /// Tons of client cargo aboard.
    #[must_use]
    pub fn contract_tons(&self) -> u32 {
        self.contracts
            .iter()
            .filter(|c| c.kind == Kind::Delivery)
            .map(|c| u32::from(c.tons))
            .sum()
    }

    /// Tons of client contraband aboard.
    #[must_use]
    pub fn contract_contraband(&self, content: &Content) -> u32 {
        self.contracts
            .iter()
            .filter(|c| {
                c.kind == Kind::Delivery
                    && content
                        .goods
                        .get(usize::from(c.good))
                        .is_some_and(|g| g.class == GoodClass::Contraband)
            })
            .map(|c| u32::from(c.tons))
            .sum()
    }

    /// Passengers aboard.
    #[must_use]
    pub fn passenger_count(&self) -> u8 {
        self.contracts
            .iter()
            .filter(|c| c.kind == Kind::Passenger)
            .fold(0u8, |a, c| a.saturating_add(c.count))
    }

    /// Whether a passenger of kind `id` is aboard.
    #[must_use]
    pub fn carries_passenger(&self, content: &Content, id: &str) -> bool {
        self.contracts.iter().any(|c| {
            c.kind == Kind::Passenger
                && content
                    .jobs
                    .passenger
                    .get(usize::from(c.pax))
                    .is_some_and(|d| d.id == id)
        })
    }

    /// The board here, for the screen.
    #[must_use]
    pub fn board_here(&self, env: &Env<'_>) -> Vec<Offer> {
        let Some(station) = self.docked else {
            return Vec::new();
        };
        board(
            env.content,
            env.frontier,
            station,
            env.today,
            &self.standing,
        )
        .into_iter()
        .filter(|o| !self.taken.contains(&o.key))
        .collect()
    }

    /// Take offer number `index` of the board here.
    pub(crate) fn accept(&mut self, env: &Env<'_>, index: usize) -> Result<Vec<Event>, Refusal> {
        self.docked_at(env)?;
        let offers = self.board_here(env);
        let offer = offers.get(index).ok_or(Refusal::NoSuchThing)?;
        if let Some(need) = offer.locked {
            return Err(Refusal::NeedStanding {
                faction: offer.contract.faction,
                need,
            });
        }
        if self.contracts.len() >= MAX_CONTRACTS {
            return Err(Refusal::TooManyJobs);
        }
        let c = &offer.contract;
        if c.kind == Kind::Delivery {
            let stats = self.stats(env.content);
            let free = u32::from(stats.cargo_cap).saturating_sub(self.hold_used());
            if free < u32::from(c.tons) {
                return Err(Refusal::HoldWouldOverflow);
            }
        }
        if c.kind == Kind::Passenger
            && self.berths_used() + usize::from(c.count) > self.berths(env.content)
        {
            return Err(Refusal::NoBerth);
        }
        if c.kind == Kind::Escort && self.contracts.iter().any(|x| x.kind == Kind::Escort) {
            return Err(Refusal::TooManyJobs);
        }
        let (key, contract) = (offer.key, offer.contract.clone());
        self.take_key(key);
        let kind = contract.kind;
        let to = contract.to;
        self.contracts.push(contract);
        Ok(vec![Event::Accepted { kind, to }])
    }

    /// Give a job up: standing falls with the client.
    pub(crate) fn abandon(&mut self, index: usize) -> Result<Vec<Event>, Refusal> {
        if index >= self.contracts.len() {
            return Err(Refusal::NoSuchThing);
        }
        let c = self.contracts.remove(index);
        let mut events = vec![Event::ContractFailed {
            kind: c.kind,
            faction: c.faction,
            late: false,
        }];
        self.shift_standing_with_rival(c.faction, -3, &mut events);
        Ok(events)
    }

    /// A system was scanned or charted: survey jobs for it are done.
    pub(crate) fn contract_scanned(&mut self, system: u16) {
        for c in &mut self.contracts {
            if c.kind == Kind::Survey && c.target == system {
                c.progress = 1;
            }
        }
    }

    /// Passengers' mood each real day; late jobs fail.
    pub(crate) fn contract_upkeep(&mut self, today: i64, elapsed: i64, events: &mut Vec<Event>) {
        let fed = self.rations_until >= today;
        for c in &mut self.contracts {
            if c.kind == Kind::Passenger {
                let days = u8::try_from(elapsed.clamp(0, 10)).unwrap_or(1);
                c.mood = if fed {
                    c.mood.min(10)
                } else {
                    c.mood.saturating_sub(days)
                };
            }
        }
        let mut failed = Vec::new();
        self.contracts.retain(|c| {
            let late = c.due < today;
            if late {
                failed.push((c.kind, c.faction));
            }
            !late
        });
        for (kind, faction) in failed {
            events.push(Event::ContractFailed {
                kind,
                faction,
                late: true,
            });
            self.shift_standing_with_rival(faction, -4, events);
        }
    }

    /// On docking at `station`: deliver what is owed here.
    pub(crate) fn contract_docked(&mut self, env: &Env<'_>, station: u16, events: &mut Vec<Event>) {
        let today = env.today;
        let mut i = 0;
        while i < self.contracts.len() {
            let c = &self.contracts[i];
            let done = c.to == station
                && c.due >= today
                && match c.kind {
                    Kind::Delivery | Kind::Passenger => true,
                    Kind::Bounty | Kind::Survey | Kind::Escort => c.progress >= 1,
                };
            if !done {
                i += 1;
                continue;
            }
            let c = self.contracts.remove(i);
            let mut reward = i64::from(c.reward);
            if c.kind == Kind::Passenger {
                reward = reward * (60 + i64::from(c.mood) * 8) / 100;
            }
            if c.kind == Kind::Escort {
                reward = reward * i64::from(c.progress.clamp(1, 100)) / 100;
            }
            self.credit(Cause::Contract, reward);
            self.stats.contracts_done = self.stats.contracts_done.saturating_add(1);
            events.push(Event::ContractDone {
                kind: c.kind,
                faction: c.faction,
                reward,
            });
            self.shift_standing_with_rival(c.faction, 2, events);
            self.goal_progress(GoalKind::Contracts);
            self.gain(Skill::Trade, 8, events);
        }
    }
}

/// Write a list of contracts.
pub fn write_all(w: &mut Writer, list: &[Contract]) {
    w.len(list.len());
    for c in list {
        c.write(w);
    }
}
