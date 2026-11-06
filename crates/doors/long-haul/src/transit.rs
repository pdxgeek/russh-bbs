//! Transit events: distress calls, pirates, inspections, drive faults, strange signals.
//!
//! Events are data (`content/transit.toml`). Each has **conditions** (where it can happen),
//! **mods** that scale its weight by the system, the cargo, the captain's standing and the crew,
//! and **options**. An option can carry **requirements** (a crew role at a skill, a standing, a
//! flag set by an earlier choice, cargo, a skill): a captain who does not meet them sees the
//! option closed and the reason, so two builds see different choices in the same scene. Every
//! event keeps at least one option with no requirement, so none is a dead end. An outcome is a
//! weighted draw, and an **engineer, a medic, a face** and so on can tilt the weights.

use russh_bbs_door::content::Table;
use serde::Deserialize;

use crate::content::{Content, Faction};
use crate::crew::Role;
use crate::engine::Env;
use crate::events::{Event, Refusal};
use crate::frontier::Frontier;
use crate::player::{Cause, Player};
use crate::scene::Scene;
use crate::skills::Skill;

/// Most named flags an event table may define (they live in one `u32`).
pub const MAX_FLAGS: usize = 32;

fn one() -> u8 {
    1
}

/// Who an effect lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum Who {
    /// A random fit hand.
    Random,
    /// The best hand of a role.
    Role(Role),
}

impl TryFrom<String> for Who {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        if s == "random" {
            return Ok(Self::Random);
        }
        Role::ALL
            .iter()
            .find(|r| r.name().eq_ignore_ascii_case(&s))
            .map(|r| Self::Role(*r))
            .ok_or_else(|| format!("unknown crew target `{s}`"))
    }
}

/// A condition on a captain, a ship or a system.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Req {
    /// A fit hand of this role with at least this skill.
    Role {
        /// The role.
        role: Role,
        /// Least skill.
        #[serde(default = "one")]
        min: u8,
    },
    /// A hand of this role who is sworn to the ship.
    Sworn {
        /// The role.
        role: Role,
    },
    /// At least this many crew.
    Crew {
        /// Least crew.
        min: u8,
    },
    /// No crew at all.
    Alone,
    /// The captain's own skill (crew help does not count here).
    Skill {
        /// Which.
        skill: Skill,
        /// Least level.
        min: u8,
    },
    /// Standing with a faction in a range.
    Standing {
        /// Which.
        faction: Faction,
        /// At least.
        #[serde(default)]
        min: Option<i16>,
        /// At most.
        #[serde(default)]
        max: Option<i16>,
    },
    /// A flag is set.
    Flag {
        /// Its name.
        name: String,
    },
    /// A flag is not set.
    NoFlag {
        /// Its name.
        name: String,
    },
    /// At least this many tons of contraband (cargo or client).
    Contraband {
        /// Tons.
        tons: u16,
    },
    /// At least this many tons of a good.
    Good {
        /// The good's id.
        good: String,
        /// Tons.
        tons: u16,
    },
    /// At least this much cash.
    Cash {
        /// Credits.
        min: i64,
    },
    /// A passenger of this kind (or any) is aboard.
    Passenger {
        /// A passenger kind's id, or none for any.
        #[serde(default)]
        pax: Option<String>,
    },
    /// The arrival system has this hazard (`none`, `nebula`, `radiation`, `debris`).
    Hazard {
        /// Its name.
        hazard: String,
    },
    /// The arrival system has a station of this economy.
    Economy {
        /// The economy's id.
        economy: String,
    },
    /// The system was just charted by this arrival.
    Fresh,
    /// The arrival system has a gas giant.
    Gas,
    /// At least this much weapon power.
    Weapons {
        /// Power.
        min: u8,
    },
    /// A working scanner of at least this grade.
    Scanner {
        /// Grade.
        min: u8,
    },
    /// The ship carries a contract of this kind.
    Contract {
        /// The kind.
        contract: crate::contracts::Kind,
    },
    /// The ship is damaged below this percent of its hull.
    Battered {
        /// Percent.
        below: u8,
    },
    /// The fuel tank is below this percent.
    Thirsty {
        /// Percent.
        below: u8,
    },
    /// The captain started as this background.
    Background {
        /// Its id.
        id: String,
    },
    /// The captain is about to play this step of an arc (it has done the one before).
    ArcAt {
        /// The arc's id.
        arc: String,
        /// The step, from 1.
        step: u8,
    },
    /// A branch of an arc was taken.
    ArcBranch {
        /// The arc's id.
        arc: String,
        /// The branch's name.
        branch: String,
    },
    /// At least this many days have passed since the arc's last step.
    ArcWait {
        /// The arc's id.
        arc: String,
        /// Days.
        days: u8,
    },
    /// The condition does not hold.
    Not {
        /// The condition.
        req: Box<Req>,
    },
}

