//! Expedition content as data: events (short choice scenes), foes and the groups they fight in,
//! and what each kind of tile yields when the party works it. The tables are TOML in
//! `content/`, parsed and validated at load, so a writer adds an event without touching the
//! engine. Conditions and effects are small strings (`trade:healer:2`, `gain:salt:2-5`) parsed
//! here once, so a typo fails the schema test and not a live board.

use std::fmt;

use russh_bbs_door::content::{Table, plain_text};
use serde::Deserialize;

use crate::map::{PoiKind, TileKind};
use crate::story::{Faction, Flag};
use crate::types::{Bundle, Resource, Trade};

/// Events table text: the scene files are written by area and joined here.
pub const EVENTS_TOML: &str = concat!(
    include_str!("../content/events.toml"),
    "\n",
    include_str!("../content/events-flats.toml"),
    "\n",
    include_str!("../content/events-wild.toml"),
    "\n",
    include_str!("../content/events-places.toml"),
    "\n",
    include_str!("../content/events-rare.toml"),
    "\n",
    include_str!("../content/events-quests-a.toml"),
    "\n",
    include_str!("../content/events-quests-b.toml"),
    "\n",
    include_str!("../content/events-quests-c.toml"),
);
/// Foes and groups table text.
pub const FOES_TOML: &str = include_str!("../content/foes.toml");
/// Gathering table text.
pub const GATHER_TOML: &str = include_str!("../content/gather.toml");

/// The water, in three bands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Water {
    /// Below 40.
    Low,
    /// 40 to 64.
    Mid,
    /// 65 and over.
    High,
}

impl Water {
    /// The band of a water level.
    #[must_use]
    pub const fn of(level: i32) -> Self {
        if level < 40 {
            Self::Low
        } else if level < 65 {
            Self::Mid
        } else {
            Self::High
        }
    }
}

/// The time of day on the clan's clock (a day starts at midnight).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Daypart {
    /// 22:00 to 04:00.
    Night,
    /// 04:00 to 07:00.
    Dawn,
    /// 07:00 to 18:00.
    Day,
    /// 18:00 to 22:00.
    Dusk,
}

impl Daypart {
    /// The part of the day at an absolute minute.
    #[must_use]
    pub const fn of(minute: i64) -> Self {
        let m = minute.rem_euclid(1440);
        if m < 240 || m >= 1320 {
            Self::Night
        } else if m < 420 {
            Self::Dawn
        } else if m < 1080 {
            Self::Day
        } else {
            Self::Dusk
        }
    }
}

/// How far into the season the clan is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    /// The first weeks.
    Early,
    /// The middle.
    Mid,
    /// The last weeks.
    Late,
}

impl Stage {
    /// The stage after `days` days of the season.
    #[must_use]
    pub const fn of(days: i64) -> Self {
        if days < 19 {
            Self::Early
        } else if days < 38 {
            Self::Mid
        } else {
            Self::Late
        }
    }
}

/// What a condition asks of the party and the clan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Req {
    /// A fit member of this trade with at least this level in it.
    Trade(Trade, u8),
    /// A fit member with at least this level in the skill (whatever their trade).
    Skill(Trade, u8),
    /// A fit member with this trait.
    Trait(String),
    /// A fit member carrying a scar.
    Scar,
    /// A bonded pair in the party.
    Bond,
    /// The clan has this mark in its history.
    Mark(String),
    /// The party carries at least this many of a resource.
    Carry(Resource, u32),
    /// At least this many in the party.
    Size(u8),
    /// The party has a boat.
    Boat,
    /// The clan's standing with a faction is at least this tier (0 to 4).
    Tier(Faction, u8),
    /// The clan has finished at least this many quest chains.
    Quests(u8),
    /// The clan has finished at least this many contracts this season.
    Contracts(u16),
}

/// A requirement, possibly negated (`!trait:limp`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Cond {
    /// Holds when the requirement does not.
    pub not: bool,
    /// The requirement.
    pub req: Req,
}

fn trade_of(id: &str) -> Result<Trade, String> {
    Trade::ALL
        .into_iter()
        .find(|t| t.id() == id)
        .ok_or_else(|| format!("`{id}` is not a trade"))
}

fn number<T: std::str::FromStr>(text: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("`{text}` is not a number"))
}

