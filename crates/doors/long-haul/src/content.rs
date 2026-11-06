//! Content as data: goods, economies, news, hulls, modules, backgrounds and names live in the
//! TOML tables under `content/`, embedded and validated at load (`tests/content.rs` runs the
//! same check, so a bad edit fails in CI and not on a live board).

use std::collections::HashSet;

use russh_bbs_door::content::{self, ContentError, Table, plain_text};
use serde::Deserialize;

const GOODS_TOML: &str = include_str!("../content/goods.toml");
const ECONOMIES_TOML: &str = include_str!("../content/economies.toml");
const NEWS_TOML: &str = include_str!("../content/news.toml");
const SHIPS_TOML: &str = include_str!("../content/ships.toml");
const BACKGROUNDS_TOML: &str = include_str!("../content/backgrounds.toml");
const NAMES_TOML: &str = include_str!("../content/names.toml");
const CREW_TOML: &str = include_str!("../content/crew.toml");
const TRANSIT_TOML: &str = include_str!("../content/transit.toml");
const ENEMIES_TOML: &str = include_str!("../content/enemies.toml");
const DERELICTS_TOML: &str = include_str!("../content/derelicts.toml");
const JOBS_TOML: &str = include_str!("../content/jobs.toml");
const SOCIAL_TOML: &str = include_str!("../content/social.toml");
const ARCS_TOML: &str = include_str!("../content/arcs.toml");

/// The embedded tables as `(name, text)`, for the schema test.
pub const TABLES: [(&str, &str); 13] = [
    ("goods", GOODS_TOML),
    ("economies", ECONOMIES_TOML),
    ("news", NEWS_TOML),
    ("ships", SHIPS_TOML),
    ("backgrounds", BACKGROUNDS_TOML),
    ("names", NAMES_TOML),
    ("crew", CREW_TOML),
    ("transit", TRANSIT_TOML),
    ("enemies", ENEMIES_TOML),
    ("derelicts", DERELICTS_TOML),
    ("jobs", JOBS_TOML),
    ("social", SOCIAL_TOML),
    ("arcs", ARCS_TOML),
];

pub(crate) fn ident_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 24
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

pub(crate) fn text_ok(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max && plain_text(text)
}

pub(crate) fn duplicates<'a>(
    ids: impl Iterator<Item = &'a str>,
    what: &str,
    problems: &mut Vec<String>,
) {
    let mut seen = HashSet::new();
    for id in ids {
        if !ident_ok(id) {
            problems.push(format!("{what} id `{id}` must be a-z, 0-9, _ and -"));
        }
        if !seen.insert(id) {
            problems.push(format!("{what} id `{id}` is used twice"));
        }
    }
}

/// Whether a good may be carried and sold openly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoodClass {
    /// Sold at ordinary markets.
    Legal,
    /// Seized in an inspection; sold through black markets.
    Contraband,
}

/// One trade good.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Good {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Price of a ton at a balanced market, in credits.
    pub base: u32,
    /// Legal or contraband.
    pub class: GoodClass,
    /// Nightly wobble, per-mille of the market's depth.
    pub volatility: u32,
    /// Tons a size-1 station stocks at balance.
    pub depth: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GoodsTable {
    good: Vec<Good>,
}

impl Table for GoodsTable {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        duplicates(
            self.good.iter().map(|g| g.id.as_str()),
            "good",
            &mut problems,
        );
        if self.good.len() > 60 {
            problems.push("at most 60 goods (cargo lots are indexed by a byte)".into());
        }
        for g in &self.good {
            if !text_ok(&g.name, 24) {
                problems.push(format!(
                    "good `{}`: name must be 1 to 24 plain characters",
                    g.id
                ));
            }
            if g.base == 0 || g.base > 5_000 {
                problems.push(format!("good `{}`: base must be 1 to 5000", g.id));
            }
            if g.depth == 0 || g.depth > 2_000 {
                problems.push(format!("good `{}`: depth must be 1 to 2000", g.id));
            }
            if g.volatility > 200 {
                problems.push(format!(
                    "good `{}`: volatility is per-mille, at most 200",
                    g.id
                ));
            }
        }
        problems
    }
}

