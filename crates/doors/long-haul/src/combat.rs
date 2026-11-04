//! Ship-to-ship combat: range bands, power, and an enemy who shows its hand.
//!
//! A fight is a short run of rounds over three **range bands** (far, mid, close). Each round the
//! enemy's **intent** is shown first (it can feint, less often the better the captain's scanner
//! and pilot), then the captain **allocates the reactor** between weapons, shields and engines
//! and picks a manoeuvre, and both sides resolve. A captain can always try to **run**.
//!
//! The fight is a plain state machine over [`Combat`]: no clock, and every draw comes from the
//! caller's `DoorRng`, so a recorded log replays it exactly. It is saved with the captain, so
//! dropping the line mid-fight does not re-roll anything.
//!
//! What a build changes here: weapons decide the damage and the band they like; a gunner adds to
//! it; a pilot (aboard or the captain) shrinks the enemy's hit chance and speeds the getaway;
//! an engineer keeps patching the hull between exchanges; a medic keeps the crew alive; the
//! scanner and pilot see through feints; a shield soaks a volley; a crew mix decides whether a
//! boarding party is turned back.

use russh_bbs_door::DoorRng;
use russh_bbs_door::content::Table;
use serde::Deserialize;

use crate::codec::{Reader, Writer};
use crate::content::{Content, Faction, SlotKind};
use crate::crew::{DANGER, Role};
use crate::engine::{Env, can_pay};
use crate::events::{Event, Refusal};
use crate::player::{Cause, Player};
use crate::scene::Scene;
use crate::skills::Skill;

/// A range band. Far is 0, close is 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Range {
    /// Long range: rails and snipers.
    Far,
    /// Middle range: lances and gunboats.
    Mid,
    /// Close range: scatter guns, rams and boarders.
    Close,
}

impl Range {
    /// Band number, 0 (far) to 2 (close).
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::Far => 0,
            Self::Mid => 1,
            Self::Close => 2,
        }
    }

    /// The band at a number.
    #[must_use]
    pub const fn from_index(i: u8) -> Option<Self> {
        match i {
            0 => Some(Self::Far),
            1 => Some(Self::Mid),
            2 => Some(Self::Close),
            _ => None,
        }
    }

    /// Display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Far => "far",
            Self::Mid => "mid",
            Self::Close => "close",
        }
    }

    /// One band closer, or the same at close.
    #[must_use]
    pub const fn closer(self) -> Self {
        match self {
            Self::Far => Self::Mid,
            Self::Mid | Self::Close => Self::Close,
        }
    }

    /// One band farther, or the same at far.
    #[must_use]
    pub const fn farther(self) -> Self {
        match self {
            Self::Far | Self::Mid => Self::Far,
            Self::Close => Self::Mid,
        }
    }
}

/// How well a weapon (or a gun) that likes band `reach` works at `range`, percent.
#[must_use]
pub const fn reach_pct(reach: Range, range: Range) -> u32 {
    match reach.index().abs_diff(range.index()) {
        0 => 100,
        1 => 60,
        _ => 25,
    }
}

/// How an enemy fights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    /// Charges to close range and stays there.
    Rusher,
    /// Holds mid range and trades fire.
    Skirmisher,
    /// Stays at far range.
    Sniper,
    /// Closes and grapples.
    Boarder,
    /// Sits still behind its shields.
    Guard,
}

/// What an enemy means to do this round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Close one band.
    Close,
    /// Open one band.
    Open,
    /// Stand and fire with everything.
    Volley,
    /// Shields up, guns quiet.
    Brace,
    /// Run a grapple across (only works at close range).
    Grapple,
    /// Break off and run.
    Flee,
}

impl Intent {
    const ALL: [Self; 6] = [
        Self::Close,
        Self::Open,
        Self::Volley,
        Self::Brace,
        Self::Grapple,
        Self::Flee,
    ];

