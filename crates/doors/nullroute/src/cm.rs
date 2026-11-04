//! Countermeasures and encounters (M4-N2).
//!
//! A countermeasure is an enemy with a **pattern**: a short cycle of intents it repeats. An
//! encounter is the exchange between one countermeasure and your deck: each beat the machine shows
//! what it is about to do, you play a script from the deck (or brace, or break off), and then it
//! does it. The trace ticks on every strike. It lasts three to eight moves.
//!
//! This module is pure state: it never touches the trace, the deck or the RNG. [`Encounter::damage`]
//! says what a script does to the machine and [`Encounter::resolve`] says what the machine does
//! back; the run turns that into trace. Every number comes from the content tables.
//!
//! **The RPG rule here.** What a script does depends on its family and on the machine:
//!
//! - a break script hurts a machine most when its lock family is the machine's `weak` one (+2);
//! - a script tuned to the machine's kind (a coded `target`, or a stock tuned jammer or purge) adds
//!   +3, and a **warden** is hardened: nothing else hurts it at all;
//! - armor takes points off every hit (`pierce` ignores some), and a *guard* beat adds 3 for that
//!   move, so the wrong move at the wrong beat does nothing;
//! - a *seal* beat locks the family you just used, so a deck of one family runs dry;
//! - no single hit takes more than a third of the machine's integrity, so a fight is at least three
//!   moves, and on the eighth the machine's patience runs out and you are forced off.

use russh_bbs_door::DoorRng;

use crate::content::{CmDef, CmKind, Content, CorpDef, Effect, Family, Intent, Lock, ScriptDef};
use crate::net::{NodeId, Patrol};

/// The most moves in an encounter: on the next beat you are forced off.
pub const ROUNDS_MAX: u32 = 8;

/// Armor a guard beat adds against your move.
pub const GUARD_ARMOR: u32 = 3;

/// Damage a tuned script adds.
pub const TUNED_BONUS: u32 = 3;

/// Damage a break script adds against the machine's weak lock family.
pub const WEAK_BONUS: u32 = 2;

/// The kind of countermeasure a patrol is.
#[must_use]
pub const fn kind_of(patrol: Patrol) -> Option<CmKind> {
    match patrol {
        Patrol::None => None,
        Patrol::Sentry => Some(CmKind::Sentry),
        Patrol::Tracer => Some(CmKind::Tracer),
        Patrol::BlackWall => Some(CmKind::Wall),
    }
}

/// Which countermeasure patrols a node. A pure function of the corporation, the tier and the label
/// (the node's name and level), so the same network has the same machines for everyone and nothing
/// has to be stored. A corporation's `cm_bias` triples the weight of the machines with its tags:
/// that is how corporations differ in their mix.
#[must_use]
pub fn pick<'a>(
    content: &'a Content,
    corp: &CorpDef,
    tier: u8,
    kind: CmKind,
    label: &str,
) -> Option<&'a CmDef> {
    let weight = |c: &CmDef| {
        let tagged = c.tags.iter().any(|t| corp.cm_bias.contains(t));
        c.weight * if tagged { 3 } else { 1 }
    };
    let mut pool: Vec<&CmDef> = content
        .cms
        .cm
        .iter()
        .filter(|c| c.kind == kind && c.tier_min <= tier && tier <= c.tier_max)
        .collect();
    if pool.is_empty() {
        // A hand-built network may put a machine where the tables do not: any of its kind will do.
        pool = content.cms.cm.iter().filter(|c| c.kind == kind).collect();
    }
    let total: u32 = pool.iter().map(|c| weight(c)).sum();
    if total == 0 {
        return None;
    }
    let mut roll = DoorRng::from_label(label, i64::from(corp.num)).below(total);
    for c in &pool {
        let w = weight(c);
        if roll < w {
            return Some(c);
        }
        roll -= w;
    }
    pool.last().copied()
}

/// The warden that hunts runs on a corporation's networks.
#[must_use]
pub fn warden_of<'a>(content: &'a Content, corp: &CorpDef) -> Option<&'a CmDef> {
    content.cm(corp.warden).filter(|c| c.kind == CmKind::Warden)
}

