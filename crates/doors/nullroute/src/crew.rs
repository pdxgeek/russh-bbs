//! Crews and crew runs (M4-N4).
//!
//! A **crew** is up to five players. A **crew run** is one hack of one network by several of them,
//! asynchronously across the day, with **one shared trace meter**, one warden, and a map that shows
//! every member's position.
//!
//! **The shared run is an event log.** The world holds an [`Op`]: the contract, a snapshot of the
//! world's holes and learning at the start, each member's deck as they brought it, and the list of
//! actions taken (two bytes each). Nobody stores a `Run`: every session rebuilds the crew's runs by
//! replaying the log ([`Op::live`]), seeding each action's dice from the op and its position in
//! the log, so the replay is the same on every process. A move is accepted by the world only if
//! the log is as long as the mover last saw it and the engine accepts the action, so two members
//! acting at once cannot both be applied to stale state (the loser is told to look again).
//!
//! **What is shared** between the members' runs: the trace, the locks anyone opened, what anyone
//! scanned, the alarm, and one warden (it wakes at trace 55 or an alarm, arrives three moves
//! later at whoever moved, and once down is down for everyone). **What is not**: each member's
//! position, deck, charges, cycles and fights, so a member can be deep in a fight while another
//! walks on. Each member adds only a share of the noise (less each member there is), which is what
//! makes a crew worth the shared risk; a noisy member can still burn everybody.

use russh_bbs_door::DoorRng;

use crate::code::{self, Build};
use crate::content::{Content, Effect as ScriptEffect};
use crate::net::{Network, NodeId};
use crate::run::{
    Act, Refused, Run, RunResult, RunSetup, Status, TRACE_MAX, WARDEN_ARRIVES_IN, WARDEN_WAKES_AT,
};
use crate::social::{Lines, Social, same};
use crate::world::{Contract, World};

/// Members in a crew.
pub const CREW_MAX: usize = 5;
/// Crews on a board.
pub const CREWS_MAX: usize = 16;
/// Crew runs in the world at once.
pub const OPS_MAX: usize = 8;
/// Actions in one crew run.
pub const ACTIONS_MAX: usize = 500;
/// Days of silence after which a crew run goes cold.
pub const STALE_DAYS: i64 = 2;
/// Days a finished run is kept for its members to collect.
pub const KEEP_DAYS: i64 = 7;
/// The most cycles a member commits to a crew run (besides the jack-in cost).
pub const COMMIT_MAX: u32 = 32;
/// The longest crew name.
pub const NAME_MAX: usize = 20;
/// Pending invitations a crew may have out.
pub const INVITES_MAX: usize = 5;
/// The contract template number that marks the finale (never a real template).
pub const FINALE_TMPL: u8 = 250;
/// The finale's corporation.
pub const FINALE_CORP: u8 = 9;
/// Members needed to start the finale.
pub const FINALE_MEMBERS: usize = 3;
/// What the finale pays the crew, in marks, split like any crew job.
pub const FINALE_REWARD: u32 = 900;

/// Percent of the noise a member adds, by crew size: the shared meter is shared work.
pub const TRACE_SHARE: [u32; CREW_MAX] = [100, 50, 36, 28, 23];

/// Percent of the contract's pay a crew earns, by crew size.
pub const PAY_PCT: [u32; CREW_MAX] = [100, 170, 250, 330, 410];

/// A crew.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crew {
    /// Unique.
    pub id: u32,
    /// The name (a name the framework vetted).
    pub name: String,
    /// The leader's handle.
    pub leader: String,
    /// The members, leader first.
    pub members: Vec<String>,
    /// Handles invited and not yet answered.
    pub invited: Vec<String>,
    /// The day it was made.
    pub made: i64,
    /// Crew runs finished with the objective.
    pub jobs: u16,
}

impl Crew {
    /// Whether `handle` is a member.
    #[must_use]
    pub fn has(&self, handle: &str) -> bool {
        self.members.iter().any(|m| same(m, handle))
    }
}

