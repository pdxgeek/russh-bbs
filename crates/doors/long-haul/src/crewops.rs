//! What the crew does to the captain's state: skills, hiring, upkeep, unrest, injuries, goals.
//!
//! The pure data is in [`crate::crew`]; this file is the `Player` side. Every credit goes through
//! [`Player::credit`] and [`Player::debit`] under [`Cause::Crew`], so the ledger test still
//! balances, and every refusal changes nothing.

use russh_bbs_door::DoorRng;

use crate::content::{Content, Faction, GoodClass};
use crate::crew::{
    CrewMember, FOOD, GoalKind, MAX_CREW, PAY, RATION_COST, RATION_DAYS, RECRUIT_REFUSAL, Role,
    TREATMENT, WAGE_DAYS, epoch_of, recruit_faction, recruit_key, recruits,
};
use crate::engine::Env;
use crate::events::{Event, Refusal};
use crate::player::{Cause, MAX_TAKEN, Player};
use crate::skills::Skill;

impl Player {
    /// What the best fit hand of a role adds (0 if none aboard or all hurt).
    #[must_use]
    pub fn crew_bonus(&self, role: Role) -> u8 {
        self.crew
            .iter()
            .filter(|c| c.role == role)
            .map(CrewMember::bonus)
            .max()
            .unwrap_or(0)
    }

    /// The captain's skill with the crew's help: their own level plus what the best hand of the
    /// matching role adds, never above 10. Experience is earned on the captain's own level only.
    #[must_use]
    pub fn eff(&self, skill: Skill) -> u8 {
        let help = Role::ALL
            .iter()
            .filter(|r| r.skill() == Some(skill))
            .map(|r| self.crew_bonus(*r))
            .max()
            .unwrap_or(0);
        (self.level(skill) + help).min(10)
    }

    /// Whether a fit hand of `role` with skill at least `min` is aboard.
    #[must_use]
    pub fn has_role(&self, role: Role, min: u8) -> bool {
        self.crew
            .iter()
            .any(|c| c.role == role && c.fit() && c.effective() >= min)
    }

    /// Whether a sworn hand of `role` is aboard.
    #[must_use]
    pub fn has_sworn(&self, role: Role) -> bool {
        self.crew
            .iter()
            .any(|c| c.role == role && c.sworn && c.fit())
    }

    /// Berths on the ship: the hull's own and the cabins'.
    #[must_use]
    pub fn berths(&self, content: &Content) -> usize {
        let hull = content
            .hulls
            .get(usize::from(self.ship.hull))
            .map_or(0, |h| usize::from(h.berths));
        hull + usize::from(self.stats(content).berths)
    }

    /// Berths taken by crew and passengers.
    #[must_use]
    pub fn berths_used(&self) -> usize {
        self.crew.len() + usize::from(self.passenger_count())
    }

    /// Hands of one role, for the screen and tests.
    #[must_use]
    pub fn crew_of(&self, role: Role) -> usize {
        self.crew.iter().filter(|c| c.role == role).count()
    }

