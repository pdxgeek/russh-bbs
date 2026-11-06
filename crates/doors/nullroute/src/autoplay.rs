//! A headless player for the balance numbers: it drives [`Player`], [`Run`] and [`World`] through
//! days exactly as a person would through the screen, using only what a person can see.
//!
//! It is part of the crate (not a test helper) so later slices can rerun the same harness after
//! they change the numbers (`docs/doors/README.md`: each game ships an auto-play harness). It
//! reads no clock and uses no RNG but the one it is given.
//!
//! **What the bots do** (M4-N4 widened this from solo tier 1 and 2):
//!
//! * they **plan a deck against the network** (the planner asks the generator whether the deck
//!   reaches the objective, as a careful player reads the briefing), so deeper tiers are played,
//!   not just allowed;
//! * they fight, code tuned scripts from found fragments, and down wardens when they can;
//! * they work **crews** (a share of the bots, in groups of three): a crew run through the same
//!   world edits a session makes, with the members taking turns;
//! * they hack rivals' hideouts, set honeypots, post bounties, and one bot opts out;
//! * they take the fixers' **arcs** and make choices, spread by a personality so every branch is
//!   tried across the bots;
//! * at the end the season closes into the hall and every bot gets an epilogue.

use std::collections::BTreeMap;

use russh_bbs_door::DoorRng;

use crate::code::{self, CODE_CYCLES};
use crate::content::{Content, Effect, Lock, ScriptDef};
use crate::crew::{FINALE_CORP, FINALE_REWARD, FINALE_TMPL, OpState};
use crate::learn::{self, Stage};
use crate::net::{Capability, Network, Overlay, Patrol};
use crate::player::{JACK_IN_CYCLES, Player, part};
use crate::run::{Act, Run, Status};
use crate::social::{HACK_CYCLES, same};
use crate::story::ARC_TMPL;
use crate::world::{Contract, World};

/// What to simulate.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// Players sharing one world.
    pub players: usize,
    /// Days to run.
    pub days: u32,
    /// The seed for everything.
    pub seed: u64,
    /// The season (networks are generated from it).
    pub season: u32,
    /// Whether the bots play the social layer: crews, hacks, bounties, honeypots, arcs.
    pub social: bool,
}

impl Config {
    /// A configuration with the social layer off (the solo harness of N1 and N2).
    #[must_use]
    pub const fn solo(players: usize, days: u32, seed: u64, season: u32) -> Self {
        Self {
            players,
            days,
            seed,
            season,
            social: false,
        }
    }

    /// A configuration with every system on.
    #[must_use]
    pub const fn full(players: usize, days: u32, seed: u64, season: u32) -> Self {
        Self {
            players,
            days,
            seed,
            season,
            social: true,
        }
    }
}

/// Runs of one tier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TierStats {
    /// Runs started.
    pub runs: u32,
    /// Objectives done and jacked out.
    pub done: u32,
    /// Burned.
    pub burned: u32,
}

impl TierStats {
    fn add(&mut self, status: Status, objective: bool) {
        self.runs += 1;
        match status {
            Status::Burned => self.burned += 1,
            _ if objective => self.done += 1,
            _ => {}
        }
    }
}

/// What the social layer did, as the world counted it plus what the bots did.
#[derive(Debug, Clone, Default)]
pub struct SocialStats {
    /// Hacks filed.
    pub hacks_filed: u32,
    /// Hacks that got in.
    pub hacks_in: u32,
    /// Hacks a honeypot caught.
    pub hacks_caught: u32,
    /// Hacks refused by the caps (and given back).
    pub hacks_refused: u32,
    /// Bounties posted by players.
    pub bounties_posted: u32,
    /// Bounties paid out.
    pub bounties_paid: u32,
    /// The most bounties a fixer had up unasked at once.
    pub bounties_auto: u32,
    /// Bots with at least one honeypot level at the end.
    pub honeypot_owners: u32,
    /// Honeypot levels in all.
    pub honeypot_levels: u32,
    /// Crews founded.
    pub crews: u32,
    /// Hacks filed on the bot that opted out after it opted out (always zero).
    pub hits_on_opted_out: u32,
}

/// Which arcs, steps, choices and endings the bots reached.
#[derive(Debug, Clone, Default)]
pub struct StoryStats {
    /// Steps whose job was done: (arc, step) to count.
    pub steps: BTreeMap<(u8, u8), u32>,
    /// Choices made: (arc, step, choice index) to count.
    pub choices: BTreeMap<(u8, u8, usize), u32>,
    /// Endings reached, by ending number.
    pub endings: BTreeMap<u8, u32>,
    /// Epilogues the bots ended the season with, by epilogue number.
    pub epilogues: BTreeMap<u8, u32>,
}

/// What a simulation measured.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Days simulated.
    pub days: u32,
    /// Players.
    pub players: usize,
    /// Solo runs per tier (index 0 is tier 1).
    pub tiers: [TierStats; 3],
    /// Crew runs per tier.
    pub crew_tiers: [TierStats; 3],
    /// Runs that ended without the objective and without a burn.
    pub dry: u32,
    /// Marks earned in total by all players.
    pub earned: u64,
    /// Marks on hand at the end, per player.
    pub marks: Vec<u32>,
    /// Heat at the end, per player.
    pub heat: Vec<u32>,
    /// Reputation at the end, per player.
    pub rep: Vec<u32>,
    /// Total rig levels at the end, per player.
    pub rig: Vec<u32>,
    /// Bots at the end, per player.
    pub bots: Vec<u32>,
    /// Scripts owned at the end, per player.
    pub owned: Vec<u32>,
    /// Marks earned by all players on each day.
    pub per_day: Vec<u64>,
    /// Network breaches the world recorded.
    pub breaches: u32,
    /// Contracts on the board at the end.
    pub board: usize,
    /// Days a player had nothing they could take.
    pub idle_days: u32,
    /// The highest trace of every run.
    pub peaks: Vec<u32>,
    /// Fights begun.
    pub fights: u32,
    /// Countermeasures put down.
    pub defeated: u32,
    /// Times a fight was broken off or the machine threw the player out.
    pub broke_off: u32,
    /// Runs in which a warden woke, by tier (index 0 is tier 1).
    pub wardens_woke_by_tier: [u32; 3],
    /// Runs in which a warden was put down, by tier.
    pub wardens_down_by_tier: [u32; 3],
    /// Runs in which a warden woke.
    pub wardens_woke: u32,
    /// Runs in which a warden was put down.
    pub wardens_down: u32,
    /// Fragments brought out of runs.
    pub fragments: u32,
    /// Scripts coded at workbenches.
    pub coded: u32,
    /// Script identities a corporation had patched at the end (rows at or over the patch line).
    pub patched_rows: u32,
    /// Runs that began with at least one loaded script the corporation had patched or was wary of.
    pub degraded_decks: u32,
    /// The first day (1-based) any bot ran a tier 3 network, if any did.
    pub first_tier3_day: Option<u32>,
    /// Social layer counters.
    pub social: SocialStats,
    /// The arcs.
    pub story: StoryStats,
    /// Crew runs started.
    pub crew_runs: u32,
    /// Whether a crew cracked the finale.
    pub finale: bool,
    /// The season's ending, once the season is closed (a `season_ending` number).
    pub season_ending: u8,
    /// The market's price level each night.
    pub market: Vec<u8>,
    /// Script identities the corporations were wary of or had patched, each night.
    pub pressure: Vec<u32>,
    /// Marks earned by bots that never joined a crew.
    pub solo_earned: u64,
    /// Marks earned by bots in crews.
    pub crew_earned: u64,
    /// Marks each bot earned in all.
    pub earned_by: Vec<u64>,
    /// Bots that ended holding a coded script tuned to wardens.
    pub tuned_owners: u32,
    /// Solo runs in which a warden reached the player: (all, with a tuned script in the deck, of
    /// those burned, of those that put it down).
    pub warden_met: [u32; 4],
    /// Why runs ended without the objective and without a burn: out of cycles, the bot bailed on
    /// the trace, or it ran out of ideas.
    pub dry_why: [u32; 3],
}

