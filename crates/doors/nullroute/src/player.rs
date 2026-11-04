//! One player: the character, the rig, the deck, the hideout, and the rules that change them.
//!
//! Nothing here touches the clock, the store or the RNG except through its arguments, so the
//! player is as replayable as the run.

use russh_bbs_door::DoorRng;

use crate::cm;
use crate::code::{self, Build, CODE_CYCLES, CODED_BASE, CODED_MAX, FRAGMENTS_MAX};
use crate::content::{Content, Family, ScriptDef};
use crate::net::Network;
use crate::run::{Run, RunResult, RunSetup, Status};
use crate::world::{Contract, World};

/// Marks a new character starts with: enough for one tool script and a proxy.
pub const START_MARKS: u32 = 90;
/// The deck's hard limit on scripts.
pub const DECK_MAX: usize = 6;
/// Cycles a day with a bare rig.
pub const BASE_CYCLES: u32 = 40;
/// Cycles to jack in.
pub const JACK_IN_CYCLES: u32 = 5;
/// The most days of catch-up applied after time away.
pub const MAX_CATCH_UP_DAYS: i64 = 7;
/// Heat is capped here.
pub const HEAT_MAX: u32 = 100;
/// Contract ids remembered as done.
pub const DONE_MAX: usize = 48;
/// The most fixers a save tracks.
pub const FIXERS_MAX: usize = 8;
/// Rig parts: cpu, memory, cooling, interface, shield.
pub const PARTS: usize = 5;
/// Skill thresholds: total uses of a family needed for level 1 to 5.
pub const SKILL_STEPS: [u32; 5] = [4, 12, 30, 60, 100];

/// The part numbers.
pub mod part {
    /// Cycles a day.
    pub const CPU: usize = 0;
    /// Deck memory and slots.
    pub const MEMORY: usize = 1;
    /// Fry resistance.
    pub const COOLING: usize = 2;
    /// Success and script speed.
    pub const IFACE: usize = 3;
    /// Trace resistance.
    pub const SHIELD: usize = 4;
}

/// A character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    /// Seeds the nightly catch-up so reconnecting cannot reroll a night.
    pub seed: u64,
    /// The season the character was last played in.
    pub season: u32,
    /// The handle.
    pub name: String,
    /// The last day rolled over.
    pub day: i64,
    /// Cycles left today.
    pub cycles: u32,
    /// Marks on hand.
    pub marks: u32,
    /// Marks in the safe: raids cannot take them.
    pub stash: u32,
    /// Heat, 0 to 100.
    pub heat: u32,
    /// Reputation.
    pub rep: u32,
    /// Rig levels, 0 to 3 each.
    pub rig: [u8; PARTS],
    /// Bit set of fried parts.
    pub fried: u8,
    /// Scripts owned, by number.
    pub owned: Vec<u8>,
    /// Scripts loaded, by number.
    pub deck: Vec<u8>,
    /// Uses per family, for the skills.
    pub xp: [u32; 5],
    /// Bots at the hideout.
    pub bots: u8,
    /// Whether the workbench is built (coding is the next slice).
    pub workbench: bool,
    /// Fence level.
    pub fence: u8,
    /// Standing with each fixer.
    pub standing: [i8; FIXERS_MAX],
    /// Contracts done (ids).
    pub done: Vec<u32>,
    /// A run in flight: the contract id. Still set on open means the player dropped the line.
    pub pending: Option<u32>,
    /// Proxies bought for the next run.
    pub proxies: u8,
    /// Runs started.
    pub runs: u32,
    /// Objectives completed.
    pub jobs: u32,
    /// Times burned.
    pub burns: u32,
    /// Marks earned in total.
    pub earned: u64,
    /// Fragments carried, for the workbench.
    pub fragments: Vec<u8>,
    /// Coded scripts: slot `n` is script number `CODED_BASE + n`.
    pub coded: [Option<Build>; CODED_MAX],
}

/// The skill level for `xp` uses.
#[must_use]
pub fn skill_level(xp: u32) -> u8 {
    u8::try_from(SKILL_STEPS.iter().filter(|step| xp >= **step).count()).unwrap_or(5)
}

