//! What the social layer and the story ask of a character (M4-N4): posting a hideout, folding in
//! what the world delivered, filing hacks and bounties, honeypots, the `PvP` opt-out, joining and
//! settling a crew run, and the fixers' arcs.
//!
//! Like the rest of [`Player`], nothing here touches the clock, the store or the world: the
//! session passes in the day, the nonce and the world's social state, and the world edits
//! themselves are built in `session.rs`.

use russh_bbs_door::DoorRng;

use crate::cond::Cond;
use crate::content::Content;
use crate::crew::{COMMIT_MAX, FINALE_REWARD, FINALE_TMPL, Live, Member, Op, PAY_PCT};
use crate::player::{
    ArcState, JACK_IN_CYCLES, LEGEND_MAX, Legend, PARTS, Player, SENT_MAX, Sent, Stake, stat,
};
use crate::run::Status;
use crate::social::{
    DELIVERY_DAYS, Delivery, Effect, HACK_CYCLES, HONEYPOT_MAX, OPT_IN_DAYS, Posted, Social, same,
};
use crate::story::{ARC_TMPL, ChoiceDef, arc_id};
use crate::world::Contract;

/// What a honeypot level costs to set, by the level it reaches.
pub const HONEYPOT_COST: [u32; HONEYPOT_MAX as usize] = [60, 120, 240];

/// The smallest commitment to a crew run, in cycles (besides the jack-in cost).
pub const COMMIT_MIN: u32 = 8;

fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

impl Player {
    /// The tier the character works at, by reputation.
    #[must_use]
    pub const fn tier(&self) -> u8 {
        if self.rep >= 40 {
            3
        } else if self.rep >= 10 {
            2
        } else {
            1
        }
    }

    /// What a scout could see of the hideout.
    #[must_use]
    pub fn posted(&self, today: i64) -> Posted {
        let skills: u32 = self.skills().iter().map(|k| u32::from(*k)).sum();
        let edge = (skills / 2 + u32::from(self.level(crate::player::part::IFACE))).min(10);
        Posted {
            handle: self.name.clone(),
            tier: self.tier(),
            heat: u8::try_from(self.heat.min(100)).unwrap_or(100),
            honeypot: self.ties.honeypot,
            shield: self.level(crate::player::part::SHIELD),
            edge: u8::try_from(edge).unwrap_or(10),
            cash: u16::try_from((self.marks / 10).min(u32::from(u16::MAX))).unwrap_or(u16::MAX),
            rep: self.rep,
            born: self.ties.born,
            seen: today,
            peace_until: 0,
            pvp: self.ties.pvp,
            crews_open: self.ties.crews_open,
        }
    }

    /// A number to tie a world edit to its receipt: unique among the ones still waiting.
    #[must_use]
    pub fn fresh_nonce(&self, kind: u8) -> u32 {
        let mut n = self.seed
            ^ (u64::try_from(self.day).unwrap_or(0) << 20)
            ^ (u64::from(self.runs) << 40)
            ^ (u64::from(kind) << 56);
        loop {
            n = mix(n);
            let nonce = u32::try_from(n & 0xFFFF_FFFF).unwrap_or(1).max(1);
            if !self.ties.sent.iter().any(|s| s.nonce == nonce) {
                return nonce;
            }
        }
    }

    /// Turn the rivals board on or off. Off takes the character off the board (no hacks sent or
    /// received, no bounties); on again is allowed only after [`OPT_IN_DAYS`].
    ///
    /// # Errors
    /// Why not.
    pub fn set_pvp(&mut self, on: bool, today: i64) -> Result<String, String> {
        if on == self.ties.pvp {
            return Ok(format!(
                "Rivals board already {}.",
                if on { "on" } else { "off" }
            ));
        }
        if on {
            if today - self.ties.pvp_off_on < OPT_IN_DAYS {
                return Err(format!(
                    "You turned the rivals board off recently. It can come back after {OPT_IN_DAYS} days."
                ));
            }
            self.ties.pvp = true;
            return Ok("You are on the rivals board. Hacks and bounties can find you.".into());
        }
        self.ties.pvp = false;
        self.ties.pvp_off_on = today;
        Ok("You are off the rivals board: nobody can hack you, and you cannot hack or claim bounties.".into())
    }

