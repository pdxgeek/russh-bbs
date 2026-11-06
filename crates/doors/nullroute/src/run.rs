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

use crate::cm::{self, Encounter};
use crate::code::CODED_BASE;
use crate::content::{CmKind, Content, CorpDef, Effect, Family, Intent, Lock, ScriptDef};
use crate::learn::{self, Stage};
use crate::net::{Network, NodeId, Patrol};

/// The trace at which you are burned.
pub const TRACE_MAX: u32 = 100;

/// The most log lines kept.
pub const LOG_MAX: usize = 60;

/// The most cycles a script action can cost (a guard for the tests).
pub const MAX_SCRIPT_CYCLES: u32 = 3;

/// Trace at which a warden, if the network keeps one, wakes.
pub const WARDEN_WAKES_AT: u32 = 55;

/// Moves between a warden waking and its arrival.
pub const WARDEN_ARRIVES_IN: u32 = 3;

/// Moves before a warden you broke off from comes back.
pub const WARDEN_RETURNS_IN: u32 = 1;

/// The most fragments one run can bring out.
pub const FRAGMENTS_PER_RUN: usize = 6;

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
    /// The definitions of coded scripts in the deck (numbers from `CODED_BASE` up).
    pub extra: Vec<ScriptDef>,
    /// Runs against this corporation that used each script identity (from the world).
    pub learned: Vec<(u16, u32)>,
    /// The warden (a countermeasure number) this run will wake, if any.
    pub warden: Option<u8>,
    /// Percent of trace this run adds: 100 alone, less in a crew, where the trace is shared and
    /// each member's noise is a smaller part of it (see `crew`).
    pub trace_share: u32,
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
    /// In an encounter: spend a cycle to take the next strike at three quarters.
    Brace,
    /// In an encounter: give up the fight and step back (it costs trace; the damage you did stays).
    BreakOff,
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
    /// You are in an exchange: play a script, brace or break off.
    Engaged,
    /// The family was sealed by the countermeasure this move.
    Sealed,
    /// The corporation has learned the script: it does nothing here.
    Patched,
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
            Self::Engaged => "Mid-exchange: play a script, brace [w], or break off [x].",
            Self::Sealed => "That family is sealed this move. Play another.",
            Self::Patched => "They have patched that script here. Use another.",
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
    /// The identities of the scripts that worked this run (the world counts them for the
    /// corporation's learning).
    pub used: Vec<u16>,
    /// Fragments found, to be added to the character.
    pub fragments: Vec<u8>,
    /// Countermeasures put down.
    pub defeated: u32,
    /// A warden was put down.
    pub warden_down: bool,
    /// Fights begun (a fight resumed after breaking off counts again).
    pub fights: u32,
    /// Times you broke off, or were thrown out.
    pub broke_off: u32,
    /// Whether a warden woke.
    pub warden_woke: bool,
}

/// What the player knows of one node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    /// The node's id.
    pub id: NodeId,
    /// The name, such as `FW-3`.
    pub name: String,
    /// The type's name.
    pub kind: String,
    /// The type's two-letter code.
    pub code: String,
    /// How deep the node is (0 is the entry).
    pub depth: u8,
    /// Whether it has been scanned.
    pub scanned: bool,
    /// Whether it can be entered.
    pub open: bool,
    /// The lock and its level, if scanned and still locked.
    pub lock: Option<(Lock, u8)>,
    /// The patrol, if scanned.
    pub patrol: Patrol,
    /// The patrol's level.
    pub patrol_level: u8,
    /// Whether a jam switched the patrol off.
    pub patrol_off: bool,
    /// A black wall that was purged.
    pub wall_breached: bool,
    /// A scan showed it is a honeypot.
    pub honeypot: bool,
    /// The countermeasure's name, if scanned and still standing.
    pub cm: Option<String>,
}