/// Why a crew edit was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrewWhy {
    /// Already in a crew.
    InCrew,
    /// Not in a crew.
    NoCrew,
    /// The name is taken.
    NameTaken,
    /// The board has enough crews.
    Full,
    /// Only the leader can.
    NotLeader,
    /// The crew is full.
    CrewFull,
    /// Enough invitations are out.
    TooManyInvites,
    /// No such invitation.
    NoInvite,
    /// A crew run is in the way.
    Busy,
    /// No such run.
    NoOp,
    /// Not allowed in that state.
    State,
    /// Not in this run.
    NotMember,
    /// The finale was cracked already this season.
    FinaleDone,
    /// Too few members.
    TooFew,
    /// Another member moved first: look again.
    Stale,
    /// The engine would not allow it.
    Refused(Refused),
    /// The log is full.
    LogFull,
}

impl CrewWhy {
    /// The word for the player.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::InCrew => "you are in a crew already",
            Self::NoCrew => "you are not in a crew",
            Self::NameTaken => "a crew has that name",
            Self::Full => "the board has enough crews",
            Self::NotLeader => "only the leader can do that",
            Self::CrewFull => "the crew is full",
            Self::TooManyInvites => "enough invitations are out",
            Self::NoInvite => "there is no such invitation",
            Self::Busy => "a crew run is already open",
            Self::NoOp => "that run is gone",
            Self::State => "not now: the run is not in that state",
            Self::NotMember => "you are not in this run",
            Self::FinaleDone => "the finale was cracked this season",
            Self::TooFew => "the crew is too small for this",
            Self::Stale => "another member moved first: reopen to see the run",
            Self::Refused(r) => r.text(),
            Self::LogFull => "the run is spent: jack out",
        }
    }
}

/// What a member brought to a crew run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The handle.
    pub handle: String,
    /// The deck (script numbers; coded ones are 128 and up).
    pub deck: Vec<u8>,
    /// The builds behind the coded numbers in the deck.
    pub builds: Vec<(u8, Build)>,
    /// Skill levels in `Family::ALL` order.
    pub skill: [u8; 5],
    /// Shield level.
    pub shield: u8,
    /// Interface level.
    pub iface: u8,
    /// Cycles committed to the run (after the jack-in cost).
    pub cycles: u32,
    /// Trace absorbed up front (proxies).
    pub buffer: u32,
}

/// How a crew run stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpState {
    /// Members are joining.
    Lobby,
    /// Under way.
    Live,
    /// Over: members collect their share.
    Done,
}

impl OpState {
    /// The byte in the world.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Lobby => 0,
            Self::Live => 1,
            Self::Done => 2,
        }
    }

    /// From the byte.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::Lobby,
            1 => Self::Live,
            2 => Self::Done,
            _ => return None,
        })
    }
}

/// A crew run, as the world keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Op {
    /// Unique (chosen by the opener's session, checked for a clash).
    pub id: u32,
    /// The crew.
    pub crew: u32,
    /// The job.
    pub contract: Contract,
    /// The season (it shapes the network).
    pub season: u32,
    /// How it stands.
    pub state: OpState,
    /// The day it was opened.
    pub made: i64,
    /// The day of the last action (or of opening).
    pub last: i64,
    /// The day it ended (0 while it lasts).
    pub ended: i64,
    /// Holes in the network when it started.
    pub holes: Vec<NodeId>,
    /// Alert when it started.
    pub alert: u8,
    /// A backdoor when it started.
    pub backdoor: bool,
    /// What the corporation had learned when it started.
    pub learned: Vec<(u16, u32)>,
    /// The members, in joining order.
    pub members: Vec<Member>,
    /// The actions, packed ([`pack`]).
    pub log: Vec<u16>,
    /// Members who have collected their share.
    pub settled: Vec<String>,
}

impl Op {
    /// The index of a member by handle.
    #[must_use]
    pub fn index_of(&self, handle: &str) -> Option<usize> {
        self.members.iter().position(|m| same(&m.handle, handle))
    }

    /// Whether this is the season's finale.
    #[must_use]
    pub const fn is_finale(&self) -> bool {
        self.contract.tmpl == FINALE_TMPL
    }

    /// The network of this run.
    #[must_use]
    pub fn network(&self, content: &Content) -> Option<Network> {
        let corp = content.corp(self.contract.corp)?;
        if self.is_finale() {
            Network::generate_finale(content, self.season, corp)
        } else {
            Network::generate(content, self.season, corp, self.contract.tier)
        }
    }

