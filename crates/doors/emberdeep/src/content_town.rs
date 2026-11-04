//! The town slice's content as data (M4-E2): affixes and ember-cores, recipes, town projects,
//! tavern contracts, companions and rumours. Like the engine tables these are TOML embedded in
//! the crate and validated at load; [`crate::content::Content::cross_check`] proves the ids one
//! table names exist in the table it names.
//!
//! Everything here is built from the engine's closed sets ([`Effect`], [`Need`]), so a table can
//! combine them freely but cannot invent a new rule.

use russh_bbs_door::content::{Table, plain_text};
use serde::Deserialize;

use crate::content::{
    Attr, Effect, Need, RoomKind, check_effects, check_ids, check_text, id_ok, text_ok,
};

/// The affixes and ember-cores table.
pub const AFFIXES_TOML: &str = include_str!("../content/affixes.toml");
/// The recipes table.
pub const RECIPES_TOML: &str = include_str!("../content/recipes.toml");
/// The town projects table.
pub const PROJECTS_TOML: &str = include_str!("../content/projects.toml");
/// The contracts, companions and rumours table.
pub const TOWN_TOML: &str = include_str!("../content/town.toml");

/// Where a piece of gear is worn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Slot {
    /// The hand.
    Weapon,
    /// The body.
    Armour,
    /// Either (affixes only).
    Any,
}

impl Slot {
    /// Whether something made for `self` fits the `worn` slot.
    #[must_use]
    pub fn fits(self, worn: Self) -> bool {
        self == Self::Any || self == worn
    }
}

/// A modifier a piece of gear can carry. Its effects fold into the hero's derived sheet exactly
/// as a trait's do, so an affix can change numbers, grant a capability tag (and so open room
/// options) or a combat move.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Affix {
    /// Unique id.
    pub id: String,
    /// The word put before the item's name.
    pub name: String,
    /// Which gear can carry it.
    pub slot: Slot,
    /// The lowest depth band it turns up in.
    pub band: u32,
    /// What it does.
    pub effects: Vec<Effect>,
}

/// An ember-core: slotted into a piece of gear, it changes how the gear behaves.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Core {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// One line.
    pub text: String,
    /// What it can be slotted into.
    pub slot: Slot,
    /// The lowest depth band it turns up in.
    pub band: u32,
    /// What the market asks for it (0: not sold).
    #[serde(default)]
    pub price: u32,
    /// What it does while slotted.
    pub effects: Vec<Effect>,
}

/// The affixes and cores.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Affixes {
    /// Affixes.
    pub affix: Vec<Affix>,
    /// Ember-cores.
    pub core: Vec<Core>,
}

/// What a recipe makes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Makes {
    /// A weapon or armour base, with fixed affixes.
    Item {
        /// The base id (a weapon or an armour).
        base: String,
        /// Affixes it comes with.
        #[serde(default)]
        affixes: Vec<String>,
    },
    /// An ember-core.
    Core {
        /// The core id.
        id: String,
    },
}

/// How a hero comes to know a recipe.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Learn {
    /// Every hero of this calling knows it from the start.
    Calling {
        /// The calling id.
        id: String,
    },
    /// Taking this skill node teaches it.
    Node {
        /// The node id.
        id: String,
    },
    /// Found below (a room option or a contract hands it over).
    Found,
    /// Read out of a relic once the town has a library.
    Library,
}

/// A recipe the forge can follow.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// One line.
    pub text: String,
    /// What it makes.
    pub makes: Makes,
    /// Salvage from the town store.
    pub salvage: u32,
    /// Coin.
    pub coin: u32,
    /// The forge level it needs (1 is the starting forge).
    pub forge: u32,
    /// How it is learned.
    pub learn: Learn,
    /// Conditions on the crafter (for example a capability tag).
    #[serde(default)]
    pub need: Vec<Need>,
}

/// The recipes.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipes {
    /// Recipes.
    pub recipe: Vec<Recipe>,
}

/// What a finished town project changes for everyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectEffect {
    /// A better forge: cheaper repairs, higher-level recipes.
    Forge {
        /// The forge level it brings the town to.
        level: u32,
    },
    /// A faster temple: no turns lost being tended, cheaper rites.
    Temple {
        /// The temple level.
        level: u32,
    },
    /// A bigger market: more stock, better stock.
    Market {
        /// The market level.
        level: u32,
    },
    /// Guild halls that teach more.
    Guild {
        /// The guild level.
        level: u32,
    },
    /// A library that reads relics.
    Library,
    /// A lift from the town to a deeper level.
    Lift {
        /// The level it reaches.
        depth: u8,
    },
    /// A reliquary that keeps a fallen hero's weapon.
    Reliquary,
}