/// A faction, for standing. Three run stations; the Guild and the Church keep offices and
/// post jobs but own no dock (so only the first three can bar a ship from a station).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Faction {
    /// The Station League (and its navy).
    League,
    /// The Miners' Union.
    Miners,
    /// The Free Captains.
    Free,
    /// The Cartographers' Guild.
    Guild,
    /// The Quiet Church, who run the old gates and want them back.
    Church,
}

/// How many factions there are.
pub const FACTIONS: usize = 5;

impl Faction {
    /// Every faction, in save order.
    pub const ALL: [Self; FACTIONS] = [
        Self::League,
        Self::Miners,
        Self::Free,
        Self::Guild,
        Self::Church,
    ];

    /// Index into a standing array.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::League => 0,
            Self::Miners => 1,
            Self::Free => 2,
            Self::Guild => 3,
            Self::Church => 4,
        }
    }

    /// Display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::League => "Station League",
            Self::Miners => "Miners' Union",
            Self::Free => "Free Captains",
            Self::Guild => "Cartographers' Guild",
            Self::Church => "Quiet Church",
        }
    }

    /// A short name for narrow screens.
    #[must_use]
    pub const fn short(self) -> &'static str {
        match self {
            Self::League => "League",
            Self::Miners => "Union",
            Self::Free => "Captains",
            Self::Guild => "Guild",
            Self::Church => "Church",
        }
    }

    /// The faction this one is at odds with, if any: a deed that raises one lowers the other
    /// by a third as much.
    #[must_use]
    pub const fn rival(self) -> Option<Self> {
        match self {
            Self::League => Some(Self::Free),
            Self::Free => Some(Self::League),
            Self::Miners => Some(Self::Church),
            Self::Church => Some(Self::Miners),
            Self::Guild => None,
        }
    }
}

/// A station economy type.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Economy {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One-letter code for the ASCII tier.
    pub letter: String,
    /// Who runs it.
    pub faction: Faction,
    /// Inspection chance for a ship carrying contraband, per-mille.
    pub law_pm: u32,
    /// Fuel price scale, per-mille.
    pub fuel_pm: u32,
    /// How common it is.
    pub weight: u32,
    /// Goods it makes (cheap, well stocked).
    pub produces: Vec<String>,
    /// Goods it lacks (dear, short).
    pub needs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EconomiesTable {
    economy: Vec<Economy>,
}

impl Table for EconomiesTable {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        duplicates(
            self.economy.iter().map(|e| e.id.as_str()),
            "economy",
            &mut problems,
        );
        let letters: HashSet<&str> = self.economy.iter().map(|e| e.letter.as_str()).collect();
        if letters.len() != self.economy.len() {
            problems.push("economy letters must be unique".into());
        }
        for e in &self.economy {
            if !text_ok(&e.name, 24) {
                problems.push(format!(
                    "economy `{}`: name must be 1 to 24 plain characters",
                    e.id
                ));
            }
            if e.letter.chars().count() != 1 || !e.letter.chars().all(|c| c.is_ascii_uppercase()) {
                problems.push(format!("economy `{}`: letter must be one A-Z", e.id));
            }
            if e.law_pm > 1_000 || e.fuel_pm == 0 || e.fuel_pm > 3_000 || e.weight == 0 {
                problems.push(format!(
                    "economy `{}`: law_pm 0-1000, fuel_pm 1-3000, weight above 0",
                    e.id
                ));
            }
            if e.produces.len() < 3 || e.needs.len() < 3 {
                problems.push(format!(
                    "economy `{}`: needs at least 3 produced and 3 needed goods",
                    e.id
                ));
            }
        }
        problems
    }
}

/// One effect of a news event on a good's target stock.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    /// The good.
    pub good: String,
    /// Per-mille of the usual target stock (low is a shortage, high a glut).
    pub target_pm: u32,
    /// Economy ids it touches; empty means every station.
    pub only: Vec<String>,
}