    /// Save position.
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::Close => 0,
            Self::Open => 1,
            Self::Volley => 2,
            Self::Brace => 3,
            Self::Grapple => 4,
            Self::Flee => 5,
        }
    }

    fn from_index(i: u8) -> Option<Self> {
        Self::ALL.get(usize::from(i)).copied()
    }

    /// The telegraph the captain reads.
    #[must_use]
    pub const fn telegraph(self) -> &'static str {
        match self {
            Self::Close => "turning in to close the distance",
            Self::Open => "backing off, opening the range",
            Self::Volley => "charging every gun for a full volley",
            Self::Brace => "angling its shields toward you, guns cold",
            Self::Grapple => "running out grapples, looking to board",
            Self::Flee => "burning hard for the dark",
        }
    }

    /// Percent of the enemy's damage this intent puts in a volley.
    const fn volley_pct(self) -> u32 {
        match self {
            Self::Volley => 150,
            Self::Close | Self::Open => 70,
            Self::Grapple => 60,
            Self::Flee => 40,
            Self::Brace => 0,
        }
    }
}

/// How a captain moves this round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maneuver {
    /// Hold the band.
    Hold,
    /// Try to close one band (needs engine power).
    Close,
    /// Try to open one band (needs engine power).
    Open,
}

/// What a captain orders for a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Orders {
    /// Reactor points to the guns.
    pub weapons: u8,
    /// Reactor points to the shields.
    pub shields: u8,
    /// Reactor points to the engines.
    pub engines: u8,
    /// The manoeuvre.
    pub maneuver: Maneuver,
}

/// Where a fight stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Rounds are being fought.
    Fight,
    /// The enemy is wrecked; the captain may board the wreck or leave it.
    Won,
}

/// An enemy ship type.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnemyDef {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Whose it is.
    pub faction: Faction,
    /// One line.
    pub blurb: String,
    /// Structural points.
    pub hull: u16,
    /// Shield points, regenerating a quarter each round.
    pub shield: u16,
    /// Speed, 1 to 6.
    pub speed: u8,
    /// Hit chance before the target's evasion, percent.
    pub accuracy: u8,
    /// Damage of a base volley.
    pub damage: u8,
    /// The band its guns like.
    pub reach: Range,
    /// How it fights.
    pub style: Style,
    /// Chance per-mille that its telegraph lies, before the captain's scanner and pilot.
    pub feint_pm: u32,
    /// Boarders it can put across.
    pub boarders: u8,
    /// Hull percent under which it tries to flee.
    pub flee_pct: u8,
    /// Credits in the wreck (low, high).
    pub loot: [u32; 2],
    /// Standing change with its faction for wrecking it (negative).
    pub kill_standing: i16,
    /// The derelict layout of its wreck, if it can be boarded.
    #[serde(default)]
    pub wreck: Option<String>,
}

/// The enemies.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnemyTable {
    /// All types.
    pub enemy: Vec<EnemyDef>,
}

impl Table for EnemyTable {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        crate::content::duplicates(self.enemy.iter().map(|e| e.id.as_str()), "enemy", &mut p);
        if self.enemy.len() < 8 || self.enemy.len() > 40 {
            p.push("between 8 and 40 enemy types".into());
        }
        for e in &self.enemy {
            if !crate::content::text_ok(&e.name, 28) || !crate::content::text_ok(&e.blurb, 100) {
                p.push(format!("enemy `{}`: name <= 28, blurb <= 100", e.id));
            }
            if e.hull == 0
                || e.hull > 400
                || e.shield > 200
                || !(1..=6).contains(&e.speed)
                || e.accuracy == 0
                || e.accuracy > 95
                || e.damage == 0
                || e.damage > 40
                || e.flee_pct > 60
                || e.feint_pm > 600
                || e.loot[0] > e.loot[1]
                || e.kill_standing > 0
            {
                p.push(format!("enemy `{}`: a stat is out of range", e.id));
            }
        }
        p
    }
}

/// A fight in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combat {
    /// Index into the content's enemies.
    pub enemy: u8,
    /// The enemy's structure left.
    pub e_hull: i16,
    /// The enemy's shield points held.
    pub e_shield: i16,
    /// The current band.
    pub range: Range,
    /// Rounds fought.
    pub round: u8,
    /// What the enemy will really do.
    pub intent: Intent,
    /// What it appears to be doing.
    pub shown: Intent,
    /// Boarders the enemy still has.
    pub boarders: u8,
    /// A ward being escorted (hull percent), 0 if none.
    pub ward: u8,
    /// Where the fight stands.
    pub phase: Phase,
    /// The captain's hull when the fight began, for the report.
    pub hull_at_start: u16,
}