impl Player {
    /// A new character with the starter scripts and nothing else.
    #[must_use]
    pub fn new(content: &Content, seed: u64, name: &str, season: u32, today: i64) -> Self {
        let owned: Vec<u8> = content
            .scripts
            .script
            .iter()
            .filter(|s| s.starter)
            .map(|s| s.num)
            .collect();
        let mut player = Self {
            seed,
            season,
            name: name.to_owned(),
            day: today,
            cycles: BASE_CYCLES,
            marks: START_MARKS,
            stash: 0,
            heat: 0,
            rep: 0,
            rig: [0; PARTS],
            fried: 0,
            owned,
            deck: Vec::new(),
            xp: [0; 5],
            bots: 0,
            workbench: false,
            fence: 0,
            standing: [0; FIXERS_MAX],
            done: Vec::new(),
            pending: None,
            proxies: 0,
            runs: 0,
            jobs: 0,
            burns: 0,
            earned: 0,
            fragments: Vec::new(),
            coded: [None; CODED_MAX],
        };
        // A sensible first deck: one scan, the first two breaks and a copier, within memory.
        for num in [1u8, 5, 8, 23] {
            let _ = player.load(content, num);
        }
        player.cycles = player.max_cycles();
        player
    }

    /// A character carried into a new season: the name, the skills, the scripts and the rig stay;
    /// money, heat, standing, bots and jobs start over.
    pub fn new_season(&mut self, content: &Content, season: u32, today: i64) {
        let keep = Self::new(content, self.seed, &self.name, season, today);
        self.season = season;
        self.day = today;
        self.marks = keep.marks;
        self.stash = 0;
        self.heat = 0;
        self.rep /= 2;
        self.bots = 0;
        self.fence = 0;
        self.standing = [0; FIXERS_MAX];
        self.done.clear();
        self.pending = None;
        self.proxies = 0;
        self.fried = 0;
        self.cycles = self.max_cycles();
    }

    /// A part's effective level: a fried part counts as level 0 until it is repaired.
    #[must_use]
    pub fn level(&self, part: usize) -> u8 {
        if self.fried & (1 << part) != 0 {
            0
        } else {
            self.rig.get(part).copied().unwrap_or(0)
        }
    }

    /// Cycles a day.
    #[must_use]
    pub fn max_cycles(&self) -> u32 {
        BASE_CYCLES + 4 * u32::from(self.level(part::CPU))
    }

    /// A script by number, stock or coded.
    #[must_use]
    pub fn script(&self, content: &Content, num: u8) -> Option<ScriptDef> {
        if num >= CODED_BASE {
            let build = (*self.coded.get(usize::from(num - CODED_BASE))?)?;
            code::assemble(content, build, num).ok()
        } else {
            content.script(num).cloned()
        }
    }

    /// Deck memory.
    #[must_use]
    pub fn memory(&self) -> u32 {
        8 + 4 * u32::from(self.level(part::MEMORY))
    }

    /// Deck slots.
    #[must_use]
    pub fn slots(&self) -> usize {
        (4 + usize::from(self.level(part::MEMORY))).min(DECK_MAX)
    }

    /// Memory the deck uses.
    #[must_use]
    pub fn deck_memory(&self, content: &Content) -> u32 {
        self.deck
            .iter()
            .filter_map(|n| self.script(content, *n))
            .map(|s| s.memory)
            .sum()
    }

    /// The skill level in a family.
    #[must_use]
    pub fn skill(&self, family: Family) -> u8 {
        skill_level(self.xp.get(family.index()).copied().unwrap_or(0))
    }

    /// The skill levels in `Family::ALL` order.
    #[must_use]
    pub fn skills(&self) -> [u8; 5] {
        Family::ALL.map(|f| self.skill(f))
    }

