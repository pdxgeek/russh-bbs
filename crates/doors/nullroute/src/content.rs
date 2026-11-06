//! Content as data: corporations, node types, scripts, contract templates and the rig, as TOML
//! tables embedded in the crate and checked by a schema test (`tests/content.rs`).
//!
//! Every table has stable numeric ids (`num`) because saves and the shared world store the number,
//! not the name: add rows, never renumber.

use russh_bbs_door::content::{self, Table, plain_text};
use serde::Deserialize;

/// Corporations and fixers.
pub const CORPS_TOML: &str = include_str!("../content/corps.toml");
/// Node types.
pub const NODES_TOML: &str = include_str!("../content/nodes.toml");
/// Scripts.
pub const SCRIPTS_TOML: &str = include_str!("../content/scripts.toml");
/// Contract templates.
pub const CONTRACTS_TOML: &str = include_str!("../content/contracts.toml");
/// Rig parts, the hideout and the fence.
pub const RIG_TOML: &str = include_str!("../content/rig.toml");
/// Countermeasures and wardens (M4-N2).
pub const COUNTERMEASURES_TOML: &str = include_str!("../content/countermeasures.toml");
/// Fragments for the module editor (M4-N2).
pub const FRAGMENTS_TOML: &str = include_str!("../content/fragments.toml");

/// The three ways a node can be locked, and the three break effects that open them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lock {
    /// A password: opened by a `crack` script.
    Crack,
    /// A badge or certificate: opened by a `spoof` script.
    Spoof,
    /// A buffer to overrun: opened by an `overflow` script.
    Overflow,
}

impl Lock {
    /// All three, in a fixed order.
    pub const ALL: [Self; 3] = [Self::Crack, Self::Spoof, Self::Overflow];

    /// A short word for the screen.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Crack => "password",
            Self::Spoof => "badge",
            Self::Overflow => "buffer",
        }
    }

    /// The index in [`Lock::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Crack => 0,
            Self::Spoof => 1,
            Self::Overflow => 2,
        }
    }
}

/// A script's family. Each family has its own skill that grows with use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    /// Scan, map, reveal.
    Probe,
    /// Crack, spoof, overflow.
    Break,
    /// Mask, decoy, reroute.
    Hide,
    /// Jam, fork, purge.
    Fight,
    /// Copy, plant, shred, cut a feed, scrub logs.
    Utility,
}

impl Family {
    /// All five, in a fixed order (the order of the skill array in a save).
    pub const ALL: [Self; 5] = [
        Self::Probe,
        Self::Break,
        Self::Hide,
        Self::Fight,
        Self::Utility,
    ];

    /// The index in [`Family::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Probe => 0,
            Self::Break => 1,
            Self::Hide => 2,
            Self::Fight => 3,
            Self::Utility => 4,
        }
    }

    /// The name on the screen.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Probe => "Probe",
            Self::Break => "Break",
            Self::Hide => "Hide",
            Self::Fight => "Fight",
            Self::Utility => "Utility",
        }
    }
}

/// What a script does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    /// Reveal the neighbours of the current node (and hidden links up to its power).
    Scan,
    /// Reveal the whole floor plan (and hidden links up to its power).
    Map,
    /// Open a password lock next door.
    Crack,
    /// Open a badge lock next door.
    Spoof,
    /// Open a buffer lock next door.
    Overflow,
    /// Halve the noise of the next actions.
    Mask,
    /// Absorb trace.
    Decoy,
    /// Shed trace and end a tracer.
    Reroute,
    /// Switch off a sentry or tracer next door.
    Jam,
    /// Absorb the next patrol hits.
    Fork,
    /// Cut through a black wall next door.
    Purge,
    /// Copy data out of a vault.
    Copy,
    /// Plant a backdoor.
    Plant,
    /// Shred a record.
    Shred,
    /// Cut a camera feed.
    Cutfeed,
    /// Scrub the logs.
    Wipelogs,
}

impl Effect {
    /// The break effect's lock, if it is one.
    #[must_use]
    pub const fn lock(self) -> Option<Lock> {
        match self {
            Self::Crack => Some(Lock::Crack),
            Self::Spoof => Some(Lock::Spoof),
            Self::Overflow => Some(Lock::Overflow),
            _ => None,
        }
    }

    /// Whether the script must be aimed at a neighbouring node.
    #[must_use]
    pub const fn needs_target(self) -> bool {
        matches!(
            self,
            Self::Crack | Self::Spoof | Self::Overflow | Self::Jam | Self::Purge
        )
    }
}

