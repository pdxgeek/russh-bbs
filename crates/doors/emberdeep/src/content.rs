//! Emberdeep's content as data: callings, skill nodes, traits, scars, wounds, moves, monsters,
//! traps, gear and room templates. Each table is TOML embedded in the crate and validated at
//! load by the framework's `content` helpers; `tests/content.rs` runs the schema check, and
//! [`Content::cross_check`] proves every id one table names exists in the table it names.
//!
//! Adding content is a TOML edit. The engine knows a closed set of *effects* ([`Effect`],
//! [`Gain`], [`Need`], [`MoveKind`], [`Intent`]); a table can combine them freely but cannot
//! invent a new one.

use russh_bbs_door::content::{self, ContentError, Table, plain_text};
use serde::Deserialize;

/// The callings table.
pub const CALLINGS_TOML: &str = include_str!("../content/callings.toml");
/// The skill nodes table.
pub const NODES_TOML: &str = include_str!("../content/nodes.toml");
/// The traits and scars table.
pub const TRAITS_TOML: &str = include_str!("../content/traits.toml");
/// The combat moves table.
pub const MOVES_TOML: &str = include_str!("../content/moves.toml");
/// The monsters and traps table.
pub const BESTIARY_TOML: &str = include_str!("../content/bestiary.toml");
/// The room templates table.
pub const ROOMS_TOML: &str = include_str!("../content/rooms.toml");
/// The wounds table.
pub const WOUNDS_TOML: &str = include_str!("../content/wounds.toml");
/// The weapons and armour table.
pub const GEAR_TOML: &str = include_str!("../content/gear.toml");

/// The most characters of one content text.
pub const TEXT_MAX: usize = 220;

/// One of the four attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Attr {
    /// Strength and toughness.
    Might,
    /// Speed and precision.
    Finesse,
    /// Perception, lore and craft.
    Wits,
    /// Will and faith.
    Spirit,
}

impl Attr {
    /// All four, in save order.
    pub const ALL: [Self; 4] = [Self::Might, Self::Finesse, Self::Wits, Self::Spirit];

    /// Position in [`Attr::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Might => 0,
            Self::Finesse => 1,
            Self::Wits => 2,
            Self::Spirit => 3,
        }
    }

    /// The name shown to the player.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Might => "Might",
            Self::Finesse => "Finesse",
            Self::Wits => "Wits",
            Self::Spirit => "Spirit",
        }
    }
}

/// A permanent modifier. Nodes, traits, scars, wounds all use these, and the hero's derived
/// numbers, tags and moves are computed from the list, so every one of them changes outcomes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Effect {
    /// Add to an attribute.
    Attr {
        /// Which attribute.
        attr: Attr,
        /// How much (negative for a wound).
        amount: i32,
    },
    /// Add to the hero's maximum hit points.
    MaxHp {
        /// How much.
        amount: i32,
    },
    /// Flat damage taken off every hit.
    Armor {
        /// How much.
        amount: i32,
    },
    /// Flat damage added to every hit the hero deals.
    Damage {
        /// How much.
        amount: i32,
    },
    /// Percentage points of critical hits.
    CritPct {
        /// How many.
        amount: i32,
    },
    /// Percentage points of dodging.
    EvadePct {
        /// How many.
        amount: i32,
    },
    /// Percentage points of surviving a killing blow.
    DeathSave {
        /// How many.
        amount: i32,
    },
    /// Salvage the hero carries before climbing costs extra turns.
    Carry {
        /// How much.
        amount: i32,
    },
    /// Turns knocked off the climb back to town.
    ClimbEase {
        /// How many.
        amount: i32,
    },
    /// Extra focus at the start of a fight.
    StartFocus {
        /// How much.
        amount: i32,
    },
    /// Percent more coin and salvage from fights.
    LootPct {
        /// How many percent.
        amount: i32,
    },
    /// Added to the roll that spots traps.
    TrapSense {
        /// How much.
        amount: i32,
    },
    /// Percent less fatigue gained.
    FatigueGuard {
        /// How many percent.
        amount: i32,
    },
    /// A capability that rooms ask for (`lockpick`, `runecraft`, ...).
    Tag {
        /// The tag.
        tag: String,
    },
    /// A combat move the hero may use.
    Move {
        /// The move's id.
        id: String,
    },
}

/// Melee reaches the front row; ranged reaches anyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reach {
    /// Hits the front row only.
    Melee,
    /// Hits any row.
    Ranged,
}

/// Which row a fighter stands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Row {
    /// In melee.
    Front,
    /// Behind.
    Back,
}

/// What a blow is made of; wounds are chosen by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DamageKind {
    /// Cuts and stabs.
    Blade,
    /// Hammers and falls.
    Crush,
    /// Heat.
    Fire,
    /// Frost and the dark.
    Cold,
}

/// What an enemy announces it will do next round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Intent {
    /// An ordinary blow.
    Strike,
    /// A slow, heavy blow: double damage, and a dodging hero is never hit by it.
    Heavy,
    /// A shot from range: hits any row.
    Shoot,
    /// Takes half damage this round and does not attack.
    Brace,
    /// Shouts: allies hit harder next round.
    Rally,
    /// Closes the distance to a hero in the back row.
    Advance,
    /// A guardian's breath: heavy fire that hits any row.
    Breathe,
}

impl Intent {
    /// The line shown under the enemy's name.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Strike => "readies a blow",
            Self::Heavy => "winds up a crushing blow",
            Self::Shoot => "takes aim",
            Self::Brace => "braces behind its guard",
            Self::Rally => "draws breath to rally",
            Self::Advance => "closes in",
            Self::Breathe => "gathers a furnace breath",
        }
    }
}

/// What a combat move does besides damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MoveKind {
    /// Damage only.
    Strike,
    /// Damage, and the target's announced intent is cancelled this round.
    Interrupt,
    /// Heal the hero by `amount`.
    Mend,
    /// Absorb the next `amount` damage.
    Ward,
    /// The target takes `amount` percent more from every hit for the fight.
    Mark,
    /// Damage, and the target's defense drops by `amount`.
    Sunder,
    /// Damage, and the target loses `amount` hit points at the end of each round.
    Bleed,
    /// Damage, and the target is shoved to the back row.
    Shove,
}