impl Report {
    /// Total solo runs.
    #[must_use]
    pub fn runs(&self) -> u32 {
        self.tiers.iter().map(|t| t.runs).sum()
    }

    /// Total objectives done in solo runs.
    #[must_use]
    pub fn done(&self) -> u32 {
        self.tiers.iter().map(|t| t.done).sum()
    }

    /// Total solo burns.
    #[must_use]
    pub fn burned(&self) -> u32 {
        self.tiers.iter().map(|t| t.burned).sum()
    }
}

enum Want {
    Script(u8),
    Rig(usize),
    Bot,
    Workbench,
}

/// What the bot buys, in order. The rig and the power 4 breaks come early; the tier 3 tools
/// (power 6 breaks need skill 3 in Break) and the helpers that make deeper runs survivable after.
const WANTS: &[Want] = &[
    Want::Script(24),
    Want::Rig(part::MEMORY),
    Want::Script(6),
    Want::Script(9),
    Want::Script(12),
    Want::Rig(part::CPU),
    Want::Script(25),
    Want::Bot,
    Want::Script(18),
    Want::Script(14),
    Want::Rig(part::MEMORY),
    Want::Script(21),
    Want::Script(2),
    Want::Script(7),
    Want::Script(10),
    Want::Script(13),
    Want::Script(58),
    Want::Script(59),
    Want::Workbench,
    Want::Rig(part::SHIELD),
    Want::Rig(part::COOLING),
    Want::Script(22),
    Want::Rig(part::MEMORY),
    Want::Rig(part::IFACE),
    Want::Script(16),
    Want::Script(29),
    Want::Bot,
    Want::Rig(part::CPU),
    Want::Script(32),
    Want::Script(34),
    Want::Script(36),
    Want::Script(38),
    Want::Script(39),
    Want::Script(40),
    Want::Rig(part::SHIELD),
    Want::Rig(part::COOLING),
    Want::Rig(part::CPU),
    Want::Rig(part::IFACE),
    Want::Bot,
    Want::Script(17),
    Want::Script(19),
    Want::Script(31),
    Want::Script(33),
    Want::Script(35),
    Want::Script(37),
];

fn wanted(content: &Content, p: &Player, want: &Want) -> bool {
    match want {
        Want::Script(n) => {
            !p.owned.contains(n)
                && content
                    .script(*n)
                    .is_some_and(|s| s.price > 0 && p.rep >= s.min_rep)
        }
        Want::Rig(i) => p.upgrade_cost(content, *i).is_some(),
        Want::Bot => p.bots < content.rig.hideout.bot_max.min(3),
        Want::Workbench => !p.workbench,
    }
}

/// Spend marks on the first thing on the list that is still wanted. Returns whether it bought.
fn shop(content: &Content, p: &mut Player, market: u32) -> bool {
    if p.fried != 0
        && let Some(part) = (0..crate::player::PARTS).find(|i| p.fried & (1 << i) != 0)
        && p.repair(content, part).is_ok()
    {
        return true;
    }
    for want in WANTS {
        if !wanted(content, p, want) {
            continue;
        }
        return match want {
            Want::Script(n) => p.buy_script(content, *n, market).is_ok(),
            Want::Rig(i) => p.upgrade(content, *i).is_ok(),
            Want::Bot => p.buy_bot(content).is_ok(),
            Want::Workbench => p.buy_workbench(content).is_ok(),
        };
    }
    false
}

/// What the bot knows when it plans a deck: whom it is hitting and what they have learned. A
/// careful player reads the briefing's "they know" line, so the bot does too.
#[derive(Clone, Copy)]
struct Target<'a> {
    world: &'a World,
    corp: u8,
}

impl Target<'_> {
    fn stage(&self, script: &ScriptDef) -> Stage {
        let uses = self
            .world
            .learned_by(self.corp)
            .iter()
            .find(|(sig, _)| *sig == script.sig)
            .map_or(0, |(_, n)| *n);
        learn::stage(script, uses)
    }
}

/// The bot's usable scripts against a target: owned, with the skill to load them, not patched.
fn usable(content: &Content, p: &Player, target: Target<'_>) -> Vec<ScriptDef> {
    p.owned
        .iter()
        .filter_map(|n| p.script(content, *n))
        .filter(|s| u32::from(p.skill(s.family)) >= s.min_skill)
        .filter(|s| target.stage(s) != Stage::Patched)
        .collect()
}

fn best_of(list: &[ScriptDef], pick: impl Fn(&ScriptDef) -> bool) -> Option<ScriptDef> {
    list.iter()
        .filter(|s| pick(s))
        .max_by_key(|s| (s.power, std::cmp::Reverse(s.memory)))
        .cloned()
}