/// A fixer: who posts contracts, and whose clients you may be hitting.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixerDef {
    /// Stable id.
    pub num: u8,
    /// Name.
    pub name: String,
    /// One line.
    pub blurb: String,
}

/// A corporation and its style. Percent fields are 0..=100 unless noted.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpDef {
    /// Stable id.
    pub num: u8,
    /// Name.
    pub name: String,
    /// One line of street intel.
    pub style: String,
    /// The fixer who counts this corporation as a client.
    pub patron: u8,
    /// Trace multiplier in percent (100 is normal).
    pub trace_pct: u32,
    /// Chance a lockable node is locked.
    pub lock_pct: u32,
    /// Added to every security level (may be negative).
    pub sec_bonus: i32,
    /// Chance a mid node is patrolled.
    pub patrol_pct: u32,
    /// Of the patrols, how many are tracers (the rest are sentries).
    pub tracer_share: u32,
    /// Chance the objective floor has a decoy vault.
    pub honey_pct: u32,
    /// Chance an extra link is hidden.
    pub hidden_pct: u32,
    /// Trace added by every action anywhere: a human is watching.
    pub watch: u32,
    /// Chance each night that an open hole is patched.
    pub patch_pct: u32,
    /// Chance (tier 2 and up) a deep mid node is a black wall.
    pub blackwall_pct: u32,
    /// The lock family the corporation leans on.
    pub favoured: Lock,
    /// Countermeasure tags the corporation favours: their weight triples when a patrol is picked.
    #[serde(default)]
    pub cm_bias: Vec<String>,
    /// The warden (a countermeasure number) that hunts runs on its tier-3 networks.
    #[serde(default)]
    pub warden: u8,
}

/// A node type's role in a network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Where every run starts.
    Entry,
    /// A node on the way.
    Mid,
    /// A node a contract can aim at.
    Objective,
}

/// A node type.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDef {
    /// Stable id.
    pub num: u8,
    /// Two letters shown in the node's name.
    pub code: String,
    /// Name.
    pub name: String,
    /// Role.
    pub role: Role,
    /// Weight when picking a mid node.
    pub weight: u32,
    /// Base security.
    pub sec: i32,
    /// Chance of a lock.
    pub lock_pct: u32,
    /// Lock family weights `[crack, spoof, overflow]`.
    pub bias: [u32; 3],
    /// Trace added on entering.
    pub noise: u32,
    /// A live camera feed.
    pub watches: bool,
    /// The one corporation whose networks use this node type (its home template); `None` is a
    /// type every corporation uses. A corporation's own types are added to its pool only, so the
    /// networks of the other corporations are unchanged by them.
    #[serde(default)]
    pub corp: Option<u8>,
}

/// A script.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptDef {
    /// Stable id.
    pub num: u8,
    /// Name.
    pub name: String,
    /// Family.
    pub family: Family,
    /// Effect.
    pub effect: Effect,
    /// Power.
    pub power: u32,
    /// Deck memory.
    pub memory: u32,
    /// Trace added.
    pub noise: u32,
    /// Cycles to run it.
    pub cycles: u32,
    /// Uses per run (0 is unlimited).
    pub charges: u32,
    /// Shop price (0: not for sale).
    pub price: u32,
    /// Reputation needed to buy.
    pub min_rep: u32,
    /// Family skill needed to load.
    pub min_skill: u32,
    /// Owned from the first visit.
    pub starter: bool,
    /// One line.
    pub blurb: String,
    /// The kind of countermeasure the script is tuned to (+3 power against it in an encounter).
    #[serde(default)]
    pub target: Option<CmKind>,
    /// Armor ignored in an encounter.
    #[serde(default)]
    pub pierce: u32,
    /// A corporation learns it half as fast.
    #[serde(default)]
    pub slow: bool,
    /// The identity a corporation learns (set on load; not in the table).
    #[serde(skip)]
    pub sig: u16,
}

/// What a countermeasure is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CmKind {
    /// Watches a node and hits when you stay.
    Sentry,
    /// Follows your trace.
    Tracer,
    /// A black wall: closes a node until it is breached.
    Wall,
    /// A corporate AI that hunts you from node to node.
    Warden,
}

impl CmKind {
    /// All four, in a fixed order.
    pub const ALL: [Self; 4] = [Self::Sentry, Self::Tracer, Self::Wall, Self::Warden];