/// A combat move.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveDef {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// One line.
    pub text: String,
    /// Who it can reach.
    pub reach: Reach,
    /// Focus it costs.
    pub cost: u32,
    /// Damage as a percentage of a basic attack (0 for a move that does none).
    pub power: u32,
    /// The extra effect.
    pub kind: MoveKind,
    /// The effect's size.
    #[serde(default)]
    pub amount: u32,
}

/// A calling: the hero's trade.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Calling {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// One line.
    pub blurb: String,
    /// Starting might, finesse, wits, spirit.
    pub attrs: [u32; 4],
    /// Base hit points.
    pub hp: u32,
    /// Starting weapon id.
    pub weapon: String,
    /// Starting armour id.
    pub armour: String,
    /// Capabilities the calling has from the start.
    pub tags: Vec<String>,
    /// Moves the calling has from the start.
    pub moves: Vec<String>,
}

/// One node of a calling's skill tree.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    /// Unique id.
    pub id: String,
    /// The calling whose tree it is in.
    pub calling: String,
    /// 1 to 5: the tier sets the level needed.
    pub tier: u32,
    /// Shown name.
    pub name: String,
    /// One line.
    pub text: String,
    /// Nodes of which at least one must already be taken (empty: none needed).
    #[serde(default)]
    pub requires: Vec<String>,
    /// Nodes that become impossible once this one is taken, and the reverse.
    #[serde(default)]
    pub excludes: Vec<String>,
    /// What it does.
    pub effects: Vec<Effect>,
}

/// A trait, or a scar (the same shape; scars are earned, traits are chosen).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraitDef {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// One line.
    pub text: String,
    /// What it does.
    pub effects: Vec<Effect>,
}

/// A wound: a penalty that lasts days and may leave a scar.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WoundDef {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// The kind of blow that causes it (`None`: only fixed sources, such as a trap or a brink).
    #[serde(default)]
    pub source: Option<DamageKind>,
    /// How many days it lasts.
    pub days: u32,
    /// What it does while it lasts.
    pub effects: Vec<Effect>,
    /// A boon (a good thing that wears off) rather than an injury.
    #[serde(default)]
    pub boon: bool,
    /// Percent chance it leaves a scar when it runs out.
    #[serde(default)]
    pub scar_pct: u32,
    /// The scar it leaves.
    #[serde(default)]
    pub scar: Option<String>,
}

/// A weapon.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weapon {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// Melee or ranged.
    pub reach: Reach,
    /// The attribute that adds to damage.
    pub attr: Attr,
    /// Lowest and highest base damage.
    pub damage: [u32; 2],
    /// The depth band it belongs to (a band-n item needs hero level `3n - 2`).
    #[serde(default = "one")]
    pub band: u32,
    /// What the market asks (0: not sold; a starting kit or a find).
    #[serde(default)]
    pub price: u32,
}

/// Armour.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Armour {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// Flat damage taken off every hit.
    pub armor: u32,
    /// The depth band it belongs to (a band-n item needs hero level `3n - 2`).
    #[serde(default = "one")]
    pub band: u32,
    /// What the market asks (0: not sold; a starting kit or a find).
    #[serde(default)]
    pub price: u32,
}

/// An entry in the intent table of a monster.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntentWeight {
    /// The intent.
    pub intent: Intent,
    /// How likely, against the others.
    pub weight: u32,
}

/// A monster or a guardian.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Monster {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// One line.
    pub text: String,
    /// The depth band it lives in (1 to 5).
    pub band: u32,
    /// Hit points.
    pub hp: u32,
    /// Base damage.
    pub attack: u32,
    /// Flat damage taken off every hit.
    pub defense: u32,
    /// Whether its blows reach the back row.
    pub reach: Reach,
    /// Where it starts.
    pub row: Row,
    /// What its blows are made of.
    pub kind: DamageKind,
    /// Experience for the kill.
    pub xp: u32,
    /// Lowest and highest coin.
    pub coin: [u32; 2],
    /// Lowest and highest salvage.
    pub salvage: [u32; 2],
    /// What it may announce.
    pub intents: Vec<IntentWeight>,
    /// A guardian of the band's last level.
    #[serde(default)]
    pub guardian: bool,
    /// Monsters that fight beside it.
    #[serde(default)]
    pub adds: Vec<String>,
    /// The ember-core a guardian drops.
    #[serde(default)]
    pub core: Option<String>,
}

/// A trap.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trap {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// One line.
    pub text: String,
    /// The lowest band it appears in.
    pub band: u32,
    /// The roll needed to spot it, and to disarm it without the tag.
    pub dc: u32,
    /// Lowest and highest damage when it springs.
    pub damage: [u32; 2],
    /// A wound it may cause.
    #[serde(default)]
    pub wound: Option<String>,
    /// Experience for disarming it.
    pub xp: u32,
}

/// The monsters and the traps.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bestiary {
    /// Monsters and guardians.
    pub monster: Vec<Monster>,
    /// Traps.
    pub trap: Vec<Trap>,
}

/// What kind of room a template is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomKind {
    /// Where a level is entered.
    Entry,
    /// The way down.
    Exit,
    /// The way down, held by a guardian.
    Guardian,
    /// A passage.
    Corridor,
    /// A store room.
    Vault,
    /// An old forge.
    Forge,
    /// A flooded hall.
    Flooded,
    /// A shrine.
    Shrine,
    /// A monster's den.
    Lair,
    /// A broken lift, rope line or bridge.
    Shaft,
}

impl RoomKind {
    /// A stable index for tallies (0 to 9).
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Entry => 0,
            Self::Exit => 1,
            Self::Guardian => 2,
            Self::Corridor => 3,
            Self::Vault => 4,
            Self::Forge => 5,
            Self::Flooded => 6,
            Self::Shrine => 7,
            Self::Lair => 8,
            Self::Shaft => 9,
        }
    }
}

/// The kinds of shortcut a hero can open for everyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShortcutKind {
    /// A lift: one turn, either way.
    Lift,
    /// A bridge: two turns.
    Bridge,
    /// A rope line: three turns.
    Rope,
}

impl ShortcutKind {
    /// The flat turn cost of the whole trip between the surface and the level.
    #[must_use]
    pub const fn flat_cost(self) -> u32 {
        match self {
            Self::Lift => 1,
            Self::Bridge => 2,
            Self::Rope => 3,
        }
    }

    /// The name shown to the player.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Lift => "lift",
            Self::Bridge => "bridge",
            Self::Rope => "rope line",
        }
    }
}

