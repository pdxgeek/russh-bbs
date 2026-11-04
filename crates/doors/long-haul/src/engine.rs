//! The rules: a captain's state plus an action gives a new state plus events.
//!
//! Nothing here reads a clock or a random source of its own. Time arrives as [`Env::today`]
//! (a real day number) and randomness as the [`DoorRng`] in the [`Env`], so a recorded run
//! reproduces the saved state exactly. Every action checks all it needs before it changes
//! anything: a [`Refusal`] leaves the captain, the world and the RNG untouched.
//!
//! This file holds the loop (days, the loan, docking, jumping, skimming, scanning, towing);
//! `trade.rs` holds the market, the yard and the repair dock.

use russh_bbs_door::DoorRng;

use crate::content::{Content, Economy, Faction, Perk, SlotKind};
use crate::events::{Event, Refusal};
use crate::frontier::{Frontier, Hazard, System};
use crate::market::step_mid;
use crate::player::{ALLOWANCE, Cause, Intel, MAX_SURVEY, Player, SurveyLot};
use crate::ship::{Ship, Stats, wear_gain};
use crate::skills::Skill;
use crate::world::{Delta, World};

/// Drive-days a dock visit costs.
pub const DOCK_DAYS: i32 = 1;
/// Dock fee per station size, in credits.
pub const DOCK_FEE_PER_SIZE: i64 = 10;
/// Daily interest on the loan, per-mille.
pub const INTEREST_PM: i64 = 4;
/// Most days of interest one return can add.
pub const MAX_INTEREST_DAYS: i64 = 28;
/// The lender's smallest weekly payment.
pub const MIN_PAYMENT: i64 = 150;
/// The weekly payment as a percent of what is owed.
pub const PAYMENT_PCT: i64 = 5;
/// Penalty for a missed payment, percent of what is owed.
pub const MISSED_PENALTY_PCT: i64 = 3;
/// Payments missed in a row before the lender takes the ship.
pub const REPO_MISSES: u8 = 3;
/// Fee added to the loan on repossession, percent.
pub const REPO_FEE_PCT: i64 = 12;
/// The most the lender carries at the start.
pub const BASE_CREDIT_LIMIT: i64 = 36_000;
/// Credit limit lost for each repossession.
pub const LIMIT_LOSS_PER_REPO: i64 = 9_000;
/// The lowest the limit falls.
pub const MIN_CREDIT_LIMIT: i64 = 9_000;
/// At or below this standing a faction's docks refuse the ship.
pub const BAR_STANDING: i16 = -40;
/// At or above this Free Captains standing the military posts refuse the ship.
pub const SMUGGLER_STANDING: i16 = 50;
/// Credits of sales that raise standing by one.
pub const TURNOVER_PER_STANDING: i64 = 2_000;

/// Everything an action needs from outside the captain.
pub struct Env<'a> {
    /// The content tables.
    pub content: &'a Content,
    /// The map.
    pub frontier: &'a Frontier,
    /// The shared world, as this session sees it.
    pub world: &'a mut World,
    /// The seeded random numbers.
    pub rng: &'a mut DoorRng,
    /// Today, a real day number.
    pub today: i64,
    /// Changes made to the world, to be sent to the stored copy.
    pub deltas: Vec<Delta>,
}

impl<'a> Env<'a> {
    /// A new environment with no changes recorded.
    pub fn new(
        content: &'a Content,
        frontier: &'a Frontier,
        world: &'a mut World,
        rng: &'a mut DoorRng,
        today: i64,
    ) -> Self {
        Self {
            content,
            frontier,
            world,
            rng,
            today,
            deltas: Vec::new(),
        }
    }

    /// Change the world here and record the change for the stored copy.
    pub fn push(&mut self, delta: Delta) {
        self.world.apply(self.content, self.frontier, &delta);
        self.deltas.push(delta);
    }
}

/// What a repair covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairTarget {
    /// The hull and every module.
    All,
    /// The hull only.
    Hull,
    /// One installed module, by position.
    Module(usize),
}