    /// The name on the screen.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Sentry => "sentry",
            Self::Tracer => "tracer",
            Self::Wall => "black wall",
            Self::Warden => "warden",
        }
    }
}

/// What a countermeasure does on its beat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Intent {
    /// A light touch of trace.
    Probe,
    /// The real hit.
    Strike,
    /// The next strike is doubled.
    Charge,
    /// Armor +3 against your move this beat.
    Guard,
    /// The family you used last cannot be used next beat.
    Seal,
    /// Starts a tracer and adds trace.
    Trace,
    /// Heals.
    Mend,
    /// A break script you play is echoed back as trace.
    Counter,
}

impl Intent {
    /// The word shown above the fight.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Probe => "PROBE",
            Self::Strike => "STRIKE",
            Self::Charge => "CHARGE",
            Self::Guard => "GUARD",
            Self::Seal => "SEAL",
            Self::Trace => "TRACE",
            Self::Mend => "MEND",
            Self::Counter => "COUNTER",
        }
    }

    /// What it means, in a few words.
    #[must_use]
    pub const fn gloss(self) -> &'static str {
        match self {
            Self::Probe => "light touch",
            Self::Strike => "hits hard",
            Self::Charge => "next strike x2: jam it",
            Self::Guard => "armor up: do not attack",
            Self::Seal => "seals your last family",
            Self::Trace => "starts a tracer",
            Self::Mend => "heals itself",
            Self::Counter => "echoes break scripts",
        }
    }
}

/// A countermeasure: an enemy with a pattern to learn. Wardens are the `Warden` kind.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CmDef {
    /// Stable id.
    pub num: u8,
    /// Name.
    pub name: String,
    /// Kind.
    pub kind: CmKind,
    /// Lowest tier it appears in.
    pub tier_min: u8,
    /// Highest tier it appears in.
    pub tier_max: u8,
    /// Base integrity (the node's security adds 1 a level).
    pub integrity: u32,
    /// Base armor (damage taken falls by this).
    pub armor: u32,
    /// Trace of a strike, before the corporation's multipliers.
    pub strike: u32,
    /// The lock family whose break scripts hurt it +2.
    pub weak: Lock,
    /// The cycle of intents it repeats.
    pub pattern: Vec<Intent>,
    /// Weight when picking a patrol.
    pub weight: u32,
    /// Tags, matched with a corporation's `cm_bias`.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Integrity a mend heals.
    #[serde(default)]
    pub mend: u32,
    /// Percent chance a defeated one leaves a fragment.
    pub drop: u32,
    /// One line.
    pub blurb: String,
}

/// What a fragment contributes to a coded script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FragKind {
    /// The effect.
    Effect,
    /// The target type.
    Target,
    /// A modifier.
    Modifier,
}

/// A fragment of code, found on runs and combined at the workbench.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FragmentDef {
    /// Stable id (1 to 31: it is packed into a script's identity).
    pub num: u8,
    /// Name.
    pub name: String,
    /// Which slot of the editor it fills.
    pub kind: FragKind,
    /// The effect, for an effect fragment.
    #[serde(default)]
    pub effect: Option<Effect>,
    /// The target type, for a target fragment.
    #[serde(default)]
    pub target: Option<CmKind>,
    /// The modifier's name, for a modifier fragment.
    #[serde(default)]
    pub modifier: Option<String>,
    /// Added to the build's power.
    #[serde(default)]
    pub power: i32,
    /// Added to the build's memory.
    #[serde(default)]
    pub memory: i32,
    /// Added to the build's noise.
    #[serde(default)]
    pub noise: i32,
    /// Added to the build's cycles.
    #[serde(default)]
    pub cycles: i32,
    /// Armor the build ignores.
    #[serde(default)]
    pub pierce: u32,
    /// Limits the build to this many uses a run (0 leaves it as the effect has it).
    #[serde(default)]
    pub charges: u32,
    /// A corporation learns the build half as fast.
    #[serde(default)]
    pub slow: bool,
    /// Lowest tier of network it can be found in.
    pub rank: u8,
    /// Drop weight.
    pub weight: u32,
    /// One line.
    pub blurb: String,
}