/// A condition on a room option; every one listed must hold.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Need {
    /// The hero has this capability tag.
    Tag {
        /// The tag.
        tag: String,
    },
    /// The hero does not have this tag (a fallback for those without the tools).
    NoTag {
        /// The tag.
        tag: String,
    },
    /// An attribute of at least `min`.
    Attr {
        /// Which.
        attr: Attr,
        /// How much.
        min: u32,
    },
    /// The hero's calling.
    Calling {
        /// The calling id.
        id: String,
    },
    /// The hero has this trait or scar.
    Trait {
        /// The trait id.
        id: String,
    },
    /// The hero has taken this skill node.
    Node {
        /// The node id.
        id: String,
    },
    /// Hero level of at least `min`.
    Level {
        /// How high.
        min: u32,
    },
    /// The hero has made this choice in a questline (a flag set for good).
    Flag {
        /// The flag.
        id: String,
    },
    /// The hero has not made this choice.
    NoFlag {
        /// The flag.
        id: String,
    },
}

/// A roll an option asks for: d20 plus the attribute's modifier, against `dc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    /// The attribute.
    pub attr: Attr,
    /// The number to reach.
    pub dc: u32,
}

/// What an option gives or takes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Gain {
    /// Coin (more with `LootPct`).
    Coin {
        /// How much.
        amount: u32,
    },
    /// Salvage (more with `LootPct`).
    Salvage {
        /// How much.
        amount: u32,
    },
    /// Experience.
    Xp {
        /// How much.
        amount: u32,
    },
    /// Hit points back.
    Heal {
        /// How many.
        amount: u32,
    },
    /// Hit points lost (armour does not help).
    Hurt {
        /// How many.
        amount: u32,
    },
    /// Fatigue gained (negative: rested).
    Fatigue {
        /// How much.
        amount: i32,
    },
    /// Torch-turns gained or lost.
    Torch {
        /// How many turns.
        amount: i32,
    },
    /// Reveal every room of the level on the map.
    Reveal,
    /// Start a fight with the room's pack.
    Fight,
    /// The room's pack no longer bars the way (it was sneaked past, stared down or lured off).
    Clear,
    /// Open a shortcut at this level for everyone.
    Shortcut {
        /// Which kind.
        shortcut: ShortcutKind,
    },
    /// Cause a wound.
    Wound {
        /// The wound id.
        id: String,
    },
    /// A relic of the old forges.
    Relic,
    /// Teaches a forge recipe.
    Recipe {
        /// The recipe id.
        id: String,
    },
    /// An ember-core.
    Core {
        /// The core id.
        id: String,
    },
}

fn one() -> u32 {
    1
}

fn yes() -> bool {
    true
}

/// One thing a hero may do in a room.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomOption {
    /// Unique within the room.
    pub id: String,
    /// The line shown in the menu.
    pub label: String,
    /// Every condition must hold for the option to be offered at all.
    #[serde(default)]
    pub need: Vec<Need>,
    /// Torch-turns it costs.
    pub cost: u32,
    /// The roll it asks for, if any.
    #[serde(default)]
    pub check: Option<Check>,
    /// Whether it can be done once a day.
    #[serde(default = "yes")]
    pub once: bool,
    /// What happens on success (or always, with no check).
    pub ok: Vec<Gain>,
    /// What happens on a failed roll.
    #[serde(default)]
    pub fail: Vec<Gain>,
    /// What the player reads on success.
    pub ok_text: String,
    /// What the player reads on failure.
    #[serde(default)]
    pub fail_text: String,
}

/// A room template.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomTemplate {
    /// Unique id.
    pub id: String,
    /// The kind.
    pub kind: RoomKind,
    /// The bands it can appear in.
    pub bands: Vec<u32>,
    /// Shown name.
    pub name: String,
    /// What the hero sees.
    pub text: String,
    /// Turns it costs to walk in (default 1).
    #[serde(default)]
    pub move_cost: u32,
    /// Percent chance of a trap in the room on a given day.
    #[serde(default)]
    pub trap_pct: u32,
    /// What can be done here.
    #[serde(default)]
    pub option: Vec<RoomOption>,
}

/// A table of callings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Callings {
    /// The callings.
    pub calling: Vec<Calling>,
}

/// A table of skill nodes.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nodes {
    /// The nodes.
    pub node: Vec<Node>,
}

/// Traits and scars.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Traits {
    /// Chosen at creation.
    #[serde(rename = "trait")]
    pub traits: Vec<TraitDef>,
    /// Earned from wounds.
    pub scar: Vec<TraitDef>,
}

/// A table of moves.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Moves {
    /// The moves.
    #[serde(rename = "move")]
    pub moves: Vec<MoveDef>,
}

/// A table of wounds.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wounds {
    /// The wounds.
    pub wound: Vec<WoundDef>,
}

/// Weapons and armour.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gear {
    /// Weapons.
    pub weapon: Vec<Weapon>,
    /// Armour.
    pub armour: Vec<Armour>,
}

/// A table of room templates.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rooms {
    /// The templates.
    pub room: Vec<RoomTemplate>,
}

pub(crate) fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub(crate) fn text_ok(text: &str) -> bool {
    !text.trim().is_empty() && text.chars().count() <= TEXT_MAX && plain_text(text)
}

pub(crate) fn check_ids<'a>(
    table: &str,
    ids: impl Iterator<Item = &'a str>,
    problems: &mut Vec<String>,
) {
    let mut seen: Vec<&str> = Vec::new();
    for id in ids {
        if !id_ok(id) {
            problems.push(format!("[{table}] id `{id}` must be a-z, 0-9 and -"));
        }
        if seen.contains(&id) {
            problems.push(format!("[{table}] id `{id}` is used twice"));
        }
        seen.push(id);
    }
}

pub(crate) fn check_text(table: &str, id: &str, text: &str, problems: &mut Vec<String>) {
    if !text_ok(text) {
        problems.push(format!(
            "[{table}] `{id}`: text must be 1 to {TEXT_MAX} plain characters"
        ));
    }
}

pub(crate) fn check_effects(table: &str, id: &str, effects: &[Effect], problems: &mut Vec<String>) {
    if effects.is_empty() {
        problems.push(format!("[{table}] `{id}`: needs at least one effect"));
    }
    for effect in effects {
        match effect {
            Effect::Tag { tag } if !id_ok(tag) => {
                problems.push(format!("[{table}] `{id}`: bad tag `{tag}`"));
            }
            Effect::Attr { amount: 0, .. }
            | Effect::MaxHp { amount: 0 }
            | Effect::Armor { amount: 0 }
            | Effect::Damage { amount: 0 } => {
                problems.push(format!("[{table}] `{id}`: an effect of 0 does nothing"));
            }
            _ => {}
        }
    }
}