    /// The crew's runs, rebuilt from the start by replaying the log. `None` if the network will
    /// not load. A logged action the engine now refuses (it never should: the world checked it)
    /// is skipped, so a damaged log cannot stop everyone from reading it.
    #[must_use]
    pub fn live(&self, content: &Content) -> Option<Live> {
        let mut live = Live::new(content, self)?;
        for (nth, packed) in self.log.iter().enumerate() {
            if *packed == COLD {
                live.go_cold();
                continue;
            }
            if let Some((idx, act)) = unpack(*packed) {
                let _ = live.apply(content, self, idx, act, nth);
            }
        }
        Some(live)
    }
}

/// The packed value for "the line went cold": everyone still in is out.
pub const COLD: u16 = 0xFFFF;

/// Pack a member's action into two bytes: member (3 bits), kind (3), slot (5), target (5).
#[must_use]
pub fn pack(idx: usize, act: Act) -> u16 {
    let who = u16::try_from(idx & 7).unwrap_or(0) << 13;
    let (kind, slot, target): (u16, u16, u16) = match act {
        Act::Move(n) => (0, 0, u16::from(n) & 31),
        Act::Use { slot, target } => (
            1,
            u16::try_from(slot & 31).unwrap_or(0),
            target.map_or(31, |n| u16::from(n) & 31),
        ),
        Act::JackOut => (2, 0, 0),
        Act::Brace => (3, 0, 0),
        Act::BreakOff => (4, 0, 0),
    };
    who | (kind << 10) | (slot << 5) | target
}

/// Unpack an action, or `None` for a value that is not one.
#[must_use]
pub fn unpack(v: u16) -> Option<(usize, Act)> {
    let idx = usize::from(v >> 13);
    let kind = (v >> 10) & 7;
    let slot = usize::from((v >> 5) & 31);
    let target = u8::try_from(v & 31).ok()?;
    let act = match kind {
        0 => Act::Move(target),
        1 => Act::Use {
            slot,
            target: (target != 31).then_some(target),
        },
        2 => Act::JackOut,
        3 => Act::Brace,
        4 => Act::BreakOff,
        _ => return None,
    };
    Some((idx, act))
}

#[derive(Debug, Clone)]
struct CrewWarden {
    cm: u8,
    awake: bool,
    arrives_in: u32,
    down: bool,
    owner: Option<usize>,
}

/// The crew's runs, rebuilt from an [`Op`].
#[derive(Debug, Clone)]
pub struct Live {
    /// One run per member, in the op's order.
    pub runs: Vec<Run>,
    /// The shared trace.
    pub trace: u32,
    /// Whether the objective is done.
    pub objective: bool,
    /// What the crew has said and seen, newest last.
    pub chatter: Vec<String>,
    warden: Option<CrewWarden>,
}

impl Live {
    fn new(content: &Content, op: &Op) -> Option<Self> {
        let net = op.network(content)?;
        let corp = content.corp(op.contract.corp)?;
        let (target, effect) = target_of(content, &op.contract)?;
        let n = op.members.len().clamp(1, CREW_MAX);
        let mut runs = Vec::with_capacity(op.members.len());
        for m in &op.members {
            let extra = m
                .builds
                .iter()
                .filter_map(|(num, build)| code::assemble(content, *build, *num).ok())
                .collect();
            let setup = RunSetup {
                net: net.clone(),
                holes: op.holes.clone(),
                alert: op.alert,
                backdoor: op.backdoor,
                target: target.clone(),
                effect,
                deck: m.deck.clone(),
                shield: m.shield,
                iface: m.iface,
                skill: m.skill,
                cycles: m.cycles,
                buffer: m.buffer,
                extra,
                learned: op.learned.clone(),
                warden: None,
                trace_share: TRACE_SHARE[n - 1],
            };
            runs.push(Run::new(content, setup)?);
        }
        let warden = warden_of(content, op, corp).map(|cm| CrewWarden {
            cm,
            awake: false,
            arrives_in: 0,
            down: false,
            owner: None,
        });
        Some(Self {
            runs,
            trace: 0,
            objective: false,
            chatter: Vec::new(),
            warden,
        })
    }

    /// Whether the crew run is over: nobody is left in the network.
    #[must_use]
    pub fn over(&self) -> bool {
        self.runs.iter().all(|r| r.status != Status::Active)
    }