impl TryFrom<String> for Cond {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        let (not, body) = text
            .strip_prefix('!')
            .map_or((false, text.as_str()), |rest| (true, rest));
        let parts: Vec<&str> = body.split(':').collect();
        let req = match parts.as_slice() {
            ["trade", t] => Req::Trade(trade_of(t)?, 0),
            ["trade", t, n] => Req::Trade(trade_of(t)?, number(n)?),
            ["skill", t, n] => Req::Skill(trade_of(t)?, number(n)?),
            ["trait", id] => Req::Trait((*id).to_owned()),
            ["scar"] => Req::Scar,
            ["bond"] => Req::Bond,
            ["mark", id] => Req::Mark((*id).to_owned()),
            ["carry", r] => Req::Carry(resource_of(r)?, 1),
            ["carry", r, n] => Req::Carry(resource_of(r)?, number(n)?),
            ["size", n] => Req::Size(number(n)?),
            ["boat"] => Req::Boat,
            ["tier", f, n] => Req::Tier(
                Faction::from_id(f).ok_or_else(|| format!("`{f}` is not a faction"))?,
                number(n)?,
            ),
            ["quests", n] => Req::Quests(number(n)?),
            ["contracts", n] => Req::Contracts(number(n)?),
            _ => return Err(format!("`{text}` is not a condition")),
        };
        Ok(Self { not, req })
    }
}

fn resource_of(id: &str) -> Result<Resource, String> {
    Resource::from_id(id).ok_or_else(|| format!("`{id}` is not a resource"))
}

/// Who an effect lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    /// One member, at random.
    Any,
    /// Everyone.
    All,
    /// The first in the party.
    Lead,
    /// The (first) member of this trade, if there is one.
    Trade(Trade),
}

fn who_of(id: &str) -> Result<Who, String> {
    Ok(match id {
        "any" => Who::Any,
        "all" => Who::All,
        "lead" => Who::Lead,
        other => Who::Trade(trade_of(other)?),
    })
}

/// How an injury is rolled.
pub use crate::content::Cause;

fn cause_of(id: &str) -> Result<Cause, String> {
    Ok(match id {
        "hurt" => Cause::Hurt,
        "water" => Cause::Water,
        "sickness" => Cause::Sickness,
        "cold" => Cause::Cold,
        "fight" => Cause::Fight,
        other => return Err(format!("`{other}` is not an injury cause")),
    })
}

/// What an outcome does.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum Effect {
    /// Goods into the pack: low and high.
    Gain(Resource, u32, u32),
    /// Goods out of the pack (then the kit).
    Lose(Resource, u32),
    /// Tallies into the pack: low and high.
    Tallies(u32, u32),
    /// Tallies out of the pack.
    Pay(u32),
    /// An injury of this cause, softened by steady traits.
    Hurt(Who, Cause),
    /// A death.
    Die(Who),
    /// Remember something about the clan.
    Mark(String),
    /// Forget it.
    Unmark(String),
    /// Turns pass.
    Turns(u8),
    /// Shared hardship for every pair in the party.
    Bond(u8),
    /// Experience in a skill for the best hand at it.
    Xp(Trade, u16),
    /// The clan's morale moves.
    Morale(i8),
    /// A plan found: the clan learns a recipe.
    Recipe(String),
    /// Chart the tiles around the party.
    Chart(u8),
    /// Days off an injury.
    Heal(Who, u8),
    /// A fight with this group. It must be the last effect.
    Fight(String),
    /// Standing with a faction moves.
    Rep(Faction, i8),
    /// A change to the marsh for the whole board, sent to the world after the trip.
    World(Flag),
}

fn range(text: &str) -> Result<(u32, u32), String> {
    if let Some((lo, hi)) = text.split_once('-') {
        let (lo, hi): (u32, u32) = (number(lo)?, number(hi)?);
        if lo > hi {
            return Err(format!("`{text}` runs backwards"));
        }
        return Ok((lo, hi));
    }
    let n = number(text)?;
    Ok((n, n))
}