    /// Turn crew invitations on or off.
    pub fn set_crews_open(&mut self, on: bool) -> String {
        self.ties.crews_open = on;
        if on {
            "You take crew invitations.".into()
        } else {
            "Crew invitations are off.".into()
        }
    }

    /// Set the next honeypot level at the hideout.
    ///
    /// # Errors
    /// Why not.
    pub fn buy_honeypot(&mut self) -> Result<String, String> {
        let level = self.ties.honeypot;
        if level >= HONEYPOT_MAX {
            return Err("The hideout holds as many honeypots as it can.".into());
        }
        let cost = HONEYPOT_COST[usize::from(level)];
        if self.marks < cost {
            return Err(format!("A honeypot costs {cost} marks."));
        }
        self.marks -= cost;
        self.ties.honeypot += 1;
        Ok(format!(
            "Honeypot level {}: a hack on your hideout has a {} percent chance to trip it.",
            self.ties.honeypot,
            crate::social::TRAP_PCT * u32::from(self.ties.honeypot)
        ))
    }

    /// Spend the cycles for a hack and note it as sent. The session then files the order.
    ///
    /// # Errors
    /// Why not; nothing is spent then.
    pub fn begin_hack(&mut self) -> Result<u32, String> {
        if !self.ties.pvp {
            return Err("You are off the rivals board.".into());
        }
        if self.cycles < HACK_CYCLES {
            return Err(format!("A hack costs {HACK_CYCLES} cycles."));
        }
        if self.ties.sent.len() >= SENT_MAX {
            return Err("Wait for your earlier orders to land first.".into());
        }
        let nonce = self.fresh_nonce(0);
        self.cycles -= HACK_CYCLES;
        self.ties.sent.push(Sent {
            nonce,
            kind: 0,
            day: self.day,
            marks: 0,
            cycles: u8::try_from(HACK_CYCLES).unwrap_or(6),
        });
        self.ties.bump(stat::HACKS);
        Ok(nonce)
    }

    /// Take the stake for a bounty and note it as sent.
    ///
    /// # Errors
    /// Why not.
    pub fn begin_bounty(&mut self, stake: u32) -> Result<u32, String> {
        if !self.ties.pvp {
            return Err("You are off the rivals board.".into());
        }
        if self.marks < stake {
            return Err(format!("You do not have {stake} marks."));
        }
        if self.ties.sent.len() >= SENT_MAX {
            return Err("Wait for your earlier orders to land first.".into());
        }
        let nonce = self.fresh_nonce(1);
        self.marks -= stake;
        self.ties.sent.push(Sent {
            nonce,
            kind: 1,
            day: self.day,
            marks: stake,
            cycles: 0,
        });
        Ok(nonce)
    }

    /// Apply what the world delivered since the last visit, and settle edits that never got a
    /// receipt. Returns the lines to show.
    pub fn fold(&mut self, content: &Content, social: &Social, today: i64) -> Vec<String> {
        let mut notes = Vec::new();
        let mine: Vec<&Delivery> = social
            .deliveries
            .iter()
            .filter(|d| same(&d.to, &self.name))
            .collect();
        for d in &mine {
            if d.nonce != 0 {
                self.ties.sent.retain(|s| s.nonce != d.nonce);
            }
        }
        let credited = self.ties.credited_to;
        for d in mine.iter().filter(|d| d.id > credited) {
            self.apply(content, d);
            let n = i64::from(d.n).abs();
            notes.push(content.say(d.line, &d.who, n));
            self.ties.credited_to = self.ties.credited_to.max(d.id);
        }
        // An edit with no receipt never ran: take the stake back, unless it is so old the receipt
        // could have been pruned (then it is forgotten, not refunded twice).
        for s in std::mem::take(&mut self.ties.sent) {
            if today - s.day < DELIVERY_DAYS {
                self.marks = self.marks.saturating_add(s.marks);
                self.cycles = (self.cycles + u32::from(s.cycles)).min(self.max_cycles());
                notes.push(format!(
                    "An order you sent never ran. Your {} is back.",
                    if s.kind == 0 { "cycles" } else { "stake" }
                ));
            }
        }
        notes
    }