    /// Put a script in the deck.
    ///
    /// # Errors
    /// Why not.
    pub fn load(&mut self, content: &Content, num: u8) -> Result<String, String> {
        let script = self.script(content, num).ok_or("No such script.")?;
        if !self.owned.contains(&num) {
            return Err("You do not own that script.".into());
        }
        if self.deck.contains(&num) {
            return Err("Already loaded.".into());
        }
        if self.deck.len() >= self.slots() {
            return Err(format!(
                "No free slot ({} of {}).",
                self.deck.len(),
                self.slots()
            ));
        }
        if self.deck_memory(content) + script.memory > self.memory() {
            return Err(format!(
                "Not enough memory: {} + {} is over {}.",
                self.deck_memory(content),
                script.memory,
                self.memory()
            ));
        }
        if self.skill(script.family) < u8::try_from(script.min_skill).unwrap_or(5) {
            return Err(format!(
                "{} needs {} skill {}.",
                script.name,
                script.family.word(),
                script.min_skill
            ));
        }
        self.deck.push(num);
        Ok(format!("{} loaded.", script.name))
    }

    /// Take a script out of the deck.
    ///
    /// # Errors
    /// If it was not loaded.
    pub fn unload(&mut self, content: &Content, num: u8) -> Result<String, String> {
        let at = self
            .deck
            .iter()
            .position(|n| *n == num)
            .ok_or("Not loaded.")?;
        self.deck.remove(at);
        Ok(format!(
            "{} unloaded.",
            self.script(content, num)
                .map_or_else(|| "script".to_owned(), |s| s.name)
        ))
    }

    /// Buy a script at the market price.
    ///
    /// # Errors
    /// Why not.
    pub fn buy_script(
        &mut self,
        content: &Content,
        num: u8,
        market_pct: u32,
    ) -> Result<String, String> {
        let script = content.script(num).ok_or("No such script.")?;
        if self.owned.contains(&num) {
            return Err("You already own it.".into());
        }
        if script.price == 0 {
            return Err("Not for sale.".into());
        }
        if self.rep < script.min_rep {
            return Err(format!(
                "Nobody sells you that yet (reputation {} needed).",
                script.min_rep
            ));
        }
        let price = script.price * market_pct / 100;
        if self.marks < price {
            return Err(format!("{price} marks needed."));
        }
        self.marks -= price;
        self.owned.push(num);
        Ok(format!("Bought {} for {price}.", script.name))
    }

    /// The cost to upgrade a part, if it can be.
    #[must_use]
    pub fn upgrade_cost(&self, content: &Content, part: usize) -> Option<u32> {
        let def = content.rig.part.get(part)?;
        def.levels
            .get(usize::from(*self.rig.get(part)?))
            .map(|l| l.cost)
    }

    /// Upgrade a rig part.
    ///
    /// # Errors
    /// Why not.
    pub fn upgrade(&mut self, content: &Content, part: usize) -> Result<String, String> {
        if self.fried & (1 << part) != 0 {
            return Err("Repair it first.".into());
        }
        let cost = self
            .upgrade_cost(content, part)
            .ok_or("Already at the top.")?;
        if self.marks < cost {
            return Err(format!("{cost} marks needed."));
        }
        self.marks -= cost;
        if let Some(level) = self.rig.get_mut(part) {
            *level += 1;
        }
        let name = content
            .rig
            .part
            .get(part)
            .and_then(|p| p.levels.get(usize::from(self.rig[part]).saturating_sub(1)))
            .map_or("part", |l| l.name.as_str());
        Ok(format!("Fitted: {name}."))
    }

    /// The cost to repair a fried part.
    #[must_use]
    pub fn repair_cost(&self, content: &Content, part: usize) -> u32 {
        let level = usize::from(self.rig.get(part).copied().unwrap_or(0)).max(1);
        content
            .rig
            .part
            .get(part)
            .and_then(|p| p.levels.get(level - 1))
            .map_or(0, |l| l.cost * content.rig.hideout.repair_percent / 100)
    }

    /// Repair a fried part.
    ///
    /// # Errors
    /// Why not.
    pub fn repair(&mut self, content: &Content, part: usize) -> Result<String, String> {
        if self.fried & (1 << part) == 0 {
            return Err("It is not fried.".into());
        }
        let cost = self.repair_cost(content, part);
        if self.marks < cost {
            return Err(format!("{cost} marks needed."));
        }
        self.marks -= cost;
        self.fried &= !(1 << part);
        Ok("Repaired.".into())
    }