/// A contract template.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractDef {
    /// Stable id.
    pub num: u8,
    /// Title.
    pub title: String,
    /// The verb (Steal, Wipe, Plant, Ransom).
    pub verb: String,
    /// Objective node code.
    pub target: String,
    /// Effect needed on it.
    pub effect: Effect,
    /// Tier 1 to 3.
    pub tier: u8,
    /// Reward range in marks.
    pub reward_min: u32,
    /// Reward range in marks.
    pub reward_max: u32,
    /// Heat the job adds.
    pub heat: u32,
}

/// One upgrade level of a rig part.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelDef {
    /// Name of the part at this level.
    pub name: String,
    /// Cost to reach it.
    pub cost: u32,
}

/// A rig part.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartDef {
    /// Stable id (the order: cpu, memory, cooling, interface, shield).
    pub num: u8,
    /// Name.
    pub name: String,
    /// Levels 1 to 3.
    pub levels: Vec<LevelDef>,
}

/// Hideout prices and numbers.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HideoutDef {
    /// Price of a bot.
    pub bot_cost: u32,
    /// Marks a bot earns a night.
    pub bot_income: u32,
    /// Most bots.
    pub bot_max: u8,
    /// Price of the workbench.
    pub workbench_cost: u32,
    /// Price of a proxy at heat below the surcharge line.
    pub proxy_cost: u32,
    /// Trace one proxy absorbs.
    pub proxy_buffer: u32,
    /// Most proxies a run.
    pub proxy_max: u8,
    /// Marks moved by one stash step.
    pub stash_step: u32,
    /// Repair cost as a percent of the part's upgrade cost.
    pub repair_percent: u32,
    /// Cycles to lay low.
    pub lay_low_cycles: u32,
    /// Heat shed by laying low.
    pub lay_low_heat: u32,
    /// Heat shed each night.
    pub heat_decay: u32,
}

/// A fence level.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FenceDef {
    /// Name.
    pub name: String,
    /// Percent of a loot's value paid.
    pub rate: u32,
    /// Cost to reach it.
    pub cost: u32,
}

/// `corps.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpsTable {
    /// Fixers.
    pub fixer: Vec<FixerDef>,
    /// Corporations.
    pub corp: Vec<CorpDef>,
}

/// `nodes.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodesTable {
    /// Node types.
    pub node: Vec<NodeDef>,
}

/// `scripts.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptsTable {
    /// Scripts.
    pub script: Vec<ScriptDef>,
}

/// `contracts.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractsTable {
    /// Templates.
    pub contract: Vec<ContractDef>,
}

/// `countermeasures.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CmTable {
    /// Countermeasures and wardens.
    pub cm: Vec<CmDef>,
}

/// `fragments.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FragmentsTable {
    /// Fragments.
    pub fragment: Vec<FragmentDef>,
}

/// `rig.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigTable {
    /// Parts.
    pub part: Vec<PartDef>,
    /// Hideout.
    pub hideout: HideoutDef,
    /// Fence levels.
    pub fence: Vec<FenceDef>,
}

fn id_problems<'a>(
    table: &str,
    nums: impl Iterator<Item = u8>,
    names: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    let mut problems = Vec::new();
    let nums: Vec<u8> = nums.collect();
    for (n, num) in nums.iter().enumerate() {
        if nums[..n].contains(num) {
            problems.push(format!("[{table}] num {num} is used twice"));
        }
    }
    for (n, name) in names.enumerate() {
        if name.trim().is_empty() || name.chars().count() > 40 {
            problems.push(format!("[{table}] #{n}: name must be 1 to 40 characters"));
        }
        if !plain_text(name) {
            problems.push(format!(
                "[{table}] #{n}: name has characters the board strips"
            ));
        }
    }
    problems
}