/// A shared town project: players donate coin and salvage toward it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// One line.
    pub text: String,
    /// Coin it needs in all.
    pub coin: u32,
    /// Salvage it needs in all.
    pub salvage: u32,
    /// Projects that must be finished first.
    #[serde(default)]
    pub requires: Vec<String>,
    /// What it changes.
    pub effect: ProjectEffect,
    /// The line the tavern board prints when it is done.
    pub done_text: String,
}

/// The projects.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projects {
    /// Projects.
    pub project: Vec<Project>,
}

/// What a contract asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContractKind {
    /// Kill a named beast (a monster of a kind, at a depth).
    Beast,
    /// Recover an heirloom (reach a kind of room at a depth).
    Heirloom,
    /// Bring a lost soul home (reach a kind of room, then carry them up).
    Rescue,
}

impl ContractKind {
    /// The word on the board.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Beast => "Beast",
            Self::Heirloom => "Heirloom",
            Self::Rescue => "Rescue",
        }
    }
}

/// A tavern contract.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    /// Unique id.
    pub id: String,
    /// The line on the board.
    pub name: String,
    /// What the poster says.
    pub text: String,
    /// What it asks.
    pub kind: ContractKind,
    /// The level it must be done at, or deeper.
    pub depth: u32,
    /// The monster to kill (beast contracts).
    #[serde(default)]
    pub monster: Option<String>,
    /// The kind of room to reach (heirloom and rescue contracts).
    #[serde(default)]
    pub room: Option<RoomKind>,
    /// Who the board offers it to (every condition must hold).
    #[serde(default)]
    pub need: Vec<Need>,
    /// Coin paid.
    pub coin: u32,
    /// Experience paid.
    pub xp: u32,
    /// Days the poster waits (including the day it is taken).
    pub days: u32,
    /// Whether the soul brought home slows the climb by a turn.
    #[serde(default)]
    pub escort: bool,
    /// A recipe taught as part of the pay.
    #[serde(default)]
    pub recipe: Option<String>,
    /// An ember-core given as part of the pay.
    #[serde(default)]
    pub core: Option<String>,
    /// What the hero reads on turning it in.
    pub done_text: String,
}

/// One rung of a companion's thread.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanionStep {
    /// What the bond is called at this rung.
    pub name: String,
    /// What the hero reads on reaching it.
    pub text: String,
    /// What the companion gives the hero from now on.
    pub effects: Vec<Effect>,
}

/// A companion the hero may court. A hero keeps one at a time.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Companion {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// One line about them.
    pub text: String,
    /// The attribute courting them is rolled on.
    pub attr: Attr,
    /// The number to reach.
    pub dc: u32,
    /// Coin a day's courting costs.
    pub gift: u32,
    /// The thread: three rungs.
    pub steps: Vec<CompanionStep>,
}

/// What a rumour does.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RumourEffect {
    /// A boon for a few days (a row of the wounds table marked as a boon).
    Boon {
        /// The boon id.
        id: String,
    },
    /// A little coin changes hands.
    Coin {
        /// How much.
        amount: u32,
    },
    /// The next descent to this level finds the whole map open.
    Tip {
        /// The level.
        depth: u8,
    },
}

/// Something overheard at the tavern.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rumour {
    /// Unique id.
    pub id: String,
    /// What is said.
    pub text: String,
    /// What it does.
    pub effect: RumourEffect,
}

/// Contracts, companions and rumours.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TownTables {
    /// The contract pool.
    pub contract: Vec<Contract>,
    /// The people to court.
    pub companion: Vec<Companion>,
    /// What is overheard.
    pub rumour: Vec<Rumour>,
}

fn name_ok(table: &str, id: &str, name: &str, problems: &mut Vec<String>) {
    if name.trim().is_empty() || name.chars().count() > 40 || !plain_text(name) {
        problems.push(format!("[{table}] `{id}`: bad name"));
    }
}

impl Table for Affixes {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids("affix", self.affix.iter().map(|a| a.id.as_str()), &mut problems);
        check_ids("core", self.core.iter().map(|a| a.id.as_str()), &mut problems);
        for a in &self.affix {
            check_effects("affix", &a.id, &a.effects, &mut problems);
            name_ok("affix", &a.id, &a.name, &mut problems);
            if !(1..=5).contains(&a.band) {
                problems.push(format!("[affix] `{}`: band must be 1 to 5", a.id));
            }
        }
        for c in &self.core {
            check_effects("core", &c.id, &c.effects, &mut problems);
            name_ok("core", &c.id, &c.name, &mut problems);
            check_text("core", &c.id, &c.text, &mut problems);
            if !(1..=5).contains(&c.band) {
                problems.push(format!("[core] `{}`: band must be 1 to 5", c.id));
            }
            if c.slot == Slot::Any {
                problems.push(format!("[core] `{}`: a core fits weapon or armour", c.id));
            }
        }
        problems
    }
}