    /// Buy a bot for the hideout.
    ///
    /// # Errors
    /// Why not.
    pub fn buy_bot(&mut self, content: &Content) -> Result<String, String> {
        let h = &content.rig.hideout;
        if self.bots >= h.bot_max {
            return Err("The hideout cannot power more bots.".into());
        }
        if self.marks < h.bot_cost {
            return Err(format!("{} marks needed.", h.bot_cost));
        }
        self.marks -= h.bot_cost;
        self.bots += 1;
        Ok(format!(
            "A bot starts earning: {} marks a night.",
            h.bot_income
        ))
    }

    /// Build the workbench.
    ///
    /// # Errors
    /// Why not.
    pub fn buy_workbench(&mut self, content: &Content) -> Result<String, String> {
        let cost = content.rig.hideout.workbench_cost;
        if self.workbench {
            return Err("You have a workbench.".into());
        }
        if self.marks < cost {
            return Err(format!("{cost} marks needed."));
        }
        self.marks -= cost;
        self.workbench = true;
        Ok("The workbench is up. Fragments you find on runs can be coded into scripts.".into())
    }

    /// How many of fragment `num` the character carries.
    #[must_use]
    pub fn fragment_count(&self, num: u8) -> usize {
        self.fragments
            .iter()
            .fold(0, |n, f| n + usize::from(*f == num))
    }

    /// Code a new script at the workbench: burns the three fragments and [`CODE_CYCLES`] cycles.
    ///
    /// # Errors
    /// Why not; nothing is spent then.
    pub fn code(&mut self, content: &Content, build: Build) -> Result<String, String> {
        if !self.workbench {
            return Err("You need a workbench to code.".into());
        }
        if self.cycles < CODE_CYCLES {
            return Err(format!("{CODE_CYCLES} cycles needed to code."));
        }
        let slot = self
            .coded
            .iter()
            .position(Option::is_none)
            .ok_or("No room for another coded script. Scrap one.")?;
        if self.coded.contains(&Some(build)) {
            return Err("You already have that build.".into());
        }
        let num = CODED_BASE + u8::try_from(slot).unwrap_or(0);
        let script = code::assemble(content, build, num)?;
        let mut left = self.fragments.clone();
        for frag in [build.effect, build.target, build.modifier] {
            if frag == 0 {
                continue;
            }
            let at = left
                .iter()
                .position(|n| *n == frag)
                .ok_or("You do not have those fragments.")?;
            left.remove(at);
        }
        self.fragments = left;
        self.cycles -= CODE_CYCLES;
        self.coded[slot] = Some(build);
        self.owned.push(num);
        Ok(format!(
            "Coded {} (power {}, memory {}, noise {}).",
            script.name, script.power, script.memory, script.noise
        ))
    }

    /// Delete a coded script (its fragments are gone for good).
    ///
    /// # Errors
    /// If it is not a coded script of yours.
    pub fn scrap(&mut self, num: u8) -> Result<String, String> {
        let slot = num
            .checked_sub(CODED_BASE)
            .map(usize::from)
            .filter(|s| self.coded.get(*s).is_some_and(Option::is_some))
            .ok_or("That is not a coded script.")?;
        self.coded[slot] = None;
        self.owned.retain(|n| *n != num);
        self.deck.retain(|n| *n != num);
        Ok("Scrapped.".into())
    }

    /// Find a better fence.
    ///
    /// # Errors
    /// Why not.
    pub fn upgrade_fence(&mut self, content: &Content) -> Result<String, String> {
        let next = content
            .rig
            .fence
            .get(usize::from(self.fence) + 1)
            .ok_or("This is the best fence there is.")?;
        if self.marks < next.cost {
            return Err(format!("{} marks needed.", next.cost));
        }
        self.marks -= next.cost;
        self.fence += 1;
        Ok(format!(
            "{} now takes your data at {} percent.",
            next.name, next.rate
        ))
    }