/// What the player's own ship brings to a fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gear {
    /// Reactor points to hand out.
    pub budget: u8,
    /// Shield percent from modules.
    pub shield_pct: u8,
    /// Scanner grade.
    pub scan: u8,
}

impl Combat {
    /// Start a fight against enemy type `ix`.
    #[must_use]
    pub fn new(content: &Content, ix: u8, hull_now: u16, ward: u8, rng: &mut DoorRng) -> Self {
        let def = &content.enemies.enemy[usize::from(ix)];
        let range = match def.style {
            Style::Sniper => Range::Far,
            Style::Rusher | Style::Boarder => Range::Mid,
            Style::Skirmisher | Style::Guard => {
                if rng.chance(1, 2) {
                    Range::Mid
                } else {
                    Range::Far
                }
            }
        };
        Self {
            enemy: ix,
            e_hull: i16::try_from(def.hull).unwrap_or(i16::MAX),
            e_shield: i16::try_from(def.shield).unwrap_or(0),
            range,
            round: 0,
            intent: Intent::Close,
            shown: Intent::Close,
            boarders: def.boarders,
            ward,
            phase: Phase::Fight,
            hull_at_start: hull_now,
        }
    }

    /// The enemy's definition.
    #[must_use]
    pub fn def<'a>(&self, content: &'a Content) -> &'a EnemyDef {
        &content.enemies.enemy[usize::from(self.enemy)]
    }

    /// Choose and telegraph the enemy's next move.
    pub fn plan(&mut self, content: &Content, scan: u8, pilot: u8, rng: &mut DoorRng) {
        let def = self.def(content);
        let hull_pct = i32::from(self.e_hull.max(0)) * 100 / i32::from(def.hull.max(1));
        let flee = hull_pct < i32::from(def.flee_pct) && rng.chance(65, 100);
        let intent = if flee {
            Intent::Flee
        } else {
            match def.style {
                Style::Rusher => {
                    if self.range == Range::Close {
                        if self.boarders > 0 && rng.chance(30, 100) {
                            Intent::Grapple
                        } else if rng.chance(75, 100) {
                            Intent::Volley
                        } else {
                            Intent::Brace
                        }
                    } else {
                        Intent::Close
                    }
                }
                Style::Skirmisher => match self.range {
                    Range::Far => Intent::Close,
                    Range::Close => Intent::Open,
                    Range::Mid => {
                        if rng.chance(65, 100) {
                            Intent::Volley
                        } else {
                            Intent::Brace
                        }
                    }
                },
                Style::Sniper => {
                    if self.range == Range::Far {
                        if rng.chance(75, 100) {
                            Intent::Volley
                        } else {
                            Intent::Brace
                        }
                    } else {
                        Intent::Open
                    }
                }
                Style::Boarder => {
                    if self.range == Range::Close {
                        if self.boarders > 0 && rng.chance(70, 100) {
                            Intent::Grapple
                        } else {
                            Intent::Volley
                        }
                    } else {
                        Intent::Close
                    }
                }
                Style::Guard => {
                    if rng.chance(50, 100) {
                        Intent::Volley
                    } else {
                        Intent::Brace
                    }
                }
            }
        };
        self.intent = intent;
        // A feint shows a different move. A good scanner and a good pilot see through it.
        let lie = def
            .feint_pm
            .saturating_sub(u32::from(scan) * 60 + u32::from(pilot) * 12);
        self.shown = if lie > 0 && rng.chance(lie, 1000) {
            let others: Vec<Intent> = Intent::ALL
                .iter()
                .copied()
                .filter(|i| *i != intent && *i != Intent::Flee)
                .collect();
            others[rng.below(u32::try_from(others.len()).unwrap_or(1)) as usize]
        } else {
            intent
        };
    }

    /// Write to a save.
    pub fn write(&self, w: &mut Writer) {
        w.u8(self.enemy);
        w.i32(i32::from(self.e_hull));
        w.i32(i32::from(self.e_shield));
        w.u8(self.range.index());
        w.u8(self.round);
        w.u8(self.intent.index());
        w.u8(self.shown.index());
        w.u8(self.boarders);
        w.u8(self.ward);
        w.u8(match self.phase {
            Phase::Fight => 0,
            Phase::Won => 1,
        });
        w.u16(self.hull_at_start);
    }

    /// Read from a save, or `None` if anything is out of range.
    pub fn read(r: &mut Reader<'_>, content: &Content) -> Option<Self> {
        let enemy = r.u8()?;
        let def = content.enemies.enemy.get(usize::from(enemy))?;
        let e_hull = i16::try_from(r.i32()?).ok()?;
        let e_shield = i16::try_from(r.i32()?).ok()?;
        if e_hull > i16::try_from(def.hull).ok()? || e_shield > i16::try_from(def.shield).ok()? {
            return None;
        }
        let range = Range::from_index(r.u8()?)?;
        let round = r.u8()?;
        let intent = Intent::from_index(r.u8()?)?;
        let shown = Intent::from_index(r.u8()?)?;
        let boarders = r.u8()?;
        let ward = r.u8()?.min(100);
        let phase = match r.u8()? {
            0 => Phase::Fight,
            1 => Phase::Won,
            _ => return None,
        };
        Some(Self {
            enemy,
            e_hull,
            e_shield,
            range,
            round,
            intent,
            shown,
            boarders: boarders.min(def.boarders),
            ward,
            phase,
            hull_at_start: r.u16()?,
        })
    }
}