impl TryFrom<String> for Effect {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        let parts: Vec<&str> = text.split(':').collect();
        Ok(match parts.as_slice() {
            ["gain", r, n] => {
                let (lo, hi) = range(n)?;
                Self::Gain(resource_of(r)?, lo, hi)
            }
            ["lose", r, n] => Self::Lose(resource_of(r)?, number(n)?),
            ["tallies", n] => {
                let (lo, hi) = range(n)?;
                Self::Tallies(lo, hi)
            }
            ["pay", n] => Self::Pay(number(n)?),
            ["hurt", w, c] => Self::Hurt(who_of(w)?, cause_of(c)?),
            ["die", w] => Self::Die(who_of(w)?),
            ["mark", id] => Self::Mark((*id).to_owned()),
            ["unmark", id] => Self::Unmark((*id).to_owned()),
            ["turns", n] => Self::Turns(number(n)?),
            ["bond", n] => Self::Bond(number(n)?),
            ["xp", t, n] => Self::Xp(trade_of(t)?, number(n)?),
            ["morale", n] => Self::Morale(number(n)?),
            ["recipe", id] => Self::Recipe((*id).to_owned()),
            ["chart", n] => Self::Chart(number(n)?),
            ["heal", w, n] => Self::Heal(who_of(w)?, number(n)?),
            ["fight", g] => Self::Fight((*g).to_owned()),
            ["rep", f, n] => Self::Rep(
                Faction::from_id(f).ok_or_else(|| format!("`{f}` is not a faction"))?,
                number(n)?,
            ),
            ["world", f] => Self::World(
                Flag::from_id(f).ok_or_else(|| format!("`{f}` is not a world change"))?,
            ),
            _ => return Err(format!("`{text}` is not an effect")),
        })
    }
}

/// What happens when a choice is made.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    /// What the player reads.
    pub text: String,
    /// What changes.
    #[serde(default)]
    pub fx: Vec<Effect>,
}

/// One choice in a scene.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionDef {
    /// The button text.
    pub label: String,
    /// The party and clan must satisfy all of these for the choice to be offered.
    #[serde(default)]
    pub needs: Vec<Cond>,
    /// The skill that makes it likelier to go well.
    #[serde(default)]
    pub skill: Option<Trade>,
    /// The base chance (percent) that it goes badly. Zero means it always goes well.
    #[serde(default)]
    pub risk: u8,
    /// When it goes well.
    pub win: Outcome,
    /// When it does not (required when `risk` is above zero).
    #[serde(default)]
    pub lose: Option<Outcome>,
}

const fn weight_default() -> u32 {
    8
}

/// One scene.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventDef {
    /// Stable id.
    pub id: String,
    /// Heading.
    pub title: String,
    /// The scene.
    pub text: String,
    /// How often it is drawn, against the others that fit.
    #[serde(default = "weight_default")]
    pub weight: u32,
    /// Rare: seen at most once per clan per season.
    #[serde(default)]
    pub rare: bool,
    /// Drawn only while the party camps.
    #[serde(default)]
    pub camp: bool,
    /// Tile kinds it can happen on (empty: any).
    #[serde(default)]
    pub tiles: Vec<TileKind>,
    /// Points of interest it happens at (empty: not a place's scene).
    #[serde(default)]
    pub poi: Vec<PoiKind>,
    /// Water bands it needs (empty: any).
    #[serde(default)]
    pub water: Vec<Water>,
    /// Times of day (empty: any).
    #[serde(default)]
    pub day: Vec<Daypart>,
    /// Stages of the season (empty: any).
    #[serde(default)]
    pub stage: Vec<Stage>,
    /// All must hold for the scene to be drawn (the clan's history, the party).
    #[serde(default)]
    pub needs: Vec<Cond>,
    /// If any holds the scene is drawn three times as often.
    #[serde(default)]
    pub draws: Vec<Cond>,
    /// The choices (two to four).
    #[serde(rename = "option")]
    pub options: Vec<OptionDef>,
}

/// The events table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Events {
    /// Every event.
    #[serde(rename = "event")]
    pub entries: Vec<EventDef>,
}

/// Where a foe stands at the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RowPref {
    /// The front.
    Front,
    /// The middle.
    Middle,
    /// The back.
    Back,
}

impl RowPref {
    /// The row index: 0 front, 2 back.
    #[must_use]
    pub const fn row(self) -> u8 {
        match self {
            Self::Front => 0,
            Self::Middle => 1,
            Self::Back => 2,
        }
    }
}