/// Code every build the fragments allow, best first, while there are cycles to spare.
fn code_all(content: &Content, p: &mut Player, coded: &mut u32) {
    while p.workbench && p.cycles >= CODE_CYCLES + 20 {
        let Some(build) = code::best_build(content, &p.fragments) else {
            return;
        };
        if p.code(content, build).is_err() {
            return;
        }
        *coded += 1;
    }
}

fn capability_of_deck(content: &Content, p: &Player) -> Capability {
    crate::run::capability_of_defs(p.deck.iter().filter_map(|n| p.script(content, *n)))
}

fn net_for(content: &Content, season: u32, c: &Contract) -> Option<Network> {
    let corp = content.corp(c.corp)?;
    if c.tmpl == FINALE_TMPL {
        Network::generate_finale(content, season, corp)
    } else {
        Network::generate(content, season, corp, c.tier)
    }
}

fn objective_ids(content: &Content, net: &Network, code: &str) -> Vec<u8> {
    net.nodes
        .iter()
        .filter(|n| !n.honeypot && content.node(n.kind).is_some_and(|d| d.code == code))
        .map(|n| n.id)
        .collect()
}

/// Load the deck for a contract: the job's tool, then the break and purge scripts that make the
/// objective reachable on *this* network (the generator is asked, as a careful player studies
/// the briefing), then a scan and the helpers while memory lasts. Returns whether the objective
/// is reachable with the deck.
fn plan_deck(content: &Content, p: &mut Player, c: &Contract, world: &World, season: u32) -> bool {
    p.deck.clear();
    let target = Target {
        world,
        corp: c.corp,
    };
    let Some(job) = content.job(c) else {
        return false;
    };
    let Some(net) = net_for(content, season, c) else {
        return false;
    };
    let state = world.net(c.corp, c.tier);
    let overlay = Overlay {
        holes: state.map_or(&[][..], |s| s.holes.as_slice()),
        alert: state.map_or(0, |s| s.alert),
    };
    let goals = objective_ids(content, &net, &job.target);
    let have = usable(content, p, target);
    let Some(tool) = best_of(&have, |s| s.effect == job.effect) else {
        return false;
    };
    let _ = p.load(content, tool.num);
    let reach = |p: &Player| -> (bool, u32) {
        let cap = capability_of_deck(content, p);
        let mask = net.reachable(&cap, &overlay);
        (
            goals.iter().any(|g| mask & (1 << g) != 0),
            mask.count_ones(),
        )
    };
    // The scan first: it finds the hidden links the reach depends on.
    if let Some(scan) = best_of(&have, |s| {
        matches!(s.effect, Effect::Scan | Effect::Map) && s.memory <= 3
    }) {
        let _ = p.load(content, scan.num);
    }
    for _ in 0..p.slots() {
        let (done, now) = reach(p);
        if done {
            break;
        }
        // The break or purge script that opens up the most of the network per unit of memory.
        let mut best: Option<(u32, u8)> = None;
        for s in &have {
            let useful = s.effect.lock().is_some() || s.effect == Effect::Purge;
            if !useful || p.deck.contains(&s.num) || s.memory == 0 {
                continue;
            }
            let mut trial = p.clone();
            if trial.load(content, s.num).is_err() {
                continue;
            }
            let (_, after) = reach(&trial);
            if after <= now {
                continue;
            }
            let score = (after - now) * 100 / s.memory;
            if best.is_none_or(|(b, _)| score > b) {
                best = Some((score, s.num));
            }
        }
        match best {
            Some((_, num)) => {
                let _ = p.load(content, num);
            }
            None => break,
        }
    }
    let ok = reach(p).0;
    // A fight is won with break scripts (the weak lock family hurts most) and jammed away if
    // possible, so the kit is a break script or two, a jam, then the quiet helpers.
    for _ in 0..2 {
        if let Some(s) = best_of(&have, |s| s.effect.lock().is_some() && !p.deck.contains(&s.num))
        {
            let _ = p.load(content, s.num);
        }
    }
    let helpers: [Effect; 6] = if c.tier >= 2 {
        [
            Effect::Reroute,
            Effect::Jam,
            Effect::Mask,
            Effect::Decoy,
            Effect::Fork,
            Effect::Cutfeed,
        ]
    } else {
        [
            Effect::Jam,
            Effect::Mask,
            Effect::Reroute,
            Effect::Decoy,
            Effect::Fork,
            Effect::Cutfeed,
        ]
    };
    for effect in helpers {
        if let Some(s) = best_of(&have, |s| s.effect == effect && !p.deck.contains(&s.num)) {
            let _ = p.load(content, s.num);
        }
    }
    // A coded script tuned to wardens is worth a slot on a deep network.
    if c.tier >= 2 {
        let tuned: Vec<u8> = have
            .iter()
            .filter(|s| {
                s.num >= code::CODED_BASE && s.target == Some(crate::content::CmKind::Warden)
            })
            .map(|s| s.num)
            .collect();
        for n in tuned {
            if p.load(content, n).is_err()
                && let Some(last) = p.deck.last().copied()
                && last != tool.num
            {
                let _ = p.unload(content, last);
                let _ = p.load(content, n);
            }
        }
    }
    ok
}

fn slot_of(run: &Run, content: &Content, pick: impl Fn(Effect, u32) -> bool) -> Option<usize> {
    run.deck.iter().enumerate().position(|(i, slot)| {
        slot.left > 0
            && run.stage(content, i) != Stage::Patched
            && run
                .script(content, slot.script)
                .is_some_and(|s| pick(s.effect, s.power))
    })
}

/// The moves of a fight in the order a person would try them, with the information on the fight
/// screen: brace on a guard, reroute when the trace is high, play the script that would hurt the
/// most, brace, and break off when nothing does anything.
fn fight_moves(content: &Content, run: &Run) -> Vec<Act> {
    let mut out = Vec::new();
    let can_hurt = (0..run.deck.len()).any(|i| run.preview(content, i).is_some_and(|d| d > 0));
    if can_hurt && run.intent() == Some(crate::content::Intent::Guard) && run.cycles > 0 {
        out.push(Act::Brace);
    }
    if run.trace >= 55
        && let Some(slot) = slot_of(run, content, |e, _| e == Effect::Reroute)
    {
        out.push(Act::Use { slot, target: None });
    }
    let mut ranked: Vec<(u32, usize)> = (0..run.deck.len())
        .filter_map(|i| run.preview(content, i).map(|d| (d, i)))
        .filter(|(d, _)| *d > 0)
        .collect();
    ranked.sort_by_key(|r| std::cmp::Reverse(*r));
    let ranked_any = !ranked.is_empty();
    out.extend(
        ranked
            .into_iter()
            .map(|(_, slot)| Act::Use { slot, target: None }),
    );
    // With nothing that hurts it there is nothing to wait for: leave.
    if !ranked_any {
        out.push(Act::BreakOff);
        return out;
    }
    out.push(Act::Brace);
    out.push(Act::BreakOff);
    out
}