/// The most rounds a fight lasts before both sides break off.
pub const MAX_ROUNDS: u8 = 24;

impl Player {
    /// What the ship brings to a fight.
    #[must_use]
    pub fn gear(&self, content: &Content) -> Gear {
        let stats = self.stats(content);
        let reactor = content
            .hulls
            .get(usize::from(self.ship.hull))
            .map_or(6, |h| h.reactor);
        let eng = self.eff(Skill::Engineer);
        Gear {
            budget: reactor + u8::from(eng >= 4) + u8::from(eng >= 8),
            shield_pct: stats.shield_pct,
            scan: stats.scan,
        }
    }

    /// Weapon strength at a band: each weapon's power, scaled by wear and by how well its reach
    /// suits the band.
    #[must_use]
    pub fn firepower(&self, content: &Content, range: Range) -> u32 {
        self.ship
            .of_kind(content, SlotKind::Weapon)
            .map(|(_, inst, def)| {
                let reach = match def.reach.as_str() {
                    "close" => Range::Close,
                    "far" => Range::Far,
                    _ => Range::Mid,
                };
                u32::from(def.power) * crate::ship::condition_pct(inst.wear) / 100
                    * reach_pct(reach, range)
                    / 100
            })
            .sum()
    }

    /// Start a fight. The captain's hull and crew are what they are.
    pub(crate) fn start_combat(
        &mut self,
        env: &mut Env<'_>,
        enemy: u8,
        events: &mut Vec<Event>,
    ) {
        let ward = self
            .contracts
            .iter()
            .find(|c| c.kind == crate::contracts::Kind::Escort)
            .map_or(0, |c| c.progress.max(1));
        let mut fight = Combat::new(env.content, enemy, self.ship.hull_hp, ward, env.rng);
        let gear = self.gear(env.content);
        fight.plan(env.content, gear.scan, self.eff(Skill::Pilot), env.rng);
        events.push(Event::FightBegins { enemy });
        self.scene = Some(Scene::Combat(Box::new(fight)));
    }

    fn take_fight(&mut self) -> Result<Box<Combat>, Refusal> {
        match self.scene.take() {
            Some(Scene::Combat(c)) => Ok(c),
            other => {
                self.scene = other;
                Err(Refusal::NoFight)
            }
        }
    }

