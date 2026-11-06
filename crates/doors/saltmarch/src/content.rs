//! Content as data: the tables in `content/*.toml`, embedded, parsed and validated at load.
//! A schema test runs the same check on every table, so a bad edit fails in CI and not on a
//! live board. [`Catalog`] is the typed, cross-checked whole that the engine reads.

use russh_bbs_door::content::{self, ContentError, Table, plain_text};
use serde::Deserialize;

use crate::lore::{
    EVENTS_TOML, Effect, EventDef, Events, FOES_TOML, FoeDef, Foes, GATHER_TOML, GatherDef,
    Gathers, GroupDef, Req,
};
use crate::map::{Band, PoiKind, TileKind};
use crate::story::{
    CONTRACTS_TOML, ContractDef, Contracts, Faction, Progress, QUESTS_TOML, QuestDef, Quests,
};
use crate::types::{Bundle, Resource, Track, Trade};

/// Buildings table text.
pub const BUILDINGS_TOML: &str = include_str!("../content/buildings.toml");
/// Recipes table text.
pub const RECIPES_TOML: &str = include_str!("../content/recipes.toml");
/// Traits table text.
pub const TRAITS_TOML: &str = include_str!("../content/traits.toml");
/// Injuries table text.
pub const INJURIES_TOML: &str = include_str!("../content/injuries.toml");
/// Points-of-interest table text.
pub const POIS_TOML: &str = include_str!("../content/pois.toml");
/// Names table text.
pub const NAMES_TOML: &str = include_str!("../content/names.toml");
/// Epitaphs table text.
pub const EPITAPHS_TOML: &str = include_str!("../content/epitaphs.toml");

const fn one() -> u8 {
    1
}

/// One building.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildingDef {
    /// Stable id (saves name buildings by it).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Which track it belongs to.
    pub track: Track,
    /// 1 to 3.
    pub tier: u8,
    /// One line of text.
    pub blurb: String,
    /// Paid when the work starts.
    #[serde(default)]
    pub cost: Bundle,
    /// Turns of labour.
    pub build_turns: u8,
    /// Buildings that must already stand.
    #[serde(default)]
    pub requires: Vec<String>,
    /// How many the hold may have.
    #[serde(default = "one")]
    pub max: u8,
    /// Extra crew housed.
    #[serde(default)]
    pub crew_cap: u8,
    /// Extra days of healing a night.
    #[serde(default)]
    pub heal_steps: u8,
    /// Morale each night.
    #[serde(default)]
    pub morale: i8,
    /// Peat burned each night.
    #[serde(default)]
    pub upkeep_peat: u8,
    /// Extra recruits at the gate each day.
    #[serde(default)]
    pub recruit: u8,
    /// Extra radius when surveying.
    #[serde(default)]
    pub survey: u8,
    /// Defence against raiders.
    #[serde(default)]
    pub defence: u8,
    /// Extra defence while the sluice has flooded the approach.
    #[serde(default)]
    pub flood_defence: u8,
    /// Nightly yield at one worker's full power.
    #[serde(default)]
    pub produce: Bundle,
    /// Eaten to make it.
    #[serde(default)]
    pub consume: Bundle,
    /// The trade that works it best.
    #[serde(default)]
    pub trade: Option<Trade>,
    /// Workers it takes.
    #[serde(default)]
    pub slots: u8,
    /// Hand work only at or below this water level.
    #[serde(default)]
    pub max_level: Option<u8>,
}

impl BuildingDef {
    /// Whether this building is a sluice (the hold's flood gate).
    #[must_use]
    pub const fn is_sluice(&self) -> bool {
        self.flood_defence > 0
    }
}

/// The buildings table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Buildings {
    /// Every building.
    #[serde(rename = "building")]
    pub entries: Vec<BuildingDef>,
}

/// How a recipe is learned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Learn {
    /// Known from the start.
    Start,
    /// Known when its workshop is built.
    Workshop,
    /// A plan found in a ruin.
    Ruin,
    /// Knowledge bought from another clan.
    Trade,
}

/// One recipe.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeDef {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The building that makes it.
    pub workshop: String,
    /// What it eats.
    pub inputs: Bundle,
    /// What it makes.
    pub output: Resource,
    /// How many.
    pub qty: u8,
    /// Turns it takes.
    pub turns: u8,
    /// The skill that improves the result.
    pub skill: Trade,
    /// How the clan learns it.
    pub learn: Learn,
}