/// A kind of news event.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewsDef {
    /// Stable id.
    pub id: String,
    /// Short label.
    pub kind: String,
    /// Headline template; `{station}` is replaced by the origin's name.
    pub headline: String,
    /// Economy ids it can start at; empty means any station.
    pub origin: Vec<String>,
    /// Hops it reaches.
    pub radius: u8,
    /// Nights it lasts.
    pub duration: u8,
    /// How common it is.
    pub weight: u32,
    /// Extra inspection chance in its reach, per-mille.
    pub inspection_pm: u32,
    /// What it does to markets.
    pub effects: Vec<Effect>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewsTable {
    event: Vec<NewsDef>,
}

impl Table for NewsTable {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        duplicates(
            self.event.iter().map(|e| e.id.as_str()),
            "event",
            &mut problems,
        );
        if self.event.len() > 60 {
            problems.push("at most 60 news kinds".into());
        }
        for e in &self.event {
            if !text_ok(&e.kind, 24)
                || !text_ok(&e.headline, 90)
                || !e.headline.contains("{station}")
            {
                problems.push(format!(
                    "event `{}`: kind <= 24, headline <= 90 chars with {{station}}",
                    e.id
                ));
            }
            if e.radius > 4 || e.duration == 0 || e.duration > 20 || e.weight == 0 {
                problems.push(format!(
                    "event `{}`: radius 0-4, duration 1-20, weight above 0",
                    e.id
                ));
            }
            if e.effects.is_empty() && e.inspection_pm == 0 {
                problems.push(format!("event `{}`: does nothing", e.id));
            }
            for fx in &e.effects {
                if fx.target_pm < 50 || fx.target_pm > 4_000 {
                    problems.push(format!("event `{}`: target_pm must be 50 to 4000", e.id));
                }
            }
        }
        problems
    }
}

/// A slot kind on a hull and the kind a module fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlotKind {
    /// Main drive.
    Drive,
    /// Cargo pods.
    Cargo,
    /// Fuel tanks.
    Tank,
    /// Scanners.
    Scanner,
    /// Weapons.
    Weapon,
    /// Shields.
    Shield,
    /// Crew cabins.
    Cabin,
    /// Gas refinery.
    Refinery,
    /// Survey lab.
    Lab,
}

impl SlotKind {
    /// Every kind, in a fixed order.
    pub const ALL: [Self; 9] = [
        Self::Drive,
        Self::Cargo,
        Self::Tank,
        Self::Scanner,
        Self::Weapon,
        Self::Shield,
        Self::Cabin,
        Self::Refinery,
        Self::Lab,
    ];

    /// Display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Drive => "drive",
            Self::Cargo => "cargo",
            Self::Tank => "tank",
            Self::Scanner => "scanner",
            Self::Weapon => "weapon",
            Self::Shield => "shield",
            Self::Cabin => "cabin",
            Self::Refinery => "refinery",
            Self::Lab => "lab",
        }
    }
}

/// How many slots of each kind a hull has.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slots {
    /// Drive slots.
    pub drive: u8,
    /// Cargo slots.
    pub cargo: u8,
    /// Tank slots.
    pub tank: u8,
    /// Scanner slots.
    pub scanner: u8,
    /// Weapon slots.
    pub weapon: u8,
    /// Shield slots.
    pub shield: u8,
    /// Cabin slots.
    pub cabin: u8,
    /// Refinery slots.
    pub refinery: u8,
    /// Lab slots.
    pub lab: u8,
}

impl Slots {
    /// Slots of one kind.
    #[must_use]
    pub const fn of(&self, kind: SlotKind) -> u8 {
        match kind {
            SlotKind::Drive => self.drive,
            SlotKind::Cargo => self.cargo,
            SlotKind::Tank => self.tank,
            SlotKind::Scanner => self.scanner,
            SlotKind::Weapon => self.weapon,
            SlotKind::Shield => self.shield,
            SlotKind::Cabin => self.cabin,
            SlotKind::Refinery => self.refinery,
            SlotKind::Lab => self.lab,
        }
    }
}

fn six() -> u8 {
    6
}