    /// Check an order against the reactor.
    fn check_orders(&self, content: &Content, orders: &Orders) -> Result<(), Refusal> {
        let gear = self.gear(content);
        let total = u32::from(orders.weapons) + u32::from(orders.shields) + u32::from(orders.engines);
        if total > u32::from(gear.budget) {
            return Err(Refusal::OverPower { budget: gear.budget });
        }
        if orders.maneuver != Maneuver::Hold && orders.engines == 0 {
            return Err(Refusal::NoEnginePower);
        }
        Ok(())
    }

    /// Fight one round.
    pub(crate) fn fight_round(
        &mut self,
        env: &mut Env<'_>,
        orders: &Orders,
    ) -> Result<Vec<Event>, Refusal> {
        if !matches!(&self.scene, Some(Scene::Combat(c)) if c.phase == Phase::Fight) {
            return Err(Refusal::NoFight);
        }
        self.check_orders(env.content, orders)?;
        let mut fight = self.take_fight()?;
        let mut events = Vec::new();
        self.resolve_round(env, &mut fight, orders, &mut events);
        self.settle_fight(env, fight, &mut events);
        Ok(events)
    }

    /// Try to run. Always allowed in a fight; costs the round.
    pub(crate) fn run_away(&mut self, env: &mut Env<'_>) -> Result<Vec<Event>, Refusal> {
        if !matches!(&self.scene, Some(Scene::Combat(c)) if c.phase == Phase::Fight) {
            return Err(Refusal::NoFight);
        }
        let mut fight = self.take_fight()?;
        let content = env.content;
        let def = fight.def(content).clone();
        let gear = self.gear(content);
        let pilot = self.eff(Skill::Pilot);
        let band: i32 = match fight.range {
            Range::Far => 25,
            Range::Mid => 5,
            Range::Close => -20,
        };
        let chance = (30
            + i32::from(gear.budget) * 7
            + i32::from(pilot) * 3
            + band
            - i32::from(def.speed) * 6)
            .clamp(5, 95);
        let mut events = Vec::new();
        let cost = u16::from(self.ship.fuel >= 3) * 3;
        if env.rng.chance(u32::try_from(chance).unwrap_or(5), 100) {
            self.ship.fuel -= cost;
            self.stats.fled = self.stats.fled.saturating_add(1);
            self.train_role(Role::Pilot, &mut events);
            self.gain(Skill::Pilot, 4, &mut events);
            events.push(Event::Ran { fuel: cost });
            self.end_fight(&fight, &mut events);
            return Ok(events);
        }
        events.push(Event::RunFailed);
        // The enemy gets its round: engines were all the captain had, so no shields.
        let orders = Orders {
            weapons: 0,
            shields: 0,
            engines: gear.budget,
            maneuver: Maneuver::Hold,
        };
        self.enemy_turn(env, &mut fight, &orders, &mut events);
        self.settle_fight(env, fight, &mut events);
        Ok(events)
    }

    fn resolve_round(
        &mut self,
        env: &mut Env<'_>,
        fight: &mut Combat,
        orders: &Orders,
        events: &mut Vec<Event>,
    ) {
        let content = env.content;
        let def = fight.def(content).clone();
        let pilot = self.eff(Skill::Pilot);
        let gunner = self.crew_bonus(Role::Gunner);
        fight.round = fight.round.saturating_add(1);

        // 1. Movement: the faster hand wins a contested band.
        let want_p: i8 = match orders.maneuver {
            Maneuver::Hold => 0,
            Maneuver::Close => 1,
            Maneuver::Open => -1,
        };
        let want_e: i8 = match fight.intent {
            Intent::Close => 1,
            Intent::Open | Intent::Flee => -1,
            _ => 0,
        };
        let delta = if want_p == want_e || want_e == 0 {
            want_p
        } else if want_p == 0 {
            want_e
        } else {
            let mine = u32::from(orders.engines) * 2 + u32::from(pilot);
            let theirs = u32::from(def.speed) * 2 + env.rng.below(4);
            if mine >= theirs { want_p } else { want_e }
        };
        let before = fight.range;
        fight.range = match delta {
            1 => fight.range.closer(),
            -1 => fight.range.farther(),
            _ => fight.range,
        };
        if fight.range != before {
            events.push(Event::RangeShift {
                to: fight.range,
                by_you: delta == want_p && want_p != 0,
            });
        }

        // 2. The captain fires.
        let power = self.firepower(content, fight.range);
        if power > 0 && orders.weapons > 0 {
            let evasive = matches!(fight.intent, Intent::Open | Intent::Flee);
            let hit = (75 + i32::from(gunner) * 3
                - i32::from(def.speed) * 3
                - if evasive { 15 } else { 0 })
            .clamp(15, 95);
            if env.rng.chance(u32::try_from(hit).unwrap_or(15), 100) {
                let raw = (power * (u32::from(orders.weapons) + u32::from(gunner)) / 3).max(1);
                let mut dmg = i32::try_from(raw).unwrap_or(i32::MAX);
                if fight.intent == Intent::Brace {
                    dmg = dmg * 6 / 10;
                }
                let soak = dmg.min(i32::from(fight.e_shield));
                fight.e_shield -= i16::try_from(soak).unwrap_or(0);
                let through = dmg - soak;
                fight.e_hull -= i16::try_from(through).unwrap_or(i16::MAX);
                events.push(Event::YouHit {
                    shield: u16::try_from(soak).unwrap_or(0),
                    hull: u16::try_from(through).unwrap_or(0),
                });
                self.train_role(Role::Gunner, events);
            } else {
                events.push(Event::YouMissed);
            }
        }
        if fight.e_hull <= 0 {
            return;
        }
        self.enemy_turn(env, fight, orders, events);
    }