    fn apply(&mut self, content: &Content, d: &Delivery) {
        let n = d.n;
        let amount = n.unsigned_abs();
        match d.effect {
            Effect::None => {}
            Effect::Marks => {
                if n >= 0 {
                    self.marks = self.marks.saturating_add(amount);
                    self.earned = self.earned.saturating_add(u64::from(amount));
                } else {
                    self.marks -= amount.min(self.marks);
                }
            }
            Effect::Heat => {
                self.heat = if n >= 0 {
                    (self.heat + amount).min(crate::player::HEAT_MAX)
                } else {
                    self.heat.saturating_sub(amount)
                };
            }
            Effect::Rep => self.rep = self.rep.saturating_add(amount),
            Effect::Cycles => self.cycles = (self.cycles + amount).min(self.max_cycles()),
            Effect::Fry => {
                let mut rng =
                    DoorRng::from_label(&format!("nullroute-fry-{}", self.seed), i64::from(d.id));
                let live: Vec<usize> = (0..PARTS).filter(|p| self.fried & (1 << p) == 0).collect();
                if !live.is_empty() {
                    let p = live[rng.below(u32::try_from(live.len()).unwrap_or(1)) as usize];
                    self.fried |= 1 << p;
                }
            }
        }
        let _ = content;
        match d.line {
            crate::social::Lines::HACK_ATT => self.ties.bump(stat::IN),
            crate::social::Lines::TRAP_DEF => self.ties.bump(stat::CAUGHT),
            crate::social::Lines::BOUNTY_ATT => self.ties.bump(stat::BOUNTIES),
            _ => {}
        }
    }

    // ---- crews ----

    /// What this character brings to a crew run, with `commit` cycles.
    #[must_use]
    pub fn as_member(&self, content: &Content, commit: u32) -> Member {
        Member {
            handle: self.name.clone(),
            deck: self.deck.clone(),
            builds: self
                .deck
                .iter()
                .filter(|n| **n >= crate::code::CODED_BASE)
                .filter_map(|n| {
                    self.coded
                        .get(usize::from(*n - crate::code::CODED_BASE))
                        .copied()
                        .flatten()
                        .map(|b| (*n, b))
                })
                .collect(),
            skill: self.skills(),
            shield: self.level(crate::player::part::SHIELD),
            iface: self.level(crate::player::part::IFACE),
            cycles: commit,
            buffer: u32::from(self.proxies) * content.rig.hideout.proxy_buffer,
        }
    }

    /// Commit to a crew run: checks the deck and the cycles, takes them and notes the stake.
    ///
    /// # Errors
    /// Why not; nothing is spent then.
    pub fn begin_crew_run(
        &mut self,
        content: &Content,
        contract: &Contract,
        op: u32,
    ) -> Result<Member, String> {
        if self.ties.stake.is_some() {
            return Err("You are in a crew run already. Collect it first.".into());
        }
        if self.pending.is_some() {
            return Err("You have a run in flight.".into());
        }
        if contract.tmpl != FINALE_TMPL
            && let Some(reason) = self.closed(content, contract)
        {
            return Err(reason);
        }
        if contract.tmpl == FINALE_TMPL && (self.rep < 40 || self.heat >= crate::player::HEAT_TIER3)
        {
            return Err("The finale wants reputation 40 and a cool head (heat under 60).".into());
        }
        if self.deck.is_empty() {
            return Err("Your deck is empty.".into());
        }
        if self.deck.len() > self.slots() || self.deck_memory(content) > self.memory() {
            return Err("Your deck no longer fits the rig. Unload something.".into());
        }
        let free = self.cycles.saturating_sub(JACK_IN_CYCLES);
        if free < COMMIT_MIN {
            return Err(format!(
                "A crew run needs {} cycles ({JACK_IN_CYCLES} to jack in, {COMMIT_MIN} to work).",
                JACK_IN_CYCLES + COMMIT_MIN
            ));
        }
        let commit = free.min(COMMIT_MAX);
        let member = self.as_member(content, commit);
        self.cycles -= JACK_IN_CYCLES + commit;
        self.proxies = 0;
        self.runs += 1;
        self.ties.stake = Some(Stake { op, cycles: commit });
        self.ties.bump(stat::CREW_RUNS);
        Ok(member)
    }