    /// Whether the crew's warden woke, and whether it is down.
    #[must_use]
    pub fn warden_state(&self) -> Option<(bool, bool)> {
        self.warden.as_ref().map(|w| (w.awake, w.down))
    }

    fn say(&mut self, line: String) {
        self.chatter.push(line);
        if self.chatter.len() > 40 {
            self.chatter.remove(0);
        }
    }

    /// The line went cold: everyone still in is out.
    pub fn go_cold(&mut self) {
        for r in &mut self.runs {
            r.crew_end(false);
        }
        self.say("The line went cold. Everyone still in is out.".to_owned());
    }

    /// Take member `idx`'s action. Nothing changes on a refusal.
    ///
    /// # Errors
    /// Why the engine refused it.
    pub fn apply(
        &mut self,
        content: &Content,
        op: &Op,
        idx: usize,
        act: Act,
        nth: usize,
    ) -> Result<(), Refused> {
        let trace = self.trace;
        let Some(run) = self.runs.get_mut(idx) else {
            return Err(Refused::Over);
        };
        if run.status != Status::Active {
            return Err(Refused::Over);
        }
        let mut trial = run.clone();
        trial.crew_set_trace(trace);
        let before = trial.broken_nodes().len();
        let mut rng = DoorRng::from_label(
            "nullroute-crew",
            i64::from(op.id) * 1024 + i64::try_from(nth).unwrap_or(0),
        );
        trial.act(content, act, &mut rng)?;
        // Accepted: the member's run becomes the trial, and the crew takes what it shares.
        let opened: Vec<NodeId> = trial
            .broken_nodes()
            .get(before..)
            .unwrap_or_default()
            .to_vec();
        self.trace = trial.trace;
        self.runs[idx] = trial;
        let mine = self.runs[idx].clone();
        for (j, other) in self.runs.iter_mut().enumerate() {
            if j != idx {
                other.crew_open(&opened);
                other.crew_learn(&mine);
            }
        }
        if mine.objective_done() {
            self.objective = true;
        }
        self.warden_step(content, idx, matches!(act, Act::Move(_)));
        if self.trace >= TRACE_MAX {
            for r in &mut self.runs {
                r.crew_end(true);
            }
            self.say("The crew's trace is complete. Everyone is burned.".to_owned());
        } else {
            // A member with nothing left and no fight to finish is out.
            let r = &mut self.runs[idx];
            if r.status == Status::Active && r.cycles == 0 && r.encounter().is_none() {
                r.crew_end(false);
            }
        }
        Ok(())
    }

    fn warden_step(&mut self, content: &Content, idx: usize, moved: bool) {
        let alarm = self.runs.iter().any(Run::alarm);
        let trace = self.trace;
        let Some(w) = self.warden.as_mut() else {
            return;
        };
        if w.down {
            return;
        }
        if !w.awake {
            if trace >= WARDEN_WAKES_AT || alarm {
                w.awake = true;
                w.arrives_in = WARDEN_ARRIVES_IN;
                self.say(format!(
                    "A warden wakes in the core. {WARDEN_ARRIVES_IN} moves."
                ));
            }
            return;
        }
        if let Some(owner) = w.owner {
            if self.runs[owner].warden().is_some_and(|x| x.down) {
                w.down = true;
                self.say("The warden is down. The network is quiet.".to_owned());
                return;
            }
            if self.runs[owner].status != Status::Active {
                w.owner = None;
                w.arrives_in = WARDEN_ARRIVES_IN;
            }
            return;
        }
        if moved {
            w.arrives_in = w.arrives_in.saturating_sub(1);
            if w.arrives_in == 0 {
                w.owner = Some(idx);
                let cm = w.cm;
                self.runs[idx].crew_summon_warden(content, cm);
                let name = self.runs[idx].name_here();
                self.say(format!("The warden finds {name}."));
            }
        }
    }

    /// Each member's points: what they did toward the job, for the split.
    #[must_use]
    pub fn points(&self) -> Vec<u32> {
        self.runs
            .iter()
            .map(|r| {
                let res = r.result();
                u32::try_from(res.broken.len()).unwrap_or(0)
                    + 2 * res.defeated
                    + if r.objective_done() { 3 } else { 0 }
                    + if res.warden_down { 4 } else { 0 }
                    + r.known_mask().count_ones() / 4
            })
            .collect()
    }