    /// The enemy's half of a round.
    fn enemy_turn(
        &mut self,
        env: &mut Env<'_>,
        fight: &mut Combat,
        orders: &Orders,
        events: &mut Vec<Event>,
    ) {
        let content = env.content;
        let def = fight.def(content).clone();
        let pilot = self.eff(Skill::Pilot);
        let gear = self.gear(content);
        let max_hull = content
            .hulls
            .get(usize::from(self.ship.hull))
            .map_or(1, |h| h.hull_points);
        // 3. The enemy fires (not when it braces).
        let volley_pct = fight.intent.volley_pct();
        if volley_pct > 0 {
            let hit = (i32::from(def.accuracy)
                - i32::from(orders.engines) * 5
                - i32::from(pilot) * 2
                - if fight.range == Range::Far { 10 } else { 0 })
            .clamp(5, 95);
            if env.rng.chance(u32::try_from(hit).unwrap_or(5), 100) {
                let raw = u32::from(def.damage) * reach_pct(def.reach, fight.range) / 100
                    * volley_pct
                    / 100;
                let soak = u32::from(orders.shields) * 3 + u32::from(gear.shield_pct) / 5;
                let dmg = raw.saturating_sub(soak);
                // The shot may have been meant for the ward.
                if fight.ward > 0 && env.rng.chance(30, 100) {
                    let loss = u8::try_from((raw.min(30)).max(1)).unwrap_or(5);
                    fight.ward = fight.ward.saturating_sub(loss).max(1);
                    for c in &mut self.contracts {
                        if c.kind == crate::contracts::Kind::Escort {
                            c.progress = fight.ward;
                        }
                    }
                    events.push(Event::WardHit { ward: fight.ward });
                } else if dmg == 0 {
                    events.push(Event::Shielded);
                } else {
                    let dmg16 = u16::try_from(dmg).unwrap_or(u16::MAX);
                    self.ship.hull_hp = self.ship.hull_hp.saturating_sub(dmg16).max(1);
                    events.push(Event::YouWereHit { hull: dmg16 });
                    self.scare_crew(-1);
                    self.hurt_in_fight(env.rng, dmg16, max_hull, events);
                }
            } else {
                events.push(Event::EnemyMissed);
            }
        }
        // 4. A grapple that lands at close range.
        if fight.intent == Intent::Grapple && fight.range == Range::Close && fight.boarders > 0 {
            let hit = (i32::from(def.accuracy) + 10
                - i32::from(orders.engines) * 5
                - i32::from(pilot) * 2)
            .clamp(10, 95);
            if env.rng.chance(u32::try_from(hit).unwrap_or(10), 100) {
                self.repel_boarders(env, fight, events);
            } else {
                events.push(Event::GrappleFailed);
            }
        }
        // 5. Shields recover; the engineer patches the hull.
        let regen = i16::try_from(def.shield / 4).unwrap_or(0);
        fight.e_shield =
            (fight.e_shield + regen).min(i16::try_from(def.shield).unwrap_or(i16::MAX));
        let patch = u16::from(self.crew_bonus(Role::Engineer) / 2);
        if patch > 0 && self.ship.hull_hp < max_hull {
            let before = self.ship.hull_hp;
            self.ship.hull_hp = (self.ship.hull_hp + patch).min(max_hull);
            events.push(Event::Patched {
                hull: self.ship.hull_hp - before,
            });
            self.train_role(Role::Engineer, events);
        }
        // 6. The next move.
        let scan = gear.scan;
        fight.plan(content, scan, pilot, env.rng);
    }

