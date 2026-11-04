//! One run through a network: the trace, the deck and the node map.
//!
//! The run is a pure state machine. [`Run::act`] takes an action and the session's seeded RNG and
//! changes only the run; every number it uses comes from the content tables, the rig the player
//! brought and the RNG. It never sees the clock, the store or the shared world: the world's holes
//! and alert come in at the start ([`RunSetup`]) and the changes go out at the end
//! ([`Run::result`]).
//!
//! **The trace** climbs with every action: a node's noise on entry, a script's noise, a failed
//! break, a patrol, a live camera feed, an alarm, the corporation's own watchfulness. At 100 you
//! are burned.
//!
//! **The RPG rule here.** A locked node cannot be entered without the right family of break script
//! at enough power; a black wall cannot be entered without a purge. What the deck carries decides
//! what [`Run::act`] allows.

use russh_bbs_door::DoorRng;

use crate::content::{Content, CorpDef, Effect, Family, Lock, ScriptDef};
use crate::net::{Network, NodeId, Patrol};

/// The trace at which you are burned.
pub const TRACE_MAX: u32 = 100;

/// The most log lines kept.
pub const LOG_MAX: usize = 60;

/// The most cycles a script action can cost (a guard for the tests).
pub const MAX_SCRIPT_CYCLES: u32 = 3;

/// What a run was set up with.
#[derive(Debug, Clone)]
pub struct RunSetup {
    /// The generated network.
    pub net: Network,
    /// Holes players left open (from the world).
    pub holes: Vec<NodeId>,
    /// The corporation's alert level (from the world).
    pub alert: u8,
    /// A planted backdoor on this network (from the world): a trace discount.
    pub backdoor: bool,
    /// The contract's objective: the node code and the effect needed.
    pub target: String,
    /// The effect that completes the objective.
    pub effect: Effect,
    /// Script numbers in the deck, in slot order.
    pub deck: Vec<u8>,
    /// The shield level (0 to 3).
    pub shield: u8,
    /// The interface level (0 to 3).
    pub iface: u8,
    /// The skill level per family, in `Family::ALL` order.
    pub skill: [u8; 5],
    /// Cycles available.
    pub cycles: u32,
    /// Trace absorbed up front by proxies.
    pub buffer: u32,
}

/// One script in the loaded deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// The script's number.
    pub script: u8,
    /// Uses left this run (`u32::MAX` for unlimited).
    pub left: u32,
}

/// How a run stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Still jacked in.
    Active,
    /// You jacked out.
    Out,
    /// The trace reached 100.
    Burned,
}

/// An action in a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Move to a neighbouring node.
    Move(NodeId),
    /// Run the script in `slot`, aimed at `target` if it needs one.
    Use {
        /// The deck slot.
        slot: usize,
        /// The neighbouring node it is aimed at.
        target: Option<NodeId>,
    },
    /// Leave.
    JackOut,
}

/// Why an action was refused. A refusal costs nothing: no cycles, no trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The run is over.
    Over,
    /// That node is not next door, or you do not know the link.
    NotAdjacent,
    /// The node is locked (a break script is needed) or walled (a purge is needed).
    Locked,
    /// No such slot.
    NoSlot,
    /// The script has no charges left.
    NoCharges,
    /// The script must be aimed at a neighbouring node.
    NeedsTarget,
    /// Not enough cycles.
    NoCycles,
    /// Nothing to do there.
    NothingToDo,
}

impl Refused {
    /// The line for the log.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Over => "The run is over.",
            Self::NotAdjacent => "That node is not next door.",
            Self::Locked => "Locked. You need a script that opens it.",
            Self::NoSlot => "No script in that slot.",
            Self::NoCharges => "That script is spent for this run.",
            Self::NeedsTarget => "Aim it at a node next door.",
            Self::NoCycles => "Not enough cycles. Jack out.",
            Self::NothingToDo => "Nothing to do there.",
        }
    }
}

