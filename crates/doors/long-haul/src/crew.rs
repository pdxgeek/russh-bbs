//! The crew: up to eight named hands with a role, a skill, a mood, and a goal of their own.
//!
//! A crew member is more than a bonus. Their role adds to the captain's own skill (see
//! [`CrewMember::bonus`]) and opens options in events, combat and boarding that a captain alone
//! does not have. Their **mood** (pay, food, danger, treatment) moves their **loyalty** every
//! day; meeting their personal **goal** makes them loyal for good (they stay, they work a point
//! harder, and they speak up for the captain), and ignoring it lets loyalty run down until they
//! desert or, with company, mutiny at the next dock. They can be hurt, scarred and lost.
//!
//! Everything here is pure data and arithmetic. Random draws come from the caller's `DoorRng`,
//! and the hiring list is a function of the season, the station and the day, so it is the same
//! for every captain who looks and cannot be re-rolled by reconnecting.

use russh_bbs_door::DoorRng;
use russh_bbs_door::content::Table;
use serde::Deserialize;

use crate::codec::{Reader, Writer};
use crate::content::{Content, Faction};
use crate::frontier::Frontier;
use crate::skills::Skill;

/// Most crew a ship can carry.
pub const MAX_CREW: usize = 8;
/// Days between wage days.
pub const WAGE_DAYS: i64 = 7;
/// Longest a crew name may be.
pub const NAME_MAX: usize = 24;
/// Loyalty at or below which a crew member may leave at a dock.
pub const RESTLESS: u8 = 15;
/// A crew member whose goal is met never falls below this.
pub const SWORN_FLOOR: u8 = 70;
/// Credits to feed one mouth for a day.
pub const RATION_COST: i64 = 6;
/// Days one provisioning covers.
pub const RATION_DAYS: i64 = 7;

/// A crew role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Flies the ship: jump economy, evasion.
    Pilot,
    /// Keeps the drive alive: wear, repairs, jury-rigging in a fight.
    Engineer,
    /// Works the guns: damage in combat, the edge in a boarding.
    Gunner,
    /// Patches the hurt: faster healing, fewer deaths.
    Medic,
    /// Reads the signals and the samples: survey value, strange finds.
    Scientist,
    /// Counts the stores and hides the cargo: rations, inspections.
    Quartermaster,
    /// Talks: trade spreads, negotiations, bluffs.
    Face,
}

impl Role {
    /// Every role, in save order.
    pub const ALL: [Self; 7] = [
        Self::Pilot,
        Self::Engineer,
        Self::Gunner,
        Self::Medic,
        Self::Scientist,
        Self::Quartermaster,
        Self::Face,
    ];

    /// Position in save order.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Pilot => 0,
            Self::Engineer => 1,
            Self::Gunner => 2,
            Self::Medic => 3,
            Self::Scientist => 4,
            Self::Quartermaster => 5,
            Self::Face => 6,
        }
    }

    /// The role at a save position.
    #[must_use]
    pub fn from_index(i: u8) -> Option<Self> {
        Self::ALL.get(usize::from(i)).copied()
    }

    /// Display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pilot => "Pilot",
            Self::Engineer => "Engineer",
            Self::Gunner => "Gunner",
            Self::Medic => "Medic",
            Self::Scientist => "Scientist",
            Self::Quartermaster => "Quartermaster",
            Self::Face => "Face",
        }
    }

    /// The captain skill this role adds to, if it has one.
    #[must_use]
    pub const fn skill(self) -> Option<Skill> {
        match self {
            Self::Pilot => Some(Skill::Pilot),
            Self::Engineer => Some(Skill::Engineer),
            Self::Scientist => Some(Skill::Survey),
            Self::Quartermaster => Some(Skill::Stealth),
            Self::Face => Some(Skill::Trade),
            Self::Gunner | Self::Medic => None,
        }
    }
}

/// What a personal goal asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalKind {
    /// Reach a particular system.
    Visit,
    /// Win this many fights while aboard.
    Fights,
    /// Chart this many new systems while aboard.
    Charts,
    /// Finish this many contracts while aboard.
    Contracts,
    /// Hold this many hundreds of credits in the purse.
    Wealth,
    /// Dock at this many different stations.
    Docks,
}