    /// A grapple landed: the crew either turns the boarders back or loses cargo to them.
    fn repel_boarders(&mut self, env: &mut Env<'_>, fight: &mut Combat, events: &mut Vec<Event>) {
        let fit = self.crew.iter().filter(|c| c.fit()).count();
        let defence = 4
            + u32::try_from(fit).unwrap_or(0) * 3
            + u32::from(self.crew_bonus(Role::Gunner)) * 3
            + u32::from(self.level(Skill::Stealth).min(3));
        let attack = u32::from(fight.boarders) * 5 + env.rng.below(6);
        if defence >= attack {
            fight.boarders = fight.boarders.saturating_sub(1);
            fight.e_hull = (fight.e_hull - 8).max(1);
            events.push(Event::BoardersRepelled);
            self.train_role(Role::Gunner, events);
            return;
        }
        let tons = self.strip_cargo(30);
        let cash = (self.cash / 10).max(0);
        self.debit(Cause::Fine, cash);
        events.push(Event::Boarded { tons, cash });
        self.injure_random(env.rng, 2, 4, events);
        self.scare_crew(-2);
        fight.boarders = fight.boarders.saturating_sub(1);
    }

    /// The fight is over or goes on: the enemy wrecked, escaped, the captain beaten, or another
    /// round.
    fn settle_fight(&mut self, env: &mut Env<'_>, mut fight: Box<Combat>, events: &mut Vec<Event>) {
        let content = env.content;
        let def = fight.def(content).clone();
        let max_hull = content
            .hulls
            .get(usize::from(self.ship.hull))
            .map_or(1, |h| h.hull_points);
        if fight.e_hull <= 0 {
            self.win_fight(env, &mut fight, &def, events);
            if self.scene.is_none() && fight.phase == Phase::Won {
                self.scene = Some(Scene::Combat(fight));
            }
            return;
        }
        if u32::from(self.ship.hull_hp) * 10 <= u32::from(max_hull) {
            self.lose_fight(env, &fight, &def, events);
            return;
        }
        if fight.intent == Intent::Flee
            && env.rng.chance(
                40 + u32::from(def.speed) * 8 + if fight.range == Range::Far { 20 } else { 0 },
                100,
            )
        {
            events.push(Event::EnemyEscaped);
            self.end_fight(&fight, events);
            return;
        }
        if fight.round >= MAX_ROUNDS {
            events.push(Event::BrokeOff);
            self.end_fight(&fight, events);
            return;
        }
        self.scene = Some(Scene::Combat(fight));
    }

    fn win_fight(
        &mut self,
        env: &mut Env<'_>,
        fight: &mut Combat,
        def: &EnemyDef,
        events: &mut Vec<Event>,
    ) {
        let loot = i64::from(env.rng.range(def.loot[0], def.loot[1]));
        self.credit(Cause::Salvage, loot);
        self.stats.fights_won = self.stats.fights_won.saturating_add(1);
        events.push(Event::Wrecked {
            enemy: fight.enemy,
            loot,
        });
        // Killing a faction's ship costs standing with it, and its rival is pleased.
        if def.kill_standing != 0 {
            self.shift_standing_with_rival(def.faction, def.kill_standing, events);
        }
        self.gain(Skill::Pilot, 6, events);
        self.credit_fight_goal();
        for c in &mut self.contracts {
            if c.kind == crate::contracts::Kind::Bounty
                && usize::from(c.pax) == usize::from(fight.enemy)
                && c.progress == 0
            {
                c.progress = 1;
                events.push(Event::BountyHit { enemy: fight.enemy });
            }
        }
        self.scare_crew(2);
        if def.wreck.is_some() && self.docked.is_none() {
            fight.phase = Phase::Won;
        } else {
            self.end_fight(fight, events);
        }
    }