fn bit(id: u8) -> u32 {
    1 << (id & 31)
}

/// A bot at the controls of one run: it decides the next action from what a person can see.
struct Pilot {
    target: String,
    effect: Effect,
    scanned_at: u32,
    opening: u8,
    bail_at: u32,
    fails: u32,
    steps: u32,
    jammed: Vec<(u8, usize)>,
    /// Moves the engine just refused: not tried again until something changes.
    bad: Vec<Act>,
    /// In a crew a member who cannot go on waits for a crewmate to open the way, not leaves.
    crew: bool,
}

impl Pilot {
    fn new(run: &Run, bail_at: u32) -> Self {
        let (t, e) = run.objective_target();
        Self {
            target: t.to_owned(),
            effect: e,
            scanned_at: 0,
            opening: 0,
            bail_at,
            fails: 0,
            steps: 0,
            jammed: Vec::new(),
            bad: Vec::new(),
            crew: false,
        }
    }

    fn in_crew(run: &Run, bail_at: u32) -> Self {
        Self {
            crew: true,
            ..Self::new(run, bail_at)
        }
    }

    /// Tell the pilot how its last action went.
    fn result(&mut self, act: Act, ok: bool) {
        if ok {
            self.bad.clear();
            self.fails = 0;
        } else {
            self.bad.push(act);
            self.fails += 1;
        }
    }

    /// The next action. `others_done` means the crew's objective is done: get out.
    fn decide(&mut self, content: &Content, run: &Run, others_done: bool) -> Option<Act> {
        let act = self.decide_one(content, run, others_done);
        // A crew member with nowhere to go yet (the way is shut to their kit) waits.
        if self.crew && act == Act::JackOut && self.waiting(run, others_done) {
            self.steps = self.steps.saturating_sub(1);
            return None;
        }
        Some(act)
    }

    fn waiting(&self, run: &Run, others_done: bool) -> bool {
        !(run.objective_done() || others_done || run.trace >= self.bail_at || run.cycles <= 2)
            && self.steps <= 300
            && self.fails < 6
            && run.encounter().is_none()
    }

    fn decide_one(&mut self, content: &Content, run: &Run, others_done: bool) -> Act {
        self.steps += 1;
        if self.steps > 300 || self.fails >= 6 {
            return if run.encounter().is_some() {
                Act::BreakOff
            } else {
                Act::JackOut
            };
        }
        if run.encounter().is_some() {
            return fight_moves(content, run)
                .into_iter()
                .find(|a| !self.bad.contains(a))
                .unwrap_or(Act::BreakOff);
        }
        if run.objective_done() || others_done || run.trace >= self.bail_at || run.cycles <= 2 {
            return Act::JackOut;
        }
        while self.opening < 2 {
            let want = [Effect::Mask, Effect::Decoy][usize::from(self.opening)];
            self.opening += 1;
            if let Some(slot) = slot_of(run, content, |e, _| e == want) {
                return Act::Use { slot, target: None };
            }
        }
        if run.trace >= 45
            && let Some(slot) = slot_of(run, content, |e, _| e == Effect::Reroute)
        {
            return Act::Use { slot, target: None };
        }
        let Some(here) = run.view(content, run.at) else {
            return Act::JackOut;
        };
        if here.code == self.target && !here.honeypot {
            let effect = self.effect;
            return slot_of(run, content, |e, _| e == effect)
                .map_or(Act::JackOut, |slot| Act::Use { slot, target: None });
        }
        if run.feed_live()
            && let Some(slot) = slot_of(run, content, |e, _| e == Effect::Cutfeed)
        {
            return Act::Use { slot, target: None };
        }
        if self.scanned_at & bit(run.at) == 0 {
            self.scanned_at |= bit(run.at);
            if let Some(slot) = slot_of(run, content, |e, _| e == Effect::Scan) {
                return Act::Use { slot, target: None };
            }
        }
        let mut cands: Vec<_> = run
            .exits()
            .into_iter()
            .filter(|id| run.visited & bit(*id) == 0)
            .filter_map(|id| run.view(content, id))
            .filter(|v| !v.honeypot)
            .collect();
        cands.sort_by_key(|v| {
            std::cmp::Reverse(
                (i32::from(v.code == self.target) * 100)
                    + i32::from(v.depth) * 10
                    + i32::from(v.open) * 5,
            )
        });
        for v in &cands {
            if v.open {
                // A sentry or tracer next door: jam it first if we can.
                if matches!(v.patrol, Patrol::Sentry | Patrol::Tracer)
                    && !v.patrol_off
                    && let Some(slot) = slot_of(run, content, |e, pow| {
                        e == Effect::Jam && pow >= u32::from(v.patrol_level)
                    })
                    && !self.jammed.contains(&(v.id, slot))
                {
                    self.jammed.push((v.id, slot));
                    return Act::Use {
                        slot,
                        target: Some(v.id),
                    };
                }
                return Act::Move(v.id);
            }
            if let Some((lock, level)) = v.lock {
                if let Some(slot) = slot_of(run, content, |e, pow| {
                    e.lock() == Some(lock) && pow >= u32::from(level)
                }) {
                    return Act::Use {
                        slot,
                        target: Some(v.id),
                    };
                }
            } else if v.patrol == Patrol::BlackWall
                && let Some(slot) = slot_of(run, content, |e, pow| {
                    e == Effect::Purge && pow >= u32::from(v.patrol_level)
                })
            {
                return Act::Use {
                    slot,
                    target: Some(v.id),
                };
            }
        }
        // Nowhere new to go: step back toward a visited neighbour with a smaller depth, or leave.
        // (A crew member stays where they are and waits for a crewmate to open the way.)
        if self.crew {
            return Act::JackOut;
        }
        run.exits()
            .into_iter()
            .filter_map(|id| run.view(content, id))
            .filter(|v| v.open && v.depth < here.depth)
            .min_by_key(|v| v.depth)
            .map_or(Act::JackOut, |v| Act::Move(v.id))
    }
}