/// Something a captain can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Dock at a station in this system.
    Dock(u16),
    /// Leave the dock.
    Undock,
    /// Jump along a lane to a neighbouring system.
    Jump(u16),
    /// Buy tons of the good in a market slot.
    Buy {
        /// The slot in the station's goods list.
        slot: u8,
        /// Tons wanted.
        tons: u16,
    },
    /// Sell tons of the good in a market slot.
    Sell {
        /// The slot in the station's goods list.
        slot: u8,
        /// Tons to sell.
        tons: u16,
    },
    /// Buy fuel.
    Refuel(u16),
    /// Skim a gas giant for fuel.
    Skim,
    /// Scan the system in depth for survey data.
    DeepScan,
    /// Sell all survey data to the Guild.
    SellSurvey,
    /// Repair.
    Repair(RepairTarget),
    /// Buy a module at a shipyard.
    BuyModule(u8),
    /// Sell an installed module.
    SellModule(usize),
    /// Buy a hull, trading in the old one.
    BuyHull(u8),
    /// Borrow more.
    Borrow(i64),
    /// Pay toward the loan.
    Repay(i64),
    /// Call a tow when the ship cannot jump anywhere.
    CallTow,
}

/// Whether a pool of cash can cover `amount`.
pub(crate) fn can_pay(cash: i64, amount: i64) -> Result<(), Refusal> {
    if cash >= amount {
        Ok(())
    } else {
        Err(Refusal::NoCash { need: amount })
    }
}

impl Player {
    /// The ship's stats with this captain's skills.
    #[must_use]
    pub fn stats(&self, content: &Content) -> Stats {
        self.ship.stats(
            content,
            self.level(Skill::Pilot),
            self.level(Skill::Engineer),
        )
    }

    /// Add experience, noting a level gained.
    pub fn gain(&mut self, skill: Skill, amount: u32, events: &mut Vec<Event>) {
        let before = self.level(skill);
        self.xp[skill.index()] = self.xp[skill.index()].saturating_add(amount);
        let after = self.level(skill);
        if after > before {
            events.push(Event::SkillUp {
                skill,
                level: after,
            });
        }
    }

    /// Change standing with a faction, clamped to -100..=100.
    pub fn shift_standing(&mut self, faction: Faction, change: i16, events: &mut Vec<Event>) {
        let slot = &mut self.standing[faction.index()];
        let new = slot.saturating_add(change).clamp(-100, 100);
        let applied = new - *slot;
        *slot = new;
        if applied != 0 {
            events.push(Event::Standing {
                faction,
                change: applied,
            });
        }
    }

    /// What the lender will carry now.
    #[must_use]
    pub fn credit_limit(&self) -> i64 {
        (BASE_CREDIT_LIMIT - LIMIT_LOSS_PER_REPO * i64::from(self.repossessions))
            .max(MIN_CREDIT_LIMIT)
    }

    /// The next weekly payment.
    #[must_use]
    pub fn weekly_payment(&self) -> i64 {
        if self.debt.principal <= 0 {
            return 0;
        }
        (self.debt.principal * PAYMENT_PCT / 100)
            .max(MIN_PAYMENT)
            .min(self.debt.principal)
    }

    /// Pay `amount` from the purse for `cause`; whatever the purse cannot cover is added to the
    /// loan instead, so a broke captain is never locked out of a dock or a tow.
    pub(crate) fn pay_or_owe(&mut self, cause: Cause, amount: i64) -> i64 {
        let paid = amount.min(self.cash.max(0));
        self.debit(cause, paid);
        self.debt.principal += amount - paid;
        amount
    }

    /// Start a new real day: the drive allowance, interest, the weekly payment, a slow thaw of
    /// a bad name. Safe to call at every key; it does nothing until the day changes.
    pub fn settle(&mut self, env: &mut Env<'_>) -> Vec<Event> {
        let mut events = Vec::new();
        let today = env.today;
        if today <= self.last_day {
            return events;
        }
        let elapsed = (today - self.last_day).min(30);
        self.last_day = today;
        self.days_left = ALLOWANCE + self.days_left.clamp(-(ALLOWANCE - 1), 0);
        events.push(Event::NewDay {
            days: self.days_left,
        });
        for standing in &mut self.standing {
            if *standing < 0 {
                *standing = (*standing + i16::try_from(elapsed).unwrap_or(30)).min(0);
            }
        }
        if self.debt.principal > 0 {
            let days = (today - self.debt.interest_day).clamp(0, MAX_INTEREST_DAYS);
            let before = self.debt.principal;
            for _ in 0..days {
                self.debt.principal += (self.debt.principal * INTEREST_PM + 999) / 1000;
            }
            if self.debt.principal > before {
                events.push(Event::Interest {
                    added: self.debt.principal - before,
                });
            }
        }
        self.debt.interest_day = today;
        let mut periods = 0;
        while self.debt.principal > 0 && self.debt.next_due <= today && periods < 2 {
            self.collect_due(env, &mut events);
            self.debt.next_due += 7;
            periods += 1;
        }
        if self.debt.next_due <= today {
            self.debt.next_due += 7 * ((today - self.debt.next_due) / 7 + 1);
        }
        events
    }