impl Table for CorpsTable {
    fn validate(&self) -> Vec<String> {
        let mut problems = id_problems(
            "fixer",
            self.fixer.iter().map(|f| f.num),
            self.fixer.iter().map(|f| f.name.as_str()),
        );
        problems.extend(id_problems(
            "corp",
            self.corp.iter().map(|c| c.num),
            self.corp.iter().map(|c| c.name.as_str()),
        ));
        if self.fixer.is_empty() || self.corp.is_empty() {
            problems.push("needs at least one fixer and one corp".into());
        }
        for fixer in &self.fixer {
            if !plain_text(&fixer.blurb) || fixer.blurb.chars().count() > 80 {
                problems.push(format!(
                    "fixer `{}`: blurb must be plain and at most 80 characters",
                    fixer.name
                ));
            }
        }
        for corp in &self.corp {
            if !self.fixer.iter().any(|f| f.num == corp.patron) {
                problems.push(format!(
                    "corp `{}`: patron {} is not a fixer",
                    corp.name, corp.patron
                ));
            }
            if !plain_text(&corp.style) || corp.style.chars().count() > 80 {
                problems.push(format!(
                    "corp `{}`: style must be plain and at most 80 characters",
                    corp.name
                ));
            }
            for (what, value) in [
                ("lock_pct", corp.lock_pct),
                ("patrol_pct", corp.patrol_pct),
                ("tracer_share", corp.tracer_share),
                ("honey_pct", corp.honey_pct),
                ("hidden_pct", corp.hidden_pct),
                ("patch_pct", corp.patch_pct),
                ("blackwall_pct", corp.blackwall_pct),
            ] {
                if value > 100 {
                    problems.push(format!("corp `{}`: {what} is over 100", corp.name));
                }
            }
            if !(50..=300).contains(&corp.trace_pct) {
                problems.push(format!("corp `{}`: trace_pct must be 50 to 300", corp.name));
            }
            if !(-3..=3).contains(&corp.sec_bonus) || corp.watch > 5 {
                problems.push(format!(
                    "corp `{}`: sec_bonus must be -3 to 3 and watch at most 5",
                    corp.name
                ));
            }
        }
        problems
    }
}

impl Table for NodesTable {
    fn validate(&self) -> Vec<String> {
        let mut problems = id_problems(
            "node",
            self.node.iter().map(|n| n.num),
            self.node.iter().map(|n| n.name.as_str()),
        );
        for role in [Role::Entry, Role::Mid, Role::Objective] {
            if !self.node.iter().any(|n| n.role == role) {
                problems.push(format!("no node has the role {role:?}"));
            }
        }
        if !self
            .node
            .iter()
            .any(|n| n.role == Role::Mid && n.weight > 0)
        {
            problems.push("no mid node has a weight".into());
        }
        for node in &self.node {
            if node.code.len() != 2 || !node.code.bytes().all(|b| b.is_ascii_uppercase()) {
                problems.push(format!(
                    "node `{}`: code must be two capital letters",
                    node.name
                ));
            }
            if self.node.iter().filter(|n| n.code == node.code).count() > 1 {
                problems.push(format!("node code `{}` is used twice", node.code));
            }
            if node.lock_pct > 100 || !(0..=6).contains(&node.sec) {
                problems.push(format!(
                    "node `{}`: lock_pct must be 0 to 100 and sec 0 to 6",
                    node.name
                ));
            }
            if node.lock_pct > 0 && node.bias.iter().sum::<u32>() == 0 {
                problems.push(format!(
                    "node `{}`: a lockable node needs a lock bias",
                    node.name
                ));
            }
        }
        problems
    }
}

impl Table for ScriptsTable {
    fn validate(&self) -> Vec<String> {
        let mut problems = id_problems(
            "script",
            self.script.iter().map(|s| s.num),
            self.script.iter().map(|s| s.name.as_str()),
        );
        for s in &self.script {
            let family_ok = match s.effect {
                Effect::Scan | Effect::Map => s.family == Family::Probe,
                Effect::Crack | Effect::Spoof | Effect::Overflow => s.family == Family::Break,
                Effect::Mask | Effect::Decoy | Effect::Reroute => s.family == Family::Hide,
                Effect::Jam | Effect::Fork | Effect::Purge => s.family == Family::Fight,
                _ => s.family == Family::Utility,
            };
            if !family_ok {
                problems.push(format!(
                    "script `{}`: effect {:?} does not belong to family {:?}",
                    s.name, s.effect, s.family
                ));
            }
            if !(1..=9).contains(&s.power)
                || !(1..=8).contains(&s.memory)
                || !(1..=3).contains(&s.cycles)
                || s.noise > 30
                || s.min_skill > 5
            {
                problems.push(format!(
                    "script `{}`: power 1-9, memory 1-8, cycles 1-3, noise <= 30, min_skill <= 5",
                    s.name
                ));
            }
            if s.starter && (s.price != 0 || s.min_rep != 0 || s.min_skill != 0) {
                problems.push(format!(
                    "script `{}`: a starter script has no price, rep or skill gate",
                    s.name
                ));
            }
            if !s.starter && s.price == 0 {
                problems.push(format!(
                    "script `{}`: a script that is not a starter needs a price",
                    s.name
                ));
            }
            if !plain_text(&s.blurb) || s.blurb.chars().count() > 70 {
                problems.push(format!(
                    "script `{}`: blurb must be plain and at most 70 characters",
                    s.name
                ));
            }
        }
        let starter_memory: u32 = self
            .script
            .iter()
            .filter(|s| s.starter)
            .map(|s| s.memory)
            .sum();
        if starter_memory <= 8 {
            problems.push(
                "the starter scripts must not all fit the base deck (the deck must be a choice)"
                    .into(),
            );
        }
        problems
    }
}

