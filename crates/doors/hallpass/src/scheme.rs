//! Schemes: target, plan, setup, and the sneak / act / escape run; heat, the file, legend,
//! staff who learn, campaigns, and big schemes that change the school for everyone.

use std::sync::Arc;

use russh_bbs_door::DoorRng;

use crate::content::{Method, Target};
use crate::game::{Action, Event, Game, Phase, PhaseResult, Refused, Spent};
use crate::state::{Campaign, Run, Scheme, Status, map_get, map_set, to_i16, to_u8};
use crate::world::WorldDelta;

/// The chance of a phase, with the reasons, so the screen can show why and tests can compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chance {
    /// The final chance in percent, 5 to 95.
    pub total: i32,
    /// Each contribution, by name.
    pub parts: Vec<(String, i32)>,
}

/// What a scheme still needs before it can be run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Readiness {
    /// Items not carried.
    pub missing_items: Vec<String>,
    /// Roles nobody has been recruited for.
    pub missing_roles: Vec<String>,
    /// Scouting level so far.
    pub scout_have: u8,
    /// Scouting level needed.
    pub scout_need: u8,
    /// Whether a time is set.
    pub scheduled: bool,
}

impl Readiness {
    /// Whether nothing is missing.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.missing_items.is_empty()
            && self.missing_roles.is_empty()
            && self.scout_have >= self.scout_need
            && self.scheduled
    }
}

fn has_role(scheme: &Scheme, role: &str) -> bool {
    scheme.allies.iter().any(|(_, r)| r == role)
}

fn fill(text: &str, target: &str, player: &str, place: &str) -> String {
    text.replace("{target}", target)
        .replace("{player}", player)
        .replace("{place}", place)
}

fn roll100(rng: &mut DoorRng) -> i32 {
    i32::try_from(rng.below(100)).unwrap_or(0) + 1
}

impl Game {
    // ------------------------------------------------------------ looking things up

    fn parts(&self, scheme: &Scheme) -> Option<(Method, Target)> {
        Some((
            self.content.method(&scheme.method)?.clone(),
            self.content.target(&scheme.target)?.clone(),
        ))
    }

    /// Where a scheme's deed happens.
    #[must_use]
    pub fn scheme_place(&self, scheme: &Scheme) -> Option<String> {
        let (m, t) = self.parts(scheme)?;
        Some(self.content.act_place(&m, &t).to_owned())
    }

    /// The staff member or prefect a scheme is aimed at, if it is aimed at one.
    fn target_npc(&self, scheme: &Scheme) -> Option<String> {
        self.content.target(&scheme.target)?.npc.clone()
    }

    /// Roles the scheme cannot run without.
    #[must_use]
    pub fn needed_roles(&self, scheme: &Scheme) -> Vec<String> {
        let mut roles: Vec<String> = Vec::new();
        if let Some(m) = self.content.method(&scheme.method) {
            roles.extend(m.roles.iter().cloned());
        }
        for t in scheme.twists.iter().filter_map(|t| self.content.twist(t)) {
            roles.extend(t.roles.iter().cloned());
        }
        if let Some(g) = self.content.getaway(&scheme.getaway) {
            roles.extend(g.roles.iter().cloned());
        }
        roles.sort();
        roles.dedup();
        roles
    }

    /// The scouting level a scheme needs: its method's, and more when the target has learned.
    #[must_use]
    pub fn required_scout(&self, scheme: &Scheme) -> u8 {
        let base = self.content.method(&scheme.method).map_or(0, |m| m.scout);
        let wary = self
            .target_npc(scheme)
            .map_or(0, |n| self.player.wary_of(&n));
        let extra = i32::from(wary >= 3) + i32::from(wary >= 5);
        to_u8((base + extra).min(3))
    }

    /// How many times this exact trick has been played on this target already.
    fn repeats(&self, scheme: &Scheme) -> i32 {
        let key = format!("{}/{}", scheme.target, scheme.method);
        map_get(&self.player.done, &key).map_or(0, |n| i32::from(*n))
    }

    /// What a scheme still needs.
    #[must_use]
    pub fn readiness(&self, scheme: &Scheme) -> Readiness {
        let mut r = Readiness {
            scout_have: scheme.scout,
            scout_need: self.required_scout(scheme),
            scheduled: scheme.when.is_some(),
            ..Readiness::default()
        };
        if let Some(m) = self.content.method(&scheme.method) {
            r.missing_items = m
                .needs
                .iter()
                .filter(|i| self.player.count(i) == 0)
                .cloned()
                .collect();
        }
        r.missing_roles = self
            .needed_roles(scheme)
            .into_iter()
            .filter(|role| !scheme.allies.iter().any(|(_, r)| r == role))
            .collect();
        r
    }