impl Table for Callings {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids(
            "calling",
            self.calling.iter().map(|c| c.id.as_str()),
            &mut problems,
        );
        for c in &self.calling {
            check_text("calling", &c.id, &c.blurb, &mut problems);
            if c.name.trim().is_empty() || !plain_text(&c.name) {
                problems.push(format!("[calling] `{}`: bad name", c.id));
            }
            if c.attrs.iter().any(|a| !(6..=14).contains(a)) {
                problems.push(format!("[calling] `{}`: attributes must be 6 to 14", c.id));
            }
            if !(20..=60).contains(&c.hp) {
                problems.push(format!("[calling] `{}`: hp must be 20 to 60", c.id));
            }
            if c.moves.is_empty() {
                problems.push(format!("[calling] `{}`: needs a starting move", c.id));
            }
        }
        problems
    }
}

impl Table for Nodes {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids(
            "node",
            self.node.iter().map(|n| n.id.as_str()),
            &mut problems,
        );
        for n in &self.node {
            check_text("node", &n.id, &n.text, &mut problems);
            check_effects("node", &n.id, &n.effects, &mut problems);
            if !(1..=5).contains(&n.tier) {
                problems.push(format!("[node] `{}`: tier must be 1 to 5", n.id));
            }
            if n.name.trim().is_empty() || !plain_text(&n.name) {
                problems.push(format!("[node] `{}`: bad name", n.id));
            }
            if n.tier > 1 && n.requires.is_empty() {
                problems.push(format!("[node] `{}`: tier above 1 needs `requires`", n.id));
            }
            for other in n.requires.iter().chain(&n.excludes) {
                match self.node.iter().find(|m| m.id == *other) {
                    None => problems.push(format!("[node] `{}`: unknown node `{other}`", n.id)),
                    Some(m) if m.calling != n.calling => problems.push(format!(
                        "[node] `{}`: `{other}` is in another calling's tree",
                        n.id
                    )),
                    Some(_) => {}
                }
            }
            for other in &n.excludes {
                let mirrored = self
                    .node
                    .iter()
                    .find(|m| m.id == *other)
                    .is_some_and(|m| m.excludes.contains(&n.id));
                if !mirrored {
                    problems.push(format!(
                        "[node] `{}`: excludes `{other}` but not the reverse",
                        n.id
                    ));
                }
            }
        }
        problems
    }
}

impl Table for Traits {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids(
            "trait",
            self.traits.iter().map(|t| t.id.as_str()),
            &mut problems,
        );
        check_ids(
            "scar",
            self.scar.iter().map(|t| t.id.as_str()),
            &mut problems,
        );
        for (table, list) in [("trait", &self.traits), ("scar", &self.scar)] {
            for t in list {
                check_text(table, &t.id, &t.text, &mut problems);
                check_effects(table, &t.id, &t.effects, &mut problems);
                if t.name.trim().is_empty() || !plain_text(&t.name) {
                    problems.push(format!("[{table}] `{}`: bad name", t.id));
                }
            }
        }
        for t in &self.traits {
            if self.scar.iter().any(|s| s.id == t.id) {
                problems.push(format!("[trait] `{}` is also a scar id", t.id));
            }
        }
        problems
    }
}

impl Table for Moves {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids(
            "move",
            self.moves.iter().map(|m| m.id.as_str()),
            &mut problems,
        );
        for m in &self.moves {
            check_text("move", &m.id, &m.text, &mut problems);
            if m.name.trim().is_empty() || !plain_text(&m.name) {
                problems.push(format!("[move] `{}`: bad name", m.id));
            }
            if !(1..=4).contains(&m.cost) {
                problems.push(format!("[move] `{}`: cost must be 1 to 4", m.id));
            }
            let needs_amount = !matches!(
                m.kind,
                MoveKind::Strike | MoveKind::Interrupt | MoveKind::Shove
            );
            if needs_amount && m.amount == 0 {
                problems.push(format!("[move] `{}`: its kind needs an amount", m.id));
            }
            let hits = !matches!(m.kind, MoveKind::Mend | MoveKind::Ward);
            if hits && m.power == 0 {
                problems.push(format!("[move] `{}`: an attacking move needs power", m.id));
            }
        }
        problems
    }
}

impl Table for Wounds {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids(
            "wound",
            self.wound.iter().map(|w| w.id.as_str()),
            &mut problems,
        );
        for w in &self.wound {
            check_effects("wound", &w.id, &w.effects, &mut problems);
            if w.name.trim().is_empty() || !plain_text(&w.name) {
                problems.push(format!("[wound] `{}`: bad name", w.id));
            }
            if !(1..=14).contains(&w.days) {
                problems.push(format!("[wound] `{}`: days must be 1 to 14", w.id));
            }
            if w.scar_pct > 100 || (w.scar_pct > 0) != w.scar.is_some() {
                problems.push(format!(
                    "[wound] `{}`: scar and scar_pct go together, pct at most 100",
                    w.id
                ));
            }
        }
        problems
    }
}

impl Table for Gear {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids(
            "weapon",
            self.weapon.iter().map(|w| w.id.as_str()),
            &mut problems,
        );
        check_ids(
            "armour",
            self.armour.iter().map(|a| a.id.as_str()),
            &mut problems,
        );
        for w in &self.weapon {
            if w.damage[0] == 0 || w.damage[0] > w.damage[1] {
                problems.push(format!(
                    "[weapon] `{}`: damage must be low..=high, above 0",
                    w.id
                ));
            }
            if w.name.trim().is_empty() || !plain_text(&w.name) {
                problems.push(format!("[weapon] `{}`: bad name", w.id));
            }
        }
        for a in &self.armour {
            if a.name.trim().is_empty() || !plain_text(&a.name) {
                problems.push(format!("[armour] `{}`: bad name", a.id));
            }
        }
        problems
    }
}