/// What a finished (or abandoned) run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    /// How it ended.
    pub status: Status,
    /// Whether the contract's objective was done.
    pub objective: bool,
    /// Data copied from vaults, in marks before the fence's cut.
    pub loot: u32,
    /// Heat shaved off by scrubbed logs.
    pub heat_cut: u32,
    /// The highest the trace got.
    pub peak: u32,
    /// Cycles left.
    pub cycles_left: u32,
    /// Locks broken this run (the world records them as holes).
    pub broken: Vec<NodeId>,
    /// A backdoor was planted.
    pub planted: bool,
    /// Skill use per family, in `Family::ALL` order.
    pub xp: [u32; 5],
}

/// A run in progress.
#[derive(Debug, Clone)]
pub struct Run {
    setup: RunSetup,
    corp_trace_pct: u32,
    watch: u32,
    /// Where you are.
    pub at: NodeId,
    /// The trace, 0 to 100.
    pub trace: u32,
    /// The highest the trace got.
    pub peak: u32,
    /// The loaded deck.
    pub deck: Vec<Slot>,
    /// Cycles left.
    pub cycles: u32,
    /// How the run stands.
    pub status: Status,
    /// What happened, newest last.
    pub log: Vec<String>,
    /// Nodes whose type you know.
    pub known: u32,
    /// Nodes you scanned (lock, security, patrol).
    pub scanned: u32,
    /// Nodes where a scan found a decoy.
    pub decoys: u32,
    /// Nodes entered.
    pub visited: u32,
    opened: u32,
    jammed: u32,
    copied: u32,
    revealed: Vec<(NodeId, NodeId)>,
    buffer: u32,
    mask: u32,
    fork: u32,
    tracer: bool,
    feed: bool,
    alarm: bool,
    objective: bool,
    planted: bool,
    loot: u32,
    heat_cut: u32,
    broken: Vec<NodeId>,
    xp: [u32; 5],
}

const fn bit(id: NodeId) -> u32 {
    1u32 << (id & 31)
}

impl Run {
    /// Start a run at the entry node. `None` if the corporation is unknown.
    #[must_use]
    pub fn new(content: &Content, setup: RunSetup) -> Option<Self> {
        let corp = content.corp(setup.net.corp)?;
        let deck = setup
            .deck
            .iter()
            .filter_map(|n| content.script(*n))
            .map(|s| Slot {
                script: s.num,
                left: if s.charges == 0 { u32::MAX } else { s.charges },
            })
            .collect();
        let mut run = Self {
            corp_trace_pct: corp.trace_pct,
            watch: corp.watch,
            at: 0,
            trace: 0,
            peak: 0,
            deck,
            cycles: setup.cycles,
            status: Status::Active,
            log: Vec::new(),
            known: 0,
            scanned: 0,
            decoys: 0,
            visited: 1,
            opened: 0,
            jammed: 0,
            copied: 0,
            revealed: Vec::new(),
            buffer: setup.buffer + if setup.backdoor { 12 } else { 0 },
            mask: 0,
            fork: 0,
            tracer: false,
            feed: false,
            alarm: false,
            objective: false,
            planted: false,
            loot: 0,
            heat_cut: 0,
            broken: Vec::new(),
            xp: [0; 5],
            setup,
        };
        run.say(format!("Jacked in to {}.", corp.name));
        if run.setup.backdoor {
            run.say("An old backdoor shaves the first hops.".to_owned());
        }
        run.discover(0);
        Some(run)
    }

    /// The network.
    #[must_use]
    pub const fn net(&self) -> &Network {
        &self.setup.net
    }

    /// The objective's node code and effect.
    #[must_use]
    pub fn objective_target(&self) -> (&str, Effect) {
        (&self.setup.target, self.setup.effect)
    }

    /// Whether the objective is done.
    #[must_use]
    pub const fn objective_done(&self) -> bool {
        self.objective
    }

    /// Whether the alarm is up (a honeypot was tripped).
    #[must_use]
    pub const fn alarm(&self) -> bool {
        self.alarm
    }

    /// Whether a tracer is counting your hops.
    #[must_use]
    pub const fn tracer(&self) -> bool {
        self.tracer
    }

    /// Whether a camera feed is live.
    #[must_use]
    pub const fn feed_live(&self) -> bool {
        self.feed
    }

    /// Trace still absorbed by proxies and decoys.
    #[must_use]
    pub const fn buffer(&self) -> u32 {
        self.buffer
    }