    /// The pay split for a finished run: each member who got out with the objective done gets a
    /// share of the crew's pay in proportion to `points + 2`; burned members get nothing.
    #[must_use]
    pub fn shares(&self, total: u32) -> Vec<u32> {
        let points = self.points();
        let weights: Vec<u32> = self
            .runs
            .iter()
            .zip(&points)
            .map(|(r, p)| {
                if self.objective && r.status == Status::Out {
                    p + 2
                } else {
                    0
                }
            })
            .collect();
        let sum: u32 = weights.iter().sum();
        weights
            .iter()
            .map(|w| (total * w).checked_div(sum).unwrap_or(0))
            .collect()
    }

    /// Member `idx`'s result as a run: the crew's objective counts for everyone who got out.
    #[must_use]
    pub fn result_of(&self, idx: usize) -> Option<RunResult> {
        let r = self.runs.get(idx)?;
        let mut res = r.result();
        res.objective = self.objective && r.status == Status::Out;
        res.peak = res.peak.max(self.trace.min(TRACE_MAX));
        if self.warden.as_ref().is_some_and(|w| w.down) {
            res.warden_down = self.warden.as_ref().and_then(|w| w.owner) == Some(idx);
        }
        Some(res)
    }

    /// One result for the world: the whole crew's mark on the network.
    #[must_use]
    pub fn merged(&self) -> RunResult {
        let mut out = self.result_of(0).unwrap_or_else(|| self.runs[0].result());
        for i in 1..self.runs.len() {
            if let Some(r) = self.result_of(i) {
                for h in r.broken {
                    if !out.broken.contains(&h) {
                        out.broken.push(h);
                    }
                }
                for s in r.used {
                    if !out.used.contains(&s) {
                        out.used.push(s);
                    }
                }
                out.planted |= r.planted;
                out.objective |= r.objective;
                out.defeated += r.defeated;
                out.warden_down |= r.warden_down;
                out.warden_woke |= r.warden_woke;
                out.fights += r.fights;
            }
        }
        out.status = if self.trace >= TRACE_MAX {
            Status::Burned
        } else {
            Status::Out
        };
        out
    }
}

fn target_of(content: &Content, contract: &Contract) -> Option<(String, ScriptEffect)> {
    if contract.tmpl == FINALE_TMPL {
        return Some(("AC".to_owned(), ScriptEffect::Plant));
    }
    let t = content.contract(contract.tmpl)?;
    Some((t.target.clone(), t.effect))
}

/// The warden a crew run faces: always at tier 3, in one of three at tier 2, decided by the run.
fn warden_of(content: &Content, op: &Op, corp: &crate::content::CorpDef) -> Option<u8> {
    let chance = match op.contract.tier {
        3 => 100,
        2 => 35,
        _ => return None,
    };
    let mut rng = DoorRng::from_label("nullroute-crew-warden", i64::from(op.id));
    if !rng.chance(chance, 100) {
        return None;
    }
    crate::cm::warden_of(content, corp).map(|c| c.num)
}

/// The job a contract stands for, for the screens: its title, verb, target and effect.
#[must_use]
pub fn job_of(
    content: &Content,
    contract: &Contract,
) -> Option<(String, String, String, ScriptEffect)> {
    if contract.tmpl == FINALE_TMPL {
        return Some((
            "The Null Vault".to_owned(),
            "Heist".to_owned(),
            "AC".to_owned(),
            ScriptEffect::Plant,
        ));
    }
    let t = content.contract(contract.tmpl)?;
    Some((t.title.clone(), t.verb.clone(), t.target.clone(), t.effect))
}

impl Social {
    /// The crew a player is in.
    #[must_use]
    pub fn crew_of(&self, handle: &str) -> Option<&Crew> {
        self.crews.iter().find(|c| c.has(handle))
    }

    /// Crews that have invited `handle`.
    #[must_use]
    pub fn invites_for(&self, handle: &str) -> Vec<&Crew> {
        self.crews
            .iter()
            .filter(|c| c.invited.iter().any(|i| same(i, handle)))
            .collect()
    }