impl Table for Bestiary {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids(
            "monster",
            self.monster.iter().map(|m| m.id.as_str()),
            &mut problems,
        );
        check_ids(
            "trap",
            self.trap.iter().map(|t| t.id.as_str()),
            &mut problems,
        );
        for m in &self.monster {
            check_text("monster", &m.id, &m.text, &mut problems);
            if m.name.trim().is_empty() || !plain_text(&m.name) {
                problems.push(format!("[monster] `{}`: bad name", m.id));
            }
            if !(1..=5).contains(&m.band) {
                problems.push(format!("[monster] `{}`: band must be 1 to 5", m.id));
            }
            if m.hp == 0 || m.attack == 0 {
                problems.push(format!(
                    "[monster] `{}`: hp and attack must be above 0",
                    m.id
                ));
            }
            if m.coin[0] > m.coin[1] || m.salvage[0] > m.salvage[1] {
                problems.push(format!("[monster] `{}`: ranges must be low..=high", m.id));
            }
            if m.intents.is_empty() || m.intents.iter().all(|w| w.weight == 0) {
                problems.push(format!("[monster] `{}`: needs a weighted intent", m.id));
            }
            for w in &m.intents {
                if w.intent == Intent::Advance {
                    problems.push(format!(
                        "[monster] `{}`: `advance` is chosen by the engine, not listed",
                        m.id
                    ));
                }
                if w.intent == Intent::Shoot && m.reach != Reach::Ranged {
                    problems.push(format!("[monster] `{}`: only ranged monsters shoot", m.id));
                }
                if w.intent == Intent::Breathe && !m.guardian {
                    problems.push(format!("[monster] `{}`: only guardians breathe", m.id));
                }
            }
            for add in &m.adds {
                if !self.monster.iter().any(|o| o.id == *add && !o.guardian) {
                    problems.push(format!("[monster] `{}`: unknown add `{add}`", m.id));
                }
            }
        }
        for band in 1..=5 {
            let guardians = self
                .monster
                .iter()
                .filter(|m| m.guardian && m.band == band)
                .count();
            if guardians != 1 {
                problems.push(format!(
                    "band {band} needs exactly one guardian, has {guardians}"
                ));
            }
            if self
                .monster
                .iter()
                .filter(|m| !m.guardian && m.band == band)
                .count()
                < 2
            {
                problems.push(format!("band {band} needs at least two ordinary monsters"));
            }
        }
        for t in &self.trap {
            check_text("trap", &t.id, &t.text, &mut problems);
            if t.damage[0] == 0 || t.damage[0] > t.damage[1] {
                problems.push(format!("[trap] `{}`: damage must be low..=high", t.id));
            }
            if !(1..=5).contains(&t.band) {
                problems.push(format!("[trap] `{}`: band must be 1 to 5", t.id));
            }
        }
        if !self.trap.iter().any(|t| t.band == 1) {
            problems.push("there must be a trap for band 1".to_owned());
        }
        problems
    }
}

pub(crate) fn check_option(who: &str, o: &RoomOption, problems: &mut Vec<String>) {
    if o.label.trim().is_empty() || o.label.chars().count() > 60 || !plain_text(&o.label) {
        problems.push(format!("{who}: label must be 1 to 60 plain characters"));
    }
    if o.cost > 4 {
        problems.push(format!("{who}: cost at most 4"));
    }
    if !text_ok(&o.ok_text) || (o.check.is_some() && !text_ok(&o.fail_text)) {
        problems.push(format!(
            "{who}: ok_text (and fail_text with a check) must be plain text"
        ));
    }
    if o.check.is_none() && !o.fail.is_empty() {
        problems.push(format!("{who}: `fail` without a check never happens"));
    }
    if o.ok.is_empty() {
        problems.push(format!("{who}: `ok` is empty"));
    }
    for gain in o.ok.iter().chain(&o.fail) {
        if let Some(why) = gain_problem(gain) {
            problems.push(format!("{who}: {why}"));
        }
    }
}

fn gain_problem(gain: &Gain) -> Option<&'static str> {
    match gain {
        Gain::Coin { amount: 0 }
        | Gain::Salvage { amount: 0 }
        | Gain::Xp { amount: 0 }
        | Gain::Heal { amount: 0 }
        | Gain::Hurt { amount: 0 } => Some("a gain of 0 does nothing"),
        Gain::Fatigue { amount: 0 } | Gain::Torch { amount: 0 } => {
            Some("a change of 0 does nothing")
        }
        _ => None,
    }
}

impl Table for Rooms {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids(
            "room",
            self.room.iter().map(|r| r.id.as_str()),
            &mut problems,
        );
        for r in &self.room {
            check_text("room", &r.id, &r.text, &mut problems);
            if r.name.trim().is_empty() || !plain_text(&r.name) {
                problems.push(format!("[room] `{}`: bad name", r.id));
            }
            if r.bands.is_empty() || r.bands.iter().any(|b| !(1..=5).contains(b)) {
                problems.push(format!("[room] `{}`: bands must be listed, 1 to 5", r.id));
            }
            if r.trap_pct > 100 || r.move_cost > 3 {
                problems.push(format!(
                    "[room] `{}`: trap_pct at most 100, move_cost at most 3",
                    r.id
                ));
            }
            check_ids(
                &format!("room {} option", r.id),
                r.option.iter().map(|o| o.id.as_str()),
                &mut problems,
            );
            for o in &r.option {
                let who = format!("[room] `{}` option `{}`", r.id, o.id);
                check_option(&who, o, &mut problems);
                let shortcut = o.ok.iter().any(|g| matches!(g, Gain::Shortcut { .. }));
                if shortcut && r.kind != RoomKind::Shaft {
                    problems.push(format!("{who}: only a shaft room can open a shortcut"));
                }
            }
        }
        problems
    }
}

/// Every table, loaded.
#[derive(Debug, Clone)]
pub struct Content {
    /// Callings.
    pub callings: Callings,
    /// Skill nodes.
    pub nodes: Nodes,
    /// Traits and scars.
    pub traits: Traits,
    /// Moves.
    pub moves: Moves,
    /// Monsters and traps.
    pub bestiary: Bestiary,
    /// Room templates.
    pub rooms: Rooms,
    /// Wounds.
    pub wounds: Wounds,
    /// Weapons and armour.
    pub gear: Gear,
    /// Affixes and ember-cores.
    pub affixes: crate::content_town::Affixes,
    /// Forge recipes.
    pub recipes: crate::content_town::Recipes,
    /// Town projects.
    pub projects: crate::content_town::Projects,
    /// Contracts, companions and rumours.
    pub town: crate::content_town::TownTables,
    /// Dungeon events.
    pub events: crate::content_story::Events,
    /// Questlines.
    pub quests: crate::content_story::Quests,
}