/// What happens as a result.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    /// Gain or lose credits (a loss that the purse cannot cover goes on the loan).
    Credits {
        /// Credits.
        amount: i32,
    },
    /// Gain or lose fuel.
    Fuel {
        /// Units.
        amount: i16,
    },
    /// Gain or lose structure.
    Hull {
        /// Points.
        amount: i16,
    },
    /// Lose drive-days.
    Days {
        /// Days.
        amount: u8,
    },
    /// Wear on every module, tenths of a percent.
    Wear {
        /// Tenths.
        amount: u16,
    },
    /// Standing with a faction (its rival echoes a third).
    Standing {
        /// Which.
        faction: Faction,
        /// Change.
        amount: i16,
    },
    /// Gain (free) or lose tons of a good.
    Cargo {
        /// The good's id.
        good: String,
        /// Tons.
        tons: i16,
    },
    /// Lose a percentage of the contraband aboard.
    LoseContraband {
        /// Percent.
        pct: u8,
    },
    /// Lose a percentage of all cargo.
    LoseCargo {
        /// Percent.
        pct: u8,
    },
    /// Set a flag.
    Flag {
        /// Its name.
        name: String,
    },
    /// Clear a flag.
    ClearFlag {
        /// Its name.
        name: String,
    },
    /// Hurt a hand for some days.
    Hurt {
        /// Who.
        who: Who,
        /// Days.
        days: u8,
    },
    /// Lose a hand for good.
    Kill {
        /// Who.
        who: Who,
    },
    /// Move the crew's treatment mood.
    Treat {
        /// By how much.
        amount: i8,
    },
    /// Move the crew's danger mood.
    Scare {
        /// By how much.
        amount: i8,
    },
    /// Move the passengers' contentment.
    Content {
        /// By how much.
        amount: i8,
    },
    /// Experience in a captain skill.
    Xp {
        /// Which.
        skill: Skill,
        /// Points.
        amount: u32,
    },
    /// Start a ship fight.
    Combat {
        /// The enemy's id.
        enemy: String,
    },
    /// Start a boarding.
    Board {
        /// The derelict's id.
        derelict: String,
    },
    /// A stowaway signs on if there is a berth.
    Stowaway {
        /// Their role.
        role: Role,
    },
    /// A data lot for the Guild.
    Survey {
        /// Its value.
        value: u32,
    },
    /// Move an arc on one step.
    ArcStep {
        /// The arc's id.
        arc: String,
    },
    /// Take a branch of an arc.
    ArcBranch {
        /// The arc's id.
        arc: String,
        /// The branch's name.
        branch: String,
    },
    /// End an arc with one of its endings.
    Ending {
        /// The arc's id.
        arc: String,
        /// The ending's id.
        ending: String,
    },
}

/// A weight tilt on an outcome.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bonus {
    /// When it applies.
    pub req: Req,
    /// Weight added.
    pub add: u32,
}

/// One possible result of an option.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    /// Base weight.
    pub weight: u32,
    /// What the captain reads.
    pub text: String,
    /// What it does.
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// Tilts on the weight.
    #[serde(default)]
    pub bonus: Vec<Bonus>,
}

/// A choice in an event.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventOption {
    /// Short label.
    pub label: String,
    /// Requirements; empty means anyone can pick it.
    #[serde(default)]
    pub req: Vec<Req>,
    /// What the requirement is, in words, shown when it is not met.
    #[serde(default)]
    pub needs: String,
    /// The possible results.
    pub outcomes: Vec<Outcome>,
}

/// A weight scale.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mod {
    /// When it applies.
    pub req: Req,
    /// Per-mille of the weight when it does (500 halves it, 3000 triples it).
    pub pm: u32,
}

/// A transit event.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventDef {
    /// Stable id.
    pub id: String,
    /// Which family: distress, pirate, inspection, drive, signal, hazard, passenger, derelict,
    /// crew, trade or omen.
    pub family: String,
    /// Heading.
    pub title: String,
    /// The scene.
    pub text: String,
    /// Base weight.
    pub weight: u32,
    /// Hard conditions.
    #[serde(default)]
    pub when: Vec<Req>,
    /// Weight scales.
    #[serde(default)]
    pub mods: Vec<Mod>,
    /// A flag that makes it happen at most once to a captain (set when it fires).
    #[serde(default)]
    pub once: String,
    /// The arc this scene belongs to (arc scenes only).
    #[serde(default)]
    pub arc: String,
    /// Which step of the arc it is, from 1 (arc scenes only).
    #[serde(default)]
    pub step: u8,
    /// The choices.
    pub options: Vec<EventOption>,
}