/// The recipes table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipes {
    /// Every recipe.
    #[serde(rename = "recipe")]
    pub entries: Vec<RecipeDef>,
}

/// One trait. Percentages are relative.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraitDef {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One line of text.
    pub blurb: String,
    /// Earned from an injury, never hired with.
    #[serde(default)]
    pub scar: bool,
    /// Change to what the crew member produces.
    #[serde(default)]
    pub yield_pct: i32,
    /// Change to the chance of an accident.
    #[serde(default)]
    pub accident_pct: i32,
    /// Chance of an extra day of healing each night.
    #[serde(default)]
    pub heal_pct: i32,
    /// Change to the clan's morale each night.
    #[serde(default)]
    pub morale: i32,
    /// Share of a shift's yield pocketed.
    #[serde(default)]
    pub pocket_pct: u32,
    /// Change to the yield when the water is up.
    #[serde(default)]
    pub wet_pct: i32,
    /// Chance a shift's yield doubles.
    #[serde(default)]
    pub luck_pct: u32,
    /// Change to skill gained by training and use.
    #[serde(default)]
    pub learn_pct: i32,
    /// Tallies spent on them each night.
    #[serde(default)]
    pub upkeep_tallies: u32,
    /// Will not work at the water's edge when it is high.
    #[serde(default)]
    pub refuses_high: bool,
    /// Change to the asking price.
    #[serde(default)]
    pub hire_pct: i32,
    /// Change to attack in a skirmish.
    #[serde(default)]
    pub fight_atk: i32,
    /// Change to armour in a skirmish.
    #[serde(default)]
    pub fight_def: i32,
    /// Change to speed in a skirmish.
    #[serde(default)]
    pub fight_spd: i32,
    /// Change to hit points in a skirmish.
    #[serde(default)]
    pub fight_hp: i32,
}

/// The traits table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Traits {
    /// Every trait.
    #[serde(rename = "trait")]
    pub entries: Vec<TraitDef>,
}

/// Where the engine rolls an injury.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cause {
    /// An accident at work.
    Hurt,
    /// High water.
    Water,
    /// A hold short of food.
    Sickness,
    /// A hold short of fuel.
    Cold,
    /// A fight on an expedition.
    Fight,
}

/// One injury.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InjuryDef {
    /// Stable id.
    pub id: String,
    /// Text for "got {name}".
    pub name: String,
    /// Where it is rolled.
    pub cause: Cause,
    /// Days to heal.
    pub days: u8,
    /// Work lost while it lasts.
    pub work_pct: u32,
    /// 1 to 3.
    pub severity: u8,
    /// The trait it can scar into.
    #[serde(default)]
    pub scar: Option<String>,
    /// Chance it does.
    #[serde(default)]
    pub scar_pct: u32,
}

/// The injuries table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Injuries {
    /// Every injury.
    #[serde(rename = "injury")]
    pub entries: Vec<InjuryDef>,
}

/// One point-of-interest template.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoiTemplate {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One line of text.
    pub blurb: String,
    /// What sort of place.
    pub kind: PoiKind,
    /// Tile kinds it may stand on.
    pub tiles: Vec<TileKind>,
    /// How far from the Tidecourt.
    pub band: Band,
    /// How many to try to place.
    pub count: u8,
    /// Turn its tile into this kind.
    #[serde(default)]
    pub paints: Option<TileKind>,
    /// Rooms inside, for ruins.
    #[serde(default)]
    pub rooms: u8,
}

/// The points-of-interest table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pois {
    /// Every template.
    #[serde(rename = "poi")]
    pub entries: Vec<PoiTemplate>,
}

/// The names table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Names {
    /// First names.
    pub first: Vec<String>,
    /// Bynames.
    pub by: Vec<String>,
}

/// Why a crew member left the hall's roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EpitaphCause {
    /// Old age, a quiet end.
    Retired,
    /// Hunger.
    Starved,
    /// The marsh took them (expeditions, later slices).
    Lost,
}

/// One epitaph line.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpitaphDef {
    /// For which end.
    pub cause: EpitaphCause,
    /// The text, with `{name}` and `{trade}`.
    pub text: String,
}