impl GoalKind {
    /// Position in save order.
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::Visit => 0,
            Self::Fights => 1,
            Self::Charts => 2,
            Self::Contracts => 3,
            Self::Wealth => 4,
            Self::Docks => 5,
        }
    }

    fn from_index(i: u8) -> Option<Self> {
        Some(match i {
            0 => Self::Visit,
            1 => Self::Fights,
            2 => Self::Charts,
            3 => Self::Contracts,
            4 => Self::Wealth,
            5 => Self::Docks,
            _ => return None,
        })
    }
}

/// A crew member's personal goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Goal {
    /// What it asks.
    pub kind: GoalKind,
    /// The target (a system id for `Visit`, a count otherwise, hundreds of credits for `Wealth`).
    pub target: u32,
    /// Progress so far.
    pub progress: u32,
    /// Which line of the goal table words it.
    pub text: u8,
}

impl Goal {
    /// Whether the target is met.
    #[must_use]
    pub const fn met(&self) -> bool {
        match self.kind {
            GoalKind::Visit => self.progress >= 1,
            _ => self.progress >= self.target,
        }
    }
}

/// A named hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrewMember {
    /// Their name.
    pub name: String,
    /// Their role.
    pub role: Role,
    /// Skill, 1 to 10.
    pub skill: u8,
    /// Times they have done their job in a scene (skills grow by use).
    pub uses: u16,
    /// Credits a week.
    pub wage: u16,
    /// Mood: pay, food, danger, treatment; each -10 to 10.
    pub mood: [i8; 4],
    /// Loyalty, 0 to 100.
    pub loyalty: u8,
    /// Their goal.
    pub goal: Goal,
    /// Whether the goal has been met (they are sworn to the ship).
    pub sworn: bool,
    /// Days until fit for duty (0 if fit).
    pub hurt_days: u8,
    /// Lasting injuries; each takes a point off their skill.
    pub scars: u8,
}

/// Index names for [`CrewMember::mood`].
pub const PAY: usize = 0;
/// Food mood.
pub const FOOD: usize = 1;
/// Danger mood.
pub const DANGER: usize = 2;
/// Treatment mood.
pub const TREATMENT: usize = 3;

impl CrewMember {
    /// Skill after scars.
    #[must_use]
    pub fn effective(&self) -> u8 {
        self.skill.saturating_sub(self.scars).max(1)
    }

    /// Whether they can work today.
    #[must_use]
    pub const fn fit(&self) -> bool {
        self.hurt_days == 0
    }

    /// What their role adds to the captain's own skill: half their skill rounded up, and a
    /// point more once sworn. An injured hand adds nothing.
    #[must_use]
    pub fn bonus(&self) -> u8 {
        if !self.fit() {
            return 0;
        }
        self.effective().div_ceil(2) + u8::from(self.sworn)
    }

    /// The sum of the four mood parts, -40 to 40.
    #[must_use]
    pub fn mood_score(&self) -> i32 {
        self.mood.iter().map(|&m| i32::from(m)).sum()
    }