    /// A joined run that the world no longer has: take the cycles back.
    pub fn drop_stake(&mut self) -> Option<String> {
        let s = self.ties.stake.take()?;
        self.cycles = (self.cycles + s.cycles + JACK_IN_CYCLES).min(self.max_cycles());
        Some("Your crew run is gone from the board. The cycles are back.".into())
    }

    /// Collect a finished crew run: pay, heat, standing, fragments and skill, like a solo run,
    /// with the crew's pay split by what each member did.
    pub fn settle_crew(
        &mut self,
        content: &Content,
        op: &Op,
        live: &Live,
        rng: &mut DoorRng,
    ) -> Vec<String> {
        let Some(idx) = op.index_of(&self.name) else {
            return Vec::new();
        };
        let Some(mut result) = live.result_of(idx) else {
            return Vec::new();
        };
        let Some(stake) = self.ties.stake.take() else {
            return Vec::new();
        };
        let _ = stake;
        let base = if op.is_finale() {
            FINALE_REWARD
        } else {
            op.contract.reward
        };
        let n = op.members.len().clamp(1, PAY_PCT.len());
        let total = base * PAY_PCT[n - 1] / 100;
        let share = live.shares(total).get(idx).copied().unwrap_or(0);
        let mut contract = op.contract;
        contract.reward = share;
        // The cycles the character did not commit are already theirs; the unused commitment comes
        // back with the result.
        result.cycles_left += self.cycles;
        let mut notes = self.finish_run(content, &contract, &result, rng);
        if result.status != Status::Burned && live.objective {
            self.ties.bump(stat::CREW_JOBS);
            if op.is_finale() {
                self.ties.bump(stat::FINALE);
            }
        }
        if result.warden_down {
            self.ties.bump(stat::WARDENS);
        }
        notes.insert(0, format!("Crew run over. Your share: {share} of {total}."));
        notes
    }

    // ---- arcs ----

    /// The job an arc offers today, if one is open.
    #[must_use]
    pub fn arc_contract(&self, content: &Content, arc: u8, today: i64) -> Option<Contract> {
        let def = content.arc(arc)?;
        let st = self.ties.arcs.get(usize::from(arc))?;
        if st.ending != 0 || st.awaiting {
            return None;
        }
        let tallies = crate::social::Tallies::default();
        let parse = |t: &str| Cond::parse(t, &content.story.flags).ok();
        if st.step == 0 {
            parse(&def.open)?.eval(Some(self), &tallies).then_some(())?;
        } else if today <= st.since {
            return None;
        }
        let id = if st.step == 0 { def.first } else { st.step };
        let step = content.step(arc, id)?;
        parse(&step.open)?
            .eval(Some(self), &tallies)
            .then_some(())?;
        Some(Contract {
            id: arc_id(arc, id),
            tmpl: ARC_TMPL,
            corp: step.corp,
            fixer: def.fixer,
            tier: step.tier,
            reward: step.reward,
            expires: today,
        })
    }

    /// Why an arc has no job today, in a few words.
    #[must_use]
    pub fn arc_status(&self, content: &Content, arc: u8, today: i64) -> String {
        let Some(st) = self.ties.arcs.get(usize::from(arc)) else {
            return "no such arc".into();
        };
        if let Some(e) = content.ending(st.ending).filter(|_| st.ending != 0) {
            return format!("ended: {}", e.title);
        }
        if st.awaiting {
            return "a choice is waiting".into();
        }
        if self.arc_contract(content, arc, today).is_some() {
            return "a job is open".into();
        }
        if st.step != 0 && today <= st.since {
            return "the fixer is quiet until tomorrow".into();
        }
        "not open to you yet".into()
    }

    /// The choices after the step the character just did, each with why it is shut, if it is.
    #[must_use]
    pub fn arc_choices(&self, content: &Content, arc: u8) -> Vec<(ChoiceDef, Option<String>)> {
        let st = self
            .ties
            .arcs
            .get(usize::from(arc))
            .copied()
            .unwrap_or_default();
        let Some(step) = content.step(arc, st.step).filter(|_| st.awaiting) else {
            return Vec::new();
        };
        let tallies = crate::social::Tallies::default();
        step.choice
            .iter()
            .map(|c| {
                let open = Cond::parse(&c.gate, &content.story.flags)
                    .is_ok_and(|g| g.eval(Some(self), &tallies));
                let why = (!open).then(|| format!("needs {}", c.gate.replace('&', " and ")));
                (c.clone(), why)
            })
            .collect()
    }