    /// Found a crew.
    ///
    /// # Errors
    /// Why not.
    pub fn crew_create(&mut self, name: &str, leader: &str, today: i64) -> Result<(), CrewWhy> {
        if self.crew_of(leader).is_some() {
            return Err(CrewWhy::InCrew);
        }
        if self.crews.iter().any(|c| same(&c.name, name)) {
            return Err(CrewWhy::NameTaken);
        }
        if self.crews.len() >= CREWS_MAX {
            return Err(CrewWhy::Full);
        }
        let id = self.id();
        self.crews.push(Crew {
            id,
            name: name.chars().take(NAME_MAX).collect(),
            leader: leader.to_owned(),
            members: vec![leader.to_owned()],
            invited: Vec::new(),
            made: today,
            jobs: 0,
        });
        Ok(())
    }

    /// The leader invites someone who has not turned crews off. The invitee gets a template
    /// line with the leader's handle; there is no free text.
    ///
    /// # Errors
    /// Why not.
    pub fn crew_invite(&mut self, by: &str, who: &str, today: i64) -> Result<(), CrewWhy> {
        let crew = self
            .crews
            .iter()
            .find(|c| c.has(by))
            .ok_or(CrewWhy::NoCrew)?
            .clone();
        if !same(&crew.leader, by) {
            return Err(CrewWhy::NotLeader);
        }
        if crew.members.len() >= CREW_MAX {
            return Err(CrewWhy::CrewFull);
        }
        if crew.invited.len() >= INVITES_MAX {
            return Err(CrewWhy::TooManyInvites);
        }
        if !self.posted_of(who).is_some_and(|p| p.crews_open) {
            return Err(CrewWhy::NoInvite);
        }
        if let Some(c) = self.crews.iter_mut().find(|c| c.id == crew.id)
            && !c.invited.iter().any(|i| same(i, who))
        {
            c.invited.push(who.to_owned());
        }
        self.deliver(
            who,
            today,
            crate::social::Effect::None,
            0,
            by,
            Lines::CREW_INVITED,
            0,
        );
        Ok(())
    }

    /// Accept an invitation (by crew id).
    ///
    /// # Errors
    /// Why not.
    pub fn crew_accept(&mut self, who: &str, crew: u32) -> Result<(), CrewWhy> {
        if self.crew_of(who).is_some() {
            return Err(CrewWhy::InCrew);
        }
        let c = self
            .crews
            .iter_mut()
            .find(|c| c.id == crew && c.invited.iter().any(|i| same(i, who)))
            .ok_or(CrewWhy::NoInvite)?;
        if c.members.len() >= CREW_MAX {
            return Err(CrewWhy::CrewFull);
        }
        c.invited.retain(|i| !same(i, who));
        c.members.push(who.to_owned());
        Ok(())
    }

    /// Leave a crew (the leader's leaving passes the crew on, or ends it when alone). Refused
    /// while the crew has a run open, so a run cannot be abandoned from underneath.
    ///
    /// # Errors
    /// Why not.
    pub fn crew_leave(&mut self, who: &str) -> Result<(), CrewWhy> {
        let id = self.crew_of(who).ok_or(CrewWhy::NoCrew)?.id;
        if self
            .ops
            .iter()
            .any(|o| o.crew == id && o.state != OpState::Done)
        {
            return Err(CrewWhy::Busy);
        }
        if let Some(c) = self.crews.iter_mut().find(|c| c.id == id) {
            c.members.retain(|m| !same(m, who));
            if c.members.is_empty() {
                self.crews.retain(|c| c.id != id);
            } else if same(&c.leader, who) {
                c.leader = c.members[0].clone();
            }
        }
        Ok(())
    }

    /// The leader removes a member.
    ///
    /// # Errors
    /// Why not.
    pub fn crew_kick(&mut self, by: &str, who: &str) -> Result<(), CrewWhy> {
        let crew = self.crew_of(by).ok_or(CrewWhy::NoCrew)?.clone();
        if !same(&crew.leader, by) {
            return Err(CrewWhy::NotLeader);
        }
        if same(by, who) || !crew.has(who) {
            return Err(CrewWhy::NotMember);
        }
        if self
            .ops
            .iter()
            .any(|o| o.crew == crew.id && o.state != OpState::Done)
        {
            return Err(CrewWhy::Busy);
        }
        if let Some(c) = self.crews.iter_mut().find(|c| c.id == crew.id) {
            c.members.retain(|m| !same(m, who));
        }
        Ok(())
    }
}