/// The event tables.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitTable {
    /// Flag names (at most [`MAX_FLAGS`]).
    pub flags: Vec<String>,
    /// The events.
    pub event: Vec<EventDef>,
}

/// The families of event.
pub const FAMILIES: [&str; 12] = [
    "distress",
    "pirate",
    "inspection",
    "drive",
    "signal",
    "hazard",
    "passenger",
    "derelict",
    "crew",
    "trade",
    "omen",
    "arc",
];

impl TransitTable {
    /// The bit of a flag, if it exists.
    #[must_use]
    pub fn flag_bit(&self, name: &str) -> Option<u32> {
        self.flags.iter().position(|f| f == name).map(|i| 1u32 << i)
    }

    /// An event's index by id.
    #[must_use]
    pub fn event_ix(&self, id: &str) -> Option<usize> {
        self.event.iter().position(|e| e.id == id)
    }
}

impl Table for TransitTable {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        if self.flags.len() > MAX_FLAGS {
            p.push(format!("at most {MAX_FLAGS} flags"));
        }
        crate::content::duplicates(self.flags.iter().map(String::as_str), "flag", &mut p);
        crate::content::duplicates(self.event.iter().map(|e| e.id.as_str()), "event", &mut p);
        for e in &self.event {
            let id = &e.id;
            if !FAMILIES.contains(&e.family.as_str()) {
                p.push(format!("event `{id}`: unknown family `{}`", e.family));
            }
            if !crate::content::text_ok(&e.title, 40) || !crate::content::text_ok(&e.text, 360) {
                p.push(format!(
                    "event `{id}`: title <= 40, text <= 360 plain characters"
                ));
            }
            let top = if e.family == "arc" { 2000 } else { 100 };
            if e.weight == 0 || e.weight > top {
                p.push(format!("event `{id}`: weight 1 to {top}"));
            }
            if !e.once.is_empty() && self.flag_bit(&e.once).is_none() {
                p.push(format!("event `{id}`: unknown once-flag `{}`", e.once));
            }
            if e.options.is_empty() || e.options.len() > 5 {
                p.push(format!("event `{id}`: 1 to 5 options"));
            }
            if !e.options.iter().any(|o| o.req.is_empty()) {
                p.push(format!("event `{id}`: needs an option anyone can take"));
            }
            for r in e.when.iter().chain(e.mods.iter().map(|m| &m.req)) {
                self.check_req(id, r, &mut p);
            }
            for m in &e.mods {
                if m.pm == 0 || m.pm > 6000 {
                    p.push(format!("event `{id}`: a mod must be 1 to 6000 per-mille"));
                }
            }
            for o in &e.options {
                if !crate::content::text_ok(&o.label, 44) {
                    p.push(format!("event `{id}`: option label <= 44"));
                }
                if o.req.is_empty() != o.needs.is_empty() {
                    p.push(format!(
                        "event `{id}` option `{}`: a requirement and its `needs` words go together",
                        o.label
                    ));
                }
                if !o.needs.is_empty() && !crate::content::text_ok(&o.needs, 48) {
                    p.push(format!("event `{id}`: needs <= 48"));
                }
                for r in &o.req {
                    self.check_req(id, r, &mut p);
                }
                if o.outcomes.is_empty() || o.outcomes.iter().map(|x| x.weight).sum::<u32>() == 0 {
                    p.push(format!(
                        "event `{id}` option `{}`: needs weighted outcomes",
                        o.label
                    ));
                }
                for out in &o.outcomes {
                    if !crate::content::text_ok(&out.text, 300) {
                        p.push(format!("event `{id}`: outcome text <= 300"));
                    }
                    for b in &out.bonus {
                        self.check_req(id, &b.req, &mut p);
                    }
                    let scenes = out
                        .effects
                        .iter()
                        .filter(|f| matches!(f, Effect::Combat { .. } | Effect::Board { .. }))
                        .count();
                    if scenes > 1 {
                        p.push(format!("event `{id}`: an outcome may start one scene"));
                    }
                    for f in &out.effects {
                        self.check_effect(id, f, &mut p);
                    }
                }
            }
        }
        p
    }
}