/// The epitaphs table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Epitaphs {
    /// Every line.
    #[serde(rename = "epitaph")]
    pub entries: Vec<EpitaphDef>,
}

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
}

fn text_ok(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max && plain_text(text)
}

fn check_ids<'a>(table: &str, ids: impl Iterator<Item = &'a str>, problems: &mut Vec<String>) {
    let mut seen: Vec<&str> = Vec::new();
    for id in ids {
        if !id_ok(id) {
            problems.push(format!("[{table}] id `{id}` must be a-z and -"));
        }
        if seen.contains(&id) {
            problems.push(format!("[{table}] id `{id}` is used twice"));
        }
        seen.push(id);
    }
}

impl Table for Buildings {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        check_ids(
            "building",
            self.entries.iter().map(|b| b.id.as_str()),
            &mut p,
        );
        for b in &self.entries {
            let id = &b.id;
            if !text_ok(&b.name, 30) || !text_ok(&b.blurb, 100) {
                p.push(format!(
                    "[building] `{id}`: name (30) and blurb (100) must be plain text"
                ));
            }
            if !(1..=3).contains(&b.tier) {
                p.push(format!("[building] `{id}`: tier must be 1 to 3"));
            }
            if b.build_turns == 0 || b.build_turns > 12 {
                p.push(format!("[building] `{id}`: build_turns must be 1 to 12"));
            }
            if b.max == 0 {
                p.push(format!("[building] `{id}`: max must be at least 1"));
            }
            if b.cost.is_empty() {
                p.push(format!("[building] `{id}`: it must cost something"));
            }
            for r in &b.requires {
                if !self.entries.iter().any(|x| &x.id == r) {
                    p.push(format!("[building] `{id}`: requires unknown `{r}`"));
                }
                if r == id {
                    p.push(format!("[building] `{id}`: requires itself"));
                }
            }
            if !b.produce.is_empty() && (b.trade.is_none() || b.slots == 0) {
                p.push(format!(
                    "[building] `{id}`: a producer needs a trade and slots"
                ));
            }
            if !b.consume.is_empty() && b.produce.is_empty() {
                p.push(format!("[building] `{id}`: consume without produce"));
            }
            if b.slots > 0 && b.trade.is_none() {
                p.push(format!("[building] `{id}`: slots without a trade"));
            }
            if b.max_level.is_some_and(|l| !(20..=110).contains(&l)) {
                p.push(format!("[building] `{id}`: max_level must be 20 to 110"));
            }
        }
        // A requirement chain must end: walk up to the table's length from each building.
        for b in &self.entries {
            let mut frontier: Vec<&str> = b.requires.iter().map(String::as_str).collect();
            for _ in 0..self.entries.len() {
                frontier = frontier
                    .iter()
                    .flat_map(|id| {
                        self.entries
                            .iter()
                            .find(|x| x.id == *id)
                            .map(|x| x.requires.iter().map(String::as_str).collect::<Vec<_>>())
                            .unwrap_or_default()
                    })
                    .collect();
            }
            if !frontier.is_empty() {
                p.push(format!("[building] `{}`: requirements form a cycle", b.id));
            }
        }
        p
    }
}

impl Table for Recipes {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        check_ids("recipe", self.entries.iter().map(|r| r.id.as_str()), &mut p);
        for r in &self.entries {
            if !text_ok(&r.name, 30) {
                p.push(format!(
                    "[recipe] `{}`: name must be plain text up to 30",
                    r.id
                ));
            }
            if r.qty == 0 || r.turns == 0 || r.turns > 8 {
                p.push(format!(
                    "[recipe] `{}`: qty must be 1+ and turns 1 to 8",
                    r.id
                ));
            }
            if r.inputs.items.is_empty() {
                p.push(format!("[recipe] `{}`: it needs inputs", r.id));
            }
            if r.inputs.of(r.output) > 0 {
                p.push(format!("[recipe] `{}`: it eats what it makes", r.id));
            }
        }
        p
    }
}