/// How a foe fights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Behaviour {
    /// Hits the front rank.
    Bruiser,
    /// Goes for the weakest, whatever the rank.
    Lurker,
    /// Throws from the back at the weakest.
    Hurler,
    /// Hits the front rank and runs when hurt.
    Skittish,
    /// Weak alone; they come in numbers.
    Swarm,
}

/// One kind of creature or bandit.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoeDef {
    /// Stable id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One line.
    pub blurb: String,
    /// Hit points.
    pub hp: u8,
    /// Attack.
    pub atk: u8,
    /// Armour.
    pub def: u8,
    /// Speed.
    pub spd: u8,
    /// How hard it is to shove (1 to 10).
    pub mass: u8,
    /// Where it starts.
    pub row: RowPref,
    /// How it fights.
    pub behaviour: Behaviour,
    /// Better in the water.
    #[serde(default)]
    pub aquatic: bool,
    /// Stronger at high water.
    #[serde(default)]
    pub tidal: bool,
    /// What it leaves.
    #[serde(default)]
    pub drops: Bundle,
}

/// A set of foes that fight together.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupDef {
    /// Stable id.
    pub id: String,
    /// The line that opens the fight.
    pub intro: String,
    /// Their ids, one entry each (one to six).
    pub foes: Vec<String>,
}

/// The foes table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Foes {
    /// Every foe.
    #[serde(rename = "foe")]
    pub foes: Vec<FoeDef>,
    /// Every group.
    #[serde(rename = "group")]
    pub groups: Vec<GroupDef>,
}

/// What working a kind of tile gives.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatherDef {
    /// The tile kind.
    pub tile: TileKind,
    /// The trade that does it best.
    pub trade: Trade,
    /// What one standard hand brings in two turns.
    pub yields: Bundle,
    /// A rare find: the resource.
    #[serde(default)]
    pub find: Option<Resource>,
    /// Percent chance of the find (plus two per level of the trade).
    #[serde(default)]
    pub find_pct: u32,
    /// Percent chance each hand is hurt.
    #[serde(default)]
    pub hazard: u32,
}

/// The gathering table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gathers {
    /// Every kind of tile that can be worked.
    #[serde(rename = "gather")]
    pub entries: Vec<GatherDef>,
}

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 30 && id.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
}

fn text_ok(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max && plain_text(text)
}

fn mark_ok(id: &str) -> bool {
    id_ok(id)
}

fn check_cond(c: &Cond, who: &str, p: &mut Vec<String>) {
    match &c.req {
        Req::Mark(m) if !mark_ok(m) => p.push(format!("{who}: mark `{m}` must be a-z and -")),
        Req::Trade(_, n) | Req::Skill(_, n) if *n > 10 => {
            p.push(format!("{who}: a level is at most 10"));
        }
        Req::Size(n) if !(1..=5).contains(n) => p.push(format!("{who}: size is 1 to 5")),
        Req::Tier(_, n) if *n > 4 => p.push(format!("{who}: a tier is 0 to 4")),
        Req::Quests(n) if *n > 12 => p.push(format!("{who}: quests is at most 12")),
        _ => {}
    }
}

fn check_effect(e: &Effect, who: &str, p: &mut Vec<String>) {
    match e {
        Effect::Mark(m) | Effect::Unmark(m) if !mark_ok(m) => {
            p.push(format!("{who}: mark `{m}` must be a-z and -"));
        }
        Effect::Gain(_, lo, hi) | Effect::Tallies(lo, hi) if *hi == 0 || *hi > 60 || lo > hi => {
            p.push(format!("{who}: a gain is 1 to 60"));
        }
        Effect::Turns(n) if *n == 0 || *n > 8 => p.push(format!("{who}: turns is 1 to 8")),
        Effect::Bond(n) if *n == 0 || *n > 6 => p.push(format!("{who}: bond is 1 to 6")),
        Effect::Xp(_, n) if *n == 0 || *n > 12 => p.push(format!("{who}: xp is 1 to 12")),
        Effect::Morale(n) if n.unsigned_abs() > 15 => p.push(format!("{who}: morale is -15 to 15")),
        Effect::Chart(n) if *n == 0 || *n > 6 => p.push(format!("{who}: chart is 1 to 6")),
        Effect::Heal(_, n) if *n == 0 || *n > 10 => p.push(format!("{who}: heal is 1 to 10 days")),
        Effect::Rep(_, n) if *n == 0 || n.unsigned_abs() > 20 => {
            p.push(format!("{who}: rep is -20 to 20 and not zero"));
        }
        _ => {}
    }
}