impl World {
    /// Open a crew run from the leader's side: the opener is its first member.
    ///
    /// # Errors
    /// Why not.
    pub fn op_open(
        &mut self,
        id: u32,
        contract: Contract,
        opener: Member,
        today: i64,
    ) -> Result<(), CrewWhy> {
        let crew = self
            .social
            .crew_of(&opener.handle)
            .ok_or(CrewWhy::NoCrew)?
            .clone();
        if self.social.ops.iter().any(|o| o.id == id) {
            return Err(CrewWhy::Busy);
        }
        if self
            .social
            .ops
            .iter()
            .any(|o| o.crew == crew.id && o.state != OpState::Done)
        {
            return Err(CrewWhy::Busy);
        }
        if self.social.ops.len() >= OPS_MAX {
            return Err(CrewWhy::Full);
        }
        if contract.tmpl == FINALE_TMPL && self.social.finale.is_some() {
            return Err(CrewWhy::FinaleDone);
        }
        self.social.ops.push(Op {
            id,
            crew: crew.id,
            contract,
            season: self.season,
            state: OpState::Lobby,
            made: today,
            last: today,
            ended: 0,
            holes: Vec::new(),
            alert: 0,
            backdoor: false,
            learned: Vec::new(),
            members: vec![opener],
            log: Vec::new(),
            settled: Vec::new(),
        });
        Ok(())
    }

    /// A crew member joins a run that has not started.
    ///
    /// # Errors
    /// Why not.
    pub fn op_join(&mut self, id: u32, member: Member) -> Result<(), CrewWhy> {
        let crew = self
            .social
            .crew_of(&member.handle)
            .ok_or(CrewWhy::NoCrew)?
            .id;
        let busy = self.social.ops.iter().any(|o| {
            o.id != id && o.state != OpState::Done && o.index_of(&member.handle).is_some()
        });
        let op = self
            .social
            .ops
            .iter_mut()
            .find(|o| o.id == id)
            .ok_or(CrewWhy::NoOp)?;
        if op.crew != crew {
            return Err(CrewWhy::NotMember);
        }
        if op.state != OpState::Lobby {
            return Err(CrewWhy::State);
        }
        if op.index_of(&member.handle).is_some() || busy {
            return Err(CrewWhy::Busy);
        }
        if op.members.len() >= CREW_MAX {
            return Err(CrewWhy::CrewFull);
        }
        op.members.push(member);
        Ok(())
    }

    /// The leader starts the run: the world's holes, alert and learning are copied into it.
    ///
    /// # Errors
    /// Why not.
    pub fn op_start(&mut self, id: u32, by: &str, today: i64) -> Result<(), CrewWhy> {
        let (corp, tier, crew) = {
            let op = self
                .social
                .ops
                .iter()
                .find(|o| o.id == id)
                .ok_or(CrewWhy::NoOp)?;
            (op.contract.corp, op.contract.tier, op.crew)
        };
        let leader_ok = self
            .social
            .crews
            .iter()
            .any(|c| c.id == crew && same(&c.leader, by));
        if !leader_ok {
            return Err(CrewWhy::NotLeader);
        }
        let state = self.net(corp, tier).cloned();
        let learned = self.learned_by(corp);
        let op = self
            .social
            .ops
            .iter_mut()
            .find(|o| o.id == id)
            .ok_or(CrewWhy::NoOp)?;
        if op.state != OpState::Lobby {
            return Err(CrewWhy::State);
        }
        if op.is_finale() && op.members.len() < FINALE_MEMBERS {
            return Err(CrewWhy::TooFew);
        }
        op.state = OpState::Live;
        op.last = today;
        op.holes = state.as_ref().map(|s| s.holes.clone()).unwrap_or_default();
        op.alert = state.as_ref().map_or(0, |s| s.alert);
        op.backdoor = state.as_ref().is_some_and(|s| s.backdoor);
        op.learned = learned;
        Ok(())
    }