impl Table for Traits {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        check_ids("trait", self.entries.iter().map(|t| t.id.as_str()), &mut p);
        for t in &self.entries {
            if !text_ok(&t.name, 24) || !text_ok(&t.blurb, 100) {
                p.push(format!(
                    "[trait] `{}`: name (24) and blurb (100) must be plain text",
                    t.id
                ));
            }
            if t.pocket_pct > 50 || t.luck_pct > 50 {
                p.push(format!(
                    "[trait] `{}`: pocket_pct and luck_pct are at most 50",
                    t.id
                ));
            }
            if t.accident_pct < -90 || t.heal_pct > 100 || t.heal_pct < 0 {
                p.push(format!(
                    "[trait] `{}`: accident_pct >= -90 and heal_pct 0 to 100",
                    t.id
                ));
            }
        }
        if self.entries.iter().filter(|t| !t.scar).count() < 6 {
            p.push("[trait] at least six hireable traits are needed".to_owned());
        }
        p
    }
}

impl Table for Injuries {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        check_ids("injury", self.entries.iter().map(|i| i.id.as_str()), &mut p);
        for i in &self.entries {
            if !text_ok(&i.name, 40) {
                p.push(format!(
                    "[injury] `{}`: name must be plain text up to 40",
                    i.id
                ));
            }
            if i.days == 0 || i.work_pct > 100 || !(1..=3).contains(&i.severity) || i.scar_pct > 100
            {
                p.push(format!(
                    "[injury] `{}`: days 1+, work_pct <= 100, severity 1-3, scar_pct <= 100",
                    i.id
                ));
            }
            if (i.scar.is_some()) != (i.scar_pct > 0) {
                p.push(format!(
                    "[injury] `{}`: scar and scar_pct go together",
                    i.id
                ));
            }
        }
        for cause in [
            Cause::Hurt,
            Cause::Water,
            Cause::Sickness,
            Cause::Cold,
            Cause::Fight,
        ] {
            if !self.entries.iter().any(|i| i.cause == cause) {
                p.push(format!("[injury] no injury has cause {cause:?}"));
            }
        }
        p
    }
}

impl Table for Pois {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        check_ids("poi", self.entries.iter().map(|t| t.id.as_str()), &mut p);
        for t in &self.entries {
            if !text_ok(&t.name, 30) || !text_ok(&t.blurb, 100) {
                p.push(format!(
                    "[poi] `{}`: name (30) and blurb (100) must be plain text",
                    t.id
                ));
            }
            if t.tiles.is_empty() || t.count == 0 {
                p.push(format!("[poi] `{}`: tiles and count are required", t.id));
            }
            if (t.band == Band::Deep) != t.tiles.iter().all(|k| *k == TileKind::Deep) {
                p.push(format!(
                    "[poi] `{}`: the deep band and the deep tile go together",
                    t.id
                ));
            }
            if (t.kind == PoiKind::Ruin) != (t.rooms > 0) {
                p.push(format!(
                    "[poi] `{}`: only ruins have rooms, and ruins must",
                    t.id
                ));
            }
            if t.paints
                .is_some_and(|k| !matches!(k, TileKind::DrownedTown | TileKind::Wreck))
            {
                p.push(format!(
                    "[poi] `{}`: paints must be drowned-town or wreck",
                    t.id
                ));
            }
        }
        p
    }
}

impl Table for Names {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        for (name, list) in [("first", &self.first), ("by", &self.by)] {
            if list.len() < 10 {
                p.push(format!("[names] `{name}` needs at least 10 entries"));
            }
            for n in list {
                if !text_ok(n, 12) || n.contains(' ') {
                    p.push(format!(
                        "[names] `{n}`: one plain word of up to 12 characters"
                    ));
                }
            }
        }
        p
    }
}

impl Table for Epitaphs {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        for c in [
            EpitaphCause::Retired,
            EpitaphCause::Starved,
            EpitaphCause::Lost,
        ] {
            if !self.entries.iter().any(|e| e.cause == c) {
                p.push(format!("[epitaph] none for {c:?}"));
            }
        }
        for e in &self.entries {
            if !text_ok(&e.text, 100) || !e.text.contains("{name}") {
                p.push(format!(
                    "[epitaph] `{}`: plain text up to 100 with {{name}}",
                    e.text
                ));
            }
        }
        p
    }
}