impl Table for Events {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for e in &self.entries {
            let id = &e.id;
            if !id_ok(id) {
                p.push(format!("[event] id `{id}` must be a-z and -"));
            }
            if seen.contains(&id.as_str()) {
                p.push(format!("[event] id `{id}` is used twice"));
            }
            seen.push(id);
            if !text_ok(&e.title, 40) || !text_ok(&e.text, 360) {
                p.push(format!(
                    "[event] `{id}`: title (40) and text (360) must be plain text"
                ));
            }
            let (low, high) = if e.rare { (1, 3) } else { (2, 20) };
            if !(low..=high).contains(&e.weight) {
                p.push(format!(
                    "[event] `{id}`: weight must be {low} to {high} ({})",
                    if e.rare { "rare" } else { "common" }
                ));
            }
            if !e.poi.is_empty() && !e.tiles.is_empty() {
                p.push(format!(
                    "[event] `{id}`: a place's scene does not name tiles"
                ));
            }
            if e.camp && (!e.poi.is_empty() || !e.tiles.is_empty()) {
                p.push(format!("[event] `{id}`: a camp scene names no tile or poi"));
            }
            if !(2..=4).contains(&e.options.len()) {
                p.push(format!("[event] `{id}`: two to four options"));
            }
            if !e.options.iter().any(|o| o.needs.is_empty()) {
                p.push(format!(
                    "[event] `{id}`: at least one option must have no requirements"
                ));
            }
            for c in e.needs.iter().chain(&e.draws) {
                check_cond(c, &format!("[event] `{id}`"), &mut p);
            }
            for (i, o) in e.options.iter().enumerate() {
                let who = format!("[event] `{id}` option {}", i + 1);
                if !text_ok(&o.label, 60) {
                    p.push(format!("{who}: label (60) must be plain text"));
                }
                if o.risk > 90 {
                    p.push(format!("{who}: risk is at most 90"));
                }
                if (o.risk > 0) != o.lose.is_some() {
                    p.push(format!("{who}: `lose` goes with a risk above zero"));
                }
                if o.skill.is_some() && o.risk == 0 {
                    p.push(format!("{who}: a skill matters only with a risk"));
                }
                for c in &o.needs {
                    check_cond(c, &who, &mut p);
                }
                for (label, out) in [("win", Some(&o.win)), ("lose", o.lose.as_ref())] {
                    let Some(out) = out else { continue };
                    if !text_ok(&out.text, 260) {
                        p.push(format!("{who} {label}: text (260) must be plain text"));
                    }
                    for (n, fx) in out.fx.iter().enumerate() {
                        check_effect(fx, &format!("{who} {label}"), &mut p);
                        if matches!(fx, Effect::Fight(_)) && n + 1 != out.fx.len() {
                            p.push(format!("{who} {label}: `fight` must be the last effect"));
                        }
                    }
                }
            }
        }
        p
    }
}

impl Table for Foes {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for f in &self.foes {
            let id = &f.id;
            if !id_ok(id) || seen.contains(&id.as_str()) {
                p.push(format!("[foe] id `{id}` must be a-z and - and unique"));
            }
            seen.push(id);
            if !text_ok(&f.name, 30) || !text_ok(&f.blurb, 120) {
                p.push(format!(
                    "[foe] `{id}`: name (30) and blurb (120) must be plain text"
                ));
            }
            if !(3..=60).contains(&f.hp)
                || f.atk > 12
                || f.def > 8
                || !(1..=12).contains(&f.spd)
                || !(1..=10).contains(&f.mass)
            {
                p.push(format!(
                    "[foe] `{id}`: hp 3-60, atk <= 12, def <= 8, spd 1-12, mass 1-10"
                ));
            }
            if f.behaviour == Behaviour::Hurler && f.row == RowPref::Front {
                p.push(format!("[foe] `{id}`: a hurler does not start in front"));
            }
        }
        let mut gseen: Vec<&str> = Vec::new();
        for g in &self.groups {
            let id = &g.id;
            if !id_ok(id) || gseen.contains(&id.as_str()) {
                p.push(format!("[group] id `{id}` must be a-z and - and unique"));
            }
            gseen.push(id);
            if !text_ok(&g.intro, 200) {
                p.push(format!("[group] `{id}`: intro (200) must be plain text"));
            }
            if g.foes.is_empty() || g.foes.len() > 6 {
                p.push(format!("[group] `{id}`: one to six foes"));
            }
            for f in &g.foes {
                if !self.foes.iter().any(|x| &x.id == f) {
                    p.push(format!("[group] `{id}`: unknown foe `{f}`"));
                }
            }
        }
        if self.foes.len() < 10 {
            p.push("[foe] at least ten kinds are needed".to_owned());
        }
        p
    }
}