    /// A word for the mood.
    #[must_use]
    pub fn mood_word(&self) -> &'static str {
        match self.mood_score() {
            i32::MIN..=-21 => "bitter",
            -20..=-6 => "sour",
            -5..=5 => "steady",
            6..=20 => "content",
            _ => "devoted",
        }
    }

    /// Whether they would walk at the next dock.
    #[must_use]
    pub fn restless(&self) -> bool {
        !self.sworn && self.loyalty <= RESTLESS
    }

    /// Count a use of their skill; true if it went up.
    pub fn train(&mut self) -> bool {
        self.uses = self.uses.saturating_add(1);
        let need = u16::from(self.skill) * 8;
        if self.skill < 10 && self.uses >= need {
            self.uses = 0;
            self.skill += 1;
            return true;
        }
        false
    }

    /// Add to a mood part, keeping it in range.
    pub fn nudge(&mut self, part: usize, by: i8) {
        if let Some(slot) = self.mood.get_mut(part) {
            *slot = slot.saturating_add(by).clamp(-10, 10);
        }
    }

    /// Change loyalty, keeping a sworn hand above the floor.
    pub fn shift_loyalty(&mut self, by: i16) {
        let floor = if self.sworn { SWORN_FLOOR } else { 0 };
        let v = (i16::from(self.loyalty) + by).clamp(i16::from(floor), 100);
        self.loyalty = u8::try_from(v).unwrap_or(0);
    }

    /// The stored form.
    pub fn write(&self, w: &mut Writer) {
        w.text(&self.name);
        w.u8(u8::try_from(self.role.index()).unwrap_or(0));
        w.u8(self.skill);
        w.u16(self.uses);
        w.u16(self.wage);
        for m in self.mood {
            w.u8(m.to_le_bytes()[0]);
        }
        w.u8(self.loyalty);
        w.u8(self.goal.kind.index());
        w.u32(self.goal.target);
        w.u32(self.goal.progress);
        w.u8(self.goal.text);
        w.u8(u8::from(self.sworn));
        w.u8(self.hurt_days);
        w.u8(self.scars);
    }

    /// Read one, or `None` if anything is out of range.
    pub fn read(r: &mut Reader<'_>, content: &Content, frontier: &Frontier) -> Option<Self> {
        let name = r.text()?;
        let role = Role::from_index(r.u8()?)?;
        let skill = r.u8()?;
        let uses = r.u16()?;
        let wage = r.u16()?;
        let mut mood = [0i8; 4];
        for slot in &mut mood {
            *slot = i8::from_le_bytes([r.u8()?]).clamp(-10, 10);
        }
        let loyalty = r.u8()?.min(100);
        let kind = GoalKind::from_index(r.u8()?)?;
        let target = r.u32()?;
        let progress = r.u32()?;
        let text = r.u8()?;
        let sworn = match r.u8()? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let hurt_days = r.u8()?;
        let scars = r.u8()?;
        if !(1..=10).contains(&skill)
            || usize::from(text) >= content.crew.goal.len()
            || (kind == GoalKind::Visit && frontier.system(u16::try_from(target).ok()?).is_none())
        {
            return None;
        }
        Some(Self {
            name,
            role,
            skill,
            uses,
            wage,
            mood,
            loyalty,
            goal: Goal {
                kind,
                target,
                progress,
                text,
            },
            sworn,
            hurt_days,
            scars: scars.min(9),
        })
    }
}

/// A goal template.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalDef {
    /// Stable id.
    pub id: String,
    /// What it asks.
    pub kind: GoalKind,
    /// How it is worded. `{target}` is the system or the count.
    pub text: String,
    /// Smallest count (ignored for `visit`).
    #[serde(default)]
    pub min: u32,
    /// Largest count (ignored for `visit`).
    #[serde(default)]
    pub max: u32,
    /// What they say when it is met.
    pub done: String,
    /// What they say when they leave with it unmet.
    pub leave: String,
}

/// A role's blurb.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleDef {
    /// Which role.
    pub role: Role,
    /// One line of what they do for the ship.
    pub blurb: String,
    /// Weekly wage before skill, credits.
    pub wage: u16,
}

/// The crew tables.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrewTable {
    /// First names.
    pub first: Vec<String>,
    /// Family names.
    pub last: Vec<String>,
    /// Goal templates.
    pub goal: Vec<GoalDef>,
    /// Role blurbs.
    pub role: Vec<RoleDef>,
}