    /// Move marks into the safe (`true`) or out of it.
    ///
    /// # Errors
    /// If there is nothing to move.
    pub fn move_stash(&mut self, content: &Content, into: bool) -> Result<String, String> {
        let step = content.rig.hideout.stash_step;
        let (from, to) = if into {
            (&mut self.marks, &mut self.stash)
        } else {
            (&mut self.stash, &mut self.marks)
        };
        let n = (*from).min(step);
        if n == 0 {
            return Err("Nothing to move.".into());
        }
        *from -= n;
        *to += n;
        Ok(format!(
            "{n} marks {} the safe.",
            if into { "into" } else { "out of" }
        ))
    }

    /// The price of one proxy now (heat makes them dearer).
    #[must_use]
    pub fn proxy_price(&self, content: &Content) -> u32 {
        let base = content.rig.hideout.proxy_cost;
        if self.heat >= 30 { base * 3 / 2 } else { base }
    }

    /// Buy a proxy for the next run.
    ///
    /// # Errors
    /// Why not.
    pub fn buy_proxy(&mut self, content: &Content) -> Result<String, String> {
        if self.proxies >= content.rig.hideout.proxy_max {
            return Err("You cannot chain more proxies.".into());
        }
        let price = self.proxy_price(content);
        if self.marks < price {
            return Err(format!("{price} marks needed."));
        }
        self.marks -= price;
        self.proxies += 1;
        Ok(format!(
            "Proxy bought for {price}: it will soak {} trace.",
            content.rig.hideout.proxy_buffer
        ))
    }

    /// Spend cycles keeping your head down.
    ///
    /// # Errors
    /// If there are not enough cycles or no heat.
    pub fn lay_low(&mut self, content: &Content) -> Result<String, String> {
        let h = &content.rig.hideout;
        if self.heat == 0 {
            return Err("You are not hot.".into());
        }
        if self.cycles < h.lay_low_cycles {
            return Err(format!("{} cycles needed.", h.lay_low_cycles));
        }
        self.cycles -= h.lay_low_cycles;
        self.heat = self.heat.saturating_sub(h.lay_low_heat);
        Ok(format!("You lie low. Heat -{}.", h.lay_low_heat))
    }

    /// Whether the fixer will deal you this contract, and if not why not.
    #[must_use]
    pub fn closed(&self, content: &Content, contract: &Contract) -> Option<String> {
        let fixer = content.fixer(contract.fixer)?;
        let standing = self
            .standing
            .get(usize::from(contract.fixer))
            .copied()
            .unwrap_or(0);
        if standing <= -3 {
            return Some(format!("{} will not deal with you.", fixer.name));
        }
        if contract.tier >= 2 && self.rep < if contract.tier == 2 { 10 } else { 40 } {
            return Some(format!(
                "Needs reputation {}.",
                if contract.tier == 2 { 10 } else { 40 }
            ));
        }
        if contract.tier == 3 && standing < 2 {
            return Some(format!(
                "{} wants to trust you first (standing 2).",
                fixer.name
            ));
        }
        if (contract.tier == 3 && self.heat >= 60) || (contract.tier == 2 && self.heat >= 85) {
            return Some("Too hot: the fixer is staying away from you.".into());
        }
        if self.done.contains(&contract.id) {
            return Some("Already done.".into());
        }
        None
    }