/// Whether an objective effect can be done at a node with this code: the rule `Run` enforces.
#[must_use]
pub fn objective_possible(target: &str, effect: Effect) -> bool {
    match effect {
        Effect::Copy | Effect::Shred => target == "VT",
        Effect::Plant => matches!(target, "AC" | "SR"),
        _ => false,
    }
}

impl Table for ContractsTable {
    fn validate(&self) -> Vec<String> {
        let mut problems = id_problems(
            "contract",
            self.contract.iter().map(|c| c.num),
            self.contract.iter().map(|c| c.title.as_str()),
        );
        for tier in 1..=3 {
            if !self.contract.iter().any(|c| c.tier == tier) {
                problems.push(format!("no contract template for tier {tier}"));
            }
        }
        for c in &self.contract {
            if !(1..=3).contains(&c.tier) || c.reward_min == 0 || c.reward_max < c.reward_min {
                problems.push(format!(
                    "contract `{}`: tier 1-3 and 0 < reward_min <= reward_max",
                    c.title
                ));
            }
            if !plain_text(&c.verb) || c.verb.is_empty() || c.verb.chars().count() > 12 {
                problems.push(format!(
                    "contract `{}`: verb must be plain and at most 12 characters",
                    c.title
                ));
            }
            if !matches!(c.effect, Effect::Copy | Effect::Plant | Effect::Shred) {
                problems.push(format!(
                    "contract `{}`: the objective effect must be copy, plant or shred",
                    c.title
                ));
            }
            if !objective_possible(&c.target, c.effect) {
                problems.push(format!(
                    "contract `{}`: {:?} cannot be done at a {} node (copy and shred work on a data vault, plant on an admin console or a server room)",
                    c.title, c.effect, c.target
                ));
            }
        }
        problems
    }
}

impl Table for CmTable {
    fn validate(&self) -> Vec<String> {
        let mut problems = id_problems(
            "cm",
            self.cm.iter().map(|c| c.num),
            self.cm.iter().map(|c| c.name.as_str()),
        );
        for c in &self.cm {
            if !(1..=3).contains(&c.tier_min) || c.tier_max < c.tier_min || c.tier_max > 3 {
                problems.push(format!("cm `{}`: tiers must be 1 to 3, min <= max", c.name));
            }
            if !(4..=40).contains(&c.integrity) || c.armor > 5 || !(1..=20).contains(&c.strike) {
                problems.push(format!(
                    "cm `{}`: integrity 4-40, armor <= 5, strike 1-20",
                    c.name
                ));
            }
            if c.pattern.len() < 2 || c.pattern.len() > 8 {
                problems.push(format!("cm `{}`: a pattern has 2 to 8 beats", c.name));
            }
            if !c.pattern.iter().any(|i| {
                matches!(
                    i,
                    Intent::Strike | Intent::Probe | Intent::Trace | Intent::Counter
                )
            }) {
                problems.push(format!(
                    "cm `{}`: its pattern never touches the trace",
                    c.name
                ));
            }
            if c.pattern.contains(&Intent::Mend) != (c.mend > 0) {
                problems.push(format!(
                    "cm `{}`: `mend` must be set exactly when the pattern mends",
                    c.name
                ));
            }
            if c.weight == 0 || c.drop > 100 {
                problems.push(format!("cm `{}`: weight above 0, drop at most 100", c.name));
            }
            if !plain_text(&c.blurb) || c.blurb.chars().count() > 90 {
                problems.push(format!(
                    "cm `{}`: blurb must be plain and at most 90 characters",
                    c.name
                ));
            }
            for tag in &c.tags {
                if tag.is_empty() || !tag.bytes().all(|b| b.is_ascii_lowercase()) {
                    problems.push(format!("cm `{}`: tags are lowercase words", c.name));
                }
            }
        }
        for tier in 1..=3u8 {
            for kind in [CmKind::Sentry, CmKind::Tracer] {
                if !self
                    .cm
                    .iter()
                    .any(|c| c.kind == kind && c.tier_min <= tier && tier <= c.tier_max)
                {
                    problems.push(format!("no {} for tier {tier}", kind.word()));
                }
            }
        }
        for tier in 2..=3u8 {
            if !self
                .cm
                .iter()
                .any(|c| c.kind == CmKind::Wall && c.tier_min <= tier && tier <= c.tier_max)
            {
                problems.push(format!("no black wall for tier {tier}"));
            }
        }
        if !self.cm.iter().any(|c| c.kind == CmKind::Warden) {
            problems.push("needs at least one warden".into());
        }
        problems
    }
}