/// Play one solo run with the information a person has. Returns when the run is over.
fn play(content: &Content, run: &mut Run, rng: &mut DoorRng, bail_at: u32) {
    let mut pilot = Pilot::new(run, bail_at);
    for _ in 0..400 {
        if run.status != Status::Active {
            return;
        }
        let act = pilot.decide(content, run, false).unwrap_or(Act::JackOut);
        let ok = run.act(content, act, rng).is_ok();
        pilot.result(act, ok);
    }
    let _ = run.act(content, Act::JackOut, rng);
}

/// Whether the bot thinks it can take a contract: it can plan a deck that reaches the objective.
fn feasible(content: &Content, p: &Player, c: &Contract, world: &World, season: u32) -> bool {
    if content.job(c).is_none() || p.closed(content, c).is_some() {
        return false;
    }
    let mut trial = p.clone();
    plan_deck(content, &mut trial, c, world, season)
}

fn arc_choice_index(bot: usize, arc: u8, step: u8, open: &[usize], rng: &mut DoorRng) -> usize {
    // Each bot has a leaning, so across a dozen bots the branches all get tried; a roll in
    // five breaks the leaning.
    let lean = (bot + usize::from(arc) + usize::from(step)) % open.len().max(1);
    if rng.chance(20, 100) {
        open[rng.below(u32::try_from(open.len()).unwrap_or(1)) as usize]
    } else {
        open[lean]
    }
}

/// What a bot remembers about how it is doing: a contract it failed is left alone until the bot
/// has bought something new, as a person would (the same network, with the same kit, fails the
/// same way).
#[derive(Debug, Clone, Default)]
struct Memory {
    failed: Vec<(u32, (usize, u32))>,
}

impl Memory {
    fn kit_of(p: &Player) -> (usize, u32) {
        (
            p.owned.len(),
            p.rig.iter().map(|l| u32::from(*l)).sum::<u32>(),
        )
    }

    fn avoids(&self, p: &Player, id: u32) -> bool {
        let kit = Self::kit_of(p);
        self.failed.iter().any(|(i, k)| *i == id && *k == kit)
    }

    fn note(&mut self, p: &Player, id: u32, good: bool) {
        if good {
            self.failed.retain(|(i, _)| *i != id);
        } else {
            self.failed.retain(|(i, _)| *i != id);
            self.failed.push((id, Self::kit_of(p)));
            if self.failed.len() > 40 {
                self.failed.remove(0);
            }
        }
    }
}

/// Simulate `cfg.players` bots sharing a world for `cfg.days` days.
#[must_use]
pub fn simulate(content: &Content, cfg: Config) -> Report {
    let mut rng = DoorRng::from_seed(cfg.seed);
    let day0: i64 = 20_000;
    let mut world = World::default();
    world.sync_season(content, cfg.season);
    let mut players: Vec<Player> = (0..cfg.players)
        .map(|i| {
            Player::new(
                content,
                rng.next_u64() ^ i as u64,
                &format!("bot{i}"),
                cfg.season,
                day0 - 30,
            )
        })
        .collect();
    let mut report = Report {
        days: cfg.days,
        players: cfg.players,
        ..Report::default()
    };
    // Bots 0..6 work in two crews of three; the rest are solo (the comparison). The last bot
    // turns the rivals board off after ten days.
    let crew_of = |i: usize| -> Option<usize> { (cfg.social && i < 6).then_some(i / 3) };
    let opt_out = cfg.players.saturating_sub(1);
    let mut mem = vec![Memory::default(); cfg.players];
    for d in 0..i64::from(cfg.days) {
        let today = day0 + d + 1;
        let mut night = DoorRng::from_label("nullroute-sim-tick", today);
        world.nightly(content, &mut night, today);
        report
            .market
            .push(u8::try_from(world.market_pct.min(255)).unwrap_or(100));
        report.pressure.push(
            u32::try_from(
                world
                    .learned
                    .iter()
                    .filter(|l| u32::from(l.uses) >= learn::WARY_AT)
                    .count(),
            )
            .unwrap_or(0),
        );
        let mut earned_today: u64 = 0;
        for (idx, p) in players.iter_mut().enumerate() {
            let before = p.earned;
            let _ = p.rollover(content, today);
            if cfg.social {
                let _ = p.fold(content, &world.social, today);
                if idx == opt_out && d == 10 {
                    let _ = p.set_pvp(false, today);
                }
                world.social.post(p.posted(today));
            }
            while shop(content, p, world.market_pct) {}
            if p.heat >= 50 {
                let _ = p.lay_low(content);
            }
            earned_today += p.earned.saturating_sub(before);
        }
        // Crews first: they commit cycles early in the day, as a crew arranged in the morning would.
        if cfg.social {
            for k in 0..2 {
                let members: Vec<usize> = (0..cfg.players)
                    .filter(|i| crew_of(*i) == Some(k))
                    .collect();
                if members.len() >= 3 {
                    let mut cx = CrewCtx {
                        content,
                        world: &mut world,
                        report: &mut report,
                        today,
                        season: cfg.season,
                        k,
                    };
                    crew_day(&mut cx, &mut players, &members, &mut rng);
                }
            }
        }
        for (idx, p) in players.iter_mut().enumerate() {
            let before = p.earned;
            solo_day(content, p, idx, &mut mem[idx], &mut world, &mut report, today, cfg, &mut rng);
            if cfg.social {
                social_day(content, p, idx, &mut world, &mut report, today, opt_out, d);
                world.social.post(p.posted(today));
            }
            earned_today += p.earned.saturating_sub(before);
        }
        report.per_day.push(earned_today);
    }
    report.earned = players.iter().map(|p| p.earned).sum();
    report.earned_by = players.iter().map(|p| p.earned).collect();
    if cfg.social {
        report.crew_earned = report.earned_by.iter().take(6).sum();
        report.solo_earned = report.earned_by.iter().skip(6).sum();
    }
    report.marks = players.iter().map(|p| p.marks + p.stash).collect();
    report.heat = players.iter().map(|p| p.heat).collect();
    report.rep = players.iter().map(|p| p.rep).collect();
    report.rig = players
        .iter()
        .map(|p| p.rig.iter().map(|l| u32::from(*l)).sum())
        .collect();
    report.bots = players.iter().map(|p| u32::from(p.bots)).collect();
    report.owned = players
        .iter()
        .map(|p| u32::try_from(p.owned.len()).unwrap_or(0))
        .collect();
    report.breaches = world.breaches;
    report.patched_rows = u32::try_from(
        world
            .learned
            .iter()
            .filter(|l| u32::from(l.uses) >= learn::LEARNED_AT)
            .count(),
    )
    .unwrap_or(0);
    report.board = world.board.len();
    let t = world.social.tallies.clone();
    report.social.hacks_in = t.hacks_in;
    report.social.hacks_caught = t.hacks_caught;
    report.social.bounties_paid = t.bounties_paid;
    report.social.honeypot_owners = u32::try_from(
        players
            .iter()
            .filter(|p| p.ties.honeypot > 0)
            .count(),
    )
    .unwrap_or(0);
    report.tuned_owners = u32::try_from(
        players
            .iter()
            .filter(|p| {
                p.owned
                    .iter()
                    .filter(|n| **n >= code::CODED_BASE)
                    .filter_map(|n| p.script(content, *n))
                    .any(|s| s.target == Some(crate::content::CmKind::Warden))
            })
            .count(),
    )
    .unwrap_or(0);
    report.social.honeypot_levels = players.iter().map(|p| u32::from(p.ties.honeypot)).sum();
    report.social.crews = u32::try_from(world.social.crews.len()).unwrap_or(0);
    report.finale = world.social.finale.is_some();
    for p in &players {
        for st in &p.ties.arcs {
            if st.ending != 0 {
                *report.story.endings.entry(st.ending).or_insert(0) += 1;
            }
        }
        *report
            .story
            .epilogues
            .entry(p.epilogue(content))
            .or_insert(0) += 1;
    }
    // Close the season: the hall records it.
    world.sync_season(content, cfg.season + 1);
    report.season_ending = world.social.hall.last().map_or(0, |h| h.ending);
    report
}