/// A hull class.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hull {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One-line description.
    pub blurb: String,
    /// Price in credits.
    pub price: u32,
    /// Structural points.
    pub hull_points: u16,
    /// Reactor points to hand out in a fight, before an engineer's help.
    #[serde(default = "six")]
    pub reactor: u8,
    /// Base cargo tons.
    pub cargo: u16,
    /// Base fuel capacity.
    pub fuel: u16,
    /// Drive speed scale, per-mille.
    pub speed_pm: u32,
    /// Berths built into the hull, before any cabin module (crew and passengers sleep here).
    #[serde(default)]
    pub berths: u8,
    /// Module slots.
    pub slots: Slots,
}

/// A ship module.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Module {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The slot it fills.
    pub slot: SlotKind,
    /// Grade, 1 to 3.
    pub grade: u8,
    /// Price in credits.
    pub price: u32,
    /// Drive: spans per drive-day.
    #[serde(default)]
    pub speed: u32,
    /// Drive: fuel per hundred spans.
    #[serde(default)]
    pub burn: u32,
    /// Cargo: tons added.
    #[serde(default)]
    pub cargo: u16,
    /// Tank: fuel added.
    #[serde(default)]
    pub fuel: u16,
    /// Scanner: grade.
    #[serde(default)]
    pub scan: u8,
    /// Weapon: power.
    #[serde(default)]
    pub power: u8,
    /// Weapon: the range band it works best at (`close`, `mid` or `far`; empty means `mid`).
    #[serde(default)]
    pub reach: String,
    /// Shield: percent of hazard damage absorbed.
    #[serde(default)]
    pub shield: u8,
    /// Cabin: berths.
    #[serde(default)]
    pub berths: u8,
    /// Refinery: extra fuel from skimming, per-mille.
    #[serde(default)]
    pub refine_pm: u32,
    /// Lab: extra survey value, per-mille.
    #[serde(default)]
    pub lab_pm: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShipsTable {
    hull: Vec<Hull>,
    module: Vec<Module>,
}

impl Table for ShipsTable {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        duplicates(
            self.hull.iter().map(|h| h.id.as_str()),
            "hull",
            &mut problems,
        );
        duplicates(
            self.module.iter().map(|m| m.id.as_str()),
            "module",
            &mut problems,
        );
        if self.hull.len() > 16 || self.module.len() > 120 {
            problems.push("at most 16 hulls and 120 modules".into());
        }
        for h in &self.hull {
            if !text_ok(&h.name, 28) || !text_ok(&h.blurb, 90) {
                problems.push(format!(
                    "hull `{}`: name <= 28 and blurb <= 90 plain characters",
                    h.id
                ));
            }
            if h.price == 0
                || h.hull_points == 0
                || h.fuel == 0
                || h.speed_pm == 0
                || h.slots.drive == 0
            {
                problems.push(format!(
                    "hull `{}`: price, points, fuel, speed and a drive slot are required",
                    h.id
                ));
            }
        }
        for m in &self.module {
            if !text_ok(&m.name, 28) || m.price == 0 || !(1..=3).contains(&m.grade) {
                problems.push(format!(
                    "module `{}`: name <= 28, price above 0, grade 1-3",
                    m.id
                ));
            }
            let does_something = match m.slot {
                SlotKind::Drive => m.speed > 0 && m.burn > 0,
                SlotKind::Cargo => m.cargo > 0,
                SlotKind::Tank => m.fuel > 0,
                SlotKind::Scanner => m.scan > 0,
                SlotKind::Weapon => m.power > 0,
                SlotKind::Shield => m.shield > 0 && m.shield <= 90,
                SlotKind::Cabin => m.berths > 0,
                SlotKind::Refinery => m.refine_pm > 0,
                SlotKind::Lab => m.lab_pm > 0,
            };
            if !matches!(m.reach.as_str(), "" | "close" | "mid" | "far") {
                problems.push(format!(
                    "module `{}`: reach must be close, mid or far",
                    m.id
                ));
            }
            if !does_something {
                problems.push(format!(
                    "module `{}`: a {} module must set its own effect",
                    m.id,
                    m.slot.name()
                ));
            }
        }
        if !self
            .module
            .iter()
            .any(|m| m.slot == SlotKind::Drive && m.grade == 1)
        {
            problems.push("there must be a grade-1 drive module".into());
        }
        problems
    }
}