impl Table for CrewTable {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        if self.first.len() < 12 || self.last.len() < 12 {
            p.push("at least 12 first and 12 family names".into());
        }
        for n in self.first.iter().chain(&self.last) {
            if !crate::content::text_ok(n, 12) {
                p.push(format!(
                    "crew name part `{n}` must be 1 to 12 plain characters"
                ));
            }
        }
        if self.goal.len() < 8 || self.goal.len() > 64 {
            p.push("between 8 and 64 goals".into());
        }
        crate::content::duplicates(self.goal.iter().map(|g| g.id.as_str()), "goal", &mut p);
        for g in &self.goal {
            if !crate::content::text_ok(&g.text, 80)
                || !crate::content::text_ok(&g.done, 100)
                || !crate::content::text_ok(&g.leave, 100)
            {
                p.push(format!(
                    "goal `{}`: text <= 80, done and leave <= 100",
                    g.id
                ));
            }
            if g.kind != GoalKind::Visit && (g.min == 0 || g.max < g.min || g.max > 100) {
                p.push(format!("goal `{}`: needs 1 <= min <= max <= 100", g.id));
            }
            if g.kind == GoalKind::Visit && !g.text.contains("{target}") {
                p.push(format!("goal `{}`: a visit goal must say {{target}}", g.id));
            }
        }
        for r in Role::ALL {
            let n = self.role.iter().filter(|d| d.role == r).count();
            if n != 1 {
                p.push(format!("role {} must have exactly one entry", r.name()));
            }
        }
        for d in &self.role {
            if !crate::content::text_ok(&d.blurb, 90) || d.wage == 0 || d.wage > 400 {
                p.push(format!(
                    "role {}: blurb <= 90, wage 1 to 400",
                    d.role.name()
                ));
            }
        }
        p
    }
}

/// Who a station's recruits tend to be, by economy id: roles listed are three times as likely.
fn favoured(economy: &str) -> &'static [Role] {
    match economy {
        "mining" => &[Role::Engineer, Role::Pilot],
        "farming" => &[Role::Quartermaster, Role::Medic],
        "industrial" => &[Role::Engineer, Role::Gunner],
        "research" => &[Role::Scientist, Role::Medic],
        "haven" => &[Role::Gunner, Role::Pilot, Role::Face],
        "military" => &[Role::Gunner, Role::Pilot],
        _ => &[],
    }
}

/// The days between new faces at a dock.
pub const RECRUIT_EPOCH_DAYS: i64 = 3;
/// Recruits on offer at once.
pub const RECRUITS: usize = 3;