    /// Times a scheme could be done: the place is open, and the target is where the method wants
    /// them (away, or there). This is where character schedules decide what is possible.
    #[must_use]
    pub fn candidate_times(&self, scheme: &Scheme) -> Vec<(i64, u8)> {
        let Some((method, target)) = self.parts(scheme) else {
            return Vec::new();
        };
        let place = self.content.act_place(&method, &target).to_owned();
        let npc = target.npc.as_deref().and_then(|n| self.content.npc(n));
        let rules = &self.content.rules;
        let today = self.player.day;
        let last =
            (self.world.term_start + rules.term_days - 1).min(today + rules.schedule_horizon_days);
        let mut out = Vec::new();
        for day in today..=last {
            for slot in 0..self.periods() {
                if day == today && slot < usize::from(self.player.slot) {
                    continue;
                }
                if !self.hours_ok(&place, day, slot) {
                    continue;
                }
                let presence_ok = npc.is_none_or(|n| {
                    let there = self.npc_place(n, day, slot) == place;
                    match method.present.as_str() {
                        "present" => there,
                        "absent" => !there,
                        _ => true,
                    }
                });
                if presence_ok {
                    out.push((day, to_u8(i32::try_from(slot).unwrap_or(0))));
                }
            }
        }
        out
    }

    /// Whether a place's hours include the period, ignoring any lock.
    fn hours_ok(&self, place: &str, day: i64, slot: usize) -> bool {
        let Some(spec) = self.content.place(place) else {
            return false;
        };
        spec.hours.iter().any(|h| h == self.kind(day, slot))
            && !self
                .content
                .effects
                .iter()
                .any(|e| self.world.has(&e.id) && e.closes.iter().any(|c| c == place))
    }

    /// Method and target pairs the character may start a scheme on.
    #[must_use]
    pub fn drafts(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for m in &self.content.methods {
            if !self.all_hold(&m.requires) {
                continue;
            }
            for t in self
                .content
                .targets
                .iter()
                .filter(|t| self.content.method_fits(m, t))
            {
                out.push((t.id.clone(), m.id.clone()));
            }
        }
        out
    }

    /// Allies the character could ask now for `scheme`: (character, role, cost), who are here,
    /// willing and not already asked lately.
    #[must_use]
    pub fn recruitable(&self, scheme: &Scheme) -> Vec<(String, String, i32)> {
        let Some(slot) = self.current_slot() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for npc in self.npcs_at(&self.player.place, self.player.day, slot) {
            if map_get(&self.player.ally_used, &npc.id).is_some()
                || self.regard(&npc.id) == crate::talk::Regard::Betrayed
            {
                continue;
            }
            for a in &npc.ally {
                if scheme.allies.iter().any(|(_, r)| *r == a.role) || !self.all_hold(&a.needs) {
                    continue;
                }
                out.push((npc.id.clone(), a.role.clone(), a.cost));
            }
        }
        out
    }

    // ------------------------------------------------------------ the odds