    /// Make a choice. Moves the character on, applies the effects, and reaches an ending if the
    /// choice is one. Returns what to show.
    ///
    /// # Errors
    /// Why not (the choice is shut, or none is waiting).
    pub fn arc_choose(&mut self, content: &Content, arc: u8, k: usize) -> Result<String, String> {
        let choices = self.arc_choices(content, arc);
        let (choice, why) = choices.get(k).cloned().ok_or("No such choice.")?;
        if let Some(why) = why {
            return Err(format!("That is shut to you: {why}."));
        }
        let today = self.day;
        for f in &choice.set {
            if let Some(bit) = content.story.flags.iter().position(|x| x == f) {
                self.ties.flags |= 1 << bit;
            }
        }
        for f in &choice.clear {
            if let Some(bit) = content.story.flags.iter().position(|x| x == f) {
                self.ties.flags &= !(1 << bit);
            }
        }
        let apply =
            |v: u32, d: i32| u32::try_from((i64::from(v) + i64::from(d)).max(0)).unwrap_or(0);
        self.rep = apply(self.rep, choice.rep);
        self.marks = apply(self.marks, choice.marks);
        if choice.marks > 0 {
            self.earned = self
                .earned
                .saturating_add(u64::from(choice.marks.unsigned_abs()));
        }
        self.heat = apply(self.heat, choice.heat).min(crate::player::HEAT_MAX);
        for [fixer, delta] in &choice.standing {
            if let Some(slot) = self.standing.get_mut(usize::from(fixer.unsigned_abs())) {
                *slot = slot.saturating_add(*delta).clamp(-5, 5);
            }
        }
        let st = self
            .ties
            .arcs
            .get_mut(usize::from(arc))
            .ok_or("No such arc.")?;
        st.awaiting = false;
        st.since = today;
        let mut text = choice.note.clone();
        if choice.ending != 0 {
            st.ending = choice.ending;
            self.ties.endings |= 1 << choice.ending;
            if let Some(e) = content.ending(choice.ending) {
                text = format!("{} {}", e.title.to_uppercase(), e.text);
            }
        } else {
            st.step = choice.next;
        }
        Ok(text)
    }

    /// The step an arc job belongs to is done: its choice is waiting, and the fixer hands over
    /// any fragments the step gives.
    pub fn arc_done(&mut self, content: &Content, arc: u8, step: u8) -> Vec<u8> {
        let given: Vec<u8> = content
            .step(arc, step)
            .map(|s| s.give.clone())
            .unwrap_or_default();
        let room = crate::code::FRAGMENTS_MAX.saturating_sub(self.fragments.len());
        let given: Vec<u8> = given.into_iter().take(room).collect();
        self.fragments.extend(given.iter().copied());
        if let Some(st) = self.ties.arcs.get_mut(usize::from(arc)) {
            st.step = step;
            st.awaiting = true;
            st.since = self.day;
        }
        given
    }

    /// The epilogue for the season that is ending (the first rule that holds).
    #[must_use]
    pub fn epilogue(&self, content: &Content) -> u8 {
        let tallies = crate::social::Tallies::default();
        content
            .seasons
            .epilogue
            .iter()
            .find(|e| Cond::parse(&e.rule, &[]).is_ok_and(|c| c.eval(Some(self), &tallies)))
            .map_or(0, |e| e.num)
    }

    /// Remember the season that is ending.
    pub fn close_season(&mut self, content: &Content) {
        let ending = self.epilogue(content);
        self.ties.legend.push(Legend {
            season: self.season,
            ending,
            rep: self.rep,
            jobs: self.jobs,
        });
        while self.ties.legend.len() > LEGEND_MAX {
            self.ties.legend.remove(0);
        }
    }

    /// A new season starts the story and the social layer from nothing, but the legend, the
    /// counters, the endings once reached and the hideout upgrades stay.
    pub fn reset_ties_for_season(&mut self) {
        self.ties.arcs = [ArcState::default(); crate::player::ARCS_MAX];
        self.ties.flags = 0;
        self.ties.stake = None;
        self.ties.sent.clear();
        self.ties.honeypot = 0;
    }
}