impl Content {
    /// Load and validate every embedded table, then cross-check them.
    ///
    /// # Errors
    /// The first table that fails its schema, or [`ContentError::Invalid`] naming every dangling
    /// reference.
    pub fn embedded() -> Result<Self, ContentError> {
        let content = Self {
            callings: content::load("callings", CALLINGS_TOML)?,
            nodes: content::load("nodes", NODES_TOML)?,
            traits: content::load("traits", TRAITS_TOML)?,
            moves: content::load("moves", MOVES_TOML)?,
            bestiary: content::load("bestiary", BESTIARY_TOML)?,
            rooms: content::load("rooms", ROOMS_TOML)?,
            wounds: content::load("wounds", WOUNDS_TOML)?,
            gear: content::load("gear", GEAR_TOML)?,
            affixes: content::load("affixes", crate::content_town::AFFIXES_TOML)?,
            recipes: content::load("recipes", crate::content_town::RECIPES_TOML)?,
            projects: content::load("projects", crate::content_town::PROJECTS_TOML)?,
            town: content::load("town", crate::content_town::TOWN_TOML)?,
            events: content::load("events", crate::content_story::EVENTS_TOML)?,
            quests: content::load("quests", crate::content_story::QUESTS_TOML)?,
        };
        let problems = content.cross_check();
        if problems.is_empty() {
            Ok(content)
        } else {
            Err(ContentError::Invalid {
                name: "emberdeep",
                problems,
            })
        }
    }

    /// Every id one table names that the table it names does not have, and every shape rule
    /// that needs two tables.
    #[must_use]
    pub fn cross_check(&self) -> Vec<String> {
        let mut problems = Vec::new();
        self.check_callings(&mut problems);
        self.check_wound_and_trap_links(&mut problems);
        self.check_effect_moves(&mut problems);
        self.check_rooms(&mut problems);
        self.check_town(&mut problems);
        self.check_story(&mut problems);
        for (id, why) in [
            ("brink", "a death save survived"),
            ("dragged-home", "a rescue"),
            ("weakened", "waking in the temple"),
        ] {
            if self.wound(id).is_none() {
                problems.push(format!("the wound `{id}` ({why}) must exist"));
            }
        }
        problems
    }

    fn check_story(&self, problems: &mut Vec<String>) {
        use crate::content_story::Goal;
        for e in &self.events.event {
            for o in &e.option {
                for need in &o.need {
                    self.check_need("event", &e.id, need, problems);
                }
                for g in o.ok.iter().chain(&o.fail) {
                    match g {
                        Gain::Recipe { id } if self.recipe(id).is_none() => {
                            problems.push(format!("event `{}`: unknown recipe `{id}`", e.id));
                        }
                        Gain::Core { id } if self.core(id).is_none() => {
                            problems.push(format!("event `{}`: unknown core `{id}`", e.id));
                        }
                        Gain::Wound { id } if self.wound(id).is_none() => {
                            problems.push(format!("event `{}`: unknown wound `{id}`", e.id));
                        }
                        _ => {}
                    }
                }
            }
        }
        let flags: Vec<&str> = self
            .quests
            .quest
            .iter()
            .flat_map(|q| q.step.iter())
            .flat_map(|s| s.choose.iter())
            .map(|c| c.flag.as_str())
            .collect();
        for needed in crate::endings::verdict_flags() {
            if !flags.contains(&needed) {
                problems.push(format!(
                    "no questline choice sets `{needed}`, which a verdict needs"
                ));
            }
        }
        let rooms = self.rooms.room.iter().map(|r| (&r.id, &r.option));
        let events = self.events.event.iter().map(|e| (&e.id, &e.option));
        for (id, options) in rooms.chain(events) {
            for o in options {
                for n in &o.need {
                    if let Need::Flag { id: f } | Need::NoFlag { id: f } = n
                        && !flags.contains(&f.as_str())
                    {
                        problems.push(format!("`{id}`: no questline choice sets `{f}`"));
                    }
                }
            }
        }
        for q in &self.quests.quest {
            for s in &q.step {
                if let Goal::Slay { monster, .. } = &s.goal
                    && self.monster(monster).is_none_or(|m| m.guardian)
                {
                    problems.push(format!(
                        "quest `{}` step `{}`: `{monster}` is not an ordinary monster",
                        q.id, s.id
                    ));
                }
                for g in &s.reward {
                    match g {
                        Gain::Recipe { id } if self.recipe(id).is_none() => problems.push(format!(
                            "quest `{}` step `{}`: unknown recipe `{id}`",
                            q.id, s.id
                        )),
                        Gain::Core { id } if self.core(id).is_none() => problems.push(format!(
                            "quest `{}` step `{}`: unknown core `{id}`",
                            q.id, s.id
                        )),
                        _ => {}
                    }
                }
            }
        }
    }

    fn check_town(&self, problems: &mut Vec<String>) {
        for w in &self.gear.weapon {
            if self.armour(&w.id).is_some() {
                problems.push(format!("`{}` is both a weapon and an armour", w.id));
            }
            if !(1..=5).contains(&w.band) {
                problems.push(format!("weapon `{}`: band must be 1 to 5", w.id));
            }
        }
        for a in &self.gear.armour {
            if !(1..=5).contains(&a.band) {
                problems.push(format!("armour `{}`: band must be 1 to 5", a.id));
            }
        }
        for a in self
            .affixes
            .affix
            .iter()
            .map(|a| (&a.id, &a.effects))
            .chain(self.affixes.core.iter().map(|c| (&c.id, &c.effects)))
        {
            for e in a.1 {
                if let Effect::Move { id } = e
                    && self.mv(id).is_none()
                {
                    problems.push(format!("`{}` names unknown move `{id}`", a.0));
                }
            }
        }
        for m in &self.bestiary.monster {
            if let Some(core) = &m.core {
                if self.core(core).is_none() {
                    problems.push(format!("monster `{}`: unknown core `{core}`", m.id));
                }
                if !m.guardian {
                    problems.push(format!("monster `{}`: only a guardian drops a core", m.id));
                }
            }
        }
        self.check_town_recipes(problems);
        self.check_town_projects(problems);
        self.check_town_people(problems);
    }