    fn lose_fight(&mut self, env: &mut Env<'_>, fight: &Combat, def: &EnemyDef, events: &mut Vec<Event>) {
        self.stats.fights_lost = self.stats.fights_lost.saturating_add(1);
        let tons = if def.faction == Faction::League {
            0
        } else {
            self.strip_cargo(40)
        };
        let cash = if def.faction == Faction::League {
            0
        } else {
            (self.cash / 4).max(0)
        };
        self.debit(Cause::Fine, cash);
        let fine = if def.faction == Faction::League {
            let seized = self.seize_all_contraband(env.content);
            let fine = 400 + seized * 40;
            self.pay_or_owe(Cause::Fine, fine);
            fine
        } else {
            0
        };
        events.push(Event::Beaten { tons, cash, fine });
        self.injure_random(env.rng, 2, 6, events);
        if self.crew.len() > 1 && env.rng.chance(20, 100) {
            self.lose_random_crew(env.rng, events);
        }
        self.scare_crew(-3);
        self.end_fight(fight, events);
    }

    /// Drop the scene and note the ward.
    fn end_fight(&mut self, fight: &Combat, events: &mut Vec<Event>) {
        self.scene = None;
        let _ = fight;
        let _ = events;
    }

    /// The captain's choice about a wreck: board it or leave it.
    pub(crate) fn wreck_choice(
        &mut self,
        env: &mut Env<'_>,
        board: bool,
    ) -> Result<Vec<Event>, Refusal> {
        let Some(Scene::Combat(c)) = &self.scene else {
            return Err(Refusal::NoFight);
        };
        if c.phase != Phase::Won {
            return Err(Refusal::NoFight);
        }
        let layout = c.def(env.content).wreck.clone();
        let mut events = Vec::new();
        self.scene = None;
        if board && let Some(id) = layout {
            self.start_boarding(env, &id, &mut events);
        }
        Ok(events)
    }

    /// Whether the player can pay a ward's fee, kept here for the invariants test.
    #[must_use]
    pub fn can_afford(&self, amount: i64) -> bool {
        can_pay(self.cash, amount).is_ok()
    }

    /// Hurt a random fit hand when a volley lands hard; sometimes lose one.
    fn hurt_in_fight(&mut self, rng: &mut DoorRng, dmg: u16, max_hull: u16, events: &mut Vec<Event>) {
        if self.crew.is_empty() {
            return;
        }
        let heavy = u32::from(dmg) * 100 / u32::from(max_hull.max(1));
        if heavy < 8 {
            return;
        }
        let medic = self.crew_bonus(Role::Medic) > 0;
        if rng.chance(if medic { 12 } else { 25 }, 100) {
            self.injure_random(rng, 2, 5, events);
        }
        if heavy >= 20 && self.crew.len() > 1 && rng.chance(if medic { 4 } else { 10 }, 100) {
            self.lose_random_crew(rng, events);
        }
    }

    /// Wreck chance numbers for tests: the chance, percent, that a run at a band succeeds.
    #[must_use]
    pub fn run_chance(&self, content: &Content, enemy: &EnemyDef, range: Range) -> u32 {
        let band: i32 = match range {
            Range::Far => 25,
            Range::Mid => 5,
            Range::Close => -20,
        };
        let gear = self.gear(content);
        u32::try_from(
            (30 + i32::from(gear.budget) * 7 + i32::from(self.eff(Skill::Pilot)) * 3 + band
                - i32::from(enemy.speed) * 6)
                .clamp(5, 95),
        )
        .unwrap_or(5)
    }

    /// Mark a danger mood on all crew.
    pub(crate) fn scare_crew(&mut self, by: i8) {
        for c in &mut self.crew {
            c.nudge(DANGER, by);
        }
    }
}