    fn say(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > LOG_MAX {
            self.log.remove(0);
        }
    }

    fn security(&self, id: NodeId) -> u8 {
        self.setup.net.security(id, self.setup.alert)
    }

    /// Whether the link from `a` to `b` is known: visible, or found by a scan or a map.
    fn link_known(&self, a: NodeId, b: NodeId) -> bool {
        self.setup.net.links.iter().any(|l| {
            ((l.a == a && l.b == b) || (l.a == b && l.b == a))
                && (l.stealth == 0
                    || self.revealed.contains(&(l.a, l.b))
                    || self.revealed.contains(&(l.b, l.a)))
        })
    }

    /// The neighbours of the current node you know a link to.
    #[must_use]
    pub fn exits(&self) -> Vec<NodeId> {
        self.setup
            .net
            .neighbours(self.at, u8::MAX)
            .into_iter()
            .filter(|n| self.link_known(self.at, *n))
            .collect()
    }

    fn discover(&mut self, at: NodeId) {
        self.known |= bit(at);
        for n in self.exits_from(at) {
            self.known |= bit(n);
        }
    }

    fn exits_from(&self, at: NodeId) -> Vec<NodeId> {
        self.setup
            .net
            .neighbours(at, u8::MAX)
            .into_iter()
            .filter(|n| self.link_known(at, *n))
            .collect()
    }

    /// Whether a node's lock is open (no lock, a hole, or broken this run).
    #[must_use]
    pub fn is_open(&self, id: NodeId) -> bool {
        let Some(node) = self.setup.net.nodes.get(usize::from(id)) else {
            return false;
        };
        if self.setup.holes.contains(&id) || self.opened & bit(id) != 0 {
            return true;
        }
        node.lock.is_none() && node.patrol != Patrol::BlackWall
    }

    /// What the deck can do, as a [`crate::net::Capability`].
    #[must_use]
    pub fn capability(&self, content: &Content) -> crate::net::Capability {
        capability_of(content, self.deck.iter().map(|s| s.script))
    }

    /// Add trace. `noisy` actions are halved by a mask.
    fn add_trace(&mut self, base: u32, noisy: bool) {
        if base == 0 || self.status != Status::Active {
            return;
        }
        let mut value = base * self.corp_trace_pct / 100;
        if self.alarm {
            value = value * 3 / 2;
        }
        value = value * (100 - 6 * u32::from(self.setup.shield)) / 100;
        if noisy && self.mask > 0 {
            value /= 2;
            self.mask -= 1;
        }
        let soaked = value.min(self.buffer);
        self.buffer -= soaked;
        value -= soaked;
        self.trace = (self.trace + value).min(TRACE_MAX);
        self.peak = self.peak.max(self.trace);
        if self.trace >= TRACE_MAX {
            self.status = Status::Burned;
            self.say("TRACE COMPLETE. They have your address. You are burned.".to_owned());
        }
    }

    fn ambient(&mut self, moving: bool) {
        let mut base = self.watch;
        if self.tracer {
            base += 2;
        }
        if self.feed && moving {
            base += 1;
        }
        self.add_trace(base, false);
    }

    fn success_chance(&self, script: &ScriptDef, need: u8) -> u32 {
        let power = i64::from(script.power);
        let need = i64::from(need);
        let skill = self
            .setup
            .skill
            .get(script.family.index())
            .copied()
            .map_or(0, i64::from);
        let chance = 75 + 5 * (power - need) + 5 * i64::from(self.setup.iface) + 3 * skill;
        u32::try_from(chance.clamp(5, 97)).unwrap_or(5)
    }

    fn script_cycles(&self, script: &ScriptDef) -> u32 {
        if script.cycles >= 2 && self.setup.iface >= 2 {
            script.cycles - 1
        } else {
            script.cycles
        }
    }

    /// Take an action.
    ///
    /// # Errors
    /// [`Refused`] when the action is not allowed; nothing changes then.
    pub fn act(&mut self, content: &Content, act: Act, rng: &mut DoorRng) -> Result<(), Refused> {
        if self.status != Status::Active {
            return Err(Refused::Over);
        }
        match act {
            Act::JackOut => {
                self.status = Status::Out;
                self.say("You jack out clean.".to_owned());
                Ok(())
            }
            Act::Move(to) => self.step(content, to),
            Act::Use { slot, target } => self.use_script(content, slot, target, rng),
        }
    }