/// A named captain's skill block.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillLevels {
    /// Trading.
    pub trade: u8,
    /// Piloting.
    pub pilot: u8,
    /// Engineering.
    pub engineer: u8,
    /// Stealth.
    pub stealth: u8,
    /// Surveying.
    pub survey: u8,
}

/// Starting standing with the factions (the Guild and the Church default to nothing).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Standing {
    /// Station League.
    pub league: i16,
    /// Miners' Union.
    pub miners: i16,
    /// Free Captains.
    pub free: i16,
    /// Cartographers' Guild.
    #[serde(default)]
    pub guild: i16,
    /// Quiet Church.
    #[serde(default)]
    pub church: i16,
}

/// The perk a background carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Perk {
    /// Cheaper fuel at League docks.
    KnownFace,
    /// Military posts: no inspection, no dock fee.
    OldCodes,
    /// Better survey sales, and a scanner to start.
    GuildContact,
    /// Dearer contraband at havens, fewer inspections.
    Fence,
}

impl Perk {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "known-face" => Some(Self::KnownFace),
            "old-codes" => Some(Self::OldCodes),
            "guild-contact" => Some(Self::GuildContact),
            "fence" => Some(Self::Fence),
            _ => None,
        }
    }
}

/// A captain's background.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Background {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One line about who they were.
    pub blurb: String,
    /// The perk's id.
    pub perk: String,
    /// What the perk does, in a sentence.
    pub perk_text: String,
    /// Starting skill levels.
    pub skills: SkillLevels,
    /// Starting credits.
    pub cash: u32,
    /// Starting loan.
    pub loan: u32,
    /// Starting standing.
    pub standing: Standing,
}

impl Background {
    /// The perk this background carries (checked at load, so always present).
    #[must_use]
    pub fn perk(&self) -> Option<Perk> {
        Perk::parse(&self.perk)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackgroundsTable {
    background: Vec<Background>,
}

impl Table for BackgroundsTable {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        duplicates(
            self.background.iter().map(|b| b.id.as_str()),
            "background",
            &mut problems,
        );
        let perks: HashSet<&str> = self.background.iter().map(|b| b.perk.as_str()).collect();
        if perks.len() != self.background.len() {
            problems.push("each background must carry a different perk".into());
        }
        if self.background.len() > 12 {
            problems.push("at most 12 backgrounds".into());
        }
        for b in &self.background {
            if Perk::parse(&b.perk).is_none() {
                problems.push(format!("background `{}`: unknown perk `{}`", b.id, b.perk));
            }
            if !text_ok(&b.name, 24) || !text_ok(&b.blurb, 90) || !text_ok(&b.perk_text, 120) {
                problems.push(format!(
                    "background `{}`: name <= 24, blurb <= 90, perk text <= 120",
                    b.id
                ));
            }
            let s = b.skills;
            if [s.trade, s.pilot, s.engineer, s.stealth, s.survey]
                .iter()
                .any(|&v| v > 3)
            {
                problems.push(format!(
                    "background `{}`: starting skills are at most 3",
                    b.id
                ));
            }
            if b.loan < 5_000 || b.loan > 40_000 || b.cash > 10_000 {
                problems.push(format!(
                    "background `{}`: loan 5000-40000, cash at most 10000",
                    b.id
                ));
            }
        }
        problems
    }
}

/// Name parts for generation.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Names {
    /// System name first halves.
    pub system_prefix: Vec<String>,
    /// System name second halves.
    pub system_suffix: Vec<String>,
    /// Station name first words.
    pub station_adjective: Vec<String>,
    /// Station nouns, mining.
    pub mining_noun: Vec<String>,
    /// Station nouns, farming.
    pub farming_noun: Vec<String>,
    /// Station nouns, industrial.
    pub industrial_noun: Vec<String>,
    /// Station nouns, research.
    pub research_noun: Vec<String>,
    /// Station nouns, haven.
    pub haven_noun: Vec<String>,
    /// Station nouns, military.
    pub military_noun: Vec<String>,
}