    fn collect_due(&mut self, env: &Env<'_>, events: &mut Vec<Event>) {
        let due = self.weekly_payment();
        if self.cash >= due {
            self.debit(Cause::Payment, due);
            self.debt.principal -= due;
            self.debt.missed = 0;
            events.push(Event::DuePaid { amount: due });
            return;
        }
        let paid = self.cash.max(0);
        self.debit(Cause::Payment, paid);
        self.debt.principal -= paid;
        let penalty = self.debt.principal * MISSED_PENALTY_PCT / 100;
        self.debt.principal += penalty;
        self.debt.missed = self.debt.missed.saturating_add(1);
        events.push(Event::DueMissed {
            short: due - paid,
            penalty,
            missed: self.debt.missed,
        });
        if self.debt.missed >= REPO_MISSES {
            self.repossess(env, events);
        }
    }

    fn repossess(&mut self, env: &Env<'_>, events: &mut Vec<Event>) {
        let fee = self.debt.principal * REPO_FEE_PCT / 100;
        self.debt.principal += fee;
        self.debt.missed = 0;
        self.repossessions = self.repossessions.saturating_add(1);
        self.ship = Ship::hand_me_down(env.content);
        let home = env.frontier.home_station;
        self.docked = Some(home);
        self.at = env.frontier.station(home).map_or(0, |s| s.system);
        self.shift_standing(Faction::League, -5, events);
        events.push(Event::Repossessed { fee });
    }

    /// Do `action`, or say why not.
    ///
    /// # Errors
    /// A [`Refusal`]; the captain, world and RNG are then unchanged.
    pub fn apply(&mut self, env: &mut Env<'_>, action: &Action) -> Result<Vec<Event>, Refusal> {
        match action {
            Action::Dock(station) => self.dock(env, *station),
            Action::Undock => {
                self.docked.take().ok_or(Refusal::NotDocked)?;
                Ok(vec![Event::Undocked])
            }
            Action::Jump(to) => self.jump(env, *to),
            Action::Buy { slot, tons } => self.buy(env, *slot, *tons),
            Action::Sell { slot, tons } => self.sell(env, *slot, *tons),
            Action::Refuel(units) => self.refuel(env, *units),
            Action::Skim => self.skim(env),
            Action::DeepScan => self.deep_scan(env),
            Action::SellSurvey => self.sell_survey(env),
            Action::Repair(target) => self.repair(env, *target),
            Action::BuyModule(module) => self.buy_module(env, *module),
            Action::SellModule(index) => self.sell_module(env, *index),
            Action::BuyHull(hull) => self.buy_hull(env, *hull),
            Action::Borrow(amount) => self.borrow(env, *amount),
            Action::Repay(amount) => self.repay(*amount),
            Action::CallTow => self.call_tow(env),
        }
    }

    /// Spend drive-days. A long jump may overdraw; the debt comes off tomorrow's allowance.
    pub(crate) fn spend_days(&mut self, days: u32) {
        self.days_left -= i32::try_from(days).unwrap_or(i32::MAX);
        self.voyage_days = self.voyage_days.saturating_add(days);
    }

    pub(crate) fn require_day(&self) -> Result<(), Refusal> {
        if self.days_left >= 1 {
            Ok(())
        } else {
            Err(Refusal::NoDays)
        }
    }