impl Table for FragmentsTable {
    fn validate(&self) -> Vec<String> {
        let mut problems = id_problems(
            "fragment",
            self.fragment.iter().map(|f| f.num),
            self.fragment.iter().map(|f| f.name.as_str()),
        );
        for f in &self.fragment {
            if !(1..=31).contains(&f.num) {
                problems.push(format!("fragment `{}`: num must be 1 to 31", f.name));
            }
            let set = (f.effect.is_some(), f.target.is_some(), f.modifier.is_some());
            let ok = match f.kind {
                FragKind::Effect => set == (true, false, false),
                FragKind::Target => set == (false, true, false),
                FragKind::Modifier => set == (false, false, true),
            };
            if !ok {
                problems.push(format!(
                    "fragment `{}`: set exactly the field its kind names",
                    f.name
                ));
            }
            if let Some(effect) = f.effect
                && !codable(effect)
            {
                problems.push(format!("fragment `{}`: {effect:?} cannot be coded", f.name));
            }
            if f.target == Some(CmKind::Warden) && f.kind != FragKind::Target {
                problems.push(format!("fragment `{}`: only a target tunes", f.name));
            }
            if !(1..=3).contains(&f.rank) || f.weight == 0 {
                problems.push(format!("fragment `{}`: rank 1-3 and a weight", f.name));
            }
            if !(-4..=6).contains(&f.power)
                || !(-3..=4).contains(&f.memory)
                || !(-4..=6).contains(&f.noise)
                || !(-1..=0).contains(&f.cycles)
                || f.pierce > 3
                || f.charges > 3
            {
                problems.push(format!("fragment `{}`: a number is out of range", f.name));
            }
            if !plain_text(&f.blurb) || f.blurb.chars().count() > 70 {
                problems.push(format!(
                    "fragment `{}`: blurb must be plain and at most 70 characters",
                    f.name
                ));
            }
        }
        for kind in [FragKind::Effect, FragKind::Target, FragKind::Modifier] {
            let n = self.fragment.iter().filter(|f| f.kind == kind).count();
            if n == 0 {
                problems.push(format!("no {kind:?} fragments"));
            }
        }
        problems
    }
}

/// The effects the module editor can build: the ones that matter in a fight or on the way in.
#[must_use]
pub const fn codable(effect: Effect) -> bool {
    matches!(
        effect,
        Effect::Scan
            | Effect::Crack
            | Effect::Spoof
            | Effect::Overflow
            | Effect::Mask
            | Effect::Decoy
            | Effect::Reroute
            | Effect::Jam
            | Effect::Fork
            | Effect::Purge
    )
}

impl Table for RigTable {
    fn validate(&self) -> Vec<String> {
        let mut problems = id_problems(
            "part",
            self.part.iter().map(|p| p.num),
            self.part.iter().map(|p| p.name.as_str()),
        );
        if self.part.len() != 5
            || !self
                .part
                .iter()
                .enumerate()
                .all(|(i, p)| usize::from(p.num) == i)
        {
            problems.push("the five parts must be numbered 0 to 4 in order".into());
        }
        for part in &self.part {
            if part.levels.len() != 3 {
                problems.push(format!("part `{}`: needs exactly three levels", part.name));
            }
            for level in &part.levels {
                if level.cost == 0 || !plain_text(&level.name) || level.name.chars().count() > 30 {
                    problems.push(format!(
                        "part `{}`: a level needs a plain name (30 characters) and a cost",
                        part.name
                    ));
                }
            }
        }
        let h = &self.hideout;
        if h.bot_cost == 0
            || h.bot_income == 0
            || h.bot_max == 0
            || h.proxy_cost == 0
            || h.stash_step == 0
            || h.proxy_max == 0
        {
            problems.push("hideout numbers must be set".into());
        }
        if self.fence.is_empty() || self.fence.first().is_some_and(|f| f.cost != 0) {
            problems.push("the first fence level must exist and cost nothing".into());
        }
        for fence in &self.fence {
            if fence.rate == 0 || fence.rate > 100 || !plain_text(&fence.name) {
                problems.push(format!("fence `{}`: rate must be 1 to 100", fence.name));
            }
        }
        problems
    }
}