/// One solo day of one bot: arcs first, then the board, until the cycles run out.
#[allow(clippy::too_many_arguments)]
fn solo_day(
    content: &Content,
    p: &mut Player,
    idx: usize,
    mem: &mut Memory,
    world: &mut World,
    report: &mut Report,
    today: i64,
    cfg: Config,
    rng: &mut DoorRng,
) {
    if cfg.social {
        arc_choices(content, p, idx, report, rng);
    }
    let mut took = 0;
    let mut failed: Vec<u32> = Vec::new();
    loop {
        if p.cycles < JACK_IN_CYCLES + 12 || p.ties.stake.is_some() && p.cycles < 30 {
            break;
        }
        let mut offers: Vec<Contract> = Vec::new();
        if cfg.social {
            for arc in 0..crate::player::ARCS_MAX {
                let arc = u8::try_from(arc).unwrap_or(0);
                if let Some(c) = p.arc_contract(content, arc, today) {
                    offers.push(c);
                }
            }
        }
        offers.extend(world.board.iter().copied());
        // A careful player starts a deep run only with the cycles to finish it.
        let spare = p.cycles.saturating_sub(JACK_IN_CYCLES);
        offers.retain(|c| {
            !failed.contains(&c.id)
                && spare >= [12, 24, 30][usize::from(c.tier.clamp(1, 3) - 1)]
                && !mem.avoids(p, c.id)
        });
        let mut offers: Vec<Contract> = offers
            .into_iter()
            .filter(|c| feasible(content, p, c, world, cfg.season))
            .collect();
        // The story first (it is why the fixers come back), then the best pay.
        offers.sort_by_key(|c| (c.tmpl != ARC_TMPL, std::cmp::Reverse(c.reward)));
        let Some(contract) = offers.first().copied() else {
            break;
        };
        code_all(content, p, &mut report.coded);
        plan_deck(content, p, &contract, world, cfg.season);
        let mut run = match p.begin_run(content, cfg.season, world, &contract) {
            Ok(run) => run,
            Err(_) => {
                failed.push(contract.id);
                continue;
            }
        };
        let target = Target {
            world,
            corp: contract.corp,
        };
        if p.deck
            .iter()
            .filter_map(|n| p.script(content, *n))
            .any(|s| target.stage(&s) != Stage::Fresh)
        {
            report.degraded_decks += 1;
        }
        took += 1;
        play(content, &mut run, rng, if idx % 3 == 0 { 99 } else { 80 });
        let result = run.result();
        if run.log.iter().any(|l| l.contains(" finds you.")) {
            let tuned = p.deck.iter().filter_map(|n| p.script(content, *n)).any(|s| {
                s.target == Some(crate::content::CmKind::Warden)
            });
            report.warden_met[0] += 1;
            if tuned {
                report.warden_met[1] += 1;
                report.warden_met[2] += u32::from(result.status == Status::Burned);
                report.warden_met[3] += u32::from(result.warden_down);
            }
        }
        report.peaks.push(result.peak);
        report.fights += result.fights;
        report.defeated += result.defeated;
        report.broke_off += result.broke_off;
        report.fragments += u32::try_from(result.fragments.len()).unwrap_or(0);
        let tier = usize::from(contract.tier.clamp(1, 3) - 1);
        report.wardens_woke_by_tier[tier] += u32::from(result.warden_woke);
        report.wardens_down_by_tier[tier] += u32::from(result.warden_down);
        report.wardens_woke += u32::from(result.warden_woke);
        report.wardens_down += u32::from(result.warden_down);
        if contract.tier == 3 && report.first_tier3_day.is_none() {
            report.first_tier3_day = Some(u32::try_from(today - 20_000).unwrap_or(0));
        }
        report.tiers[tier].add(result.status, result.objective);
        if result.status != Status::Burned && !result.objective {
            report.dry += 1;
            let why = if result.cycles_left <= 2 {
                0
            } else if result.peak >= 75 {
                1
            } else {
                2
            };
            report.dry_why[why] += 1;
            failed.push(contract.id);
        }
        mem.note(p, contract.id, result.objective && result.status != Status::Burned);
        let _ = p.finish_run(content, &contract, &result, rng);
        if let Some((arc, step)) = content.job(&contract).and_then(|j| j.arc)
            && result.objective
            && result.status != Status::Burned
        {
            *report.story.steps.entry((arc, step)).or_insert(0) += 1;
            arc_choices(content, p, idx, report, rng);
        }
        world.apply_run(contract.corp, contract.tier, &result);
        world.record_run(&p.name, contract.tier, &result, today);
        while shop(content, p, world.market_pct) {}
    }
    if took == 0 {
        report.idle_days += 1;
    }
}