    /// The station this captain is docked at.
    pub(crate) fn docked_at<'a>(
        &self,
        env: &Env<'a>,
    ) -> Result<&'a crate::frontier::Station, Refusal> {
        let id = self.docked.ok_or(Refusal::NotDocked)?;
        env.frontier.station(id).ok_or(Refusal::NoSuchThing)
    }

    fn access(&self, eco: &Economy) -> Result<(), Refusal> {
        if self.standing[eco.faction.index()] <= BAR_STANDING {
            return Err(Refusal::Barred(eco.faction));
        }
        if eco.id == "military" && self.standing[Faction::Free.index()] >= SMUGGLER_STANDING {
            return Err(Refusal::KnownSmuggler);
        }
        Ok(())
    }

    fn dock(&mut self, env: &mut Env<'_>, station: u16) -> Result<Vec<Event>, Refusal> {
        let st = env.frontier.station(station).ok_or(Refusal::NoSuchThing)?;
        if st.system != self.at || !env.world.is_charted(env.frontier, st.system) {
            return Err(Refusal::NotHere);
        }
        if self.docked == Some(station) {
            return Err(Refusal::AlreadyDocked);
        }
        self.require_day()?;
        let eco = &env.content.economies[usize::from(st.economy)];
        self.access(eco)?;
        self.spend_days(DOCK_DAYS as u32);
        Ok(self.arrive_docked(env, station, false))
    }

    /// Docking proper: the fee, the inspection, what the market looks like. A tow skips the fee.
    pub(crate) fn arrive_docked(
        &mut self,
        env: &mut Env<'_>,
        station: u16,
        towed: bool,
    ) -> Vec<Event> {
        let content = env.content;
        let mut events = Vec::new();
        let Some(st) = env.frontier.station(station) else {
            return events;
        };
        let eco = &content.economies[usize::from(st.economy)];
        let waived = towed || (eco.id == "military" && self.perk(content) == Some(Perk::OldCodes));
        let fee = if waived {
            0
        } else {
            DOCK_FEE_PER_SIZE * i64::from(st.size)
        };
        if fee > 0 {
            self.pay_or_owe(Cause::Dock, fee);
        }
        self.docked = Some(station);
        events.push(Event::Docked { station, fee });
        self.inspect(env, station, &mut events);
        self.remember_prices(env, station);
        events
    }

    fn inspect(&mut self, env: &mut Env<'_>, station: u16, events: &mut Vec<Event>) {
        let content = env.content;
        let carried = self.ship.contraband_tons(content);
        let Some(st) = env.frontier.station(station) else {
            return;
        };
        let eco = &content.economies[usize::from(st.economy)];
        if carried == 0 || eco.faction == Faction::Free {
            return;
        }
        if eco.id == "military" && self.perk(content) == Some(Perk::OldCodes) {
            return;
        }
        let bonus = env
            .world
            .inspection_bonus(content, env.frontier, station, env.today);
        let mut chance = i64::from(eco.law_pm) + bonus + i64::from(carried.min(40)) * 3;
        chance -= i64::from(self.level(Skill::Stealth)) * 40;
        if self.perk(content) == Some(Perk::Fence) {
            chance -= 60;
        }
        let chance = u32::try_from(chance.clamp(0, 900)).unwrap_or(0);
        self.stats.inspections += 1;
        if !env.rng.chance(chance, 1000) {
            self.gain(Skill::Stealth, 5, events);
            events.push(Event::Slipped);
            return;
        }
        let mut seized_paid = 0i64;
        let mut tons = 0u32;
        let goods = &content.goods;
        self.ship.cargo.retain(|lot| {
            let bad = goods
                .get(usize::from(lot.good))
                .is_some_and(|g| g.class == crate::content::GoodClass::Contraband);
            if bad {
                seized_paid += lot.paid;
                tons += u32::from(lot.tons);
            }
            !bad
        });
        let fine = (seized_paid / 4).max(100);
        self.pay_or_owe(Cause::Fine, fine);
        self.stats.caught += 1;
        events.push(Event::Seized {
            tons,
            fine,
            by: eco.faction,
        });
        self.shift_standing(eco.faction, -8, events);
    }

    fn remember_prices(&mut self, env: &Env<'_>, station: u16) {
        let Some(st) = env.frontier.station(station) else {
            return;
        };
        let prices = st
            .goods
            .iter()
            .enumerate()
            .map(|(slot, ms)| {
                let depth =
                    crate::market::depth_at(&env.content.goods, env.frontier, station, slot);
                let base = env
                    .content
                    .goods
                    .get(usize::from(ms.good))
                    .map_or(1, |g| g.base);
                u32::try_from(step_mid(base, depth, env.world.stock_of(station, slot)))
                    .unwrap_or(u32::MAX)
            })
            .collect();
        let seen = Intel {
            station,
            day: env.today,
            prices,
        };
        match self.intel.iter_mut().find(|i| i.station == station) {
            Some(old) => *old = seen,
            None => self.intel.push(seen),
        }
    }

    /// What a survey of `system` is worth to this captain's ship and skills.
    #[must_use]
    pub fn survey_value(&self, content: &Content, system: &System) -> u32 {
        let stats = self.stats(content);
        let base = 100
            + u32::from(system.richness) * 60
            + u32::from(system.planets) * 25
            + u32::from(system.belts) * 20
            + if system.hazard == Hazard::None { 0 } else { 80 };
        let scan = if stats.scan == 0 {
            250
        } else {
            1000 + u32::from(stats.scan - 1) * 150
        };
        let skill = 1000 + u32::from(self.level(Skill::Survey)) * 80 + stats.lab_pm;
        u32::try_from(u64::from(base) * u64::from(scan) / 1000 * u64::from(skill) / 1000)
            .unwrap_or(u32::MAX)
    }

    fn jump(&mut self, env: &mut Env<'_>, to: u16) -> Result<Vec<Event>, Refusal> {
        let content = env.content;
        let frontier = env.frontier;
        let dist = frontier.lane(self.at, to).ok_or(Refusal::NoLane)?;
        self.require_day()?;
        if self.ship.hull_too_weak(content) {
            return Err(Refusal::HullTooWeak);
        }
        let stats = self.stats(content);
        let need = Ship::fuel_for(&stats, dist);
        if self.ship.fuel < need {
            return Err(Refusal::NoFuel { need });
        }
        let eng = self.level(Skill::Engineer);
        let mut events = Vec::new();
        let mut days = Ship::days_for(&stats, dist);
        let mut burned = need;
        let from = self.at;
        self.docked = None;
        let drive_wear = self
            .ship
            .of_kind(content, SlotKind::Drive)
            .map(|(_, i, _)| i.wear)
            .max()
            .unwrap_or(0);
        if drive_wear > 600 && env.rng.chance(u32::from((drive_wear - 600) / 4), 1000) {
            let extra = 2;
            days += extra;
            burned = burned.saturating_add(need / 4);
            events.push(Event::DriveFault { extra_days: extra });
            self.wear_kind(content, SlotKind::Drive, wear_gain(50, eng));
        }
        self.ship.fuel = self.ship.fuel.saturating_sub(burned);
        self.wear_kind(content, SlotKind::Drive, wear_gain(8 * days, eng));
        self.at = to;
        self.stats.jumps += 1;
        events.push(Event::Jumped {
            from,
            to,
            days,
            fuel: burned,
        });
        let system = frontier.system(to).ok_or(Refusal::NoSuchThing)?;
        if !env.world.is_charted(frontier, to) {
            self.chart(env, system, &mut events);
        }
        self.hazard(env, system, &stats, &mut events);
        self.spend_days(days);
        self.gain(Skill::Pilot, days, &mut events);
        Ok(events)
    }

    fn wear_kind(&mut self, content: &Content, kind: SlotKind, amount: u16) {
        for inst in &mut self.ship.modules {
            if content
                .modules
                .get(usize::from(inst.module))
                .is_some_and(|m| m.slot == kind)
            {
                inst.wear = inst.wear.saturating_add(amount).min(crate::ship::WEAR_MAX);
            }
        }
    }

    fn chart(&mut self, env: &mut Env<'_>, system: &System, events: &mut Vec<Event>) {
        let value = self.survey_value(env.content, system);
        env.push(Delta::Chart {
            system: system.id,
            day: env.today,
            name: self.captain.clone(),
        });
        self.stats.charted += 1;
        if self.survey.len() < MAX_SURVEY {
            self.survey.push(SurveyLot {
                system: system.id,
                value,
            });
        }
        if !self.scanned.contains(&system.id) {
            self.scanned.push(system.id);
        }
        events.push(Event::Charted {
            system: system.id,
            value,
        });
        self.gain(Skill::Survey, 10, events);
    }

    fn hazard(
        &mut self,
        env: &mut Env<'_>,
        system: &System,
        stats: &Stats,
        events: &mut Vec<Event>,
    ) {
        let content = env.content;
        let eng = self.level(Skill::Engineer);
        let open = u32::from(100 - stats.shield_pct);
        let (mut hull_lost, mut days_lost, mut wear) = (0u16, 0u32, 0u16);
        let avoided = match system.hazard {
            Hazard::None => return,
            Hazard::Nebula => {
                let seen = stats.scan >= 1 || self.level(Skill::Pilot) >= 5;
                if !seen {
                    days_lost = 1;
                }
                seen
            }
            Hazard::Radiation => {
                wear = u16::try_from(u32::from(wear_gain(40, eng)) * open / 100).unwrap_or(0);
                for inst in &mut self.ship.modules {
                    inst.wear = inst.wear.saturating_add(wear).min(crate::ship::WEAR_MAX);
                }
                wear == 0
            }
            Hazard::Debris => {
                let seen = stats.scan >= 2;
                if !seen {
                    let roll = env.rng.range(4, 12);
                    hull_lost = u16::try_from((roll * open).div_ceil(100)).unwrap_or(0);
                    self.ship.hull_hp = self.ship.hull_hp.saturating_sub(hull_lost).max(1);
                }
                seen
            }
        };
        let _ = content;
        if days_lost > 0 {
            self.spend_days(days_lost);
        }
        events.push(Event::Hazard {
            kind: system.hazard,
            hull_lost,
            days_lost,
            wear,
            avoided,
        });
    }

    fn skim(&mut self, env: &mut Env<'_>) -> Result<Vec<Event>, Refusal> {
        let content = env.content;
        let system = env.frontier.system(self.at).ok_or(Refusal::NoSuchThing)?;
        if !system.gas_giant {
            return Err(Refusal::NoGasGiant);
        }
        self.require_day()?;
        let stats = self.stats(content);
        let room = stats.fuel_cap.saturating_sub(self.ship.fuel);
        if room == 0 {
            return Err(Refusal::TankFull);
        }
        let mut events = Vec::new();
        let eng = self.level(Skill::Engineer);
        let pilot = self.level(Skill::Pilot);
        self.docked = None;
        let base = 20 + env.rng.range(0, 10);
        let yield_ = u16::try_from(u64::from(base) * u64::from(1000 + stats.refine_pm) / 1000)
            .unwrap_or(u16::MAX)
            .min(room);
        let risk =
            (320u32.saturating_sub(u32::from(stats.scan) * 90 + u32::from(pilot) * 20)).max(20);
        let mut hull_lost = 0u16;
        if env.rng.chance(risk, 1000) {
            let roll = env.rng.range(5, 15);
            hull_lost = u16::try_from((roll * u32::from(100 - stats.shield_pct)).div_ceil(100))
                .unwrap_or(0);
            self.ship.hull_hp = self.ship.hull_hp.saturating_sub(hull_lost).max(1);
            self.wear_kind(content, SlotKind::Drive, wear_gain(40, eng));
        }
        self.ship.fuel += yield_;
        self.spend_days(1);
        self.gain(Skill::Pilot, 2, &mut events);
        events.insert(
            0,
            Event::Skimmed {
                units: yield_,
                hull_lost,
            },
        );
        Ok(events)
    }

    fn deep_scan(&mut self, env: &mut Env<'_>) -> Result<Vec<Event>, Refusal> {
        let content = env.content;
        let system = env.frontier.system(self.at).ok_or(Refusal::NoSuchThing)?;
        let stats = self.stats(content);
        if stats.scan == 0 {
            return Err(Refusal::NoScanner);
        }
        if self.scanned.contains(&system.id) {
            return Err(Refusal::AlreadyScanned);
        }
        self.require_day()?;
        if self.survey.len() >= MAX_SURVEY {
            return Err(Refusal::NothingToDo);
        }
        let mut events = Vec::new();
        let value = self.survey_value(content, system) / 3;
        self.wear_kind(
            content,
            SlotKind::Scanner,
            wear_gain(20, self.level(Skill::Engineer)),
        );
        self.scanned.push(system.id);
        self.survey.push(SurveyLot {
            system: system.id,
            value,
        });
        self.spend_days(1);
        events.push(Event::Scanned {
            system: system.id,
            value,
        });
        self.gain(Skill::Survey, 4, &mut events);
        Ok(events)
    }

    fn borrow(&mut self, env: &Env<'_>, amount: i64) -> Result<Vec<Event>, Refusal> {
        let _ = env;
        self.docked.ok_or(Refusal::NotDocked)?;
        if amount <= 0 {
            return Err(Refusal::NothingToDo);
        }
        let limit = self.credit_limit();
        if self.debt.principal + amount > limit {
            return Err(Refusal::OverLimit { limit });
        }
        if self.debt.principal <= 0 {
            self.debt.next_due = env.today + 7;
            self.debt.interest_day = env.today;
            self.debt.missed = 0;
        }
        self.debt.principal += amount;
        self.credit(Cause::Loan, amount);
        Ok(vec![Event::Borrowed { amount }])
    }

    fn repay(&mut self, amount: i64) -> Result<Vec<Event>, Refusal> {
        if amount <= 0 || self.debt.principal <= 0 {
            return Err(Refusal::NothingToDo);
        }
        let amount = amount.min(self.debt.principal);
        can_pay(self.cash, amount)?;
        self.debit(Cause::Payment, amount);
        self.debt.principal -= amount;
        if self.debt.principal == 0 {
            self.debt.missed = 0;
        }
        Ok(vec![Event::Repaid { amount }])
    }

    /// Whether the ship can reach any neighbouring system on the fuel it has.
    #[must_use]
    pub fn can_jump_anywhere(&self, env: &Env<'_>) -> bool {
        if self.ship.hull_too_weak(env.content) {
            return false;
        }
        let stats = self.stats(env.content);
        env.frontier
            .neighbours(self.at)
            .iter()
            .any(|&(_, dist)| Ship::fuel_for(&stats, dist) <= self.ship.fuel)
    }

    fn call_tow(&mut self, env: &mut Env<'_>) -> Result<Vec<Event>, Refusal> {
        let frontier = env.frontier;
        if self.docked.is_some() {
            return Err(Refusal::NotStranded);
        }
        if self.can_jump_anywhere(env) {
            return Err(Refusal::NotStranded);
        }
        let mut best: Option<(u32, u16, u32)> = None;
        let mut dist = vec![u32::MAX; frontier.systems.len()];
        let mut queue = std::collections::BinaryHeap::new();
        dist[usize::from(self.at)] = 0;
        queue.push(std::cmp::Reverse((0u32, self.at)));
        while let Some(std::cmp::Reverse((d, at))) = queue.pop() {
            if d > dist[usize::from(at)] {
                continue;
            }
            let Some(sys) = frontier.system(at) else {
                continue;
            };
            if best.is_none()
                && let Some(&station) = sys.stations.iter().find(|&&s| self.tow_welcome(env, s))
            {
                best = Some((d, station, d));
            }
            for &(next, len) in frontier.neighbours(at) {
                let nd = d + u32::from(len);
                if nd < dist[usize::from(next)] {
                    dist[usize::from(next)] = nd;
                    queue.push(std::cmp::Reverse((nd, next)));
                }
            }
        }
        let (spans, station, _) = best.ok_or(Refusal::NothingToDo)?;
        let fee = 300 + 10 * i64::from(spans);
        let days = spans.div_ceil(15).max(1);
        let target = frontier.station(station).ok_or(Refusal::NoSuchThing)?;
        self.at = target.system;
        self.docked = None;
        self.pay_or_owe(Cause::Tow, fee);
        self.stats.tows += 1;
        self.spend_days(days);
        let mut events = vec![Event::Towed {
            to: station,
            fee,
            days,
        }];
        events.extend(self.arrive_docked(env, station, true));
        Ok(events)
    }

    fn tow_welcome(&self, env: &Env<'_>, station: u16) -> bool {
        env.frontier.station(station).is_some_and(|st| {
            env.world.is_charted(env.frontier, st.system)
                && self
                    .access(&env.content.economies[usize::from(st.economy)])
                    .is_ok()
        })
    }
}