    fn check_town_recipes(&self, problems: &mut Vec<String>) {
        use crate::content_town::{Learn, Makes};
        for r in &self.recipes.recipe {
            match &r.makes {
                Makes::Item { base, affixes } => {
                    let Some(slot) = self.base_slot(base) else {
                        problems.push(format!("recipe `{}`: unknown base `{base}`", r.id));
                        continue;
                    };
                    for a in affixes {
                        match self.affix(a) {
                            None => {
                                problems.push(format!("recipe `{}`: unknown affix `{a}`", r.id));
                            }
                            Some(def) if !def.slot.fits(slot) => problems.push(format!(
                                "recipe `{}`: affix `{a}` does not fit a {base}",
                                r.id
                            )),
                            Some(_) => {}
                        }
                    }
                }
                Makes::Core { id } => {
                    if self.core(id).is_none() {
                        problems.push(format!("recipe `{}`: unknown core `{id}`", r.id));
                    }
                }
            }
            match &r.learn {
                Learn::Calling { id } if self.calling(id).is_none() => {
                    problems.push(format!("recipe `{}`: unknown calling `{id}`", r.id));
                }
                Learn::Node { id } if self.node(id).is_none() => {
                    problems.push(format!("recipe `{}`: unknown node `{id}`", r.id));
                }
                _ => {}
            }
            for need in &r.need {
                self.check_need("recipe", &r.id, need, problems);
            }
        }
    }

    fn check_town_projects(&self, problems: &mut Vec<String>) {
        use crate::content_town::ProjectEffect;
        for p in &self.projects.project {
            for req in &p.requires {
                if self.project(req).is_none() {
                    problems.push(format!("project `{}`: unknown requirement `{req}`", p.id));
                }
            }
            if let ProjectEffect::Forge { level }
            | ProjectEffect::Temple { level }
            | ProjectEffect::Market { level }
            | ProjectEffect::Guild { level } = p.effect
                && !(1..=3).contains(&level)
            {
                problems.push(format!("project `{}`: level must be 1 to 3", p.id));
            }
        }
    }