    /// The chance of one phase of a scheme right now.
    #[must_use]
    pub fn chance(&self, scheme: &Scheme, phase: Phase, push: u8, flubs: u8) -> Option<Chance> {
        let (method, target) = self.parts(scheme)?;
        let rules = &self.content.rules;
        let place = self.content.act_place(&method, &target).to_owned();
        let (day, slot) = scheme.when.unwrap_or((self.player.day, self.player.slot));
        let slot = usize::from(slot);
        let index = match phase {
            Phase::Sneak => 0,
            Phase::Act => 1,
            Phase::Escape => 2,
        };
        let skill = method.skills.get(index)?;
        let mut parts: Vec<(String, i32)> = vec![("base".into(), rules.base_chance)];
        parts.push((format!("skill: {skill}"), 6 * self.skill_level(skill)));
        let careful = phase != Phase::Escape;
        // Watching a place teaches the way out as well as the way in.
        parts.push(("scouting".into(), 5 * i32::from(scheme.scout)));
        parts.push(("method".into(), -method.difficulty));
        parts.push((
            "twists".into(),
            -scheme
                .twists
                .iter()
                .filter_map(|t| self.content.twist(t))
                .map(|t| t.difficulty)
                .sum::<i32>(),
        ));
        match phase {
            Phase::Sneak if has_role(scheme, "lookout") => {
                parts.push(("a lookout".into(), 10));
            }
            Phase::Sneak if has_role(scheme, "keys") => parts.push(("a key".into(), 8)),
            Phase::Act if has_role(scheme, "gadget") => parts.push(("a gadget".into(), 10)),
            Phase::Escape => {
                let esc = self
                    .content
                    .getaway(&scheme.getaway)
                    .map_or(0, |g| g.escape);
                parts.push(("getaway".into(), esc));
            }
            Phase::Sneak | Phase::Act => {}
        }
        if careful {
            let mut witnesses = self.witnesses_at(&place, day, slot);
            if has_role(scheme, "distraction") {
                witnesses = (witnesses - 2).max(0);
            }
            parts.push((format!("{witnesses} watching"), -4 * witnesses));
            parts.push(("patrols".into(), -6 * self.patrol_level(&place)));
            let rivals = self.rivals_at(&place, day, slot);
            if rivals > 0 {
                parts.push((format!("{rivals} rival eyes"), -5 * rivals));
            }
        }
        let friends = i32::try_from(
            scheme
                .allies
                .iter()
                .filter(|(n, _)| self.regard(n) == crate::talk::Regard::Friend)
                .count(),
        )
        .unwrap_or(0);
        if friends > 0 {
            parts.push(("friends helping".into(), 3 * friends));
        }
        if let Some(npc) = self.target_npc(scheme) {
            parts.push(("they have learned".into(), -5 * self.player.wary_of(&npc)));
        }
        let repeats = self.repeats(scheme);
        if repeats > 0 {
            parts.push(("an old trick".into(), -15 * repeats.min(2)));
        }
        parts.push(("heat".into(), -i32::from(self.player.heat) / 6));
        parts.push(("notoriety".into(), -i32::from(self.player.notoriety) / 8));
        parts.push(("cool".into(), (i32::from(self.player.cool) - 50) / 10));
        parts.push(("nerve".into(), i32::from(push) * rules.nerve_push_bonus));
        parts.push(("earlier slips".into(), -8 * i32::from(flubs)));
        let total = parts.iter().map(|(_, v)| v).sum::<i32>().clamp(5, 95);
        Some(Chance { total, parts })
    }

    /// The chance of being traced back to a pulled-off scheme.
    #[must_use]
    pub fn trace_chance(&self, scheme: &Scheme, flubs: u8) -> i32 {
        let npc_wary = self
            .target_npc(scheme)
            .map_or(0, |n| self.player.wary_of(&n));
        let traits: i32 = self
            .player
            .traits
            .iter()
            .filter_map(|t| self.content.trait_spec(t))
            .map(|t| t.suspicion)
            .sum();
        let twists: i32 = scheme
            .twists
            .iter()
            .filter_map(|t| self.content.twist(t))
            .map(|t| t.suspicion)
            .sum();
        let getaway = self
            .content
            .getaway(&scheme.getaway)
            .map_or(0, |g| g.suspicion);
        let campaign = self
            .player
            .campaign
            .as_ref()
            .map_or(0, |c| 3 * i32::from(c.links));
        (10 + i32::from(self.player.heat) / 2
            + npc_wary * 6
            + i32::from(flubs) * 10
            + i32::from(self.player.notoriety) / 5
            + traits
            + twists
            + getaway
            + campaign
            + ((55 - self.average_grade()) / 5).clamp(-8, 8)
            - i32::from(self.player.cool) / 8)
            .clamp(3, 90)
    }

    // ------------------------------------------------------------ actions