/// A fragment dropped by a defeated machine or a copied vault: weighted among the ones whose rank
/// the tier reaches. `None` if there are none.
#[must_use]
pub fn drop_fragment(content: &Content, tier: u8, rng: &mut DoorRng) -> Option<u8> {
    let pool: Vec<_> = content
        .fragments
        .fragment
        .iter()
        .filter(|f| f.rank <= tier)
        .collect();
    let total: u32 = pool.iter().map(|f| f.weight).sum();
    if total == 0 {
        return None;
    }
    let mut roll = rng.below(total);
    for f in &pool {
        if roll < f.weight {
            return Some(f.num);
        }
        roll -= f.weight;
    }
    None
}

/// What the machine did on its beat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beat {
    /// The intent that resolved.
    pub intent: Intent,
    /// Trace to add (before the run's multipliers).
    pub trace: u32,
    /// Whether it was a hit (a fork can take it).
    pub strike: bool,
    /// Whether it started a tracer.
    pub tracer: bool,
    /// Whether a stun made it lose the beat.
    pub stunned: bool,
}

/// One fight with one countermeasure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encounter {
    /// The countermeasure's number.
    pub cm: u8,
    /// Its name.
    pub name: String,
    /// Its kind.
    pub kind: CmKind,
    /// The node it guards.
    pub node: NodeId,
    /// Where you came from (you go back there if you break off). For a wall it is where you stand.
    pub from: NodeId,
    /// The node's security level.
    pub level: u8,
    /// Integrity left.
    pub integrity: u32,
    /// Integrity at the start.
    pub max: u32,
    /// Armor.
    pub armor: u32,
    /// Trace of a strike.
    pub strike: u32,
    /// Its weak lock family.
    pub weak: Lock,
    /// What a mend heals.
    pub mend: u32,
    /// The cycle of intents.
    pub pattern: Vec<Intent>,
    /// Moves made this encounter (it resets when you come back after breaking off).
    pub round: u32,
    /// The index in the pattern of the beat to come.
    pub step: usize,
    /// Beats it will lose to a jam.
    pub stun: u32,
    /// The next strike is doubled.
    pub charged: bool,
    /// The family you may not use this move.
    pub sealed: Option<Family>,
    /// The beats you have watched, oldest first (capped at the pattern's length).
    pub seen: Vec<Intent>,
    /// A probe read the whole cycle.
    pub scanned: bool,
    /// You braced this move.
    pub braced: bool,
}

impl Encounter {
    /// A fresh fight. `start` picks the first beat of the pattern (by the node, not the dice, so a
    /// machine behaves the same every time you meet it).
    #[must_use]
    pub fn new(def: &CmDef, node: NodeId, from: NodeId, level: u8, start: usize) -> Self {
        let max = def.integrity + u32::from(level);
        Self {
            cm: def.num,
            name: def.name.clone(),
            kind: def.kind,
            node,
            from,
            level,
            integrity: max,
            max,
            armor: def.armor + u32::from(level) / 4,
            strike: def.strike,
            weak: def.weak,
            mend: def.mend,
            pattern: def.pattern.clone(),
            round: 0,
            step: start % def.pattern.len().max(1),
            stun: 0,
            charged: false,
            sealed: None,
            seen: Vec::new(),
            scanned: false,
            braced: false,
        }
    }

    /// The beat to come: what the machine is about to do.
    #[must_use]
    pub fn intent(&self) -> Intent {
        self.pattern
            .get(self.step)
            .copied()
            .unwrap_or(Intent::Probe)
    }

    /// Whether the whole cycle is known: scanned, or watched all the way round.
    #[must_use]
    pub fn pattern_known(&self) -> bool {
        self.scanned || self.seen.len() >= self.pattern.len()
    }

    /// The beats after this one that you can see (none until the pattern is known).
    #[must_use]
    pub fn forecast(&self, n: usize) -> Vec<Intent> {
        if !self.pattern_known() || self.pattern.is_empty() {
            return Vec::new();
        }
        (1..=n)
            .map(|i| self.pattern[(self.step + i) % self.pattern.len()])
            .collect()
    }