    fn check_town_people(&self, problems: &mut Vec<String>) {
        use crate::content_town::{ContractKind, RumourEffect};
        for c in &self.town.contract {
            if let Some(m) = &c.monster {
                match self.monster(m) {
                    None => problems.push(format!("contract `{}`: unknown monster `{m}`", c.id)),
                    Some(def) if def.guardian || def.band > (c.depth - 1) / 5 + 1 => {
                        problems.push(format!(
                            "contract `{}`: `{m}` is a guardian or lives deeper than level {}",
                            c.id, c.depth
                        ));
                    }
                    Some(_) => {}
                }
            }
            if matches!(c.kind, ContractKind::Heirloom | ContractKind::Rescue)
                && c.room.is_some_and(|k| {
                    !matches!(
                        k,
                        RoomKind::Vault
                            | RoomKind::Forge
                            | RoomKind::Flooded
                            | RoomKind::Shrine
                            | RoomKind::Corridor
                    )
                })
            {
                problems.push(format!(
                    "contract `{}`: the room must be a vault, forge, flooded hall, shrine or corridor",
                    c.id
                ));
            }
            if let Some(r) = &c.recipe
                && self.recipe(r).is_none()
            {
                problems.push(format!("contract `{}`: unknown recipe `{r}`", c.id));
            }
            if let Some(r) = &c.core
                && self.core(r).is_none()
            {
                problems.push(format!("contract `{}`: unknown core `{r}`", c.id));
            }
            for need in &c.need {
                self.check_need("contract", &c.id, need, problems);
            }
        }
        for c in &self.town.companion {
            for s in &c.steps {
                for e in &s.effects {
                    if let Effect::Move { id } = e
                        && self.mv(id).is_none()
                    {
                        problems.push(format!("companion `{}`: unknown move `{id}`", c.id));
                    }
                }
            }
        }
        for r in &self.town.rumour {
            if let RumourEffect::Boon { id } = &r.effect
                && !self.wound(id).is_some_and(|w| w.boon)
            {
                problems.push(format!("rumour `{}`: `{id}` is not a boon", r.id));
            }
        }
        for id in ["hangover", "roughed-up", "drilled"] {
            if self.wound(id).is_none() {
                problems.push(format!("the wound or boon `{id}` (the town) must exist"));
            }
        }
        for room in &self.rooms.room {
            for o in &room.option {
                for g in o.ok.iter().chain(&o.fail) {
                    match g {
                        Gain::Recipe { id } if self.recipe(id).is_none() => {
                            problems.push(format!("room `{}`: unknown recipe `{id}`", room.id));
                        }
                        Gain::Core { id } if self.core(id).is_none() => {
                            problems.push(format!("room `{}`: unknown core `{id}`", room.id));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn check_need(&self, table: &str, id: &str, need: &Need, problems: &mut Vec<String>) {
        match need {
            Need::Calling { id: c } if self.calling(c).is_none() => {
                problems.push(format!("{table} `{id}`: unknown calling `{c}`"));
            }
            Need::Node { id: n } if self.node(n).is_none() => {
                problems.push(format!("{table} `{id}`: unknown node `{n}`"));
            }
            Need::Trait { id: t } if self.trait_or_scar(t).is_none() => {
                problems.push(format!("{table} `{id}`: unknown trait `{t}`"));
            }
            _ => {}
        }
    }

    fn check_callings(&self, problems: &mut Vec<String>) {
        for c in &self.callings.calling {
            if self.weapon(&c.weapon).is_none() || self.armour(&c.armour).is_none() {
                problems.push(format!("calling `{}`: unknown weapon or armour", c.id));
            }
            for m in &c.moves {
                if self.mv(m).is_none() {
                    problems.push(format!("calling `{}`: unknown move `{m}`", c.id));
                }
            }
            let nodes: Vec<&Node> = self
                .nodes
                .node
                .iter()
                .filter(|n| n.calling == c.id)
                .collect();
            if nodes.len() != 15 {
                problems.push(format!(
                    "calling `{}`: tree has {} nodes, wants 15",
                    c.id,
                    nodes.len()
                ));
            }
            for tier in 1..=5 {
                if nodes.iter().filter(|n| n.tier == tier).count() != 3 {
                    problems.push(format!("calling `{}`: tier {tier} needs 3 nodes", c.id));
                }
            }
            if !nodes.iter().any(|n| !n.excludes.is_empty()) {
                problems.push(format!(
                    "calling `{}`: a tree needs a fork (`excludes`)",
                    c.id
                ));
            }
        }
        for n in &self.nodes.node {
            if self.calling(&n.calling).is_none() {
                problems.push(format!("node `{}`: unknown calling `{}`", n.id, n.calling));
            }
        }
    }

    fn check_wound_and_trap_links(&self, problems: &mut Vec<String>) {
        for w in &self.wounds.wound {
            if let Some(s) = &w.scar
                && self.scar(s).is_none()
            {
                problems.push(format!("wound `{}`: unknown scar `{s}`", w.id));
            }
        }
        for t in &self.bestiary.trap {
            if let Some(w) = &t.wound
                && self.wound(w).is_none()
            {
                problems.push(format!("trap `{}`: unknown wound `{w}`", t.id));
            }
        }
    }

    fn check_effect_moves(&self, problems: &mut Vec<String>) {
        let effects = self
            .nodes
            .node
            .iter()
            .flat_map(|n| n.effects.iter())
            .chain(self.traits.traits.iter().flat_map(|t| t.effects.iter()))
            .chain(self.traits.scar.iter().flat_map(|t| t.effects.iter()))
            .chain(self.wounds.wound.iter().flat_map(|w| w.effects.iter()));
        for effect in effects {
            if let Effect::Move { id } = effect
                && self.mv(id).is_none()
            {
                problems.push(format!("an effect names unknown move `{id}`"));
            }
        }
    }

    fn check_rooms(&self, problems: &mut Vec<String>) {
        for r in &self.rooms.room {
            for o in &r.option {
                for need in &o.need {
                    match need {
                        Need::Calling { id } if self.calling(id).is_none() => {
                            problems.push(format!("room `{}`: unknown calling `{id}`", r.id));
                        }
                        Need::Node { id } if self.node(id).is_none() => {
                            problems.push(format!("room `{}`: unknown node `{id}`", r.id));
                        }
                        Need::Trait { id } if self.trait_or_scar(id).is_none() => {
                            problems.push(format!("room `{}`: unknown trait `{id}`", r.id));
                        }
                        _ => {}
                    }
                }
                for gain in o.ok.iter().chain(&o.fail) {
                    if let Gain::Wound { id } = gain
                        && self.wound(id).is_none()
                    {
                        problems.push(format!("room `{}`: unknown wound `{id}`", r.id));
                    }
                }
            }
        }
        for band in 1..=5 {
            for kind in [
                RoomKind::Entry,
                RoomKind::Exit,
                RoomKind::Guardian,
                RoomKind::Shaft,
                RoomKind::Lair,
            ] {
                if !self
                    .rooms
                    .room
                    .iter()
                    .any(|r| r.kind == kind && r.bands.contains(&band))
                {
                    problems.push(format!("band {band} has no {kind:?} room template"));
                }
            }
        }
    }

    /// A calling by id.
    #[must_use]
    pub fn calling(&self, id: &str) -> Option<&Calling> {
        self.callings.calling.iter().find(|c| c.id == id)
    }

    /// A node by id.
    #[must_use]
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.node.iter().find(|n| n.id == id)
    }

    /// A trait by id.
    #[must_use]
    pub fn trait_def(&self, id: &str) -> Option<&TraitDef> {
        self.traits.traits.iter().find(|t| t.id == id)
    }

    /// A scar by id.
    #[must_use]
    pub fn scar(&self, id: &str) -> Option<&TraitDef> {
        self.traits.scar.iter().find(|t| t.id == id)
    }

    /// A trait or a scar by id.
    #[must_use]
    pub fn trait_or_scar(&self, id: &str) -> Option<&TraitDef> {
        self.trait_def(id).or_else(|| self.scar(id))
    }

    /// A move by id.
    #[must_use]
    pub fn mv(&self, id: &str) -> Option<&MoveDef> {
        self.moves.moves.iter().find(|m| m.id == id)
    }

    /// A wound by id.
    #[must_use]
    pub fn wound(&self, id: &str) -> Option<&WoundDef> {
        self.wounds.wound.iter().find(|w| w.id == id)
    }

    /// A monster by id.
    #[must_use]
    pub fn monster(&self, id: &str) -> Option<&Monster> {
        self.bestiary.monster.iter().find(|m| m.id == id)
    }

    /// A weapon by id.
    #[must_use]
    pub fn weapon(&self, id: &str) -> Option<&Weapon> {
        self.gear.weapon.iter().find(|w| w.id == id)
    }

    /// Armour by id.
    #[must_use]
    pub fn armour(&self, id: &str) -> Option<&Armour> {
        self.gear.armour.iter().find(|a| a.id == id)
    }

    /// A weapon or an armour by id, with the slot it fits.
    #[must_use]
    pub fn base_slot(&self, id: &str) -> Option<crate::content_town::Slot> {
        if self.weapon(id).is_some() {
            Some(crate::content_town::Slot::Weapon)
        } else if self.armour(id).is_some() {
            Some(crate::content_town::Slot::Armour)
        } else {
            None
        }
    }

    /// An affix by id.
    #[must_use]
    pub fn affix(&self, id: &str) -> Option<&crate::content_town::Affix> {
        self.affixes.affix.iter().find(|a| a.id == id)
    }

    /// An ember-core by id.
    #[must_use]
    pub fn core(&self, id: &str) -> Option<&crate::content_town::Core> {
        self.affixes.core.iter().find(|a| a.id == id)
    }

    /// A recipe by id.
    #[must_use]
    pub fn recipe(&self, id: &str) -> Option<&crate::content_town::Recipe> {
        self.recipes.recipe.iter().find(|a| a.id == id)
    }

    /// A town project by id.
    #[must_use]
    pub fn project(&self, id: &str) -> Option<&crate::content_town::Project> {
        self.projects.project.iter().find(|a| a.id == id)
    }

    /// A contract by id.
    #[must_use]
    pub fn contract(&self, id: &str) -> Option<&crate::content_town::Contract> {
        self.town.contract.iter().find(|a| a.id == id)
    }

    /// A companion by id.
    #[must_use]
    pub fn companion(&self, id: &str) -> Option<&crate::content_town::Companion> {
        self.town.companion.iter().find(|a| a.id == id)
    }

    /// The guardian of a band.
    #[must_use]
    pub fn guardian(&self, band: u32) -> Option<&Monster> {
        self.bestiary
            .monster
            .iter()
            .find(|m| m.guardian && m.band == band)
    }
}