impl Table for Recipes {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids("recipe", self.recipe.iter().map(|r| r.id.as_str()), &mut problems);
        for r in &self.recipe {
            name_ok("recipe", &r.id, &r.name, &mut problems);
            check_text("recipe", &r.id, &r.text, &mut problems);
            if !(1..=3).contains(&r.forge) {
                problems.push(format!("[recipe] `{}`: forge level must be 1 to 3", r.id));
            }
            if r.salvage == 0 && r.coin == 0 {
                problems.push(format!("[recipe] `{}`: must cost something", r.id));
            }
        }
        problems
    }
}

impl Table for Projects {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids("project", self.project.iter().map(|p| p.id.as_str()), &mut problems);
        for p in &self.project {
            name_ok("project", &p.id, &p.name, &mut problems);
            check_text("project", &p.id, &p.text, &mut problems);
            check_text("project", &p.id, &p.done_text, &mut problems);
            if p.coin == 0 && p.salvage == 0 {
                problems.push(format!("[project] `{}`: must cost something", p.id));
            }
            if p.requires.iter().any(|r| *r == p.id) {
                problems.push(format!("[project] `{}`: requires itself", p.id));
            }
            if let ProjectEffect::Lift { depth } = p.effect
                && !(2..=25).contains(&depth)
            {
                problems.push(format!("[project] `{}`: a lift goes to level 2 to 25", p.id));
            }
        }
        problems
    }
}

impl Table for TownTables {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids("contract", self.contract.iter().map(|c| c.id.as_str()), &mut problems);
        check_ids("companion", self.companion.iter().map(|c| c.id.as_str()), &mut problems);
        check_ids("rumour", self.rumour.iter().map(|c| c.id.as_str()), &mut problems);
        for c in &self.contract {
            name_ok("contract", &c.id, &c.name, &mut problems);
            check_text("contract", &c.id, &c.text, &mut problems);
            check_text("contract", &c.id, &c.done_text, &mut problems);
            if !(1..=25).contains(&c.depth) {
                problems.push(format!("[contract] `{}`: depth must be 1 to 25", c.id));
            }
            if !(1..=7).contains(&c.days) {
                problems.push(format!("[contract] `{}`: days must be 1 to 7", c.id));
            }
            match c.kind {
                ContractKind::Beast => {
                    if c.monster.is_none() || c.room.is_some() {
                        problems.push(format!("[contract] `{}`: a beast names a monster only", c.id));
                    }
                }
                ContractKind::Heirloom | ContractKind::Rescue => {
                    if c.room.is_none() || c.monster.is_some() {
                        problems.push(format!("[contract] `{}`: it names a room kind only", c.id));
                    }
                }
            }
            if c.escort && c.kind != ContractKind::Rescue {
                problems.push(format!("[contract] `{}`: only a rescue escorts", c.id));
            }
        }
        for c in &self.companion {
            name_ok("companion", &c.id, &c.name, &mut problems);
            check_text("companion", &c.id, &c.text, &mut problems);
            if c.steps.len() != 3 {
                problems.push(format!("[companion] `{}`: a thread has three rungs", c.id));
            }
            for s in &c.steps {
                name_ok("companion", &c.id, &s.name, &mut problems);
                check_text("companion", &c.id, &s.text, &mut problems);
                check_effects("companion", &c.id, &s.effects, &mut problems);
            }
            if c.gift == 0 || !(6..=18).contains(&c.dc) {
                problems.push(format!("[companion] `{}`: gift above 0, dc 6 to 18", c.id));
            }
        }
        for r in &self.rumour {
            check_text("rumour", &r.id, &r.text, &mut problems);
            if let RumourEffect::Tip { depth } = r.effect
                && !(1..=25).contains(&depth)
            {
                problems.push(format!("[rumour] `{}`: a tip names level 1 to 25", r.id));
            }
        }
        problems
    }
}

/// Whether `id` is a well-formed content id (for cross-checks in other modules).
#[must_use]
pub fn ids_ok(ids: &[String]) -> bool {
    ids.iter().all(|i| id_ok(i))
}

/// Whether a text is well formed (re-exported for tests).
#[must_use]
pub fn text_is_ok(text: &str) -> bool {
    text_ok(text)
}