impl TransitTable {
    fn check_req(&self, id: &str, r: &Req, p: &mut Vec<String>) {
        match r {
            Req::Flag { name } | Req::NoFlag { name } => {
                if self.flag_bit(name).is_none() {
                    p.push(format!("event `{id}`: unknown flag `{name}`"));
                }
            }
            Req::Hazard { hazard } => {
                if !["none", "nebula", "radiation", "debris"].contains(&hazard.as_str()) {
                    p.push(format!("event `{id}`: unknown hazard `{hazard}`"));
                }
            }
            Req::Not { req } => self.check_req(id, req, p),
            _ => {}
        }
    }

    fn check_effect(&self, id: &str, f: &Effect, p: &mut Vec<String>) {
        match f {
            Effect::Flag { name } | Effect::ClearFlag { name } => {
                if self.flag_bit(name).is_none() {
                    p.push(format!("event `{id}`: unknown flag `{name}`"));
                }
            }
            Effect::Credits { amount } if amount.unsigned_abs() > 5000 => {
                p.push(format!("event `{id}`: a credit effect over 5000"));
            }
            Effect::Fuel { amount } if amount.unsigned_abs() > 60 => {
                p.push(format!("event `{id}`: a fuel effect over 60"));
            }
            Effect::Hull { amount } if amount.unsigned_abs() > 60 => {
                p.push(format!("event `{id}`: a hull effect over 60"));
            }
            Effect::Days { amount } if *amount > 6 => {
                p.push(format!("event `{id}`: a day loss over 6"));
            }
            _ => {}
        }
    }
}

/// What requirements are checked against.
pub struct Where<'a> {
    /// The content.
    pub content: &'a Content,
    /// The map.
    pub frontier: &'a Frontier,
    /// The system the ship arrived in.
    pub system: u16,
    /// Whether this arrival just charted it.
    pub fresh: bool,
}

impl Player {
    /// Whether a requirement holds.
    #[must_use]
    pub fn meets(&self, w: &Where<'_>, req: &Req) -> bool {
        let content = w.content;
        match req {
            Req::Role { role, min } => self.has_role(*role, *min),
            Req::Sworn { role } => self.has_sworn(*role),
            Req::Crew { min } => self.crew.len() >= usize::from(*min),
            Req::Alone => self.crew.is_empty(),
            Req::Skill { skill, min } => self.level(*skill) >= *min,
            Req::Standing { faction, min, max } => {
                let s = self.standing[faction.index()];
                min.is_none_or(|m| s >= m) && max.is_none_or(|m| s <= m)
            }
            Req::Flag { name } => content
                .transit
                .flag_bit(name)
                .is_some_and(|b| self.flags & b != 0),
            Req::NoFlag { name } => content
                .transit
                .flag_bit(name)
                .is_none_or(|b| self.flags & b == 0),
            Req::Contraband { tons } => {
                self.ship.contraband_tons(content) + self.contract_contraband(content)
                    >= u32::from(*tons)
            }
            Req::Good { good, tons } => content
                .good_ix(good)
                .and_then(|g| u8::try_from(g).ok())
                .is_some_and(|g| self.ship.tons_of(g) >= *tons),
            Req::Cash { min } => self.cash >= *min,
            Req::Passenger { pax } => match pax {
                None => self.passenger_count() > 0,
                Some(id) => self.carries_passenger(content, id),
            },
            Req::Hazard { hazard } => w
                .frontier
                .system(w.system)
                .is_some_and(|s| s.hazard.id() == hazard),
            Req::Economy { economy } => w.frontier.system(w.system).is_some_and(|s| {
                s.stations.iter().any(|&st| {
                    w.frontier
                        .station(st)
                        .is_some_and(|st| content.economies[usize::from(st.economy)].id == *economy)
                })
            }),
            Req::Fresh => w.fresh,
            Req::Gas => w.frontier.system(w.system).is_some_and(|s| s.gas_giant),
            Req::Weapons { min } => self.stats(content).power >= *min,
            Req::Scanner { min } => self.stats(content).scan >= *min,
            Req::Contract { contract } => self.contracts.iter().any(|c| c.kind == *contract),
            Req::Battered { below } => {
                content
                    .hulls
                    .get(usize::from(self.ship.hull))
                    .is_some_and(|h| {
                        u32::from(self.ship.hull_hp) * 100
                            < u32::from(h.hull_points) * u32::from(*below)
                    })
            }
            Req::Thirsty { below } => {
                let cap = u32::from(self.stats(content).fuel_cap.max(1));
                u32::from(self.ship.fuel) * 100 < cap * u32::from(*below)
            }
            Req::Background { id } => content
                .backgrounds
                .get(usize::from(self.background))
                .is_some_and(|b| b.id == *id),
            Req::ArcAt { arc, step } => content
                .arc_ix(arc)
                .and_then(|i| self.social.arcs.get(i))
                .is_some_and(|a| !a.finished() && a.step + 1 == *step),
            Req::ArcBranch { arc, branch } => content
                .arc_ix(arc)
                .zip(content.arc_branch_bit(arc, branch))
                .and_then(|(i, bit)| self.social.arcs.get(i).map(|a| a.branches & bit != 0))
                .unwrap_or(false),
            Req::ArcWait { arc, days } => content
                .arc_ix(arc)
                .and_then(|i| self.social.arcs.get(i))
                .is_some_and(|a| self.last_day - a.day >= i64::from(*days)),
            Req::Not { req } => !self.meets(w, req),
        }
    }