/// Make the choices that are waiting.
fn arc_choices(
    content: &Content,
    p: &mut Player,
    idx: usize,
    report: &mut Report,
    rng: &mut DoorRng,
) {
    for arc in 0..crate::player::ARCS_MAX {
        let arc = u8::try_from(arc).unwrap_or(0);
        let st = p.ties.arcs[usize::from(arc)];
        if !st.awaiting {
            continue;
        }
        let choices = p.arc_choices(content, arc);
        let open: Vec<usize> = choices
            .iter()
            .enumerate()
            .filter(|(_, (_, why))| why.is_none())
            .map(|(i, _)| i)
            .collect();
        if open.is_empty() {
            continue;
        }
        let k = arc_choice_index(idx, arc, st.step, &open, rng);
        if p.arc_choose(content, arc, k).is_ok() {
            *report.story.choices.entry((arc, st.step, k)).or_insert(0) += 1;
        }
    }
}

/// Hacks, honeypots and bounties for one bot's day.
#[allow(clippy::too_many_arguments)]
fn social_day(
    content: &Content,
    p: &mut Player,
    idx: usize,
    world: &mut World,
    report: &mut Report,
    today: i64,
    opt_out: usize,
    d: i64,
) {
    // Honeypots: a third of the bots, when they can pay for one and still have a rig to run.
    if idx % 3 == 1 && p.ties.honeypot < crate::social::HONEYPOT_MAX && p.marks >= 600 {
        let _ = p.buy_honeypot();
    }
    let me = p.name.clone();
    // Hacks: every fourth bot hacks the richest hideout it is allowed to, once a day.
    if idx % 4 == 0 && p.cycles >= HACK_CYCLES + 4 {
        let mut targets: Vec<_> = world
            .social
            .posted
            .iter()
            .filter(|t| t.pvp && !same(&t.handle, &me))
            .map(|t| {
                (
                    u32::from(t.cash) * 10 / (1 + u32::from(t.shield)),
                    t.handle.clone(),
                )
            })
            .collect();
        targets.sort_by(|a, b| b.cmp(a));
        for (_, handle) in targets.into_iter().take(3) {
            if world.social.check_hack(&me, &handle, today).is_err() {
                continue;
            }
            if d > 10 && handle == format!("bot{opt_out}") {
                report.social.hits_on_opted_out += 1;
            }
            if let Ok(nonce) = p.begin_hack() {
                let r = world.social.file_hack(&me, &handle, today, nonce);
                report.social.hacks_filed += 1;
                if r.is_err() {
                    report.social.hacks_refused += 1;
                }
            }
            break;
        }
    }
    // Bounties: a sixth of the bots put 100 marks on the best-known rival at their tier or above.
    if idx % 6 == 2 && p.marks >= 300 {
        let mut targets: Vec<_> = world
            .social
            .posted
            .iter()
            .filter(|t| t.pvp && !same(&t.handle, &me))
            .map(|t| (t.rep, t.handle.clone()))
            .collect();
        targets.sort_by(|a, b| b.cmp(a));
        for (_, handle) in targets.into_iter().take(3) {
            if world.social.check_bounty(&me, &handle, 100).is_err() {
                continue;
            }
            if let Ok(nonce) = p.begin_bounty(100)
                && world
                    .social
                    .post_bounty(content, &me, &handle, 100, "", today, nonce)
                    .is_ok()
            {
                report.social.bounties_posted += 1;
            }
            break;
        }
    }
    report.social.bounties_auto = report.social.bounties_auto.max(
        u32::try_from(
            world
                .social
                .bounties
                .iter()
                .filter(|b| b.poster.is_empty())
                .count(),
        )
        .unwrap_or(0),
    );
}

struct CrewCtx<'a> {
    content: &'a Content,
    world: &'a mut World,
    report: &'a mut Report,
    today: i64,
    season: u32,
    k: usize,
}