    /// Take a member's action in a live run. The action is appended only if the log is as long as
    /// the member last saw it (`expect`) and the engine accepts it; when it ends the run, the
    /// world takes the crew's mark on the network.
    ///
    /// # Errors
    /// Why not; the world is unchanged then.
    pub fn op_act(
        &mut self,
        content: &Content,
        id: u32,
        who: &str,
        act: Act,
        expect: usize,
        today: i64,
    ) -> Result<(), CrewWhy> {
        let at = self
            .social
            .ops
            .iter()
            .position(|o| o.id == id)
            .ok_or(CrewWhy::NoOp)?;
        let op = &self.social.ops[at];
        if op.state != OpState::Live {
            return Err(CrewWhy::State);
        }
        let idx = op.index_of(who).ok_or(CrewWhy::NotMember)?;
        if op.log.len() != expect {
            return Err(CrewWhy::Stale);
        }
        if op.log.len() >= ACTIONS_MAX {
            return Err(CrewWhy::LogFull);
        }
        let mut live = op.live(content).ok_or(CrewWhy::NoOp)?;
        let nth = op.log.len();
        let opc = op.clone();
        live.apply(content, &opc, idx, act, nth)
            .map_err(CrewWhy::Refused)?;
        let over = live.over();
        let op = &mut self.social.ops[at];
        op.log.push(pack(idx, act));
        op.last = today;
        if over {
            op.state = OpState::Done;
            op.ended = today;
            self.finish_op(content, at, &live, today);
        }
        Ok(())
    }

    /// The crew's run is over: the network remembers it, the board counts it and the street hears.
    fn finish_op(&mut self, content: &Content, at: usize, live: &Live, today: i64) {
        let op = self.social.ops[at].clone();
        let merged = live.merged();
        let crew_name = self
            .social
            .crews
            .iter()
            .find(|c| c.id == op.crew)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        if !op.is_finale() {
            self.apply_run(op.contract.corp, op.contract.tier, &merged);
        }
        let t = &mut self.social.tallies;
        if merged.objective {
            t.crew_jobs += 1;
            t.jobs += 1;
            if op.contract.tier == 3 {
                t.deep_breaches += 1;
            }
        }
        if merged.status == Status::Burned {
            t.burns += 1;
        }
        if merged.warden_down {
            t.wardens += 1;
            self.social.deed(today, Lines::DEED_WARDEN, &crew_name, 1);
        }
        if merged.objective {
            if let Some(c) = self.social.crews.iter_mut().find(|c| c.id == op.crew) {
                c.jobs = c.jobs.saturating_add(1);
            }
            if op.is_finale() {
                self.social.tallies.finale = 1;
                self.social.finale = Some(crew_name.clone());
                self.social.deed(today, Lines::DEED_FINALE, &crew_name, 0);
            } else {
                self.social.deed(
                    today,
                    Lines::DEED_CREW,
                    &crew_name,
                    u32::from(op.contract.tier),
                );
            }
        }
        let _ = content;
    }

    /// The member collected their share: note it, and drop the run once all have.
    pub fn op_settled(&mut self, id: u32, who: &str) {
        if let Some(op) = self.social.ops.iter_mut().find(|o| o.id == id)
            && !op.settled.iter().any(|s| same(s, who))
        {
            op.settled.push(who.to_owned());
        }
        self.social
            .ops
            .retain(|o| !(o.state == OpState::Done && o.settled.len() >= o.members.len()));
    }

    /// The night: runs nobody has touched for [`STALE_DAYS`] go cold (a live run is ended with
    /// everyone out; a lobby is dropped), and finished runs are dropped after [`KEEP_DAYS`].
    pub fn ops_night(&mut self, content: &Content, day: i64) {
        let mut done = Vec::new();
        for (at, op) in self.social.ops.iter_mut().enumerate() {
            if op.state == OpState::Live && day - op.last >= STALE_DAYS {
                op.log.push(COLD);
                op.state = OpState::Done;
                op.ended = day;
                done.push(at);
            }
        }
        for at in done {
            let op = self.social.ops[at].clone();
            if let Some(live) = op.live(content) {
                self.finish_op(content, at, &live, day);
            }
        }
        self.social.ops.retain(|o| match o.state {
            OpState::Lobby => day - o.made < STALE_DAYS,
            OpState::Live => true,
            OpState::Done => day - o.ended < KEEP_DAYS,
        });
    }
}