impl Names {
    fn lists(&self) -> [(&'static str, &Vec<String>); 9] {
        [
            ("system_prefix", &self.system_prefix),
            ("system_suffix", &self.system_suffix),
            ("station_adjective", &self.station_adjective),
            ("mining_noun", &self.mining_noun),
            ("farming_noun", &self.farming_noun),
            ("industrial_noun", &self.industrial_noun),
            ("research_noun", &self.research_noun),
            ("haven_noun", &self.haven_noun),
            ("military_noun", &self.military_noun),
        ]
    }

    /// The nouns for an economy id.
    #[must_use]
    pub fn nouns(&self, economy: &str) -> &[String] {
        match economy {
            "mining" => &self.mining_noun,
            "farming" => &self.farming_noun,
            "industrial" => &self.industrial_noun,
            "research" => &self.research_noun,
            "haven" => &self.haven_noun,
            _ => &self.military_noun,
        }
    }
}

impl Table for Names {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for (name, list) in self.lists() {
            if list.len() < 6 {
                problems.push(format!("`{name}` needs at least 6 entries"));
            }
            let unique: HashSet<&String> = list.iter().collect();
            if unique.len() != list.len() {
                problems.push(format!("`{name}` has a repeated entry"));
            }
            if list.iter().any(|s| !text_ok(s, 12) || s.contains(' ')) {
                problems.push(format!(
                    "`{name}` entries must be one plain word of 12 characters or fewer"
                ));
            }
        }
        if self.system_prefix.len() * self.system_suffix.len() < 400 {
            problems.push("system names need at least 400 combinations".into());
        }
        problems
    }
}

/// All content, loaded once.
#[derive(Debug, Clone)]
pub struct Content {
    /// Trade goods.
    pub goods: Vec<Good>,
    /// Station economies.
    pub economies: Vec<Economy>,
    /// News kinds.
    pub news: Vec<NewsDef>,
    /// Hull classes.
    pub hulls: Vec<Hull>,
    /// Modules.
    pub modules: Vec<Module>,
    /// Captain backgrounds.
    pub backgrounds: Vec<Background>,
    /// Name parts.
    pub names: Names,
    /// Crew names, goals and roles.
    pub crew: crate::crew::CrewTable,
    /// Transit events and their flags.
    pub transit: crate::transit::TransitTable,
    /// Enemy ship types.
    pub enemies: crate::combat::EnemyTable,
    /// Derelict and wreck layouts.
    pub derelicts: crate::boarding::DerelictTable,
    /// Passenger kinds and client lines.
    pub jobs: crate::contracts::JobTable,
    /// Convoy names, notes and night events; outpost names.
    pub social: crate::social::SocialTable,
    /// Story arcs, their endings, and the scenes (also appended to `transit.event`).
    pub arcs: crate::arcs::ArcTable,
}

impl Content {
    /// Load and validate the embedded tables, including the links between them.
    ///
    /// # Errors
    /// A table that does not parse or fails its checks, or a link to an id that does not exist.
    pub fn embedded() -> Result<Self, ContentError> {
        let goods: GoodsTable = content::load("goods", GOODS_TOML)?;
        let economies: EconomiesTable = content::load("economies", ECONOMIES_TOML)?;
        let news: NewsTable = content::load("news", NEWS_TOML)?;
        let ships: ShipsTable = content::load("ships", SHIPS_TOML)?;
        let backgrounds: BackgroundsTable = content::load("backgrounds", BACKGROUNDS_TOML)?;
        let names: Names = content::load("names", NAMES_TOML)?;
        let crew = content::load("crew", CREW_TOML)?;
        let mut transit: crate::transit::TransitTable = content::load("transit", TRANSIT_TOML)?;
        let enemies = content::load("enemies", ENEMIES_TOML)?;
        let derelicts = content::load("derelicts", DERELICTS_TOML)?;
        let jobs = content::load("jobs", JOBS_TOML)?;
        let social = content::load("social", SOCIAL_TOML)?;
        let arcs: crate::arcs::ArcTable = content::load("arcs", ARCS_TOML)?;
        // The arc scenes are events at the end of the table: an open scene is saved as an index,
        // so transit.toml and then arcs.toml only ever grow at their ends.
        transit.event.extend(arcs.event.iter().cloned());
        let merged = Table::validate(&transit);
        if !merged.is_empty() {
            return Err(ContentError::Invalid {
                name: "transit+arcs",
                problems: merged,
            });
        }
        let all = Self {
            goods: goods.good,
            economies: economies.economy,
            news: news.event,
            hulls: ships.hull,
            modules: ships.module,
            backgrounds: backgrounds.background,
            names,
            crew,
            transit,
            enemies,
            derelicts,
            jobs,
            social,
            arcs,
        };
        let problems = all.link_problems();
        if problems.is_empty() {
            Ok(all)
        } else {
            Err(ContentError::Invalid {
                name: "links",
                problems,
            })
        }
    }