    /// The recruits on offer at a station today who have not been hired.
    #[must_use]
    pub fn recruit_list(
        &self,
        content: &Content,
        frontier: &crate::frontier::Frontier,
        station: u16,
        today: i64,
    ) -> Vec<(u8, CrewMember)> {
        let faction = recruit_faction(content, frontier, station);
        let epoch = epoch_of(today);
        recruits(
            content,
            frontier,
            station,
            today,
            self.standing[faction.index()],
        )
        .into_iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let slot = u8::try_from(i).ok()?;
            (!self.taken.contains(&recruit_key(station, epoch, slot))).then_some((slot, c))
        })
        .collect()
    }

    pub(crate) fn take_key(&mut self, key: u64) {
        if self.taken.len() >= MAX_TAKEN {
            self.taken.remove(0);
        }
        self.taken.push(key);
    }

    /// Sign on a recruit.
    pub(crate) fn hire(&mut self, env: &Env<'_>, slot: u8) -> Result<Vec<Event>, Refusal> {
        let station = self.docked_at(env)?;
        let faction = recruit_faction(env.content, env.frontier, station.id);
        if self.standing[faction.index()] <= RECRUIT_REFUSAL {
            return Err(Refusal::Unwelcome(faction));
        }
        let list = self.recruit_list(env.content, env.frontier, station.id, env.today);
        let (_, recruit) = list
            .into_iter()
            .find(|(s, _)| *s == slot)
            .ok_or(Refusal::NoSuchThing)?;
        if self.crew.len() >= MAX_CREW {
            return Err(Refusal::CrewFull);
        }
        if self.berths_used() >= self.berths(env.content) {
            return Err(Refusal::NoBerth);
        }
        let fee = i64::from(recruit.wage) * 2;
        crate::engine::can_pay(self.cash, fee)?;
        self.debit(Cause::Crew, fee);
        let key = recruit_key(station.id, epoch_of(env.today), slot);
        self.take_key(key);
        if self.crew.is_empty() {
            self.wages_due = env.today + WAGE_DAYS;
        }
        let event = Event::Hired {
            name: recruit.name.clone(),
            role: recruit.role,
            fee,
        };
        self.crew.push(recruit);
        Ok(vec![event])
    }

    /// Let a hand go at a dock.
    pub(crate) fn dismiss(&mut self, env: &Env<'_>, index: usize) -> Result<Vec<Event>, Refusal> {
        self.docked_at(env)?;
        if index >= self.crew.len() {
            return Err(Refusal::NoSuchThing);
        }
        let gone = self.crew.remove(index);
        if gone.sworn {
            // Casting off someone who bled for the ship is noticed.
            for c in &mut self.crew {
                c.nudge(TREATMENT, -3);
            }
        }
        Ok(vec![Event::Dismissed { name: gone.name }])
    }

    /// Stock the galley for a week.
    pub(crate) fn provision(&mut self, env: &Env<'_>) -> Result<Vec<Event>, Refusal> {
        self.docked_at(env)?;
        let mouths = i64::try_from(self.berths_used()).unwrap_or(0);
        if mouths == 0 {
            return Err(Refusal::NothingToDo);
        }
        if self.rations_until >= env.today + 2 * RATION_DAYS {
            return Err(Refusal::NothingToDo);
        }
        let cost = self.provision_quote();
        crate::engine::can_pay(self.cash, cost)?;
        self.debit(Cause::Crew, cost);
        self.rations_until = self.rations_until.max(env.today) + RATION_DAYS;
        Ok(vec![Event::Provisioned {
            cost,
            days: RATION_DAYS,
        }])
    }

    /// What a week of rations costs now; a good quartermaster buys smarter.
    #[must_use]
    pub fn provision_quote(&self) -> i64 {
        let mouths = i64::try_from(self.berths_used()).unwrap_or(0);
        let off = i64::from(self.crew_bonus(Role::Quartermaster).min(5)) * 4;
        mouths * RATION_COST * RATION_DAYS * (100 - off) / 100
    }

    /// Wages owed each week.
    #[must_use]
    pub fn weekly_wages(&self) -> i64 {
        self.crew.iter().map(|c| i64::from(c.wage)).sum()
    }

    /// Credit a use of a role's skill to its best hand; a level gained is noted.
    pub(crate) fn train_role(&mut self, role: Role, events: &mut Vec<Event>) {
        let best = self
            .crew
            .iter_mut()
            .filter(|c| c.role == role && c.fit())
            .max_by_key(|c| c.effective());
        if let Some(c) = best
            && c.train()
        {
            events.push(Event::CrewSkill {
                name: c.name.clone(),
                role,
                skill: c.skill,
            });
        }
    }

    /// Day by day upkeep for the real days that passed: food, calm, healing, loyalty and wages.
    pub(crate) fn crew_upkeep(&mut self, today: i64, elapsed: i64, events: &mut Vec<Event>) {
        if self.crew.is_empty() {
            return;
        }
        let first = today - elapsed + 1;
        for day in first..=today {
            let medic_aboard = self.has_role(Role::Medic, 1);
            let fed = day <= self.rations_until;
            for c in &mut self.crew {
                c.nudge(FOOD, if fed { 1 } else { -1 });
                if c.mood[crate::crew::DANGER] < 0 {
                    c.nudge(crate::crew::DANGER, 1);
                } else if c.mood[crate::crew::DANGER] > 0 && day % 2 == 0 {
                    c.nudge(crate::crew::DANGER, -1);
                }
                if day % 3 == 0 {
                    let t = c.mood[TREATMENT];
                    c.nudge(TREATMENT, -t.signum());
                }
                if c.hurt_days > 0 {
                    let help = u8::from(medic_aboard && c.role != Role::Medic);
                    c.hurt_days = c.hurt_days.saturating_sub(1 + help);
                }
                let score = c.mood_score();
                let delta =
                    i16::try_from(((score + 3 * score.signum()) / 6).clamp(-3, 2)).unwrap_or(0);
                c.shift_loyalty(delta);
            }
            if day >= self.wages_due {
                self.pay_wages(events);
                self.wages_due += WAGE_DAYS;
            }
        }
    }

    fn pay_wages(&mut self, events: &mut Vec<Event>) {
        let due = self.weekly_wages();
        if due <= 0 {
            return;
        }
        if self.cash >= due {
            self.debit(Cause::Crew, due);
            for c in &mut self.crew {
                c.nudge(PAY, 2);
            }
            events.push(Event::WagesPaid { amount: due });
        } else {
            for c in &mut self.crew {
                c.nudge(PAY, -4);
            }
            events.push(Event::WagesMissed { owed: due });
        }
    }

    /// Hands who would leave at a dock walk, alone or together.
    pub(crate) fn crew_unrest(&mut self, env: &mut Env<'_>, events: &mut Vec<Event>) {
        let restless: Vec<usize> = self
            .crew
            .iter()
            .enumerate()
            .filter(|(_, c)| c.restless() && c.mood_score() < 0)
            .map(|(i, _)| i)
            .collect();
        if restless.is_empty() {
            return;
        }
        if restless.len() >= 2 && env.rng.chance(60, 100) {
            let stolen = (self.cash / 5).max(0);
            self.debit(Cause::Crew, stolen);
            let left = u8::try_from(restless.len()).unwrap_or(u8::MAX);
            for i in restless.iter().rev() {
                self.crew.remove(*i);
            }
            self.stats.mutinies = self.stats.mutinies.saturating_add(1);
            for c in &mut self.crew {
                c.nudge(crate::crew::DANGER, -3);
            }
            events.push(Event::Mutiny { left, stolen });
            return;
        }
        for i in restless.iter().rev() {
            if env.rng.chance(50, 100) {
                let gone = self.crew.remove(*i);
                self.stats.deserted = self.stats.deserted.saturating_add(1);
                events.push(Event::Deserted {
                    name: gone.name,
                    goal: gone.goal.text,
                });
            }
        }
    }

    /// Check every hand's goal and swear in those who met it.
    pub(crate) fn check_goals(&mut self, events: &mut Vec<Event>) {
        let hundreds = u32::try_from((self.cash / 100).max(0)).unwrap_or(u32::MAX);
        for c in &mut self.crew {
            if c.goal.kind == GoalKind::Wealth && !c.sworn {
                c.goal.progress = hundreds;
            }
            if !c.sworn && c.goal.met() {
                c.sworn = true;
                c.shift_loyalty(30);
                c.nudge(TREATMENT, 3);
                events.push(Event::GoalMet {
                    name: c.name.clone(),
                    goal: c.goal.text,
                });
            }
        }
    }

    /// The ship reached a system.
    pub(crate) fn goal_arrived(&mut self, system: u16) {
        for c in &mut self.crew {
            if c.goal.kind == GoalKind::Visit && !c.sworn && c.goal.target == u32::from(system) {
                c.goal.progress = 1;
            }
        }
    }

    /// A kind of progress happened (a new dock, a chart, a won fight, a finished contract).
    pub(crate) fn goal_progress(&mut self, kind: GoalKind) {
        for c in &mut self.crew {
            if c.goal.kind == kind && !c.sworn && c.fit() {
                c.goal.progress = c.goal.progress.saturating_add(1);
            }
        }
    }

    pub(crate) fn credit_fight_goal(&mut self) {
        self.goal_progress(GoalKind::Fights);
    }

    /// Hurt a random fit hand for `min..=max` days. A medic shortens it; without one it may scar.
    pub(crate) fn injure_random(
        &mut self,
        rng: &mut DoorRng,
        min: u32,
        max: u32,
        events: &mut Vec<Event>,
    ) {
        let fit: Vec<usize> = self
            .crew
            .iter()
            .enumerate()
            .filter(|(_, c)| c.fit())
            .map(|(i, _)| i)
            .collect();
        if fit.is_empty() {
            return;
        }
        let i = fit[rng.below(u32::try_from(fit.len()).unwrap_or(1)) as usize];
        self.injure(i, rng, min, max, events);
    }

    /// Hurt crew member `index` for `min..=max` days (a medic shortens it; without one it may
    /// scar).
    pub fn injure(
        &mut self,
        index: usize,
        rng: &mut DoorRng,
        min: u32,
        max: u32,
        events: &mut Vec<Event>,
    ) {
        let medic = self.has_role(Role::Medic, 1);
        let Some(c) = self.crew.get_mut(index) else {
            return;
        };
        let mut days = rng.range(min, max);
        if medic && c.role != Role::Medic {
            days = days.saturating_sub(1).max(1);
        }
        c.hurt_days = c.hurt_days.max(u8::try_from(days).unwrap_or(5));
        let scarred = !medic && rng.chance(15, 100);
        if scarred {
            c.scars = c.scars.saturating_add(1).min(5);
        }
        c.nudge(crate::crew::DANGER, -2);
        events.push(Event::CrewHurt {
            name: c.name.clone(),
            days: c.hurt_days,
            scarred,
        });
    }

    /// A random hand dies or is taken off the ship for good.
    pub(crate) fn lose_random_crew(&mut self, rng: &mut DoorRng, events: &mut Vec<Event>) {
        if self.crew.is_empty() {
            return;
        }
        let i = rng.below(u32::try_from(self.crew.len()).unwrap_or(1)) as usize;
        self.lose_crew(i, events);
    }

    /// Lose crew member `index` for good; the rest are shaken.
    pub fn lose_crew(&mut self, index: usize, events: &mut Vec<Event>) {
        if index >= self.crew.len() {
            return;
        }
        let gone = self.crew.remove(index);
        self.stats.crew_lost = self.stats.crew_lost.saturating_add(1);
        for c in &mut self.crew {
            c.nudge(crate::crew::DANGER, -3);
            c.nudge(TREATMENT, -1);
        }
        events.push(Event::CrewLost {
            name: gone.name,
            role: gone.role,
        });
    }

    /// Take `pct` percent of every lot of cargo away; tons lost.
    pub(crate) fn strip_cargo(&mut self, pct: u32) -> u32 {
        let mut lost = 0u32;
        for lot in &mut self.ship.cargo {
            let take = (u32::from(lot.tons) * pct)
                .div_ceil(100)
                .min(u32::from(lot.tons));
            let take16 = u16::try_from(take).unwrap_or(lot.tons);
            let paid_off = if lot.tons == 0 {
                0
            } else {
                lot.paid * i64::from(take16) / i64::from(lot.tons)
            };
            lot.tons -= take16;
            lot.paid -= paid_off;
            lost += take;
        }
        self.ship.cargo.retain(|l| l.tons > 0);
        lost
    }

    /// Remove every ton of contraband; tons taken.
    pub(crate) fn seize_all_contraband(&mut self, content: &Content) -> i64 {
        let mut tons = 0i64;
        self.ship.cargo.retain(|lot| {
            let bad = content
                .goods
                .get(usize::from(lot.good))
                .is_some_and(|g| g.class == GoodClass::Contraband);
            if bad {
                tons += i64::from(lot.tons);
            }
            !bad
        });
        tons
    }

    /// A deed with `faction`: it moves, and its rival moves about a third as far the other way.
    pub fn shift_standing_with_rival(
        &mut self,
        faction: Faction,
        change: i16,
        events: &mut Vec<Event>,
    ) {
        self.shift_standing(faction, change, events);
        if let Some(rival) = faction.rival() {
            // A third as far the other way, rounded away from zero so a small deed still counts.
            let echo = -(change + change.signum() * 2) / 3;
            if echo != 0 {
                self.shift_standing(rival, echo, events);
            }
        }
    }
}