    fn step(&mut self, content: &Content, to: NodeId) -> Result<(), Refused> {
        if !self.exits().contains(&to) {
            return Err(Refused::NotAdjacent);
        }
        if !self.is_open(to) {
            return Err(Refused::Locked);
        }
        if self.cycles == 0 {
            return Err(Refused::NoCycles);
        }
        self.cycles -= 1;
        let (name, kind, patrol, honeypot, level) = {
            let node = self.setup.net.nodes.get(usize::from(to)).ok_or(Refused::NotAdjacent)?;
            (node.name.clone(), node.kind, node.patrol, node.honeypot, self.security(to))
        };
        self.at = to;
        self.visited |= bit(to);
        self.discover(to);
        self.say(format!("Into {name}."));
        let def = content.node(kind);
        self.add_trace(def.map_or(0, |d| d.noise), true);
        if def.is_some_and(|d| d.watches) && !self.feed {
            self.feed = true;
            self.say("A camera has your face. The feed is live.".to_owned());
        }
        if matches!(patrol, Patrol::Sentry | Patrol::Tracer) && self.jammed & bit(to) == 0 {
            if self.fork > 0 {
                self.fork -= 1;
                self.say("A fork takes the alarm instead of you.".to_owned());
            } else if patrol == Patrol::Sentry {
                self.say("A sentry sees you.".to_owned());
                self.add_trace(6 + 2 * u32::from(level), false);
            } else {
                self.say("A tracer locks on. Every hop is counted now.".to_owned());
                self.tracer = true;
                self.add_trace(3, false);
            }
        }
        if honeypot && !self.alarm {
            self.alarm = true;
            self.say("ALARM: it was a honeypot. The whole floor knows.".to_owned());
            self.add_trace(25, false);
        }
        self.ambient(true);
        Ok(())
    }

    fn use_script(
        &mut self,
        content: &Content,
        slot: usize,
        target: Option<NodeId>,
        rng: &mut DoorRng,
    ) -> Result<(), Refused> {
        let slot_state = *self.deck.get(slot).ok_or(Refused::NoSlot)?;
        let script = content.script(slot_state.script).ok_or(Refused::NoSlot)?.clone();
        if slot_state.left == 0 {
            return Err(Refused::NoCharges);
        }
        let aimed = if script.effect.needs_target() {
            let target = target.ok_or(Refused::NeedsTarget)?;
            if !self.exits().contains(&target) {
                return Err(Refused::NotAdjacent);
            }
            Some(target)
        } else {
            None
        };
        let cost = self.script_cycles(&script);
        // Refusals that cost nothing are decided before the cycles are taken.
        self.check_useful(content, &script, aimed)?;
        if self.cycles < cost {
            return Err(Refused::NoCycles);
        }
        self.cycles -= cost;
        if let Some(slot_mut) = self.deck.get_mut(slot) {
            if slot_mut.left != u32::MAX {
                slot_mut.left -= 1;
            }
        }
        self.apply(content, &script, aimed, rng);
        self.ambient(false);
        Ok(())
    }