    /// Index of a good by id.
    #[must_use]
    pub fn good_ix(&self, id: &str) -> Option<usize> {
        self.goods.iter().position(|g| g.id == id)
    }

    /// Index of an economy by id.
    #[must_use]
    pub fn economy_ix(&self, id: &str) -> Option<usize> {
        self.economies.iter().position(|e| e.id == id)
    }

    /// Index of a module by id.
    #[must_use]
    pub fn module_ix(&self, id: &str) -> Option<usize> {
        self.modules.iter().position(|m| m.id == id)
    }

    /// Index of a hull by id.
    #[must_use]
    pub fn hull_ix(&self, id: &str) -> Option<usize> {
        self.hulls.iter().position(|h| h.id == id)
    }

    fn link_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for e in &self.economies {
            for g in e.produces.iter().chain(&e.needs) {
                if self.good_ix(g).is_none() {
                    problems.push(format!("economy `{}` names unknown good `{g}`", e.id));
                }
            }
            if e.produces.iter().any(|g| e.needs.contains(g)) {
                problems.push(format!("economy `{}` both produces and needs a good", e.id));
            }
        }
        for n in &self.news {
            for o in &n.origin {
                if self.economy_ix(o).is_none() {
                    problems.push(format!("event `{}` starts at unknown economy `{o}`", n.id));
                }
            }
            for fx in &n.effects {
                if self.good_ix(&fx.good).is_none() {
                    problems.push(format!(
                        "event `{}` touches unknown good `{}`",
                        n.id, fx.good
                    ));
                }
                for o in &fx.only {
                    if self.economy_ix(o).is_none() {
                        problems.push(format!("event `{}` limits to unknown economy `{o}`", n.id));
                    }
                }
            }
        }
        for g in self.goods.iter().filter(|g| g.class == GoodClass::Legal) {
            if !self.economies.iter().any(|e| e.produces.contains(&g.id)) {
                problems.push(format!("legal good `{}` is produced by no economy", g.id));
            }
            if !self.economies.iter().any(|e| e.needs.contains(&g.id)) {
                problems.push(format!("legal good `{}` is needed by no economy", g.id));
            }
        }
        for want in [
            "mining",
            "farming",
            "industrial",
            "research",
            "haven",
            "military",
        ] {
            if self.economy_ix(want).is_none() {
                problems.push(format!("the six economies must include `{want}`"));
            }
        }
        if self.hull_ix("freighter").is_none() || self.hull_ix("courier").is_none() {
            problems.push("the `freighter` and `courier` hulls are required".into());
        }
        problems.extend(self.transit.link_problems(self));
        problems.extend(crate::arcs::link_problems(self, &self.arcs));
        for e in &self.enemies.enemy {
            if let Some(w) = &e.wreck
                && !self.derelicts.derelict.iter().any(|d| d.id == *w)
            {
                problems.push(format!("enemy `{}` has unknown wreck `{w}`", e.id));
            }
        }
        for d in &self.derelicts.derelict {
            for r in &d.rooms {
                if !r.good.is_empty() && self.good_ix(&r.good).is_none() {
                    problems.push(format!(
                        "derelict `{}` holds unknown good `{}`",
                        d.id, r.good
                    ));
                }
            }
        }
        for id in ["ion1", "pod1", "tank1", "scan1"] {
            if self.module_ix(id).is_none() {
                problems.push(format!("the starter module `{id}` is required"));
            }
        }
        problems
    }
}