    /// Jack in on a contract: checks the deck, spends the cycles and builds the run.
    ///
    /// # Errors
    /// Why not; nothing is spent then.
    pub fn begin_run(
        &mut self,
        content: &Content,
        season: u32,
        world: &World,
        contract: &Contract,
    ) -> Result<Run, String> {
        if let Some(reason) = self.closed(content, contract) {
            return Err(reason);
        }
        if self.cycles < JACK_IN_CYCLES {
            return Err(format!("{JACK_IN_CYCLES} cycles needed to jack in."));
        }
        if self.deck.is_empty() {
            return Err("Your deck is empty.".into());
        }
        if self.deck.len() > self.slots() || self.deck_memory(content) > self.memory() {
            return Err(
                "Your deck no longer fits the rig (a part is fried). Unload something.".into(),
            );
        }
        let tmpl = content.contract(contract.tmpl).ok_or("Unknown contract.")?;
        if !self
            .deck
            .iter()
            .filter_map(|n| self.script(content, *n))
            .any(|s| s.effect == tmpl.effect)
        {
            return Err(format!(
                "The job needs a {:?} script in your deck.",
                tmpl.effect
            ));
        }
        let corp = content.corp(contract.corp).ok_or("Unknown corporation.")?;
        let net = Network::generate(content, season, corp, contract.tier)
            .ok_or("The network will not load.")?;
        let state = world.net(contract.corp, contract.tier);
        let setup = RunSetup {
            net,
            holes: state.map(|s| s.holes.clone()).unwrap_or_default(),
            alert: state.map_or(0, |s| s.alert),
            backdoor: state.is_some_and(|s| s.backdoor),
            target: tmpl.target.clone(),
            effect: tmpl.effect,
            deck: self.deck.clone(),
            shield: self.level(part::SHIELD),
            iface: self.level(part::IFACE),
            skill: self.skills(),
            cycles: self.cycles - JACK_IN_CYCLES,
            buffer: u32::from(self.proxies) * content.rig.hideout.proxy_buffer,
            extra: self
                .deck
                .iter()
                .filter(|n| **n >= CODED_BASE)
                .filter_map(|n| self.script(content, *n))
                .collect(),
            learned: world.learned_by(contract.corp),
            warden: self.warden_for(content, corp, contract),
        };
        let run = Run::new(content, setup).ok_or("The network will not load.")?;
        self.cycles -= JACK_IN_CYCLES;
        self.proxies = 0;
        self.pending = Some(contract.id);
        self.runs += 1;
        Ok(run)
    }

    /// The warden that will hunt this run, if any: rare at tier 2, common at tier 3. A pure function
    /// of the character and the contract, so jacking in again cannot reroll it.
    fn warden_for(
        &self,
        content: &Content,
        corp: &crate::content::CorpDef,
        contract: &Contract,
    ) -> Option<u8> {
        let chance = match contract.tier {
            3 => 55,
            2 => 15,
            _ => return None,
        };
        let mut rng = DoorRng::from_label(
            &format!("nullroute-warden-{}", self.seed),
            i64::from(contract.id),
        );
        if !rng.chance(chance, 100) {
            return None;
        }
        cm::warden_of(content, corp).map(|c| c.num)
    }

    /// Settle a finished run: pay, heat, fry, skills, standing. `result` must not be `Active`.
    pub fn finish_run(
        &mut self,
        content: &Content,
        contract: &Contract,
        result: &RunResult,
        rng: &mut DoorRng,
    ) -> Vec<String> {
        let mut notes = Vec::new();
        self.pending = None;
        self.cycles = result.cycles_left;
        for (xp, add) in self.xp.iter_mut().zip(result.xp) {
            *xp = xp.saturating_add(add);
        }
        match result.status {
            Status::Burned => {
                self.burns += 1;
                self.cycles = 0;
                let add = 10 + 3 * u32::from(contract.tier);
                self.heat = (self.heat + add).min(HEAT_MAX);
                notes.push(format!(
                    "Burned. Heat +{add}. The rest of the day is gone, and the loot with it."
                ));
                let fry = 80u32.saturating_sub(20 * u32::from(self.level(part::COOLING)));
                if rng.chance(fry, 100) {
                    let live: Vec<usize> =
                        (0..PARTS).filter(|p| self.fried & (1 << p) == 0).collect();
                    if !live.is_empty() {
                        let p = live[rng.below(u32::try_from(live.len()).unwrap_or(1)) as usize];
                        self.fried |= 1 << p;
                        notes.push(format!(
                            "Your {} is fried until repaired.",
                            content.rig.part.get(p).map_or("part", |d| d.name.as_str())
                        ));
                    }
                } else {
                    notes.push("The cooling held: nothing fried.".into());
                }
            }
            Status::Out | Status::Active => {
                let sold = result.loot * content.fence_rate(self.fence) / 100;
                let mut pay = sold;
                if result.objective {
                    pay += contract.reward;
                    self.jobs += 1;
                    self.rep += 3 * u32::from(contract.tier);
                    let heat = u32::from(contract.tier).pow(2)
                        + content.contract(contract.tmpl).map_or(0, |t| t.heat);
                    let heat = heat + if result.peak >= 80 { 3 } else { 0 };
                    let heat = heat.saturating_sub(result.heat_cut);
                    self.heat = (self.heat + heat).min(HEAT_MAX);
                    self.touch_standing(content, contract);
                    if self.done.len() >= DONE_MAX {
                        self.done.remove(0);
                    }
                    self.done.push(contract.id);
                    notes.push(format!(
                        "Job done: {} paid, {sold} for the data. Heat +{heat}.",
                        contract.reward
                    ));
                } else if sold > 0 {
                    notes.push(format!(
                        "You got out with data worth {sold}, but not the job."
                    ));
                } else {
                    notes.push("You got out with nothing.".into());
                }
                self.marks = self.marks.saturating_add(pay);
                self.earned = self.earned.saturating_add(u64::from(pay));
                // Fragments come out with you; a burned run loses them with the loot.
                let room = FRAGMENTS_MAX.saturating_sub(self.fragments.len());
                let got: Vec<u8> = result.fragments.iter().copied().take(room).collect();
                if !got.is_empty() {
                    notes.push(format!("{} fragment(s) brought out.", got.len()));
                    self.fragments.extend(got);
                }
                if result.warden_down {
                    let bonus = 4 * u32::from(contract.tier);
                    self.rep += bonus;
                    notes.push(format!("You put a warden down. Reputation +{bonus}."));
                }
            }
        }
        notes
    }