/// Every table, parsed, validated and cross-checked.
#[derive(Debug, Clone)]
pub struct Catalog {
    /// Buildings.
    pub buildings: Vec<BuildingDef>,
    /// Recipes.
    pub recipes: Vec<RecipeDef>,
    /// Traits.
    pub traits: Vec<TraitDef>,
    /// Injuries.
    pub injuries: Vec<InjuryDef>,
    /// Point-of-interest templates.
    pub pois: Vec<PoiTemplate>,
    /// Crew names.
    pub names: Names,
    /// Epitaph lines.
    pub epitaphs: Vec<EpitaphDef>,
    /// Expedition scenes.
    pub events: Vec<EventDef>,
    /// Creatures and bandits.
    pub foes: Vec<FoeDef>,
    /// Fights, as sets of foes.
    pub groups: Vec<GroupDef>,
    /// What each kind of tile yields.
    pub gathers: Vec<GatherDef>,
    /// Letter templates between clans.
    pub letters: crate::social::letters::Letters,
    /// The hand-written quest chains.
    pub quests: Vec<QuestDef>,
    /// The contract templates the faction boards draw from.
    pub contracts: Vec<ContractDef>,
}

/// The text of every table, so tests can swap one and prove a bad edit is caught.
#[derive(Debug, Clone, Copy)]
pub struct Texts<'a> {
    /// Buildings.
    pub buildings: &'a str,
    /// Recipes.
    pub recipes: &'a str,
    /// Traits.
    pub traits: &'a str,
    /// Injuries.
    pub injuries: &'a str,
    /// Points of interest.
    pub pois: &'a str,
    /// Names.
    pub names: &'a str,
    /// Epitaphs.
    pub epitaphs: &'a str,
    /// Events.
    pub events: &'a str,
    /// Foes and groups.
    pub foes: &'a str,
    /// Gathering.
    pub gather: &'a str,
    /// Letter templates.
    pub letters: &'a str,
    /// Quest chains.
    pub quests: &'a str,
    /// Contracts.
    pub contracts: &'a str,
}

impl Texts<'static> {
    /// The embedded tables.
    #[must_use]
    pub const fn embedded() -> Self {
        Self {
            buildings: BUILDINGS_TOML,
            recipes: RECIPES_TOML,
            traits: TRAITS_TOML,
            injuries: INJURIES_TOML,
            pois: POIS_TOML,
            names: NAMES_TOML,
            epitaphs: EPITAPHS_TOML,
            events: EVENTS_TOML,
            foes: FOES_TOML,
            gather: GATHER_TOML,
            letters: crate::social::letters::LETTERS_TOML,
            quests: QUESTS_TOML,
            contracts: CONTRACTS_TOML,
        }
    }
}

impl Catalog {
    /// Load the embedded tables.
    ///
    /// # Errors
    /// The first table that does not parse or validate, or a cross-table problem (a recipe whose
    /// workshop is not a building, a scar that is not a trait).
    pub fn embedded() -> Result<Self, ContentError> {
        Self::from_texts(&Texts::embedded())
    }

    /// Load tables from text (tests use this to prove a bad edit is caught).
    ///
    /// # Errors
    /// See [`Catalog::embedded`].
    pub fn from_texts(t: &Texts<'_>) -> Result<Self, ContentError> {
        let foes = content::load::<Foes>("foes", t.foes)?;
        let catalog = Self {
            buildings: content::load::<Buildings>("buildings", t.buildings)?.entries,
            recipes: content::load::<Recipes>("recipes", t.recipes)?.entries,
            traits: content::load::<Traits>("traits", t.traits)?.entries,
            injuries: content::load::<Injuries>("injuries", t.injuries)?.entries,
            pois: content::load::<Pois>("pois", t.pois)?.entries,
            names: content::load::<Names>("names", t.names)?,
            epitaphs: content::load::<Epitaphs>("epitaphs", t.epitaphs)?.entries,
            events: content::load::<Events>("events", t.events)?.entries,
            foes: foes.foes,
            groups: foes.groups,
            gathers: content::load::<Gathers>("gather", t.gather)?.entries,
            letters: content::load::<crate::social::letters::Letters>("letters", t.letters)?,
            quests: content::load::<Quests>("quests", t.quests)?.entries,
            contracts: content::load::<Contracts>("contracts", t.contracts)?.entries,
        };
        let problems = catalog.cross_check();
        if problems.is_empty() {
            Ok(catalog)
        } else {
            Err(ContentError::Invalid {
                name: "catalog",
                problems,
            })
        }
    }