    pub(crate) fn scheme_action(
        &mut self,
        action: &Action,
        rng: &mut DoorRng,
    ) -> Result<Vec<Event>, Refused> {
        match action {
            Action::Draft { target, method } => self.draft(target, method),
            Action::AddTwist(id, twist) => self.add_twist(*id, twist),
            Action::RemoveTwist(id, twist) => {
                self.scheme_mut(*id)?.twists.retain(|t| t != twist);
                Ok(Vec::new())
            }
            Action::SetGetaway(id, getaway) => self.set_getaway(*id, getaway),
            Action::Drop(id) => {
                self.scheme_mut(*id)?;
                self.player.schemes.retain(|s| s.id != *id);
                Ok(vec![Event::Say(format!("You tear up plan #{id}."))])
            }
            Action::Scout(id) => self.scout(*id, rng),
            Action::Recruit { scheme, npc, role } => self.recruit(*scheme, npc, role, rng),
            Action::Schedule { scheme, day, slot } => self.schedule(*scheme, *day, *slot),
            Action::BeginRun(id) => self.begin_run(*id),
            Action::RunPhase { push } => self.run_phase(*push, rng),
            Action::Enrol(_)
            | Action::Go(_)
            | Action::Attend
            | Action::Study(_)
            | Action::Loiter
            | Action::Rest
            | Action::Buy(_)
            | Action::Sleep
            | Action::Answer(_)
            | Action::Abandon
            | Action::Talk(_)
            | Action::Choose(_)
            | Action::Join(_)
            | Action::Quit(_)
            | Action::Meet => Err(Refused::new("That is not a scheme action.")),
        }
    }