    /// The first requirement of an option that is not met.
    #[must_use]
    pub fn option_closed(&self, w: &Where<'_>, option: &EventOption) -> Option<String> {
        option
            .req
            .iter()
            .any(|r| !self.meets(w, r))
            .then(|| option.needs.clone())
    }

    /// How likely a transit event is on a jump, per-mille.
    #[must_use]
    pub fn transit_chance(&self, w: &Where<'_>, dist: u16) -> u32 {
        let mut pm = 260 + u32::from(dist.min(40)) * 6;
        if w.fresh {
            pm += 80;
        }
        if w.frontier
            .system(w.system)
            .is_some_and(|s| s.hazard != crate::frontier::Hazard::None)
        {
            pm += 60;
        }
        if self
            .contracts
            .iter()
            .any(|c| c.kind == crate::contracts::Kind::Escort)
        {
            pm += 80;
        }
        if self.passenger_count() > 0 {
            pm += 40;
        }
        pm.min(700)
    }

    /// Pick an event for an arrival, by weight.
    #[must_use]
    pub fn pick_event(&self, w: &Where<'_>, roll: u32) -> Option<usize> {
        let weights = self.event_weights(w);
        let total: u64 = weights.iter().map(|(_, wt)| u64::from(*wt)).sum();
        if total == 0 {
            return None;
        }
        let mut at = u64::from(roll) % total;
        for (i, wt) in weights {
            if at < u64::from(wt) {
                return Some(i);
            }
            at -= u64::from(wt);
        }
        None
    }