/// All of the game's content, loaded once.
#[derive(Debug, Clone, Default)]
pub struct Content {
    /// Corporations and fixers.
    pub corps: CorpsTable,
    /// Node types.
    pub nodes: NodesTable,
    /// Scripts.
    pub scripts: ScriptsTable,
    /// Contract templates.
    pub contracts: ContractsTable,
    /// Rig, hideout and fence.
    pub rig: RigTable,
    /// Countermeasures and wardens.
    pub cms: CmTable,
    /// Fragments for coding.
    pub fragments: FragmentsTable,
    /// The fixers' arcs and their endings.
    pub story: crate::story::StoryTable,
    /// Season endings and character epilogues.
    pub seasons: crate::story::SeasonsTable,
    /// The lines the social layer says.
    pub lines: crate::story::LinesTable,
}

impl Content {
    /// The embedded tables, or an empty (unplayable but safe) set if one were broken; a test rules
    /// that out.
    #[must_use]
    pub fn embedded() -> Self {
        Self::try_embedded().unwrap_or_default()
    }

    /// The embedded tables.
    ///
    /// # Errors
    /// The first table that does not load.
    pub fn try_embedded() -> Result<Self, content::ContentError> {
        let mut scripts: ScriptsTable = content::load("scripts", SCRIPTS_TOML)?;
        for s in &mut scripts.script {
            s.sig = u16::from(s.num);
        }
        Ok(Self {
            corps: content::load("corps", CORPS_TOML)?,
            nodes: content::load("nodes", NODES_TOML)?,
            scripts,
            contracts: content::load("contracts", CONTRACTS_TOML)?,
            rig: content::load("rig", RIG_TOML)?,
            cms: content::load("countermeasures", COUNTERMEASURES_TOML)?,
            fragments: content::load("fragments", FRAGMENTS_TOML)?,
            story: content::load("arcs", crate::story::ARCS_TOML)?,
            seasons: content::load("seasons", crate::story::SEASONS_TOML)?,
            lines: content::load("social", crate::story::SOCIAL_TOML)?,
        })
    }

    /// A countermeasure by number.
    #[must_use]
    pub fn cm(&self, num: u8) -> Option<&CmDef> {
        self.cms.cm.iter().find(|c| c.num == num)
    }

    /// A fragment by number.
    #[must_use]
    pub fn fragment(&self, num: u8) -> Option<&FragmentDef> {
        self.fragments.fragment.iter().find(|f| f.num == num)
    }

    /// A corporation by number.
    #[must_use]
    pub fn corp(&self, num: u8) -> Option<&CorpDef> {
        self.corps.corp.iter().find(|c| c.num == num)
    }

    /// A fixer by number.
    #[must_use]
    pub fn fixer(&self, num: u8) -> Option<&FixerDef> {
        self.corps.fixer.iter().find(|f| f.num == num)
    }

    /// A node type by number.
    #[must_use]
    pub fn node(&self, num: u8) -> Option<&NodeDef> {
        self.nodes.node.iter().find(|n| n.num == num)
    }

    /// The first node type with `role` (the entry), or one with `code`.
    #[must_use]
    pub fn node_by_code(&self, code: &str) -> Option<&NodeDef> {
        self.nodes.node.iter().find(|n| n.code == code)
    }

    /// A script by number.
    #[must_use]
    pub fn script(&self, num: u8) -> Option<&ScriptDef> {
        self.scripts.script.iter().find(|s| s.num == num)
    }

    /// A contract template by number.
    #[must_use]
    pub fn contract(&self, num: u8) -> Option<&ContractDef> {
        self.contracts.contract.iter().find(|c| c.num == num)
    }

    /// The fence levels' rate.
    #[must_use]
    pub fn fence_rate(&self, level: u8) -> u32 {
        self.rig
            .fence
            .get(usize::from(level))
            .or_else(|| self.rig.fence.last())
            .map_or(50, |f| f.rate)
    }
}