/// The key that marks a recruit as taken (so the same face is not hired twice in an epoch).
#[must_use]
pub fn recruit_key(station: u16, epoch: i64, slot: u8) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in b"cr"
        .iter()
        .copied()
        .chain(station.to_le_bytes())
        .chain(epoch.to_le_bytes())
        .chain([slot])
    {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// The epoch a day falls in.
#[must_use]
pub const fn epoch_of(day: i64) -> i64 {
    day.div_euclid(RECRUIT_EPOCH_DAYS)
}

/// The recruits looking for a berth at a station on a day. A pure function of the season,
/// the station and the epoch; `standing` is the captain's standing with the station's faction
/// (a good name brings better hands).
#[must_use]
pub fn recruits(
    content: &Content,
    frontier: &Frontier,
    station: u16,
    day: i64,
    standing: i16,
) -> Vec<CrewMember> {
    let Some(st) = frontier.station(station) else {
        return Vec::new();
    };
    let eco = &content.economies[usize::from(st.economy)];
    let epoch = epoch_of(day);
    let mut rng = DoorRng::from_label(&format!("lh-recruits-{}-{station}", frontier.season), epoch);
    let like = favoured(&eco.id);
    (0..RECRUITS)
        .map(|_| {
            let weights: Vec<u32> = Role::ALL
                .iter()
                .map(|r| if like.contains(r) { 3 } else { 1 })
                .collect();
            let total: u32 = weights.iter().sum();
            let mut pick = rng.below(total);
            let mut role = Role::Pilot;
            for (r, w) in Role::ALL.iter().zip(&weights) {
                if pick < *w {
                    role = *r;
                    break;
                }
                pick -= w;
            }
            let mut skill = 1 + rng.below(4) + u32::from(st.size > 2);
            if standing >= 25 {
                skill += 1;
            }
            let skill = u8::try_from(skill.min(6)).unwrap_or(1);
            let first = &content.crew.first[rng.below(len32(content.crew.first.len())) as usize];
            let last = &content.crew.last[rng.below(len32(content.crew.last.len())) as usize];
            let base = content
                .crew
                .role
                .iter()
                .find(|d| d.role == role)
                .map_or(60, |d| d.wage);
            let goal_ix = rng.below(len32(content.crew.goal.len())) as usize;
            let def = &content.crew.goal[goal_ix];
            let target = match def.kind {
                GoalKind::Visit => {
                    let systems = len32(frontier.systems.len()).max(1);
                    // Somewhere that is not where they are standing.
                    let mut sys = rng.below(systems);
                    if sys == u32::from(st.system) {
                        sys = (sys + 1) % systems;
                    }
                    sys
                }
                _ => rng.range(def.min, def.max),
            };
            CrewMember {
                name: format!("{first} {last}"),
                role,
                skill,
                uses: 0,
                wage: base + u16::from(skill) * 12,
                mood: [0; 4],
                loyalty: 40 + u8::try_from(rng.below(15)).unwrap_or(0),
                goal: Goal {
                    kind: def.kind,
                    target,
                    progress: 0,
                    text: u8::try_from(goal_ix).unwrap_or(0),
                },
                sworn: false,
                hurt_days: 0,
                scars: 0,
            }
        })
        .collect()
}

fn len32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl CrewMember {
    /// The goal in words.
    #[must_use]
    pub fn goal_text(&self, content: &Content, frontier: &Frontier) -> String {
        let Some(def) = content.crew.goal.get(usize::from(self.goal.text)) else {
            return String::new();
        };
        let target = match self.goal.kind {
            GoalKind::Visit => frontier
                .system(u16::try_from(self.goal.target).unwrap_or(0))
                .map_or_else(|| "somewhere".to_owned(), |s| s.name.clone()),
            _ => self.goal.target.to_string(),
        };
        def.text.replace("{target}", &target)
    }

    /// Progress in words, such as `2/3`.
    #[must_use]
    pub fn goal_progress(&self) -> String {
        match self.goal.kind {
            GoalKind::Visit => format!("{}/1", self.goal.progress.min(1)),
            _ => format!(
                "{}/{}",
                self.goal.progress.min(self.goal.target),
                self.goal.target
            ),
        }
    }
}

/// The standing change a faction's recruit pool reacts to: recruits refuse a captain this bad.
pub const RECRUIT_REFUSAL: i16 = -25;

/// Which faction's recruits a station offers.
#[must_use]
pub fn recruit_faction(content: &Content, frontier: &Frontier, station: u16) -> Faction {
    frontier.station(station).map_or(Faction::League, |s| {
        content.economies[usize::from(s.economy)].faction
    })
}

/// A stowaway or a rescued survivor who signs on: green, cheap and not yet sure of the ship.
#[must_use]
pub fn stowaway(
    content: &Content,
    frontier: &Frontier,
    role: Role,
    rng: &mut DoorRng,
) -> CrewMember {
    let first = &content.crew.first[rng.below(len32(content.crew.first.len())) as usize];
    let last = &content.crew.last[rng.below(len32(content.crew.last.len())) as usize];
    let goal_ix = rng.below(len32(content.crew.goal.len())) as usize;
    let def = &content.crew.goal[goal_ix];
    let target = match def.kind {
        GoalKind::Visit => rng.below(len32(frontier.systems.len()).max(1)),
        _ => rng.range(def.min, def.max),
    };
    let skill = u8::try_from(1 + rng.below(3)).unwrap_or(1);
    let base = content
        .crew
        .role
        .iter()
        .find(|d| d.role == role)
        .map_or(60, |d| d.wage);
    CrewMember {
        name: format!("{first} {last}"),
        role,
        skill,
        uses: 0,
        wage: base / 2 + u16::from(skill) * 8,
        mood: [0; 4],
        loyalty: 30,
        goal: Goal {
            kind: def.kind,
            target,
            progress: 0,
            text: u8::try_from(goal_ix).unwrap_or(0),
        },
        sworn: false,
        hurt_days: 0,
        scars: 0,
    }
}