    /// Every event that can fire here, with its weight (scaled by mods, in per-mille).
    #[must_use]
    pub fn event_weights(&self, w: &Where<'_>) -> Vec<(usize, u32)> {
        w.content
            .transit
            .event
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                if !e.once.is_empty()
                    && w.content
                        .transit
                        .flag_bit(&e.once)
                        .is_some_and(|b| self.flags & b != 0)
                {
                    return None;
                }
                if !e.when.iter().all(|r| self.meets(w, r)) {
                    return None;
                }
                let mut wt = u64::from(e.weight) * 1000;
                for m in &e.mods {
                    if self.meets(w, &m.req) {
                        wt = wt * u64::from(m.pm) / 1000;
                    }
                }
                let wt = u32::try_from(wt).unwrap_or(u32::MAX);
                (wt > 0).then_some((i, wt))
            })
            .collect()
    }

    /// Roll for an event on arrival, and open it as a scene.
    pub(crate) fn maybe_transit(
        &mut self,
        env: &mut Env<'_>,
        dist: u16,
        fresh: bool,
        events: &mut Vec<Event>,
    ) {
        let system = self.at;
        let chance = {
            let w = Where {
                content: env.content,
                frontier: env.frontier,
                system,
                fresh,
            };
            self.transit_chance(&w, dist)
        };
        if !env.rng.chance(chance, 1000) {
            return;
        }
        let roll = env.rng.next_u64();
        let w = Where {
            content: env.content,
            frontier: env.frontier,
            system,
            fresh,
        };
        let Some(ix) = self.pick_event(&w, u32::try_from(roll >> 32).unwrap_or(0)) else {
            return;
        };
        let def = &env.content.transit.event[ix];
        if !def.once.is_empty()
            && let Some(b) = env.content.transit.flag_bit(&def.once)
        {
            self.flags |= b;
        }
        self.stats.transits = self.stats.transits.saturating_add(1);
        events.push(Event::Transit {
            event: u16::try_from(ix).unwrap_or(0),
        });
        self.scene = Some(Scene::Event(u16::try_from(ix).unwrap_or(0)));
    }

    /// Pick option `n` of the open event.
    pub(crate) fn choose(&mut self, env: &mut Env<'_>, n: usize) -> Result<Vec<Event>, Refusal> {
        let Some(Scene::Event(ix)) = &self.scene else {
            return Err(Refusal::NoEvent);
        };
        let ix = usize::from(*ix);
        let content = env.content;
        let def = &content.transit.event[ix];
        let option = def.options.get(n).ok_or(Refusal::NoSuchThing)?;
        let w = Where {
            content,
            frontier: env.frontier,
            system: self.at,
            fresh: false,
        };
        if let Some(needs) = self.option_closed(&w, option) {
            return Err(Refusal::Needs(needs));
        }
        // Weighted draw, tilted by crew and skill.
        let weights: Vec<u32> = option
            .outcomes
            .iter()
            .map(|o| {
                o.weight
                    + o.bonus
                        .iter()
                        .filter(|b| self.meets(&w, &b.req))
                        .map(|b| b.add)
                        .sum::<u32>()
            })
            .collect();
        let total: u32 = weights.iter().sum();
        let mut roll = env.rng.below(total.max(1));
        let mut pick = 0;
        for (i, wt) in weights.iter().enumerate() {
            if roll < *wt {
                pick = i;
                break;
            }
            roll -= wt;
        }
        let outcome = &option.outcomes[pick];
        let mut events = vec![Event::Outcome {
            event: u16::try_from(ix).unwrap_or(0),
            option: u8::try_from(n).unwrap_or(0),
            outcome: u8::try_from(pick).unwrap_or(0),
        }];
        self.scene = None;
        // The role that carried the choice earns a use.
        for r in &option.req {
            if let Req::Role { role, .. } = r {
                self.train_role(*role, &mut events);
            }
        }
        self.apply_effects(env, &outcome.effects, &mut events);
        Ok(events)
    }

    /// Apply the effects of an outcome.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn apply_effects(
        &mut self,
        env: &mut Env<'_>,
        effects: &[Effect],
        events: &mut Vec<Event>,
    ) {
        let content = env.content;
        let mut scene: Option<&Effect> = None;
        for fx in effects {
            match fx {
                Effect::Credits { amount } => {
                    if *amount >= 0 {
                        self.credit(Cause::Event, i64::from(*amount));
                    } else {
                        self.pay_or_owe(Cause::Event, i64::from(amount.unsigned_abs()));
                    }
                }
                Effect::Fuel { amount } => {
                    let cap = self.stats(content).fuel_cap;
                    let now = i32::from(self.ship.fuel) + i32::from(*amount);
                    self.ship.fuel = u16::try_from(now.clamp(0, i32::from(cap))).unwrap_or(0);
                }
                Effect::Hull { amount } => {
                    let max = content
                        .hulls
                        .get(usize::from(self.ship.hull))
                        .map_or(1, |h| h.hull_points);
                    let now = i32::from(self.ship.hull_hp) + i32::from(*amount);
                    self.ship.hull_hp = u16::try_from(now.clamp(1, i32::from(max))).unwrap_or(1);
                }
                Effect::Days { amount } => self.spend_days(u32::from(*amount)),
                Effect::Wear { amount } => {
                    let eng = self.eff(Skill::Engineer);
                    let each = crate::ship::wear_gain(u32::from(*amount), eng);
                    for m in &mut self.ship.modules {
                        m.wear = m.wear.saturating_add(each).min(crate::ship::WEAR_MAX);
                    }
                }
                Effect::Standing { faction, amount } => {
                    self.shift_standing_with_rival(*faction, *amount, events);
                }
                Effect::Cargo { good, tons } => {
                    if let Some(g) = content.good_ix(good).and_then(|g| u8::try_from(g).ok()) {
                        if *tons >= 0 {
                            let got = self.load_free(content, g, tons.unsigned_abs());
                            if got > 0 {
                                events.push(Event::CargoFound { good: g, tons: got });
                            }
                        } else {
                            let lost = self.unload(g, tons.unsigned_abs());
                            if lost > 0 {
                                events.push(Event::CargoLost {
                                    good: g,
                                    tons: lost,
                                });
                            }
                        }
                    }
                }
                Effect::LoseContraband { pct } => {
                    let tons = self.lose_class(content, true, u32::from(*pct));
                    if tons > 0 {
                        events.push(Event::CargoDumped { tons });
                    }
                }
                Effect::LoseCargo { pct } => {
                    let tons = self.strip_cargo(u32::from(*pct));
                    if tons > 0 {
                        events.push(Event::CargoDumped { tons });
                    }
                }
                Effect::Flag { name } => {
                    if let Some(b) = content.transit.flag_bit(name) {
                        self.flags |= b;
                    }
                }
                Effect::ClearFlag { name } => {
                    if let Some(b) = content.transit.flag_bit(name) {
                        self.flags &= !b;
                    }
                }
                Effect::Hurt { who, days } => {
                    if let Some(i) = self.pick_crew(*who, env.rng) {
                        let d = u32::from(*days);
                        self.injure(i, env.rng, d, d, events);
                    }
                }
                Effect::Kill { who } => {
                    if let Some(i) = self.pick_crew(*who, env.rng) {
                        self.lose_crew(i, events);
                    }
                }
                Effect::Treat { amount } => {
                    for c in &mut self.crew {
                        c.nudge(crate::crew::TREATMENT, *amount);
                    }
                }
                Effect::Scare { amount } => self.scare_crew(*amount),
                Effect::Content { amount } => {
                    for c in &mut self.contracts {
                        if c.kind == crate::contracts::Kind::Passenger {
                            let now = i16::from(c.mood) + i16::from(*amount);
                            c.mood = u8::try_from(now.clamp(0, 10)).unwrap_or(0);
                        }
                    }
                }
                Effect::Xp { skill, amount } => self.gain(*skill, *amount, events),
                Effect::Combat { .. } | Effect::Board { .. } => {
                    if scene.is_none() {
                        scene = Some(fx);
                    }
                }
                Effect::Stowaway { role } => {
                    if self.crew.len() < crate::crew::MAX_CREW
                        && self.berths_used() < self.berths(content)
                    {
                        let c = crate::crew::stowaway(content, env.frontier, *role, env.rng);
                        events.push(Event::Hired {
                            name: c.name.clone(),
                            role: c.role,
                            fee: 0,
                        });
                        if self.crew.is_empty() {
                            self.wages_due = env.today + crate::crew::WAGE_DAYS;
                        }
                        self.crew.push(c);
                    }
                }
                Effect::Survey { value } => {
                    if self.survey.len() < crate::player::MAX_SURVEY {
                        self.survey.push(crate::player::SurveyLot {
                            system: self.at,
                            value: *value,
                        });
                    }
                }
                Effect::ArcStep { arc } => {
                    if let Some(a) = content
                        .arc_ix(arc)
                        .and_then(|i| self.social.arcs.get_mut(i))
                    {
                        a.step = a.step.saturating_add(1);
                        a.day = env.today;
                    }
                }
                Effect::ArcBranch { arc, branch } => {
                    if let Some((i, bit)) =
                        content.arc_ix(arc).zip(content.arc_branch_bit(arc, branch))
                        && let Some(a) = self.social.arcs.get_mut(i)
                    {
                        a.branches |= bit;
                    }
                }
                Effect::Ending { arc, ending } => self.reach_ending(env, arc, ending, events),
            }
        }
        match scene {
            Some(Effect::Combat { enemy }) => {
                // "@bounty" is the quarry of the job being worked, whatever type it is.
                let quarry = (enemy == "@bounty")
                    .then(|| {
                        self.contracts
                            .iter()
                            .find(|c| c.kind == crate::contracts::Kind::Bounty && c.progress == 0)
                            .map(|c| c.pax)
                    })
                    .flatten();
                let ix = quarry.or_else(|| {
                    let id = if enemy == "@bounty" {
                        "skiff"
                    } else {
                        enemy.as_str()
                    };
                    content
                        .enemies
                        .enemy
                        .iter()
                        .position(|e| e.id == id)
                        .and_then(|i| u8::try_from(i).ok())
                });
                if let Some(ix) = ix {
                    self.start_combat(env, ix, events);
                }
            }
            Some(Effect::Board { derelict }) => self.start_boarding(env, derelict, events),
            _ => {}
        }
    }

    fn pick_crew(&self, who: Who, rng: &mut russh_bbs_door::DoorRng) -> Option<usize> {
        match who {
            Who::Role(r) => self
                .crew
                .iter()
                .enumerate()
                .filter(|(_, c)| c.role == r)
                .max_by_key(|(_, c)| c.effective())
                .map(|(i, _)| i),
            Who::Random => {
                if self.crew.is_empty() {
                    None
                } else {
                    Some(rng.below(u32::try_from(self.crew.len()).unwrap_or(1)) as usize)
                }
            }
        }
    }

    /// Put free cargo in the hold as far as there is room; tons taken aboard.
    pub(crate) fn load_free(&mut self, content: &Content, good: u8, tons: u16) -> u16 {
        let stats = self.stats(content);
        let free = u32::from(stats.cargo_cap).saturating_sub(self.hold_used());
        let take = u16::try_from(u32::from(tons).min(free)).unwrap_or(0);
        if take == 0 {
            return 0;
        }
        if self.ship.tons_of(good) == 0 && self.ship.cargo.len() >= crate::ship::MAX_LOTS {
            return 0;
        }
        match self.ship.cargo.iter_mut().find(|l| l.good == good) {
            Some(l) => l.tons = l.tons.saturating_add(take),
            None => self.ship.cargo.push(crate::ship::Lot {
                good,
                tons: take,
                paid: 0,
            }),
        }
        take
    }

    fn unload(&mut self, good: u8, tons: u16) -> u16 {
        let Some(lot) = self.ship.cargo.iter_mut().find(|l| l.good == good) else {
            return 0;
        };
        let take = tons.min(lot.tons);
        if lot.tons > 0 {
            lot.paid -= lot.paid * i64::from(take) / i64::from(lot.tons);
        }
        lot.tons -= take;
        self.ship.cargo.retain(|l| l.tons > 0);
        take
    }

    fn lose_class(&mut self, content: &Content, contraband: bool, pct: u32) -> u32 {
        let mut lost = 0;
        for lot in &mut self.ship.cargo {
            let is_bad = content
                .goods
                .get(usize::from(lot.good))
                .is_some_and(|g| g.class == crate::content::GoodClass::Contraband);
            if is_bad == contraband {
                let take = (u32::from(lot.tons) * pct)
                    .div_ceil(100)
                    .min(u32::from(lot.tons));
                let take16 = u16::try_from(take).unwrap_or(lot.tons);
                if lot.tons > 0 {
                    lot.paid -= lot.paid * i64::from(take16) / i64::from(lot.tons);
                }
                lot.tons -= take16;
                lost += take;
            }
        }
        self.ship.cargo.retain(|l| l.tons > 0);
        lost
    }
}