/// One day for one crew: the leader opens a run the crew's decks can do, the others join, and
/// they take turns until everyone is out.
fn crew_day(cx: &mut CrewCtx<'_>, players: &mut [Player], members: &[usize], rng: &mut DoorRng) {
    let (content, today, season, k) = (cx.content, cx.today, cx.season, cx.k);
    let leader = members[0];
    let lname = players[leader].name.clone();
    if cx.world.social.crew_of(&lname).is_none() {
        let _ = cx
            .world
            .social
            .crew_create(&format!("Crew{k}"), &lname, today);
        for m in members.iter().filter(|m| **m != leader) {
            let name = players[*m].name.clone();
            let _ = cx.world.social.crew_invite(&lname, &name, today);
            if let Some(id) = cx.world.social.invites_for(&name).first().map(|c| c.id) {
                let _ = cx.world.social.crew_accept(&name, id);
            }
        }
    }
    // Collect anything finished from an earlier day first.
    for m in members {
        settle_if_done(content, &mut players[*m], cx.world, rng);
    }
    if members
        .iter()
        .any(|m| players[*m].cycles < JACK_IN_CYCLES + 14 || players[*m].ties.stake.is_some())
    {
        return;
    }
    // The finale when the crew is strong enough; otherwise the best tier 2 or 3 job its decks do.
    let mut candidates: Vec<Contract> = Vec::new();
    if cx.world.social.finale.is_none()
        && members
            .iter()
            .all(|m| players[*m].rep >= 40 && players[*m].heat < 60)
    {
        candidates.push(Contract {
            id: 0x3FFF_0000 + season,
            tmpl: FINALE_TMPL,
            corp: FINALE_CORP,
            fixer: content.corp(FINALE_CORP).map_or(0, |c| c.patron),
            tier: 3,
            reward: FINALE_REWARD,
            expires: today,
        });
    }
    let mut board: Vec<Contract> = cx
        .world
        .board
        .iter()
        .copied()
        .filter(|c| c.tier >= 2)
        .collect();
    board.sort_by_key(|c| std::cmp::Reverse((c.tier, c.reward)));
    candidates.extend(board);
    let mut chosen: Option<(Contract, Vec<Vec<u8>>)> = None;
    for c in candidates {
        let got = crew_decks(content, players, members, &c, cx.world, season);
        if let Some(decks) = got {
            chosen = Some((c, decks));
            break;
        }
    }
    let Some((contract, decks)) = chosen else {
        return;
    };
    let op_id = u32::try_from(today)
        .unwrap_or(0)
        .wrapping_mul(31)
        .wrapping_add(u32::try_from(k).unwrap_or(0) + 1);
    for (n, m) in members.iter().enumerate() {
        let p = &mut players[*m];
        p.deck.clone_from(&decks[n]);
        let Ok(member) = p.begin_crew_run(content, &contract, op_id) else {
            return;
        };
        let r = if n == 0 {
            cx.world.op_open(op_id, contract, member, today)
        } else {
            cx.world.op_join(op_id, member)
        };
        if r.is_err() {
            p.drop_stake();
        }
    }
    if cx.world.op_start(op_id, &lname, today).is_err() {
        return;
    }
    cx.report.crew_runs += 1;
    let mut pilots: Vec<Option<Pilot>> = members.iter().map(|_| None).collect();
    for _ in 0..2_000 {
        let Some(op) = cx.world.social.ops.iter().find(|o| o.id == op_id).cloned() else {
            break;
        };
        if op.state == OpState::Done {
            break;
        }
        let Some(live) = op.live(content) else { break };
        let mut moved = false;
        for (n, m) in members.iter().enumerate() {
            let run = &live.runs[n];
            if run.status != Status::Active {
                continue;
            }
            let pilot = pilots[n].get_or_insert_with(|| Pilot::in_crew(run, 85));
            let Some(act) = pilot.decide(content, run, live.objective) else {
                continue;
            };
            let name = players[*m].name.clone();
            match cx.world.op_act(content, op_id, &name, act, op.log.len(), today) {
                Ok(()) => {
                    pilot.result(act, true);
                    moved = true;
                    break;
                }
                Err(_) => {
                    pilot.result(act, false);
                    if pilot.fails >= 6 {
                        // This member is stuck: they jack out.
                        let _ = cx.world.op_act(
                            content,
                            op_id,
                            &name,
                            Act::JackOut,
                            op.log.len(),
                            today,
                        );
                        moved = true;
                        break;
                    }
                }
            }
        }
        if !moved {
            // Nobody could move (everyone in a fight they cannot leave): the line goes cold.
            cx.world.ops_night(content, today + 5);
            break;
        }
    }
    if let Some(op) = cx.world.social.ops.iter().find(|o| o.id == op_id).cloned()
        && let Some(live) = op.live(content)
    {
        let tier = usize::from(contract.tier.clamp(1, 3) - 1);
        let merged = live.merged();
        cx.report.crew_tiers[tier].add(merged.status, merged.objective);
        cx.report.wardens_woke_by_tier[tier] += u32::from(merged.warden_woke);
        cx.report.wardens_down_by_tier[tier] += u32::from(merged.warden_down);
        cx.report.wardens_woke += u32::from(merged.warden_woke);
        cx.report.wardens_down += u32::from(merged.warden_down);
        cx.report.fights += merged.fights;
        cx.report.peaks.push(merged.peak);
        if contract.tier == 3 && cx.report.first_tier3_day.is_none() {
            cx.report.first_tier3_day = Some(u32::try_from(today - 20_000).unwrap_or(0));
        }
    }
    for m in members {
        settle_if_done(content, &mut players[*m], cx.world, rng);
    }
}

/// Collect the crew run a bot has a stake in, if it is over, and count it.
fn settle_if_done(content: &Content, p: &mut Player, world: &mut World, rng: &mut DoorRng) {
    let Some(stake) = p.ties.stake else { return };
    let Some(op) = world.social.ops.iter().find(|o| o.id == stake.op).cloned() else {
        let _ = p.drop_stake();
        return;
    };
    if op.state != OpState::Done {
        return;
    }
    if let Some(live) = op.live(content) {
        let _ = p.settle_crew(content, &op, &live, rng);
        world.op_settled(op.id, &p.name);
    }
}

/// Decks for the crew's members that between them reach the objective: member `n` brings the
/// lock family `n` at its best, plus a scan and helpers; the leader brings the job's tool.
/// `None` if the union cannot do it.
fn crew_decks(
    content: &Content,
    players: &[Player],
    members: &[usize],
    c: &Contract,
    world: &World,
    season: u32,
) -> Option<Vec<Vec<u8>>> {
    let target = Target {
        world,
        corp: c.corp,
    };
    let job = content.job(c)?;
    let net = net_for(content, season, c)?;
    let state = world.net(c.corp, c.tier);
    let overlay = Overlay {
        holes: state.map_or(&[][..], |s| s.holes.as_slice()),
        alert: state.map_or(0, |s| s.alert),
    };
    let goals = objective_ids(content, &net, &job.target);
    let mut decks: Vec<Vec<u8>> = Vec::new();
    let mut union: Vec<ScriptDef> = Vec::new();
    for (n, m) in members.iter().enumerate() {
        let mut p = players[*m].clone();
        if c.tmpl != FINALE_TMPL && p.closed(content, c).is_some() {
            return None;
        }
        let have = usable(content, &p, target);
        p.deck.clear();
        let want_lock = [Lock::Crack, Lock::Spoof, Lock::Overflow][n % 3];
        // Everyone carries the tool: whoever stands at the objective can finish the job.
        let tool = best_of(&have, |s| s.effect == job.effect)?;
        let _ = p.load(content, tool.num);
        for pick in [
            best_of(&have, |s| s.effect.lock() == Some(want_lock)),
            best_of(&have, |s| {
                matches!(s.effect, Effect::Scan | Effect::Map) && s.memory <= 3
            }),
            best_of(&have, |s| s.effect == Effect::Purge),
            best_of(&have, |s| s.effect == Effect::Mask),
            best_of(&have, |s| s.effect == Effect::Reroute),
            best_of(&have, |s| s.effect == Effect::Jam),
            best_of(&have, |s| {
                s.effect.lock().is_some() && s.effect.lock() != Some(want_lock)
            }),
        ]
        .into_iter()
        .flatten()
        {
            let _ = p.load(content, pick.num);
        }
        for s in have.iter().filter(|s| {
            s.num >= code::CODED_BASE && s.target == Some(crate::content::CmKind::Warden)
        }) {
            let _ = p.load(content, s.num);
        }
        union.extend(p.deck.iter().filter_map(|x| p.script(content, *x)));
        decks.push(p.deck.clone());
    }
    let cap = crate::run::capability_of_defs(union.into_iter());
    let mask = net.reachable(&cap, &overlay);
    goals.iter().any(|g| mask & (1 << g) != 0).then_some(decks)
}