impl Table for Gathers {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        for g in &self.entries {
            let kind = g.tile.name();
            if g.yields.is_empty() || g.yields.tallies > 0 {
                p.push(format!("[gather] {kind}: yields goods and no tallies"));
            }
            if g.find_pct > 40 || g.hazard > 30 || (g.find.is_some() != (g.find_pct > 0)) {
                p.push(format!(
                    "[gather] {kind}: find and find_pct go together (at most 40), hazard at most 30"
                ));
            }
        }
        for (i, g) in self.entries.iter().enumerate() {
            if self.entries[..i].iter().any(|x| x.tile == g.tile) {
                p.push(format!("[gather] {} is listed twice", g.tile.name()));
            }
        }
        p
    }
}

impl fmt::Display for Water {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Low => "low",
            Self::Mid => "middling",
            Self::High => "high",
        })
    }
}

impl fmt::Display for Daypart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Night => "night",
            Self::Dawn => "dawn",
            Self::Day => "day",
            Self::Dusk => "dusk",
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn cond(text: &str) -> Result<Cond, String> {
        Cond::try_from(text.to_owned())
    }

    fn effect(text: &str) -> Result<Effect, String> {
        Effect::try_from(text.to_owned())
    }

    #[test]
    fn conditions_parse_and_bad_ones_are_named() {
        assert_eq!(
            cond("trade:healer:2").unwrap(),
            Cond {
                not: false,
                req: Req::Trade(Trade::Healer, 2)
            }
        );
        assert_eq!(
            cond("!trait:limp").unwrap(),
            Cond {
                not: true,
                req: Req::Trait("limp".into())
            }
        );
        assert_eq!(
            cond("carry:rope:2").unwrap().req,
            Req::Carry(Resource::Rope, 2)
        );
        assert_eq!(cond("scar").unwrap().req, Req::Scar);
        assert!(cond("trade:jester").unwrap_err().contains("not a trade"));
        assert!(cond("whim").is_err());
        assert!(cond("carry:gold").is_err());
        assert!(cond("skill:wader:x").is_err());
    }

    #[test]
    fn effects_parse_with_ranges_and_bad_ones_are_named() {
        assert_eq!(
            effect("gain:salt:2-5").unwrap(),
            Effect::Gain(Resource::Salt, 2, 5)
        );
        assert_eq!(
            effect("gain:iron:3").unwrap(),
            Effect::Gain(Resource::Iron, 3, 3)
        );
        assert_eq!(
            effect("hurt:healer:water").unwrap(),
            Effect::Hurt(Who::Trade(Trade::Healer), Cause::Water)
        );
        assert_eq!(
            effect("fight:eel-pair").unwrap(),
            Effect::Fight("eel-pair".into())
        );
        assert!(effect("gain:salt:5-2").unwrap_err().contains("backwards"));
        assert!(effect("hurt:any:bruise").is_err());
        assert!(effect("explode").is_err());
    }

    #[test]
    fn the_day_and_the_water_fall_into_bands() {
        assert_eq!(Daypart::of(0), Daypart::Night);
        assert_eq!(Daypart::of(5 * 60), Daypart::Dawn);
        assert_eq!(Daypart::of(12 * 60), Daypart::Day);
        assert_eq!(Daypart::of(19 * 60), Daypart::Dusk);
        assert_eq!(Daypart::of(23 * 60 + 1440 * 5), Daypart::Night);
        assert_eq!(Water::of(10), Water::Low);
        assert_eq!(Water::of(50), Water::Mid);
        assert_eq!(Water::of(80), Water::High);
        assert_eq!(Stage::of(0), Stage::Early);
        assert_eq!(Stage::of(30), Stage::Mid);
        assert_eq!(Stage::of(50), Stage::Late);
    }
}