fn walk_req(r: &Req, f: &mut impl FnMut(&Req)) {
    f(r);
    if let Req::Not { req } = r {
        walk_req(req, f);
    }
}

impl TransitTable {
    /// Links from events to goods, economies, enemies, derelicts and passenger kinds that do
    /// not exist (checked once all tables are loaded).
    #[must_use]
    pub fn link_problems(&self, content: &Content) -> Vec<String> {
        let mut p = Vec::new();
        for e in &self.event {
            let id = &e.id;
            let mut reqs: Vec<&Req> = e.when.iter().chain(e.mods.iter().map(|m| &m.req)).collect();
            for o in &e.options {
                reqs.extend(o.req.iter());
                for out in &o.outcomes {
                    reqs.extend(out.bonus.iter().map(|b| &b.req));
                }
            }
            for r in reqs {
                walk_req(r, &mut |r| match r {
                    Req::Good { good, .. } if content.good_ix(good).is_none() => {
                        p.push(format!("event `{id}`: unknown good `{good}`"));
                    }
                    Req::Economy { economy } if content.economy_ix(economy).is_none() => {
                        p.push(format!("event `{id}`: unknown economy `{economy}`"));
                    }
                    Req::Passenger { pax: Some(pax) }
                        if !content.jobs.passenger.iter().any(|d| d.id == *pax) =>
                    {
                        p.push(format!("event `{id}`: unknown passenger kind `{pax}`"));
                    }
                    Req::Background { id: bg }
                        if !content.backgrounds.iter().any(|b| b.id == *bg) =>
                    {
                        p.push(format!("event `{id}`: unknown background `{bg}`"));
                    }
                    _ => {}
                });
            }
            for o in &e.options {
                for out in &o.outcomes {
                    for f in &out.effects {
                        match f {
                            Effect::Cargo { good, .. } if content.good_ix(good).is_none() => {
                                p.push(format!("event `{id}`: unknown good `{good}`"));
                            }
                            Effect::Combat { enemy }
                                if enemy != "@bounty"
                                    && !content.enemies.enemy.iter().any(|x| x.id == *enemy) =>
                            {
                                p.push(format!("event `{id}`: unknown enemy `{enemy}`"));
                            }
                            Effect::Board { derelict }
                                if !content
                                    .derelicts
                                    .derelict
                                    .iter()
                                    .any(|x| x.id == *derelict) =>
                            {
                                p.push(format!("event `{id}`: unknown derelict `{derelict}`"));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        p
    }
}