    fn cross_check(&self) -> Vec<String> {
        let mut p = Vec::new();
        for r in &self.recipes {
            if self.building(&r.workshop).is_none() {
                p.push(format!(
                    "recipe `{}`: workshop `{}` is not a building",
                    r.id, r.workshop
                ));
            }
        }
        for i in &self.injuries {
            if let Some(scar) = &i.scar {
                match self.trait_def(scar) {
                    Some(t) if t.scar => {}
                    _ => p.push(format!(
                        "injury `{}`: scar `{scar}` is not a scar trait",
                        i.id
                    )),
                }
            }
        }
        if self.buildings.iter().filter(|b| b.crew_cap > 0).count() == 0 {
            p.push("no building houses crew".to_owned());
        }
        for track in Track::ALL {
            if !self
                .buildings
                .iter()
                .any(|b| b.track == track && b.tier == 1)
            {
                p.push(format!("track {} has no tier-1 building", track.name()));
            }
        }
        if !self.buildings.iter().any(BuildingDef::is_sluice) {
            p.push("no building is a sluice".to_owned());
        }
        self.cross_check_lore(&mut p);
        self.cross_check_story(&mut p);
        p
    }

    /// How many quest chains the clan (by its marks) has finished.
    #[must_use]
    pub fn quests_done(&self, has: impl Fn(&str) -> bool) -> usize {
        self.quests
            .iter()
            .filter(|q| matches!(q.progress(&has), Progress::Ended(_)))
            .count()
    }