    fn scheme_mut(&mut self, id: u16) -> Result<&mut Scheme, Refused> {
        self.player
            .schemes
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| Refused::new("You have no such plan."))
    }

    fn draft(&mut self, target: &str, method: &str) -> Result<Vec<Event>, Refused> {
        if self.player.schemes.len() >= self.content.rules.max_schemes {
            return Err(Refused::new(
                "You have enough plans on the go. Finish one or tear one up.",
            ));
        }
        let m = self
            .content
            .method(method)
            .cloned()
            .ok_or_else(|| Refused::new("That is not a trick you know."))?;
        let t = self
            .content
            .target(target)
            .cloned()
            .ok_or_else(|| Refused::new("That is not a target."))?;
        if !self.content.method_fits(&m, &t) {
            return Err(Refused::new("That trick does not suit that target."));
        }
        if !self.all_hold(&m.requires) {
            return Err(Refused::new(
                "You are not yet the sort of person who could pull that off.",
            ));
        }
        let id = self.player.next_scheme;
        self.player.next_scheme = self.player.next_scheme.saturating_add(1);
        let created = self.player.day;
        self.player.schemes.push(Scheme {
            id,
            target: target.to_owned(),
            method: method.to_owned(),
            twists: Vec::new(),
            getaway: "slip_away".to_owned(),
            scout: 0,
            allies: Vec::new(),
            when: None,
            created,
        });
        Ok(vec![Event::Say(format!(
            "Plan #{id}: {} - {}.",
            m.name, t.name
        ))])
    }

    fn add_twist(&mut self, id: u16, twist: &str) -> Result<Vec<Event>, Refused> {
        let t = self
            .content
            .twist(twist)
            .cloned()
            .ok_or_else(|| Refused::new("No such twist."))?;
        if !self.all_hold(&t.requires) {
            return Err(Refused::new(
                "You do not have the knack for that twist yet.",
            ));
        }
        let max = self.content.rules.max_twists;
        let scheme = self.scheme_mut(id)?;
        if scheme.twists.iter().any(|x| x == twist) || scheme.twists.len() >= max {
            return Err(Refused::new("That plan cannot take another twist."));
        }
        scheme.twists.push(twist.to_owned());
        Ok(vec![Event::Say(format!("Added: {}.", t.name))])
    }

    fn set_getaway(&mut self, id: u16, getaway: &str) -> Result<Vec<Event>, Refused> {
        let g = self
            .content
            .getaway(getaway)
            .cloned()
            .ok_or_else(|| Refused::new("No such getaway."))?;
        if !self.all_hold(&g.requires) {
            return Err(Refused::new("You cannot manage that getaway yet."));
        }
        getaway.clone_into(&mut self.scheme_mut(id)?.getaway);
        Ok(vec![Event::Say(format!("Getaway: {}.", g.name))])
    }

    fn scout(&mut self, id: u16, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        let slot = self
            .current_slot()
            .ok_or_else(|| Refused::new("The day is over."))?;
        let scheme = self
            .player
            .scheme(id)
            .cloned()
            .ok_or_else(|| Refused::new("You have no such plan."))?;
        let place = self
            .scheme_place(&scheme)
            .ok_or_else(|| Refused::new("That plan has gone odd."))?;
        if scheme.scout >= 3 {
            return Err(Refused::new("You know that place as well as you will."));
        }
        if !self.near(&self.player.place, &place) {
            return Err(Refused::new(
                "You can only watch a place from inside it or from next door.",
            ));
        }
        let day = self.player.day;
        let npc_wary = self
            .target_npc(&scheme)
            .map_or(0, |n| self.player.wary_of(&n));
        let chance = (70 + 6 * self.skill_level("sneak")
            - 5 * self.witnesses_at(&place, day, slot)
            - 4 * npc_wary)
            .clamp(20, 95);
        let mut events = Vec::new();
        if roll100(rng) <= chance {
            let level = scheme.scout + 1;
            self.scheme_mut(id)?.scout = level;
            events.push(Event::Say(format!(
                "You watch the {} for a period and learn its rhythm (scouting {level}/3).",
                self.content
                    .place(&place)
                    .map_or("place", |p| p.name.as_str())
            )));
            let grants: Vec<String> = self
                .content
                .grants
                .iter()
                .filter(|g| (g.place == "*" || g.place == place) && i32::from(level) >= g.level)
                .map(|g| g.know.clone())
                .collect();
            for know in grants {
                if !self.player.knows(&know) {
                    self.player.learn(&know);
                    events.push(Event::Say(format!(
                        "You have worked out something useful: {}.",
                        know.replace('_', " ")
                    )));
                }
            }
            self.gain_xp("sneak", 1);
        } else {
            self.player.heat = to_u8(i32::from(self.player.heat) + 2);
            events.push(Event::Say("Somebody looked your way, and you had to pretend to tie a shoelace. Nothing learned.".into()));
        }
        events.extend(self.end_slot(Spent::Other, rng));
        Ok(events)
    }

    fn recruit(
        &mut self,
        id: u16,
        npc_id: &str,
        role: &str,
        rng: &mut DoorRng,
    ) -> Result<Vec<Event>, Refused> {
        self.current_slot()
            .ok_or_else(|| Refused::new("The day is over."))?;
        let scheme = self
            .player
            .scheme(id)
            .cloned()
            .ok_or_else(|| Refused::new("You have no such plan."))?;
        if scheme.allies.len() >= 3 {
            return Err(Refused::new("Three helpers is already a crowd."));
        }
        let (_, _, cost) = self
            .recruitable(&scheme)
            .into_iter()
            .find(|(n, r, _)| n == npc_id && r == role)
            .ok_or_else(|| {
                Refused::new("They are not here, not willing, or you asked them too recently.")
            })?;
        if self.player.cash < cost {
            return Err(Refused::new("You cannot afford their price."));
        }
        let npc = self
            .content
            .npc(npc_id)
            .cloned()
            .ok_or_else(|| Refused::new("No such person."))?;
        let grants: Vec<String> = npc
            .ally
            .iter()
            .find(|a| a.role == role)
            .map(|a| a.grants.clone())
            .unwrap_or_default();
        self.player.cash -= cost;
        let day = self.player.day;
        map_set(&mut self.player.ally_used, npc_id, day);
        self.scheme_mut(id)?
            .allies
            .push((npc_id.to_owned(), role.to_owned()));
        if let Some(c) = &npc.clique {
            self.player.add_standing(c, 2);
        }
        for g in grants {
            self.player.learn(&g);
        }
        let mut events = self.change_bond(npc_id, 1);
        events.push(Event::Say(format!(
            "{} agrees to be your {role}.",
            npc.name
        )));
        events.extend(self.end_slot(Spent::Other, rng));
        Ok(events)
    }

    fn schedule(&mut self, id: u16, day: i64, slot: u8) -> Result<Vec<Event>, Refused> {
        let scheme = self
            .player
            .scheme(id)
            .cloned()
            .ok_or_else(|| Refused::new("You have no such plan."))?;
        if !self.candidate_times(&scheme).contains(&(day, slot)) {
            return Err(Refused::new("That is not a time that suits the plan."));
        }
        self.scheme_mut(id)?.when = Some((day, slot));
        Ok(vec![Event::Scheduled {
            scheme: id,
            day,
            slot,
        }])
    }

    fn begin_run(&mut self, id: u16) -> Result<Vec<Event>, Refused> {
        let slot = self
            .current_slot()
            .ok_or_else(|| Refused::new("The day is over."))?;
        let scheme = self
            .player
            .scheme(id)
            .cloned()
            .ok_or_else(|| Refused::new("You have no such plan."))?;
        let (method, _) = self
            .parts(&scheme)
            .ok_or_else(|| Refused::new("That plan has gone odd."))?;
        if scheme.when != Some((self.player.day, to_u8(i32::try_from(slot).unwrap_or(0)))) {
            return Err(Refused::new("This is not the moment you planned for."));
        }
        let place = self
            .scheme_place(&scheme)
            .ok_or_else(|| Refused::new("That plan has gone odd."))?;
        if self.player.place != place {
            return Err(Refused::new("You are not where the plan needs you."));
        }
        if !self.place_open(&place, self.player.day, slot) {
            return Err(Refused::new("The way in is shut to you."));
        }
        let ready = self.readiness(&scheme);
        if !ready.ready() {
            return Err(Refused::new(
                "The plan is not ready: something is still missing.",
            ));
        }
        for item in &method.needs {
            let have = self.player.count(item);
            map_set(&mut self.player.inventory, item, have.saturating_sub(1));
        }
        self.player.inventory.retain(|(_, n)| *n > 0);
        self.player.run = Some(Run {
            scheme: id,
            phase: 0,
            flubs: 0,
            mishap_heat: 0,
            covered: false,
        });
        Ok(vec![Event::Say(format!("Here goes: {}.", method.name))])
    }

    fn run_phase(&mut self, push: u8, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        let run = self
            .player
            .run
            .clone()
            .ok_or_else(|| Refused::new("No scheme is under way."))?;
        let scheme = self
            .player
            .scheme(run.scheme)
            .cloned()
            .ok_or_else(|| Refused::new("That plan has gone odd."))?;
        let phase =
            Phase::from_index(run.phase).ok_or_else(|| Refused::new("That plan has gone odd."))?;
        let push = push.min(3);
        if push > self.player.nerve {
            return Err(Refused::new("You do not have that much nerve left."));
        }
        let chance = self
            .chance(&scheme, phase, push, run.flubs)
            .ok_or_else(|| Refused::new("That plan has gone odd."))?;
        self.player.nerve -= push;
        let roll = roll100(rng);
        let (method, target) = self
            .parts(&scheme)
            .ok_or_else(|| Refused::new("That plan has gone odd."))?;
        let place_name = self
            .content
            .place(self.content.act_place(&method, &target))
            .map_or("the place", |p| p.name.as_str())
            .to_owned();
        let skill = method
            .skills
            .get(usize::from(run.phase))
            .cloned()
            .unwrap_or_default();
        let margin = roll - chance.total;
        let mut run_now = run;
        let mut events = Vec::new();
        let cover_available = has_role(&scheme, "cover") && !run_now.covered;
        let result = if margin <= 0 {
            let lines: Vec<&str> = self
                .content
                .lines
                .iter()
                .filter(|l| l.phase == phase.name())
                .map(|l| l.text.as_str())
                .collect();
            let pick = usize::try_from(rng.below(u32::try_from(lines.len().max(1)).unwrap_or(1)))
                .unwrap_or(0);
            let line = lines
                .get(pick)
                .map(|l| fill(l, &target.name, &self.player.name, &place_name))
                .unwrap_or_default();
            events.push(Event::Say(line));
            PhaseResult::Clean
        } else if margin > self.content.rules.flub_margin && !cover_available {
            PhaseResult::Busted
        } else {
            let covered = margin > self.content.rules.flub_margin;
            if covered {
                run_now.covered = true;
                events.push(Event::Say("It was about to go badly wrong. Your cover story steps in, and the moment passes.".into()));
            }
            let mishaps: Vec<(String, i32)> = self
                .content
                .mishaps
                .iter()
                .filter(|m| m.phase == phase.name())
                .map(|m| (m.text.clone(), m.heat))
                .collect();
            let pick = usize::try_from(rng.below(u32::try_from(mishaps.len().max(1)).unwrap_or(1)))
                .unwrap_or(0);
            if let Some((text, heat)) = mishaps.get(pick) {
                events.push(Event::Say(text.clone()));
                run_now.mishap_heat = run_now.mishap_heat.saturating_add(to_u8(*heat));
            }
            run_now.flubs = run_now.flubs.saturating_add(1);
            if covered {
                PhaseResult::Covered
            } else {
                PhaseResult::Flub
            }
        };
        events.insert(
            0,
            Event::Phase {
                phase,
                chance: chance.total,
                roll,
                result,
            },
        );
        if result != PhaseResult::Busted
            && let Some(skill_up) = self.gain_xp(&skill, 1)
        {
            events.push(Event::Say(format!("You are getting better at {skill_up}.")));
        }
        if result == PhaseResult::Busted {
            events.extend(self.finish_run(&scheme, &run_now, Some(phase), rng));
        } else if phase == Phase::Escape {
            events.extend(self.finish_run(&scheme, &run_now, None, rng));
        } else {
            run_now.phase += 1;
            self.player.run = Some(run_now);
        }
        Ok(events)
    }

    // ------------------------------------------------------------ the aftermath

    fn finish_run(
        &mut self,
        scheme: &Scheme,
        run: &Run,
        busted: Option<Phase>,
        rng: &mut DoorRng,
    ) -> Vec<Event> {
        let content = Arc::clone(&self.content);
        let mut events = Vec::new();
        let (Some(method), Some(target)) = (
            content.method(&scheme.method),
            content.target(&scheme.target),
        ) else {
            return events;
        };
        let place = content.act_place(method, target).to_owned();
        let key = format!("{}/{}", scheme.target, scheme.method);
        let repeats = self.repeats(scheme);
        let done = map_get(&self.player.done, &key).copied().unwrap_or(0);
        map_set(&mut self.player.done, &key, done.saturating_add(1));
        self.player.run = None;
        self.player.schemes.retain(|s| s.id != scheme.id);

        let heat_pct = 100
            + content
                .effects
                .iter()
                .filter(|e| self.world.has(&e.id))
                .map(|e| e.heat_pct)
                .sum::<i32>();
        let mut heat = method.heat
            + scheme
                .twists
                .iter()
                .filter_map(|t| content.twist(t))
                .map(|t| t.heat)
                .sum::<i32>()
            + content.getaway(&scheme.getaway).map_or(0, |g| g.heat)
            + i32::from(run.mishap_heat)
            + 3 * i32::from(run.flubs);
        if busted.is_some() {
            heat = heat * 3 / 2;
        }
        let heat = (heat * heat_pct / 100).max(0);
        self.player.heat = to_u8(i32::from(self.player.heat) + heat);

        if let Some(phase) = busted {
            self.player.caught = self.player.caught.saturating_add(1);
            self.player.campaign = None;
            self.player.cool = to_i16(i32::from(self.player.cool) - 5);
            self.player.add_standing("the_bench", 1);
            if let Some(npc) = &target.npc {
                self.bump_wary(npc, 2);
            }
            self.push_world(WorldDelta::Patrol {
                place: place.clone(),
                amount: 3,
            });
            self.player.detention =
                self.player
                    .detention
                    .saturating_add(if method.big { 2 } else { 1 });
            events.push(Event::Busted {
                scheme: scheme.id,
                phase,
            });
            events.push(Event::Say(format!(
                "Caught in the act at {}. The {} will not be forgotten.",
                place.replace('_', " "),
                method.name.to_lowercase()
            )));
            events.extend(self.add_file("caught", to_u8(method.severity)));
        } else {
            events.extend(self.pulled_off(scheme, run, method, target, &place, repeats, rng));
        }
        events.extend(self.allies_remember(&scheme.allies, busted.is_some()));
        if self.player.status == Status::Active {
            events.extend(self.end_slot(Spent::Other, rng));
            if content.getaway(&scheme.getaway).is_some_and(|g| g.lies_low)
                && busted.is_none()
                && self.current_slot().is_some()
            {
                self.player.heat =
                    to_u8(i32::from(self.player.heat) - content.rules.heat_decay_low);
                events.push(Event::Say(
                    "You spend the next period out of sight, and the heat eases.".into(),
                ));
                events.extend(self.end_slot(Spent::Other, rng));
            }
            events.extend(self.check_traits());
        }
        events
    }

    #[allow(clippy::too_many_arguments)]
    fn pulled_off(
        &mut self,
        scheme: &Scheme,
        run: &Run,
        method: &Method,
        target: &Target,
        place: &str,
        repeats: i32,
        rng: &mut DoorRng,
    ) -> Vec<Event> {
        let content = Arc::clone(&self.content);
        let rules = &content.rules;
        let mut events = Vec::new();
        let clean = run.flubs == 0;
        // Legend: the method's, scaled by twists, synergies, the campaign, the world, and how clean it was.
        let campaign_links = self
            .player
            .campaign
            .as_ref()
            .filter(|c| {
                c.theme == scheme.target
                    && self.player.day - c.last_day <= rules.campaign_window_days
            })
            .map_or(0, |c| i32::from(c.links));
        let mut pct = 100
            + scheme
                .twists
                .iter()
                .filter_map(|t| content.twist(t))
                .map(|t| t.legend_pct)
                .sum::<i32>();
        for s in &content.synergies {
            if scheme.twists.contains(&s.twist) && s.methods.contains(&method.id) {
                pct += s.legend_pct;
                events.push(Event::Say(s.text.clone()));
            }
        }
        pct += rules.campaign_step_pct * campaign_links.min(rules.campaign_max_links);
        pct += content
            .effects
            .iter()
            .filter(|e| self.world.has(&e.id))
            .map(|e| e.legend_pct)
            .sum::<i32>();
        let tier = if clean {
            rules.legend_clean_pct
        } else {
            rules.legend_messy_pct
        };
        let old_trick = if repeats > 0 { 50 } else { 100 };
        let raw = i64::from(method.legend)
            * i64::from(pct.max(10))
            * i64::from(tier)
            * i64::from(old_trick)
            / 1_000_000;
        let mut gain = u32::try_from(raw.max(1)).unwrap_or(1);
        if campaign_links > 0 {
            events.push(Event::Say(format!(
                "Link {} in a campaign against {}: it counts for more.",
                campaign_links + 1,
                target.name
            )));
        }
        if repeats > 0 {
            events.push(Event::Say(
                "They have seen this trick before. It lands softly.".into(),
            ));
        }

        // Traced back?
        let heat_after = self.trace_chance(scheme, run.flubs);
        let pinned = roll100(rng) <= heat_after;
        if pinned {
            gain = gain * u32::try_from(rules.pinned_legend_pct.max(0)).unwrap_or(100) / 100;
        }
        self.player.legend = self.player.legend.saturating_add(gain);
        self.player.pulled = self.player.pulled.saturating_add(1);
        self.player.cool = to_i16(i32::from(self.player.cool) + 2);
        self.player.notoriety = to_u8(i32::from(self.player.notoriety) + if clean { 1 } else { 2 });
        self.player.add_standing("the_bench", 3);
        self.player.add_standing("lamplighters", -3);
        if let Some(clique) = &target.clique {
            self.player
                .add_standing(clique, if pinned { -16 } else { -8 });
            if let Some(c) = content.clique(clique) {
                for rival in &c.rivals {
                    self.player.add_standing(rival, 5);
                }
            }
        }
        if let Some(npc) = &target.npc {
            self.bump_wary(npc, if method.big { 2 } else { 1 });
        }
        // The campaign grows if this extends it, and starts if it does not.
        let day = self.player.day;
        self.player.campaign = Some(Campaign {
            theme: scheme.target.clone(),
            links: to_u8(campaign_links + 1),
            last_day: day,
        });
        events.push(Event::Pulled {
            scheme: scheme.id,
            legend: gain,
            clean,
        });
        events.push(Event::Say(fill(
            &method.story,
            &target.name,
            &self.player.name,
            place,
        )));

        // The shared school remembers.
        let patrol = if clean { 1 } else { 2 };
        self.push_world(WorldDelta::Patrol {
            place: place.to_owned(),
            amount: patrol,
        });
        let name = self.player.name.clone();
        let legend = self.player.legend;
        self.push_world(WorldDelta::Hall {
            name: name.clone(),
            legend,
        });
        if let Some(effect_id) = &method.effect {
            self.player.big = self.player.big.saturating_add(1);
            self.push_world(WorldDelta::Effect {
                id: effect_id.clone(),
                by: name.clone(),
                day,
            });
            if let Some(effect) = content.effect(effect_id) {
                self.push_world(WorldDelta::Headline {
                    day,
                    text: fill(&effect.headline, &target.name, &name, place),
                });
            }
            events.push(Event::WorldChange(effect_id.clone()));
        } else if gain >= 10 {
            self.push_world(WorldDelta::Headline {
                day,
                text: fill(&method.story, &target.name, &name, place),
            });
        }
        if pinned {
            self.player.pinned = self.player.pinned.saturating_add(1);
            events.push(Event::Pinned { scheme: scheme.id });
            if let Some(npc) = &target.npc {
                self.bump_wary(npc, 1);
            }
            events.extend(self.add_file("pinned", to_u8(method.severity)));
        }
        events
    }
}