/// The warden hunting a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warden {
    /// The warden's number.
    pub cm: u8,
    /// Whether it has woken.
    pub awake: bool,
    /// Moves until it arrives.
    pub arrives_in: u32,
    /// Whether it was put down.
    pub down: bool,
    /// The fight so far, if you broke off.
    pub paused: Option<Encounter>,
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
    enc: Option<Encounter>,
    paused: Vec<Encounter>,
    beaten: u32,
    warden: Option<Warden>,
    used: Vec<u16>,
    frags: Vec<u8>,
    defeated: u32,
    fights: u32,
    broke_off: u32,
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
            .filter_map(|n| resolve(content, &setup.extra, *n))
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
            enc: None,
            paused: Vec::new(),
            beaten: 0,
            warden: setup.warden.map(|cm| Warden {
                cm,
                awake: false,
                arrives_in: 0,
                down: false,
                paused: None,
            }),
            used: Vec::new(),
            frags: Vec::new(),
            defeated: 0,
            fights: 0,
            broke_off: 0,
            setup,
        };
        run.say(format!("Jacked in to {}.", corp.name));
        if run.setup.backdoor {
            run.say("An old backdoor shaves the first hops.".to_owned());
        }
        run.discover(0);
        Some(run)
    }

    /// Crew seam: set the shared trace. The member's own run keeps no meter of its own in a crew.
    pub(crate) fn crew_set_trace(&mut self, trace: u32) {
        if self.status != Status::Active {
            return;
        }
        self.trace = trace.min(TRACE_MAX);
        self.peak = self.peak.max(self.trace);
        if self.trace >= TRACE_MAX {
            self.status = Status::Burned;
            self.say("TRACE COMPLETE. The crew's address is out. You are burned.".to_owned());
        }
    }

    /// Crew seam: the locks a crewmate opened are open for this member too.
    pub(crate) fn crew_open(&mut self, nodes: &[NodeId]) {
        for id in nodes {
            self.opened |= bit(*id);
        }
    }

    /// Crew seam: take on what a crewmate's scans found (nodes known, nodes scanned, hidden links
    /// revealed), so the map is the crew's map.
    pub(crate) fn crew_learn(&mut self, other: &Self) {
        self.known |= other.known;
        self.scanned |= other.scanned;
        for link in &other.revealed {
            if !self.revealed.contains(link) {
                self.revealed.push(*link);
            }
        }
        if other.alarm {
            self.alarm = true;
        }
        // A machine a crewmate put down stays down for everyone.
        self.beaten |= other.beaten;
    }

    /// Crew seam: the crew's warden arrives for this member.
    pub(crate) fn crew_summon_warden(&mut self, content: &Content, cm: u8) {
        if self.status != Status::Active || self.enc.is_some() {
            return;
        }
        self.warden = Some(Warden {
            cm,
            awake: true,
            arrives_in: 1,
            down: false,
            paused: None,
        });
        let at = self.at;
        self.warden_step(content, at);
    }

    /// Crew seam: the crew's trace reached 100 (or the line went cold): this member is out.
    pub(crate) fn crew_end(&mut self, burned: bool) {
        if self.status != Status::Active {
            return;
        }
        if burned {
            self.status = Status::Burned;
            self.say("TRACE COMPLETE. The crew is burned.".to_owned());
        } else {
            self.status = Status::Out;
            self.say("The line went cold. You are out.".to_owned());
        }
    }

    /// The name of the node you stand on.
    #[must_use]
    pub fn name_here(&self) -> String {
        self.setup
            .net
            .nodes
            .get(usize::from(self.at))
            .map(|n| n.name.clone())
            .unwrap_or_default()
    }

    /// The locks this run opened so far.
    #[must_use]
    pub fn broken_nodes(&self) -> &[NodeId] {
        &self.broken
    }

    /// Nodes known so far (a bit set).
    #[must_use]
    pub const fn known_mask(&self) -> u32 {
        self.known
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

    /// The fight in progress, if any.
    #[must_use]
    pub const fn encounter(&self) -> Option<&Encounter> {
        self.enc.as_ref()
    }

    /// The warden hunting this run, if the network keeps one.
    #[must_use]
    pub const fn warden(&self) -> Option<&Warden> {
        self.warden.as_ref()
    }

    /// A script in the deck by number (stock or coded).
    #[must_use]
    pub fn script(&self, content: &Content, num: u8) -> Option<ScriptDef> {
        resolve(content, &self.setup.extra, num)
    }

    /// How well the corporation knows the script in `slot`.
    #[must_use]
    pub fn stage(&self, content: &Content, slot: usize) -> Stage {
        self.deck
            .get(slot)
            .and_then(|s| self.script(content, s.script))
            .map_or(Stage::Fresh, |s| self.stage_of(&s))
    }

    fn stage_of(&self, script: &ScriptDef) -> Stage {
        let uses = self
            .setup
            .learned
            .iter()
            .find(|(sig, _)| *sig == script.sig)
            .map_or(0, |(_, n)| *n);
        learn::stage(script, uses)
    }

    /// The countermeasure on a node, if it has one and it is still standing.
    #[must_use]
    pub fn cm_at<'a>(&self, content: &'a Content, id: NodeId) -> Option<&'a crate::content::CmDef> {
        if self.beaten & bit(id) != 0 {
            return None;
        }
        let node = self.setup.net.nodes.get(usize::from(id))?;
        let kind = cm::kind_of(node.patrol)?;
        let corp = content.corp(self.setup.net.corp)?;
        let label = format!("nullroute-cm-{}-{}", node.name, node.security);
        cm::pick(content, corp, self.setup.net.tier, kind, &label)
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
    #[must_use]
    pub fn link_known(&self, a: NodeId, b: NodeId) -> bool {
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
        capability_of_defs(
            self.deck
                .iter()
                .filter_map(|s| self.script(content, s.script)),
        )
    }

    /// Add trace. `noisy` actions are halved by a mask.
    fn add_trace(&mut self, base: u32, noisy: bool) {
        if base == 0 || self.status != Status::Active {
            return;
        }
        // Deeper tiers are watched harder: x1.0, x1.3, x1.6.
        let tier_pct = 100 + 30 * (u32::from(self.setup.net.tier.clamp(1, 3)) - 1);
        let mut value = base * self.corp_trace_pct / 100 * tier_pct / 100;
        value = value * self.setup.trace_share / 100;
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
        self.wake_warden();
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
        if self.enc.is_some() {
            return match act {
                Act::Use { slot, .. } => self.play(content, slot, rng),
                Act::Brace => self.brace(),
                Act::BreakOff => {
                    self.break_off();
                    Ok(())
                }
                Act::Move(_) | Act::JackOut => Err(Refused::Engaged),
            };
        }
        match act {
            Act::JackOut => {
                self.status = Status::Out;
                self.say("You jack out clean.".to_owned());
                Ok(())
            }
            Act::Move(to) => self.step(content, to),
            Act::Use { slot, target } => self.use_script(content, slot, target, rng),
            Act::Brace | Act::BreakOff => Err(Refused::NothingToDo),
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
            let node = self
                .setup
                .net
                .nodes
                .get(usize::from(to))
                .ok_or(Refused::NotAdjacent)?;
            (
                node.name.clone(),
                node.kind,
                node.patrol,
                node.honeypot,
                self.security(to),
            )
        };
        let from = self.at;
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
        let _ = level;
        if matches!(patrol, Patrol::Sentry | Patrol::Tracer) && self.beaten & bit(to) == 0 {
            if self.fork > 0 && self.jammed & bit(to) == 0 {
                self.fork -= 1;
                self.say("A fork takes the alarm instead of you.".to_owned());
            } else {
                // A patrol you jammed from next door starts the fight blind.
                let stun = if self.jammed & bit(to) == 0 { 0 } else { 2 };
                self.jammed &= !bit(to);
                self.begin_encounter(content, to, from, stun);
            }
        }
        if honeypot && !self.alarm {
            self.alarm = true;
            self.say("ALARM: it was a honeypot. The whole floor knows.".to_owned());
            self.add_trace(25, false);
        }
        self.warden_step(content, from);
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
        let script = self.staged(content, slot_state.script)?;
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
        if script.effect == Effect::Purge
            && let Some(wall) = aimed
        {
            // A black wall is fought, not rolled: the purge is the first move of the exchange.
            self.begin_wall_encounter(content, wall);
            let played = self.play_script(content, slot, &script, rng);
            if played.is_err() {
                self.enc = None;
            }
            return played;
        }
        if self.cycles < cost {
            return Err(Refused::NoCycles);
        }
        self.cycles -= cost;
        if let Some(slot_mut) = self.deck.get_mut(slot)
            && slot_mut.left != u32::MAX
        {
            slot_mut.left -= 1;
        }
        let before: u32 = self.xp.iter().sum();
        self.apply(content, &script, aimed, rng);
        if self.xp.iter().sum::<u32>() > before {
            self.note_used(&script);
        }
        self.ambient(false);
        Ok(())
    }

    /// Whether the script can do anything at all here; if not, it is refused for free.
    fn check_useful(
        &self,
        content: &Content,
        script: &ScriptDef,
        target: Option<NodeId>,
    ) -> Result<(), Refused> {
        let here = self.setup.net.nodes.get(usize::from(self.at));
        let code_here = here
            .and_then(|n| content.node(n.kind))
            .map(|d| d.code.as_str());
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
                if !matches!(patrol, Some(Patrol::Sentry | Patrol::Tracer))
                    || self.jammed & bit(id) != 0
                    || self.beaten & bit(id) != 0
                {
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
            Effect::Cutfeed if !self.feed => return Err(Refused::NothingToDo),
            _ => {}
        }
        Ok(())
    }

    fn grow(&mut self, family: Family) {
        if let Some(x) = self.xp.get_mut(family.index()) {
            *x += 1;
        }
    }

    fn apply(
        &mut self,
        content: &Content,
        script: &ScriptDef,
        target: Option<NodeId>,
        rng: &mut DoorRng,
    ) {
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
            // A purge is the first move of a fight with the wall: see `begin_wall_encounter`.
            Effect::Purge => {}
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
            Effect::Copy => self.copy(content, script, rng),
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
            self.say(format!(
                "{}: {name} is not a {} lock. Wasted.",
                script.name,
                lock.word()
            ));
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
        let name = self
            .setup
            .net
            .nodes
            .get(usize::from(id))
            .map(|n| n.name.clone())
            .unwrap_or_default();
        if self.roll(script, level, rng) {
            self.jammed |= bit(id);
            self.say(format!("{}: the patrol at {name} goes blind.", script.name));
            self.add_trace(script.noise, true);
            self.grow(script.family);
        } else {
            self.say(format!(
                "{}: the patrol at {name} shrugs it off.",
                script.name
            ));
            self.add_trace(script.noise * 2, true);
        }
    }

    fn here(&self) -> Option<(&crate::net::Node, bool)> {
        self.setup
            .net
            .nodes
            .get(usize::from(self.at))
            .map(|n| (n, n.honeypot))
    }

    fn trip_decoy(&mut self, script: &ScriptDef) {
        self.say(format!(
            "{}: nothing but a trap. The alarm goes up.",
            script.name
        ));
        if !self.alarm {
            self.alarm = true;
            self.add_trace(15, false);
        }
    }

    fn copy(&mut self, content: &Content, script: &ScriptDef, rng: &mut DoorRng) {
        let Some((node, honey)) = self.here() else {
            return;
        };
        let (value, code) = (
            node.value,
            content
                .node(node.kind)
                .map(|d| d.code.clone())
                .unwrap_or_default(),
        );
        if honey {
            self.trip_decoy(script);
            return;
        }
        self.copied |= bit(self.at);
        self.loot += value;
        self.say(format!(
            "{}: {value} marks of data copied out.",
            script.name
        ));
        self.add_trace(script.noise, true);
        if self.setup.effect == Effect::Copy && code == self.setup.target && !self.objective {
            self.objective = true;
            self.say("Objective done. Get out.".to_owned());
        }
        // Vaults hold more than data: sometimes a bit of someone's code.
        if rng.chance(15, 100) {
            self.find_fragment(content, rng);
        }
        self.grow(script.family);
    }

    fn plant(&mut self, content: &Content, script: &ScriptDef) {
        let Some((node, _)) = self.here() else { return };
        let code = content
            .node(node.kind)
            .map(|d| d.code.clone())
            .unwrap_or_default();
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
        let Some((node, honey)) = self.here() else {
            return;
        };
        let code = content
            .node(node.kind)
            .map(|d| d.code.clone())
            .unwrap_or_default();
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
            used: self.used.clone(),
            fragments: self.frags.clone(),
            defeated: self.defeated,
            warden_down: self.warden.as_ref().is_some_and(|w| w.down),
            fights: self.fights,
            broke_off: self.broke_off,
            warden_woke: self.warden.as_ref().is_some_and(|w| w.awake || w.down),
        }
    }

    /// What the player knows of a node: the name, the depth and type always, the lock, security
    /// and patrol only once scanned.
    #[must_use]
    pub fn view(&self, content: &Content, id: NodeId) -> Option<NodeView> {
        let node = self.setup.net.nodes.get(usize::from(id))?;
        let scanned = self.scanned & bit(id) != 0;
        let open = self.is_open(id);
        let patrol = if scanned { node.patrol } else { Patrol::None };
        Some(NodeView {
            id,
            name: node.name.clone(),
            kind: content
                .node(node.kind)
                .map_or_else(|| "node".to_owned(), |d| d.name.clone()),
            code: content
                .node(node.kind)
                .map_or_else(String::new, |d| d.code.clone()),
            depth: node.layer,
            scanned,
            open,
            lock: if scanned && !open {
                node.lock.map(|l| (l, self.security(id)))
            } else {
                None
            },
            patrol,
            patrol_level: self.security(id),
            patrol_off: self.jammed & bit(id) != 0 || self.beaten & bit(id) != 0,
            wall_breached: node.patrol == Patrol::BlackWall && open && scanned,
            honeypot: self.decoys & bit(id) != 0,
            cm: if scanned {
                self.cm_at(content, id).map(|c| c.name.clone())
            } else {
                None
            },
        })
    }

    /// The text for a node as the player may know it.
    #[must_use]
    pub fn describe(&self, content: &Content, id: NodeId) -> String {
        let Some(v) = self.view(content, id) else {
            return String::new();
        };
        let mut parts = vec![format!("{} {} d{}", v.name, v.kind, v.depth)];
        if !v.scanned {
            parts.push(if v.open {
                "open".to_owned()
            } else {
                "locked?".to_owned()
            });
            return parts.join(", ");
        }
        if v.open {
            parts.push("open".to_owned());
        } else if let Some((lock, level)) = v.lock {
            parts.push(format!("{} lock {level}", lock.word()));
        }
        let off = if self.beaten & bit(id) != 0 {
            " (down)"
        } else if v.patrol_off {
            " (jammed)"
        } else {
            ""
        };
        let who =
            v.cm.as_deref()
                .map_or_else(String::new, |n| format!(" {n}"));
        match v.patrol {
            Patrol::None => {}
            Patrol::Sentry => parts.push(format!("sentry{who} {}{off}", v.patrol_level)),
            Patrol::Tracer => parts.push(format!("tracer{who} {}{off}", v.patrol_level)),
            Patrol::BlackWall if v.wall_breached => parts.push("black wall (breached)".to_owned()),
            Patrol::BlackWall => parts.push(format!("black wall{who} {}", v.patrol_level)),
        }
        if v.honeypot {
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
pub fn capability_of(
    content: &Content,
    scripts: impl Iterator<Item = u8>,
) -> crate::net::Capability {
    capability_of_defs(scripts.filter_map(|n| content.script(n).cloned()))
}

/// The best script of each kind in a list of script definitions (stock or coded).
#[must_use]
pub fn capability_of_defs(scripts: impl Iterator<Item = ScriptDef>) -> crate::net::Capability {
    let mut cap = crate::net::Capability::default();
    for script in scripts {
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

/// A script by number: stock from the tables, coded from the run's own definitions.
fn resolve(content: &Content, extra: &[ScriptDef], num: u8) -> Option<ScriptDef> {
    if num >= CODED_BASE {
        extra.iter().find(|s| s.num == num).cloned()
    } else {
        content.script(num).cloned()
    }
}

// Encounters (M4-N2): the exchange with a countermeasure, and the warden that hunts a run.
impl Run {
    fn note_used(&mut self, script: &ScriptDef) {
        if !self.used.contains(&script.sig) {
            self.used.push(script.sig);
        }
    }

    fn find_fragment(&mut self, content: &Content, rng: &mut DoorRng) {
        if self.frags.len() >= FRAGMENTS_PER_RUN {
            return;
        }
        if let Some(num) = cm::drop_fragment(content, self.setup.net.tier, rng) {
            self.frags.push(num);
            let name = content
                .fragment(num)
                .map_or("a fragment", |f| f.name.as_str())
                .to_owned();
            self.say(format!("Found a fragment: {name}."));
        }
    }

    /// The script in a deck slot as it works against this corporation: weaker and louder when they
    /// are wary of it, refused when they have patched it.
    fn staged(&self, content: &Content, num: u8) -> Result<ScriptDef, Refused> {
        let mut script = self.script(content, num).ok_or(Refused::NoSlot)?;
        match self.stage_of(&script) {
            Stage::Patched => return Err(Refused::Patched),
            Stage::Wary => {
                script.power = script.power.saturating_sub(1).max(1);
                script.noise = script.noise * 3 / 2;
            }
            Stage::Fresh => {}
        }
        Ok(script)
    }

    fn wake_warden(&mut self) {
        let loud = self.trace >= WARDEN_WAKES_AT || self.alarm;
        let woke = match self.warden.as_mut() {
            Some(w) if loud && !w.awake && !w.down => {
                w.awake = true;
                w.arrives_in = WARDEN_ARRIVES_IN;
                true
            }
            _ => false,
        };
        if woke {
            self.say(format!(
                "A warden wakes in the core and starts for you. {WARDEN_ARRIVES_IN} moves."
            ));
        }
    }

    fn warden_step(&mut self, content: &Content, from: NodeId) {
        if self.enc.is_some() {
            return;
        }
        let at = self.at;
        let level = self.security(at);
        let Some(w) = self.warden.as_mut() else {
            return;
        };
        if !w.awake || w.down {
            return;
        }
        w.arrives_in = w.arrives_in.saturating_sub(1);
        if w.arrives_in > 0 {
            return;
        }
        let num = w.cm;
        let paused = w.paused.take();
        let Some(def) = content.cm(num) else {
            return;
        };
        let mut e = if let Some(mut e) = paused {
            e.node = at;
            e.resume(from);
            e
        } else {
            Encounter::new(def, at, from, level, usize::from(at))
        };
        e.stun = 0;
        let line = format!(
            "{} finds you. A warden: only a script tuned to wardens will scratch it.",
            e.name
        );
        self.fights += 1;
        self.enc = Some(e);
        self.say(line);
    }

    fn begin_encounter(&mut self, content: &Content, node: NodeId, from: NodeId, stun: u32) {
        if let Some(at) = self.paused.iter().position(|e| e.node == node) {
            let mut e = self.paused.remove(at);
            e.resume(from);
            e.stun = stun;
            let line = format!("{} is waiting where you left it.", e.name);
            self.fights += 1;
            self.enc = Some(e);
            self.say(line);
            return;
        }
        let Some(def) = self.cm_at(content, node) else {
            return;
        };
        let level = self.security(node);
        let mut e = Encounter::new(def, node, from, level, usize::from(node));
        e.stun = stun;
        let line = format!(
            "{} {} has you. Its first beat: {}.",
            def.kind.word(),
            def.name,
            e.intent().word()
        );
        self.fights += 1;
        self.enc = Some(e);
        self.say(line);
    }

    fn begin_wall_encounter(&mut self, content: &Content, wall: NodeId) {
        let from = self.at;
        self.begin_encounter(content, wall, from, 0);
    }

    fn play(&mut self, content: &Content, slot: usize, rng: &mut DoorRng) -> Result<(), Refused> {
        let num = self.deck.get(slot).ok_or(Refused::NoSlot)?.script;
        let script = self.staged(content, num)?;
        self.play_script(content, slot, &script, rng)
    }

    /// One move of an encounter: `script` is the deck slot's script as it works here.
    fn play_script(
        &mut self,
        content: &Content,
        slot: usize,
        script: &ScriptDef,
        rng: &mut DoorRng,
    ) -> Result<(), Refused> {
        let slot_state = *self.deck.get(slot).ok_or(Refused::NoSlot)?;
        if slot_state.left == 0 {
            return Err(Refused::NoCharges);
        }
        let Some(enc) = self.enc.as_ref() else {
            return Err(Refused::NothingToDo);
        };
        if enc.sealed == Some(script.family) {
            return Err(Refused::Sealed);
        }
        if !matches!(
            script.effect,
            Effect::Crack
                | Effect::Spoof
                | Effect::Overflow
                | Effect::Jam
                | Effect::Purge
                | Effect::Fork
                | Effect::Mask
                | Effect::Decoy
                | Effect::Reroute
                | Effect::Scan
                | Effect::Map
        ) {
            return Err(Refused::NothingToDo);
        }
        // A move in a fight is quick: one cycle, whatever the script costs outside one.
        let cost = 1;
        if self.cycles < cost {
            return Err(Refused::NoCycles);
        }
        self.cycles -= cost;
        if let Some(s) = self.deck.get_mut(slot)
            && s.left != u32::MAX
        {
            s.left -= 1;
        }
        let skill = self
            .setup
            .skill
            .get(script.family.index())
            .copied()
            .unwrap_or(0);
        let before: u32 = self.xp.iter().sum();
        let mut dealt = 0;
        match script.effect {
            Effect::Crack | Effect::Spoof | Effect::Overflow | Effect::Purge | Effect::Jam => {
                let Some((name, hit)) = self
                    .enc
                    .as_ref()
                    .map(|enc| (enc.name.clone(), enc.damage(script, skill)))
                else {
                    return Err(Refused::NothingToDo);
                };
                dealt = hit;
                // The machine already knows you are there: a fight is half as quiet as a probe.
                self.add_trace(script.noise.div_ceil(2), true);
                if script.effect == Effect::Jam {
                    if let Some(e) = self.enc.as_mut() {
                        e.stun += 1;
                    }
                    self.grow(script.family);
                    self.say(format!(
                        "{}: {name} loses its next beat (-{hit}).",
                        script.name
                    ));
                } else if hit > 0 {
                    self.grow(script.family);
                    self.say(format!("{}: -{hit} on {name}.", script.name));
                } else if self.enc.as_ref().is_some_and(|e| e.kind == CmKind::Warden) {
                    self.say(format!(
                        "{}: bounces off {name}. Only a script tuned to wardens works.",
                        script.name
                    ));
                } else {
                    self.say(format!("{}: {name}'s armor soaks it.", script.name));
                }
            }
            _ => {
                if matches!(script.effect, Effect::Scan | Effect::Map)
                    && let Some(e) = self.enc.as_mut()
                {
                    e.scanned = true;
                }
                self.apply(content, script, None, rng);
            }
        }
        if self.xp.iter().sum::<u32>() > before {
            self.note_used(script);
        }
        if self.status != Status::Active {
            return Ok(());
        }
        let down = self.enc.as_mut().is_some_and(|e| e.hit(dealt));
        if down {
            self.defeat(content, rng);
            self.ambient(false);
            return Ok(());
        }
        self.round(dealt, Some(script.family), false);
        self.ambient(false);
        self.check_spent();
        Ok(())
    }

    fn brace(&mut self) -> Result<(), Refused> {
        if self.cycles == 0 {
            return Err(Refused::NoCycles);
        }
        self.cycles -= 1;
        self.say("You brace and wait for the beat.".to_owned());
        self.round(0, None, true);
        self.ambient(false);
        self.check_spent();
        Ok(())
    }

    /// The countermeasure's beat, after your move.
    fn round(&mut self, dealt: u32, family: Option<Family>, braced: bool) {
        let Some(beat) = self.enc.as_mut().map(|e| e.resolve(dealt, family)) else {
            return;
        };
        let name = self
            .enc
            .as_ref()
            .map_or_else(String::new, |e| e.name.clone());
        if beat.stunned {
            self.say(format!("{name} is blind this beat."));
            return;
        }
        let mut value = beat.trace;
        let mut note = String::new();
        if beat.strike && self.fork > 0 {
            self.fork -= 1;
            value = 0;
            note.push_str(" A fork takes the hit.");
        }
        if braced && value > 0 {
            value = value * 3 / 4;
        }
        let before = self.trace;
        if value > 0 {
            self.add_trace(value, true);
        }
        let delta = self.trace - before;
        if beat.tracer && !self.tracer {
            self.tracer = true;
            note.push_str(" Every hop is counted now.");
        }
        let cost = if delta > 0 {
            format!("trace +{delta}")
        } else {
            "no trace".to_owned()
        };
        self.say(format!("{name}: {}, {cost}.{note}", beat.intent.word()));
    }

    fn check_spent(&mut self) {
        if self.status != Status::Active {
            return;
        }
        if self.enc.as_ref().is_some_and(Encounter::spent) {
            let name = self
                .enc
                .as_ref()
                .map_or_else(String::new, |e| e.name.clone());
            self.say(format!("{name} has had enough of you. You are thrown out."));
            self.break_off();
        }
    }

    /// Leave the exchange. The damage you did and what you learned stay with the machine.
    fn break_off(&mut self) {
        let Some(e) = self.enc.take() else {
            return;
        };
        let penalty = e.strike / 2 + 2;
        if e.kind != CmKind::Wall && e.node != e.from {
            self.at = e.from;
        }
        self.broke_off += 1;
        self.say(format!("You break off from {}.", e.name));
        if e.kind == CmKind::Warden {
            if let Some(w) = self.warden.as_mut() {
                w.paused = Some(e);
                w.arrives_in = WARDEN_RETURNS_IN;
            }
        } else {
            self.paused.retain(|p| p.node != e.node);
            self.paused.push(e);
        }
        self.add_trace(penalty, false);
    }

    fn defeat(&mut self, content: &Content, rng: &mut DoorRng) {
        let Some(e) = self.enc.take() else {
            return;
        };
        self.defeated += 1;
        self.say(format!("{} goes down after {} moves.", e.name, e.round + 1));
        let mut drops = 0;
        match e.kind {
            CmKind::Wall => {
                self.opened |= bit(e.node);
                self.broken.push(e.node);
                self.say("A hole in the wall.".to_owned());
            }
            CmKind::Warden => {
                if let Some(w) = self.warden.as_mut() {
                    w.down = true;
                    w.paused = None;
                }
                self.say("The warden is down. The network is quiet.".to_owned());
                drops = 2;
            }
            CmKind::Sentry | CmKind::Tracer => {
                self.beaten |= bit(e.node);
                if e.kind == CmKind::Tracer {
                    self.tracer = false;
                    self.say("The tracer is cut loose.".to_owned());
                }
            }
        }
        if drops == 0 && content.cm(e.cm).is_some_and(|d| rng.chance(d.drop, 100)) {
            drops = 1;
        }
        for _ in 0..drops {
            self.find_fragment(content, rng);
        }
    }

    /// What the script in `slot` would do to the machine this move, if there is a fight.
    #[must_use]
    pub fn preview(&self, content: &Content, slot: usize) -> Option<u32> {
        let enc = self.enc.as_ref()?;
        let num = self.deck.get(slot)?.script;
        let script = self.staged(content, num).ok()?;
        let skill = self
            .setup
            .skill
            .get(script.family.index())
            .copied()
            .unwrap_or(0);
        Some(enc.damage(&script, skill))
    }

    /// The intent the countermeasure shows this move, if there is a fight.
    #[must_use]
    pub fn intent(&self) -> Option<Intent> {
        self.enc.as_ref().map(Encounter::intent)
    }
}