    fn touch_standing(&mut self, content: &Content, contract: &Contract) {
        let bump = |s: &mut [i8; FIXERS_MAX], fixer: u8, by: i8| {
            if let Some(slot) = s.get_mut(usize::from(fixer)) {
                *slot = slot.saturating_add(by).clamp(-5, 5);
            }
        };
        bump(&mut self.standing, contract.fixer, 1);
        if let Some(corp) = content.corp(contract.corp)
            && corp.patron != contract.fixer
        {
            bump(&mut self.standing, corp.patron, -1);
        }
    }

    /// Apply the days that passed since the last visit (at most [`MAX_CATCH_UP_DAYS`]), and reset
    /// today's cycles. The nights are seeded by the character and the day, so leaving and coming
    /// back cannot reroll them. Returns what happened, for the player to read.
    pub fn rollover(&mut self, content: &Content, today: i64) -> Vec<String> {
        let mut notes = Vec::new();
        if let Some(id) = self.pending.take() {
            let _ = id;
            self.heat = (self.heat + 8).min(HEAT_MAX);
            notes.push("You dropped the line mid-run. They traced the connection: heat +8.".into());
        }
        if today <= self.day {
            return notes;
        }
        let days = (today - self.day).min(MAX_CATCH_UP_DAYS);
        let h = &content.rig.hideout;
        let first = today - days + 1;
        for night in first..=today {
            let mut rng = DoorRng::from_label(&format!("nullroute-night-{}", self.seed), night);
            if self.bots > 0 {
                let mut income = u32::from(self.bots) * h.bot_income;
                if self.heat >= 70 {
                    income /= 2;
                }
                self.marks = self.marks.saturating_add(income);
                self.earned = self.earned.saturating_add(u64::from(income));
                notes.push(format!("Your bots earned {income}."));
            }
            if self.heat >= 50 && rng.chance(self.heat - 40, 100) {
                let lost = self.marks / 2;
                self.marks -= lost;
                let bot = if self.bots > 0 {
                    self.bots -= 1;
                    ", and a bot"
                } else {
                    ""
                };
                self.heat = self.heat.saturating_sub(10);
                notes.push(format!(
                    "RAID on the hideout: {lost} marks{bot} gone. The safe held."
                ));
            }
            self.heat = self.heat.saturating_sub(h.heat_decay);
        }
        self.day = today;
        self.cycles = self.max_cycles();
        notes
    }
}