    /// The scenes that set a mark, with whether each (event or option) needs `needed`.
    fn setters<'a>(&'a self, mark: &str) -> Vec<(&'a EventDef, Vec<&'a crate::lore::Cond>)> {
        let mut out = Vec::new();
        for e in &self.events {
            for o in &e.options {
                let sets = std::iter::once(&o.win)
                    .chain(o.lose.as_ref())
                    .flat_map(|x| &x.fx)
                    .any(|fx| matches!(fx, Effect::Mark(m) if m == mark));
                if sets {
                    out.push((e, e.needs.iter().chain(&o.needs).collect()));
                }
            }
        }
        out
    }

    /// Links between quests, marks, contracts and the places they name.
    fn cross_check_story(&self, p: &mut Vec<String>) {
        for c in &self.contracts {
            if let Some(foe) = &c.foe
                && self.foe(foe).is_none()
            {
                p.push(format!("contract `{}`: unknown foe `{foe}`", c.id));
            }
        }
        for q in &self.quests {
            let chain: Vec<&str> = q
                .steps
                .iter()
                .map(|s| s.mark.as_str())
                .chain(q.ends.iter().map(|e| e.mark.as_str()))
                .collect();
            for (i, mark) in chain.iter().enumerate() {
                let setters = self.setters(mark);
                if setters.is_empty() {
                    p.push(format!(
                        "quest `{}`: nothing sets mark `{mark}` (dead step)",
                        q.id
                    ));
                    continue;
                }
                // Past the first step, a setter must wait for the step before it (the last
                // step for an end), so a step cannot be done out of order.
                let before = if i == 0 {
                    None
                } else if i < q.steps.len() {
                    Some(q.steps[i - 1].mark.as_str())
                } else {
                    q.steps.last().map(|s| s.mark.as_str())
                };
                if let Some(before) = before
                    && !setters.iter().any(|(_, needs)| {
                        needs
                            .iter()
                            .any(|c| !c.not && matches!(&c.req, Req::Mark(m) if m == before))
                    })
                {
                    p.push(format!(
                        "quest `{}`: no scene that sets `{mark}` waits for `{before}`",
                        q.id
                    ));
                }
            }
            // Every end closes the others: some option that sets it must refuse a clan that
            // already holds another end.
            for e in &q.ends {
                let others: Vec<&str> = q
                    .ends
                    .iter()
                    .filter(|o| o.mark != e.mark)
                    .map(|o| o.mark.as_str())
                    .collect();
                let setters = self.setters(&e.mark);
                let closes = setters.iter().all(|(_, needs)| {
                    others.iter().all(|o| {
                        needs
                            .iter()
                            .any(|c| c.not && matches!(&c.req, Req::Mark(m) if m == o))
                    })
                });
                if !setters.is_empty() && !closes {
                    p.push(format!(
                        "quest `{}`: a scene that ends it with `{}` does not refuse the other ends",
                        q.id, e.mark
                    ));
                }
            }
        }
        // A mark a scene reads must be set by some scene (or by the engine).
        let engine_marks = |m: &str| m.starts_with("seen:") || m.starts_with("done:");
        for e in &self.events {
            let conds = e
                .needs
                .iter()
                .chain(&e.draws)
                .chain(e.options.iter().flat_map(|o| &o.needs));
            for c in conds {
                if let Req::Mark(m) = &c.req
                    && !engine_marks(m)
                    && self.setters(m).is_empty()
                {
                    p.push(format!("event `{}`: mark `{m}` is never set", e.id));
                }
            }
        }
        let _ = Faction::ALL;
    }

    fn cross_check_lore(&self, p: &mut Vec<String>) {
        for g in &self.groups {
            if g.foes.is_empty() {
                p.push(format!("group `{}` has no foes", g.id));
            }
        }
        for e in &self.events {
            let conds = e
                .needs
                .iter()
                .chain(&e.draws)
                .chain(e.options.iter().flat_map(|o| &o.needs));
            for c in conds {
                if let Req::Trait(t) = &c.req
                    && self.trait_def(t).is_none()
                {
                    p.push(format!("event `{}`: unknown trait `{t}`", e.id));
                }
            }
            let outcomes = e
                .options
                .iter()
                .flat_map(|o| std::iter::once(&o.win).chain(o.lose.as_ref()));
            for fx in outcomes.flat_map(|o| &o.fx) {
                match fx {
                    Effect::Fight(g) if self.group(g).is_none() => {
                        p.push(format!("event `{}`: unknown fight group `{g}`", e.id));
                    }
                    Effect::Recipe(r)
                        if self.recipe(r).is_none_or(|d| !matches!(d.learn, Learn::Ruin | Learn::Trade)) =>
                    {
                        p.push(format!(
                            "event `{}`: `{r}` is not a recipe learned from a ruin or a trade",
                            e.id
                        ));
                    }
                    _ => {}
                }
            }
        }
    }

    /// The position of an event in the table.
    #[must_use]
    pub fn event_index(&self, id: &str) -> Option<usize> {
        self.events.iter().position(|e| e.id == id)
    }

    /// The branch number of an option's outcome (two for each option, in table order:
    /// won then lost), for the telemetry in [`crate::story::Story::seen`].
    #[must_use]
    pub fn branch_bit(&self, event: usize, option: usize, lost: bool) -> usize {
        let before: usize = self.events[..event.min(self.events.len())]
            .iter()
            .map(|e| e.options.len())
            .sum();
        (before + option) * 2 + usize::from(lost)
    }

    /// How many branches the scenes have.
    #[must_use]
    pub fn branch_count(&self) -> usize {
        self.events.iter().map(|e| e.options.len()).sum::<usize>() * 2
    }

    /// A building by id.
    #[must_use]
    pub fn building(&self, id: &str) -> Option<&BuildingDef> {
        self.buildings.iter().find(|b| b.id == id)
    }

    /// A recipe by id.
    #[must_use]
    pub fn recipe(&self, id: &str) -> Option<&RecipeDef> {
        self.recipes.iter().find(|r| r.id == id)
    }

    /// A trait by id.
    #[must_use]
    pub fn trait_def(&self, id: &str) -> Option<&TraitDef> {
        self.traits.iter().find(|t| t.id == id)
    }

    /// A foe by id.
    #[must_use]
    pub fn foe(&self, id: &str) -> Option<&FoeDef> {
        self.foes.iter().find(|f| f.id == id)
    }

    /// A fight group by id.
    #[must_use]
    pub fn group(&self, id: &str) -> Option<&GroupDef> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// An event by id.
    #[must_use]
    pub fn event(&self, id: &str) -> Option<&EventDef> {
        self.events.iter().find(|e| e.id == id)
    }

    /// What working this kind of tile gives.
    #[must_use]
    pub fn gather(&self, kind: TileKind) -> Option<&GatherDef> {
        self.gathers.iter().find(|g| g.tile == kind)
    }

    /// An injury by id.
    #[must_use]
    pub fn injury(&self, id: &str) -> Option<&InjuryDef> {
        self.injuries.iter().find(|i| i.id == id)
    }
}