    /// The most one hit can take: a third of the integrity, so a fight is at least three moves.
    #[must_use]
    pub fn cap(&self) -> u32 {
        self.max.div_ceil(3)
    }

    /// The armor against your move now.
    #[must_use]
    pub fn armor_now(&self) -> u32 {
        self.armor
            + if self.intent() == Intent::Guard {
                GUARD_ARMOR
            } else {
                0
            }
    }

    /// Whether a script is tuned to this machine.
    #[must_use]
    pub fn tuned(&self, script: &ScriptDef) -> bool {
        script.target == Some(self.kind)
    }

    /// What a script does to the machine this move. `skill` is the player's skill in the script's
    /// family (half of it is added to a hit).
    #[must_use]
    pub fn damage(&self, script: &ScriptDef, skill: u8) -> u32 {
        let tuned = self.tuned(script);
        // A warden is hardened: only a script tuned to it does anything.
        if self.kind == CmKind::Warden && !tuned {
            return 0;
        }
        let base = match script.effect {
            Effect::Crack | Effect::Spoof | Effect::Overflow => {
                let mut b = script.power;
                if script.effect.lock() == Some(self.weak) {
                    b += WEAK_BONUS;
                }
                // Walls are for purges.
                if self.kind == CmKind::Wall && !tuned {
                    b /= 2;
                }
                b
            }
            Effect::Jam => (script.power / 2).max(1),
            Effect::Purge => {
                if self.kind == CmKind::Wall && script.power < u32::from(self.level) {
                    return 0;
                }
                if self.kind == CmKind::Wall {
                    script.power * 2
                } else {
                    script.power
                }
            }
            _ => return 0,
        };
        let raw = base + if tuned { TUNED_BONUS } else { 0 } + u32::from(skill) / 2;
        // Armor takes points off a hit, but a hit that lands always does something (a warden aside,
        // which is handled above): armor makes a fight longer, it does not make it impossible.
        raw.saturating_sub(self.armor_now().saturating_sub(script.pierce))
            .max(1)
            .min(self.cap())
    }

    /// Hurt the machine. Returns whether it is down.
    pub fn hit(&mut self, damage: u32) -> bool {
        self.integrity = self.integrity.saturating_sub(damage);
        self.integrity == 0
    }

    /// The machine's beat, after your move. `dealt` is what your script did to it and `family` the
    /// family you played (`None` if you braced).
    pub fn resolve(&mut self, dealt: u32, family: Option<Family>) -> Beat {
        let intent = self.intent();
        self.sealed = None;
        let mut beat = Beat {
            intent,
            trace: 0,
            strike: false,
            tracer: false,
            stunned: false,
        };
        if self.stun > 0 {
            self.stun -= 1;
            self.charged = false;
            beat.stunned = true;
        } else {
            match intent {
                Intent::Probe => beat.trace = (self.strike / 3).max(1),
                Intent::Strike => {
                    beat.trace = self.strike * if self.charged { 2 } else { 1 };
                    beat.strike = true;
                    self.charged = false;
                }
                Intent::Charge => self.charged = true,
                Intent::Guard => {}
                Intent::Seal => self.sealed = family,
                Intent::Trace => {
                    beat.trace = (self.strike / 2).max(1);
                    beat.tracer = true;
                }
                Intent::Mend => self.integrity = (self.integrity + self.mend).min(self.max),
                Intent::Counter => {
                    if family == Some(Family::Break) {
                        beat.trace = dealt.max(2);
                    }
                }
            }
        }
        if self.seen.len() < self.pattern.len() {
            self.seen.push(intent);
        }
        self.step = (self.step + 1) % self.pattern.len().max(1);
        self.round += 1;
        self.braced = false;
        beat
    }

    /// Whether the machine's patience has run out.
    #[must_use]
    pub const fn spent(&self) -> bool {
        self.round >= ROUNDS_MAX
    }

    /// Come back to a fight you broke off: the damage and what you learned stay, the count of
    /// moves starts again.
    pub fn resume(&mut self, from: NodeId) {
        self.round = 0;
        self.from = from;
        self.stun = 0;
        self.sealed = None;
        self.braced = false;
    }
}