    /// Whether the script can do anything at all here; if not, it is refused for free.
    fn check_useful(&self, content: &Content, script: &ScriptDef, target: Option<NodeId>) -> Result<(), Refused> {
        let here = self.setup.net.nodes.get(usize::from(self.at));
        let code_here = here.and_then(|n| content.node(n.kind)).map(|d| d.code.as_str());
        match script.effect {
            Effect::Crack | Effect::Spoof | Effect::Overflow => {
                let id = target.ok_or(Refused::NeedsTarget)?;
                if self.is_open(id) {
                    return Err(Refused::NothingToDo);
                }
            }
            Effect::Jam => {
                let id = target.ok_or(Refused::NeedsTarget)?;
                let patrol = self.setup.net.nodes.get(usize::from(id)).map(|n| n.patrol);
                if !matches!(patrol, Some(Patrol::Sentry | Patrol::Tracer)) || self.jammed & bit(id) != 0 {
                    return Err(Refused::NothingToDo);
                }
            }
            Effect::Purge => {
                let id = target.ok_or(Refused::NeedsTarget)?;
                let patrol = self.setup.net.nodes.get(usize::from(id)).map(|n| n.patrol);
                if patrol != Some(Patrol::BlackWall) || self.is_open(id) {
                    return Err(Refused::NothingToDo);
                }
            }
            Effect::Copy => {
                if code_here != Some("VT") || self.copied & bit(self.at) != 0 {
                    return Err(Refused::NothingToDo);
                }
            }
            Effect::Plant => {
                if !matches!(code_here, Some("AC" | "SR")) {
                    return Err(Refused::NothingToDo);
                }
            }
            Effect::Shred => {
                if code_here != Some("VT") {
                    return Err(Refused::NothingToDo);
                }
            }
            Effect::Cutfeed => {
                if !self.feed {
                    return Err(Refused::NothingToDo);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn grow(&mut self, family: Family) {
        if let Some(x) = self.xp.get_mut(family.index()) {
            *x += 1;
        }
    }

    fn apply(&mut self, content: &Content, script: &ScriptDef, target: Option<NodeId>, rng: &mut DoorRng) {
        let power = script.power;
        match script.effect {
            Effect::Scan => {
                self.say(format!("{}: sweeping the neighbours.", script.name));
                self.add_trace(script.noise, true);
                let here = self.at;
                let level = u8::try_from(power).unwrap_or(9);
                self.reveal_links(Some(here), level);
                let mut seen = vec![here];
                seen.extend(self.exits());
                for id in seen {
                    self.inspect(content, id, power);
                }
                self.grow(script.family);
            }
            Effect::Map => {
                self.say(format!("{}: the floor plan unrolls.", script.name));
                self.add_trace(script.noise, true);
                self.reveal_links(None, u8::try_from(power).unwrap_or(9));
                self.known = (1u32 << self.setup.net.nodes.len().min(31)) - 1;
                self.grow(script.family);
            }
            Effect::Crack | Effect::Spoof | Effect::Overflow => {
                if let (Some(id), Some(lock)) = (target, script.effect.lock()) {
                    self.break_lock(script, id, lock, rng);
                }
            }
            Effect::Jam => {
                if let Some(id) = target {
                    self.jam(script, id, rng);
                }
            }
            Effect::Purge => {
                if let Some(id) = target {
                    self.purge(script, id, rng);
                }
            }
            Effect::Mask => {
                self.mask = (self.mask + power + 1).min(8);
                self.say(format!("{}: you go quiet.", script.name));
                self.grow(script.family);
            }
            Effect::Decoy => {
                self.buffer += 10 * power;
                self.add_trace(script.noise, false);
                self.say(format!("{}: decoys scatter.", script.name));
                self.grow(script.family);
            }
            Effect::Reroute => {
                let shed = (6 * power).min(self.trace);
                self.trace -= shed;
                self.tracer = false;
                self.say(format!("{}: trace down by {shed}; no tracer.", script.name));
                self.grow(script.family);
            }
            Effect::Fork => {
                self.fork += 2;
                self.add_trace(script.noise, false);
                self.say(format!("{}: two of you now.", script.name));
                self.grow(script.family);
            }
            Effect::Copy => self.copy(content, script),
            Effect::Plant => self.plant(content, script),
            Effect::Shred => self.shred(content, script),
            Effect::Cutfeed => {
                self.feed = false;
                self.add_trace(script.noise, true);
                self.say(format!("{}: the camera goes dark.", script.name));
                self.grow(script.family);
            }
            Effect::Wipelogs => {
                self.heat_cut += 3 * power;
                self.say(format!("{}: the logs are scrubbed.", script.name));
                self.grow(script.family);
            }
        }
    }

    fn reveal_links(&mut self, from: Option<NodeId>, power: u8) {
        let found: Vec<(NodeId, NodeId)> = self
            .setup
            .net
            .links
            .iter()
            .filter(|l| l.stealth > 0 && l.stealth <= power)
            .filter(|l| from.is_none_or(|f| l.a == f || l.b == f))
            .map(|l| (l.a, l.b))
            .filter(|p| !self.revealed.contains(p))
            .collect();
        if !found.is_empty() {
            self.say(format!("Found {} hidden link(s).", found.len()));
        }
        for (a, b) in found {
            self.revealed.push((a, b));
            self.known |= bit(a) | bit(b);
        }
    }

    fn inspect(&mut self, content: &Content, id: NodeId, power: u32) {
        self.scanned |= bit(id);
        self.known |= bit(id);
        if let Some(node) = self.setup.net.nodes.get(usize::from(id))
            && node.honeypot
            && power >= 3
        {
            self.decoys |= bit(id);
            let name = node.name.clone();
            self.say(format!("{name} reads wrong: it is a honeypot."));
        }
        let _ = content;
    }

    fn roll(&mut self, script: &ScriptDef, level: u8, rng: &mut DoorRng) -> bool {
        // A script weaker than the lock never works; a strong enough one usually does.
        if script.power < u32::from(level) {
            return false;
        }
        let chance = self.success_chance(script, level);
        rng.chance(chance, 100)
    }

    fn break_lock(&mut self, script: &ScriptDef, id: NodeId, lock: Lock, rng: &mut DoorRng) {
        let Some(node) = self.setup.net.nodes.get(usize::from(id)) else {
            return;
        };
        let (name, node_lock) = (node.name.clone(), node.lock);
        if node_lock != Some(lock) {
            self.say(format!("{}: {name} is not a {} lock. Wasted.", script.name, lock.word()));
            self.add_trace(script.noise, true);
            return;
        }
        let level = self.security(id);
        if self.roll(script, level, rng) {
            self.opened |= bit(id);
            self.broken.push(id);
            self.say(format!("{}: {name} opens.", script.name));
            self.add_trace(script.noise, true);
            self.grow(script.family);
        } else {
            self.say(format!("{}: {name} holds (level {level}).", script.name));
            self.add_trace(script.noise * 2, true);
        }
    }

    fn jam(&mut self, script: &ScriptDef, id: NodeId, rng: &mut DoorRng) {
        let level = self.security(id);
        let name = self.setup.net.nodes.get(usize::from(id)).map(|n| n.name.clone()).unwrap_or_default();
        if self.roll(script, level, rng) {
            self.jammed |= bit(id);
            self.say(format!("{}: the patrol at {name} goes blind.", script.name));
            self.add_trace(script.noise, true);
            self.grow(script.family);
        } else {
            self.say(format!("{}: the patrol at {name} shrugs it off.", script.name));
            self.add_trace(script.noise * 2, true);
        }
    }

    fn purge(&mut self, script: &ScriptDef, id: NodeId, rng: &mut DoorRng) {
        let level = self.security(id);
        let name = self.setup.net.nodes.get(usize::from(id)).map(|n| n.name.clone()).unwrap_or_default();
        if self.roll(script, level, rng) {
            self.opened |= bit(id);
            self.broken.push(id);
            self.say(format!("{}: a hole in the black wall at {name}.", script.name));
            self.add_trace(script.noise, true);
            self.grow(script.family);
        } else {
            self.say(format!("{}: the black wall at {name} closes ranks.", script.name));
            self.add_trace(script.noise * 2, true);
        }
    }

    fn here(&self) -> Option<(&crate::net::Node, bool)> {
        self.setup.net.nodes.get(usize::from(self.at)).map(|n| (n, n.honeypot))
    }

    fn trip_decoy(&mut self, script: &ScriptDef) {
        self.say(format!("{}: nothing but a trap. The alarm goes up.", script.name));
        if !self.alarm {
            self.alarm = true;
            self.add_trace(15, false);
        }
    }

    fn copy(&mut self, content: &Content, script: &ScriptDef) {
        let Some((node, honey)) = self.here() else { return };
        let (value, code) = (
            node.value,
            content.node(node.kind).map(|d| d.code.clone()).unwrap_or_default(),
        );
        if honey {
            self.trip_decoy(script);
            return;
        }
        self.copied |= bit(self.at);
        self.loot += value;
        self.say(format!("{}: {value} marks of data copied out.", script.name));
        self.add_trace(script.noise, true);
        if self.setup.effect == Effect::Copy && code == self.setup.target && !self.objective {
            self.objective = true;
            self.say("Objective done. Get out.".to_owned());
        }
        self.grow(script.family);
    }

    fn plant(&mut self, content: &Content, script: &ScriptDef) {
        let Some((node, _)) = self.here() else { return };
        let code = content.node(node.kind).map(|d| d.code.clone()).unwrap_or_default();
        self.say(format!("{}: a backdoor is planted.", script.name));
        self.planted = true;
        self.add_trace(script.noise, true);
        if self.setup.effect == Effect::Plant && code == self.setup.target && !self.objective {
            self.objective = true;
            self.say("Objective done. Get out.".to_owned());
        }
        self.grow(script.family);
    }

    fn shred(&mut self, content: &Content, script: &ScriptDef) {
        let Some((node, honey)) = self.here() else { return };
        let code = content.node(node.kind).map(|d| d.code.clone()).unwrap_or_default();
        if honey {
            self.trip_decoy(script);
            return;
        }
        self.say(format!("{}: the record never existed.", script.name));
        self.add_trace(script.noise, true);
        if self.setup.effect == Effect::Shred && code == self.setup.target && !self.objective {
            self.objective = true;
            self.say("Objective done. Get out.".to_owned());
        }
        self.grow(script.family);
    }

    /// What the run produced so far. A burned run pays nothing: the caller checks [`Status`].
    #[must_use]
    pub fn result(&self) -> RunResult {
        RunResult {
            status: self.status,
            objective: self.objective,
            loot: self.loot,
            heat_cut: self.heat_cut,
            peak: self.peak,
            cycles_left: self.cycles,
            broken: self.broken.clone(),
            planted: self.planted,
            xp: self.xp,
        }
    }

    /// The text for a node as the player may know it: the name always, the details only if
    /// scanned.
    #[must_use]
    pub fn describe(&self, content: &Content, id: NodeId) -> String {
        let Some(node) = self.setup.net.nodes.get(usize::from(id)) else {
            return String::new();
        };
        let kind = content.node(node.kind).map_or("node", |d| d.name.as_str());
        if self.scanned & bit(id) == 0 {
            let lock = if self.is_open(id) { "open" } else { "locked?" };
            return format!("{} {kind} ({lock})", node.name);
        }
        let mut parts = vec![format!("{} {kind}", node.name)];
        if self.is_open(id) {
            parts.push("open".to_owned());
        } else if let Some(lock) = node.lock {
            parts.push(format!("{} lock {}", lock.word(), self.security(id)));
        }
        match node.patrol {
            Patrol::None => {}
            Patrol::Sentry => parts.push(if self.jammed & bit(id) != 0 { "sentry (jammed)".into() } else { "sentry".into() }),
            Patrol::Tracer => parts.push(if self.jammed & bit(id) != 0 { "tracer (jammed)".into() } else { "tracer".into() }),
            Patrol::BlackWall => parts.push(if self.is_open(id) { "black wall (breached)".into() } else { format!("black wall {}", self.security(id)) }),
        }
        if self.decoys & bit(id) != 0 {
            parts.push("HONEYPOT".to_owned());
        }
        parts.join(", ")
    }

    /// Whether the node's type is known.
    #[must_use]
    pub const fn is_known(&self, id: NodeId) -> bool {
        self.known & bit(id) != 0
    }
}

/// The best script of each kind in a list of script numbers.
#[must_use]
pub fn capability_of(content: &Content, scripts: impl Iterator<Item = u8>) -> crate::net::Capability {
    let mut cap = crate::net::Capability::default();
    for script in scripts.filter_map(|n| content.script(n)) {
        let power = u8::try_from(script.power).unwrap_or(9);
        if let Some(lock) = script.effect.lock() {
            if let Some(slot) = cap.breaks.get_mut(lock.index()) {
                *slot = (*slot).max(power);
            }
        } else if script.effect == Effect::Purge {
            cap.purge = cap.purge.max(power);
        } else if matches!(script.effect, Effect::Scan | Effect::Map) {
            cap.scan = cap.scan.max(power);
        }
    }
    cap
}

/// The corporation's note for the briefing.
#[must_use]
pub fn corp_intel(corp: &CorpDef) -> String {
    format!("{} Leans on {} locks.", corp.style, corp.favoured.word())
}
