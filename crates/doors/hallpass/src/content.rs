//! Content as data: every table the engine reads, embedded from `content/*.toml`, parsed with
//! `deny_unknown_fields`, and cross-checked when the door is built (and by the schema tests).
//!
//! Writers add places, characters, methods and effects here without touching the engine. The
//! tone rules live in [`Content::problems`] as checks: no target is a Stray, every text is plain
//! (nothing the sanitizer would change), and every reference resolves.

use std::collections::{HashMap, HashSet};

use russh_bbs_door::content::{self, ContentError, Table, plain_text};
use serde::Deserialize;

use crate::cond::Cond;
use crate::effect::Eff;

// ---------------------------------------------------------------- rules and consequences

/// The tunable numbers.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    /// Days in a term.
    pub term_days: i64,
    /// Where a character starts each day.
    pub start_place: String,
    /// Cash at enrolment.
    pub start_cash: i32,
    /// Allowance per school day.
    pub allowance: i32,
    /// Cool at enrolment.
    pub start_cool: i32,
    /// Nerve at enrolment.
    pub start_nerve_max: i32,
    /// Starting grade in every subject.
    pub start_grade: i32,
    /// Schemes in the planner at once.
    pub max_schemes: usize,
    /// Twists on one scheme.
    pub max_twists: usize,
    /// Hops through open places per period.
    pub moves_per_slot: usize,
    /// File points that make a file full.
    pub file_full: i32,
    /// Days of good conduct that strike one file point.
    pub clean_days: i32,
    /// The same for a teacher's pet.
    pub clean_days_pet: i32,
    /// Days of decay applied after an absence, at most.
    pub catch_up_days: i64,
    /// Heat lost per day.
    pub heat_decay: i32,
    /// Heat lost per day when lying low.
    pub heat_decay_low: i32,
    /// Days per point of notoriety lost.
    pub notoriety_decay_every: i64,
    /// Days per point of wariness lost.
    pub wary_decay_days: i64,
    /// Days within which the next scheme extends a campaign.
    pub campaign_window_days: i64,
    /// Days before an ally helps again.
    pub ally_cooldown_days: i64,
    /// How many days ahead a scheme can be timetabled.
    pub schedule_horizon_days: i64,
    /// The base chance of a phase, in percent.
    pub base_chance: i32,
    /// How far past the chance a roll may land and still be a flub, not a bust.
    pub flub_margin: i32,
    /// Chance added per point of nerve pushed.
    pub nerve_push_bonus: i32,
    /// Base chance of slipping out after lights-out.
    pub slip_out_base: i32,
    /// Base chance of a skipped class being noticed.
    pub skip_notice_base: i32,
    /// Place heat that posts a patrol.
    pub patrol_threshold: i32,
    /// Legend percent for a clean run.
    pub legend_clean_pct: i32,
    /// Legend percent for a messy run.
    pub legend_messy_pct: i32,
    /// Percent added per link in a campaign.
    pub campaign_step_pct: i32,
    /// Links that count in a campaign.
    pub campaign_max_links: i32,
    /// Experience needed for each skill level.
    pub skill_levels: Vec<i32>,
    /// Average grade for the honour roll.
    pub honour_roll: i32,
    /// Legend percent kept when a pulled-off scheme is traced back.
    pub pinned_legend_pct: i32,
    /// Puzzles in a lesson.
    pub lesson_rounds: usize,
    /// Grade change by the number of puzzles answered right (one more entry than rounds).
    pub lesson_gains: Vec<i32>,
    /// Skill experience by the number of puzzles answered right.
    pub lesson_xp: Vec<i32>,
    /// Days per point a subject's grade slips when it is not kept up (never under 30).
    pub grade_decay_days: i64,
    /// Grade under which a subject is a poor mark on Friday's report.
    pub poor_grade: i32,
    /// Clubs a character can belong to at once.
    pub max_clubs: usize,
    /// Friendship at which a character counts as a friend.
    pub friend_bond: i32,
    /// Friendship at or under which a character counts as a rival.
    pub rival_bond: i32,
    /// Friendship at or under which a character will no longer help (a betrayal).
    pub betray_bond: i32,
    /// Friendship a betrayal leaves.
    pub betray_to: i32,
    /// Ranking a grade into a puzzle level: grades under the first are level 0, under the second 1, else 2.
    pub level_grades: Vec<i32>,
}

/// What reaching a file level does.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tier {
    /// File points that trigger it.
    pub at: i32,
    /// `detention`, `privileges`, `letter` or `summons`.
    pub effect: String,
    /// How much (detentions, days).
    pub amount: i32,
    /// What the player is told.
    pub text: String,
}

/// A kind of file entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileKind {
    /// Its id.
    pub id: String,
    /// How the file words it.
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RulesFile {
    rules: Rules,
    tier: Vec<Tier>,
    file_kind: Vec<FileKind>,
}

// ---------------------------------------------------------------- timetable

/// A named period of the day.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Period {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
}

/// One weekday's activity per period.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaySpec {
    /// 0 = Monday.
    pub weekday: u8,
    /// One activity per period: `class:<subject>`, `chapel`, `lunch`, `free`, `after`, `evening`, `night`.
    pub slots: Vec<String>,
}

/// A school subject.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Where it is taught.
    pub room: String,
    /// Who teaches it.
    pub teacher: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TimetableFile {
    period: Vec<Period>,
    day: Vec<DaySpec>,
    subject: Vec<Subject>,
}

// ---------------------------------------------------------------- places

/// A description that applies to some kinds of period.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceChange {
    /// The slot kinds it applies to.
    pub kinds: Vec<String>,
    /// The description then.
    pub text: String,
}

/// A place on the map.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceSpec {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its wing.
    pub wing: String,
    /// Its usual description.
    pub look: String,
    /// Neighbouring places (edges are undirected).
    pub exits: Vec<String>,
    /// The slot kinds it is open in.
    pub hours: Vec<String>,
    /// Tags: `rest`, `study`, `dining`, `rumours`, `hide`, `detention`, `patrolled`, `noticeboard`.
    #[serde(default)]
    pub tags: Vec<String>,
    /// A shop id, if it sells things.
    #[serde(default)]
    pub shop: Option<String>,
    /// A lock label. Locked places open for anyone who meets any `unlock` condition.
    #[serde(default)]
    pub lock: Option<String>,
    /// Any of these opens the lock.
    #[serde(default)]
    pub unlock: Vec<Cond>,
    /// Descriptions that change by period.
    #[serde(default)]
    pub change: Vec<PlaceChange>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlacesFile {
    place: Vec<PlaceSpec>,
}

// ---------------------------------------------------------------- cast

/// A clique.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clique {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// One line.
    pub blurb: String,
    /// Cliques it rivals.
    pub rivals: Vec<String>,
}

/// Where a character is at certain times.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleRule {
    /// A slot kind, `class:<subject>`, `class`, or `*`.
    pub when: String,
    /// Weekdays it applies to (all if empty).
    #[serde(default)]
    pub days: Vec<u8>,
    /// Where they are.
    pub place: String,
}

/// A role a character can play in your scheme.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllySpec {
    /// `lookout`, `distraction`, `cover`, `gadget` or `keys`.
    pub role: String,
    /// What it takes to ask.
    pub needs: Vec<Cond>,
    /// The cost in cash.
    pub cost: i32,
    /// Knowledge they give you.
    #[serde(default)]
    pub grants: Vec<String>,
}

/// A named character.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Npc {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// `staff`, `prefect` or `student`.
    pub role: String,
    /// Their clique, if any.
    #[serde(default)]
    pub clique: Option<String>,
    /// Their post.
    pub title: String,
    /// Where they are when no rule matches.
    pub home: String,
    /// How watchful they are by nature, 0 to 3.
    pub watchful: i32,
    /// Whether a scheme can be pointed at them.
    pub prankable: bool,
    /// One line.
    pub blurb: String,
    /// Their timetable.
    #[serde(default)]
    pub schedule: Vec<ScheduleRule>,
    /// What they will do for you.
    #[serde(default)]
    pub ally: Vec<AllySpec>,
}

impl Npc {
    /// Whether this character keeps watch (staff and prefects).
    #[must_use]
    pub fn is_authority(&self) -> bool {
        self.role != "student"
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CastFile {
    clique: Vec<Clique>,
    npc: Vec<Npc>,
}

// ---------------------------------------------------------------- traits and items

/// A skill that grows by use.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// One line.
    pub blurb: String,
}

/// A trait.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraitSpec {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// One line.
    pub blurb: String,
    /// Whether it can be picked at enrolment.
    pub start: bool,
    /// Conditions that earn it in play.
    #[serde(default)]
    pub earn: Vec<Cond>,
    /// Capabilities it grants.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Flat skill-level bonuses.
    #[serde(default)]
    pub skills: Vec<(String, i32)>,
    /// Standing with cliques at enrolment.
    #[serde(default)]
    pub standing: Vec<(String, i32)>,
    /// Cool bonus.
    #[serde(default)]
    pub cool: i32,
    /// Nerve bonus.
    #[serde(default)]
    pub nerve: i32,
    /// Added to the chance of being traced back.
    #[serde(default)]
    pub suspicion: i32,
    /// Added to class gains.
    #[serde(default)]
    pub grade_bonus: i32,
    /// Whether the file clears faster.
    #[serde(default)]
    pub clean_days: bool,
    /// Notoriety never falls below this.
    #[serde(default)]
    pub notoriety_floor: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraitsFile {
    skill: Vec<Skill>,
    #[serde(rename = "trait")]
    traits: Vec<TraitSpec>,
}

/// Something you can buy.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The price.
    pub price: i32,
    /// The shop that sells it.
    pub shop: String,
    /// What it takes to be offered it.
    #[serde(default)]
    pub needs: Vec<Cond>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemsFile {
    item: Vec<Item>,
}

// ---------------------------------------------------------------- schemes

/// What a scheme can be pointed at.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// `staff`, `prefect`, `clique` or `school`.
    pub kind: String,
    /// The character, for staff and prefects.
    #[serde(default)]
    pub npc: Option<String>,
    /// The clique, for clique targets.
    #[serde(default)]
    pub clique: Option<String>,
    /// A fixed place, for school targets.
    #[serde(default)]
    pub place: Option<String>,
}

/// A way of making mischief.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Method {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// One line.
    pub blurb: String,
    /// Where it is done.
    pub place: String,
    /// Target kinds it suits.
    pub kinds: Vec<String>,
    /// Specific targets only.
    #[serde(default)]
    pub only: Vec<String>,
    /// `absent`, `present` or `any`.
    pub present: String,
    /// Items it uses up.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Ally roles it needs recruited.
    #[serde(default)]
    pub roles: Vec<String>,
    /// Conditions for the option to exist.
    #[serde(default)]
    pub requires: Vec<Cond>,
    /// Scouting level needed.
    #[serde(default)]
    pub scout: i32,
    /// Subtracted from every phase chance.
    pub difficulty: i32,
    /// Legend if pulled off.
    pub legend: i32,
    /// Heat it raises.
    pub heat: i32,
    /// File severity if it goes wrong.
    pub severity: i32,
    /// Whether it changes the school for everyone.
    #[serde(default)]
    pub big: bool,
    /// The world effect, for a big scheme.
    #[serde(default)]
    pub effect: Option<String>,
    /// Skills for the sneak, act and escape phases.
    pub skills: Vec<String>,
    /// The line that goes in the paper.
    pub story: String,
}

/// A modifier added to a method.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Twist {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// One line.
    pub blurb: String,
    /// Ally roles it needs.
    #[serde(default)]
    pub roles: Vec<String>,
    /// Conditions for the option to exist.
    #[serde(default)]
    pub requires: Vec<Cond>,
    /// Added to the difficulty (negative helps).
    pub difficulty: i32,
    /// Percent added to legend.
    pub legend_pct: i32,
    /// Heat added.
    pub heat: i32,
    /// Added to the chance of being traced.
    #[serde(default)]
    pub suspicion: i32,
}

/// A pairing of twist and methods that earns a bonus.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Synergy {
    /// The twist.
    pub twist: String,
    /// The methods it pairs with.
    pub methods: Vec<String>,
    /// Percent added to legend.
    pub legend_pct: i32,
    /// What the player is told.
    pub text: String,
}

/// How you leave.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Getaway {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// One line.
    pub blurb: String,
    /// Ally roles it needs.
    #[serde(default)]
    pub roles: Vec<String>,
    /// Conditions for the option to exist.
    #[serde(default)]
    pub requires: Vec<Cond>,
    /// Added to the escape chance.
    pub escape: i32,
    /// Added to the chance of being traced.
    pub suspicion: i32,
    /// Heat added.
    pub heat: i32,
    /// Whether it costs the next period.
    #[serde(default)]
    pub lies_low: bool,
}

/// A line told on a good phase.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseLine {
    /// `sneak`, `act` or `escape`.
    pub phase: String,
    /// Always true for now; failures come from mishaps.
    pub ok: bool,
    /// The text.
    pub text: String,
}

/// A small disaster.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mishap {
    /// The phase it can happen in.
    pub phase: String,
    /// The text.
    pub text: String,
    /// Heat it adds.
    pub heat: i32,
}

/// What scouting teaches.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// The place, or `*`.
    pub place: String,
    /// The scouting level.
    pub level: i32,
    /// What is learned.
    pub know: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemesFile {
    target: Vec<Target>,
    method: Vec<Method>,
    twist: Vec<Twist>,
    synergy: Vec<Synergy>,
    getaway: Vec<Getaway>,
    line: Vec<PhaseLine>,
    mishap: Vec<Mishap>,
    grant: Vec<Grant>,
}

// ---------------------------------------------------------------- world and endings

/// What a big scheme does to the school.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// How long it lasts; 0 is the rest of the term.
    pub days: i64,
    /// The paper's headline (`{player}`).
    pub headline: String,
    /// Places it closes.
    pub closes: Vec<String>,
    /// Characters it moves: `[npc, place]`.
    pub moves: Vec<(String, String)>,
    /// Added to every place's patrol level.
    pub patrol_mod: i32,
    /// Percent added to every legend gain.
    pub legend_pct: i32,
    /// Percent added to every heat gain.
    pub heat_pct: i32,
}

/// A way the term can end.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ending {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The words.
    pub text: String,
    /// Conditions; the first ending whose conditions hold is the one.
    pub requires: Vec<Cond>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorldFile {
    effect: Vec<Effect>,
    ending: Vec<Ending>,
}

// ---------------------------------------------------------------- lessons (H2)

/// Which subject uses which minigame, and which skill a good lesson trains.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LessonSpec {
    /// The subject.
    pub subject: String,
    /// `sequence`, `balance`, `vocab`, `timeline`, `pattern` or `relay`.
    pub kind: String,
    /// The skill a lesson trains.
    pub skill: String,
    /// What the lesson asks of the pupil, one line.
    pub intro: String,
}

/// A word of the school's made-up language.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Word {
    /// The word.
    pub term: String,
    /// What it means.
    pub meaning: String,
}

/// An event of the school's history.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEvent {
    /// What happened.
    pub name: String,
    /// The year.
    pub year: i32,
}

/// Glyphs a pattern is drawn with.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motif {
    /// The glyphs, one character each.
    pub glyphs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LessonsFile {
    lesson: Vec<LessonSpec>,
    word: Vec<Word>,
    event: Vec<HistoryEvent>,
    motif: Vec<Motif>,
}

// ---------------------------------------------------------------- clubs (H2)

/// A club.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClubSpec {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// One line.
    pub blurb: String,
    /// Where it meets.
    pub place: String,
    /// Where to sign up, if not where it meets.
    #[serde(default)]
    pub join_place: Option<String>,
    /// The slot kind it meets in.
    pub kind: String,
    /// The weekdays it meets (every day if empty).
    #[serde(default)]
    pub days: Vec<u8>,
    /// The member of staff who runs it.
    pub advisor: String,
    /// Characters who come to meetings.
    pub members: Vec<String>,
    /// What it takes to be let in.
    #[serde(default)]
    pub join: Vec<Cond>,
    /// Clubs that will not have you if you belong to this one, and the reverse.
    #[serde(default)]
    pub rivals: Vec<String>,
    /// The skill meetings train.
    pub skill: String,
    /// Ranks: name and the merit that earns it, from rank 1 at 0.
    pub ranks: Vec<(String, i32)>,
    /// The term-end event, as the paper tells it.
    pub event: String,
    /// Legend per rank above the first, earned at term end.
    pub event_legend: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClubsFile {
    club: Vec<ClubSpec>,
}

// ---------------------------------------------------------------- conversations and storylines (H2)

fn yes() -> bool {
    true
}

/// One thing the player can say or do in a conversation.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    /// What the player sees.
    pub label: String,
    /// What it takes for the option to be open. A choice that is shut shows what it wants.
    #[serde(default)]
    pub needs: Vec<Cond>,
    /// What choosing it does.
    #[serde(default)]
    pub effects: Vec<Eff>,
    /// What is said back.
    pub say: String,
    /// The node it leads to; empty ends the conversation.
    #[serde(default)]
    pub next: String,
}

/// A beat of a conversation.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    /// Its id.
    pub id: String,
    /// What is said to the player.
    pub text: String,
    /// What they can answer.
    pub choice: Vec<Choice>,
}

/// A conversation with a character.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Talk {
    /// Its id.
    pub id: String,
    /// Who it is with.
    pub npc: String,
    /// A line for the menu.
    pub title: String,
    /// What must hold for them to start it.
    #[serde(default)]
    pub when: Vec<Cond>,
    /// Whether it can be had once only.
    #[serde(default = "yes")]
    pub once: bool,
    /// Days before a repeatable one can be had again.
    #[serde(default)]
    pub cooldown: i64,
    /// The storyline this belongs to.
    #[serde(default)]
    pub arc: Option<String>,
    /// The step of that storyline it is open at.
    #[serde(default)]
    pub step: Option<String>,
    /// Days after the step began before it opens.
    #[serde(default)]
    pub wait: i64,
    /// The first node.
    pub first: String,
    /// The nodes.
    pub node: Vec<Node>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TalksFile {
    talk: Vec<Talk>,
}

/// A step of a storyline, as the journal tells it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArcStep {
    /// Its id.
    pub id: String,
    /// What the journal says while the story is here.
    pub text: String,
}

/// A way a storyline can end.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArcEnding {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The words.
    pub text: String,
    /// What it does when it comes about.
    #[serde(default)]
    pub effects: Vec<Eff>,
}

/// A multi-day storyline.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArcSpec {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// One line.
    pub blurb: String,
    /// What starts it (all must hold).
    pub start: Vec<Cond>,
    /// The step it starts at.
    pub first: String,
    /// The steps.
    pub step: Vec<ArcStep>,
    /// The endings.
    pub ending: Vec<ArcEnding>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArcsFile {
    arc: Vec<ArcSpec>,
}

// ---------------------------------------------------------------- the lot

macro_rules! file_table {
    ($t:ty, $check:expr) => {
        impl Table for $t {
            fn validate(&self) -> Vec<String> {
                #[allow(clippy::redundant_closure_call)]
                ($check)(self)
            }
        }
    };
}

fn dupes<'a>(what: &str, ids: impl Iterator<Item = &'a str>, out: &mut Vec<String>) {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            out.push(format!("{what} id `{id}` is used twice"));
        }
    }
}

fn plain(what: &str, text: &str, out: &mut Vec<String>) {
    if text.trim().is_empty() || !plain_text(text) {
        out.push(format!(
            "{what}: text is empty or has characters the board strips"
        ));
    }
}

file_table!(RulesFile, |f: &RulesFile| {
    let mut out = Vec::new();
    let r = &f.rules;
    if !(1..=366).contains(&r.term_days) {
        out.push("term_days must be 1 to 366".into());
    }
    if r.skill_levels.first() != Some(&0) || r.skill_levels.windows(2).any(|w| w[0] >= w[1]) {
        out.push("skill_levels must start at 0 and rise".into());
    }
    if r.file_full < 2 || r.max_schemes == 0 || r.moves_per_slot == 0 {
        out.push("file_full, max_schemes and moves_per_slot must be positive".into());
    }
    if !f.tier.windows(2).all(|w| w[0].at < w[1].at) || f.tier.iter().any(|t| t.at >= r.file_full) {
        out.push("tiers must rise and sit below file_full".into());
    }
    for t in &f.tier {
        if !["detention", "privileges", "letter", "summons"].contains(&t.effect.as_str()) {
            out.push(format!("tier effect `{}` is unknown", t.effect));
        }
        plain("tier", &t.text, &mut out);
    }
    dupes(
        "file kind",
        f.file_kind.iter().map(|k| k.id.as_str()),
        &mut out,
    );
    for k in &f.file_kind {
        plain("file kind", &k.text, &mut out);
    }
    out
});

file_table!(TimetableFile, |f: &TimetableFile| {
    let mut out = Vec::new();
    dupes("period", f.period.iter().map(|p| p.id.as_str()), &mut out);
    dupes("subject", f.subject.iter().map(|p| p.id.as_str()), &mut out);
    for weekday in 0..7u8 {
        match f.day.iter().filter(|d| d.weekday == weekday).count() {
            1 => {}
            n => out.push(format!("weekday {weekday} has {n} timetables")),
        }
    }
    for d in &f.day {
        if d.slots.len() != f.period.len() {
            out.push(format!(
                "weekday {} has {} slots for {} periods",
                d.weekday,
                d.slots.len(),
                f.period.len()
            ));
        }
        for s in &d.slots {
            let ok = ["chapel", "lunch", "free", "after", "evening", "night"].contains(&s.as_str())
                || s.strip_prefix("class:")
                    .is_some_and(|sub| f.subject.iter().any(|x| x.id == sub));
            if !ok {
                out.push(format!("weekday {}: unknown activity `{s}`", d.weekday));
            }
        }
    }
    for p in &f.period {
        plain("period", &p.name, &mut out);
    }
    out
});

file_table!(PlacesFile, |f: &PlacesFile| {
    let mut out = Vec::new();
    dupes("place", f.place.iter().map(|p| p.id.as_str()), &mut out);
    let ids: HashSet<&str> = f.place.iter().map(|p| p.id.as_str()).collect();
    for p in &f.place {
        plain(&p.id, &p.look, &mut out);
        plain(&p.id, &p.name, &mut out);
        for e in &p.exits {
            if !ids.contains(e.as_str()) {
                out.push(format!("place `{}` has an exit to unknown `{e}`", p.id));
            }
        }
        for h in &p.hours {
            if ![
                "class", "chapel", "lunch", "free", "after", "evening", "night",
            ]
            .contains(&h.as_str())
            {
                out.push(format!("place `{}`: unknown hours kind `{h}`", p.id));
            }
        }
        if p.lock.is_some() == p.unlock.is_empty() {
            out.push(format!(
                "place `{}`: a lock needs unlock conditions and the reverse",
                p.id
            ));
        }
        for c in &p.change {
            plain(&p.id, &c.text, &mut out);
        }
    }
    out
});

file_table!(CastFile, |f: &CastFile| {
    let mut out = Vec::new();
    dupes("clique", f.clique.iter().map(|c| c.id.as_str()), &mut out);
    dupes("npc", f.npc.iter().map(|c| c.id.as_str()), &mut out);
    let cliques: HashSet<&str> = f.clique.iter().map(|c| c.id.as_str()).collect();
    for c in &f.clique {
        plain(&c.id, &c.name, &mut out);
        plain(&c.id, &c.blurb, &mut out);
        for r in &c.rivals {
            if !cliques.contains(r.as_str()) {
                out.push(format!("clique `{}` rivals unknown `{r}`", c.id));
            }
        }
    }
    for n in &f.npc {
        plain(&n.id, &n.name, &mut out);
        plain(&n.id, &n.blurb, &mut out);
        if !["staff", "prefect", "student"].contains(&n.role.as_str()) {
            out.push(format!("npc `{}` has unknown role `{}`", n.id, n.role));
        }
        if n.clique
            .as_ref()
            .is_some_and(|c| !cliques.contains(c.as_str()))
        {
            out.push(format!("npc `{}` is in an unknown clique", n.id));
        }
        if !(0..=3).contains(&n.watchful) {
            out.push(format!("npc `{}`: watchful must be 0 to 3", n.id));
        }
        for a in &n.ally {
            if !["lookout", "distraction", "cover", "gadget", "keys"].contains(&a.role.as_str()) {
                out.push(format!("npc `{}`: unknown ally role `{}`", n.id, a.role));
            }
        }
    }
    out
});

file_table!(TraitsFile, |f: &TraitsFile| {
    let mut out = Vec::new();
    dupes("skill", f.skill.iter().map(|s| s.id.as_str()), &mut out);
    dupes("trait", f.traits.iter().map(|s| s.id.as_str()), &mut out);
    let skills: HashSet<&str> = f.skill.iter().map(|s| s.id.as_str()).collect();
    for t in &f.traits {
        plain(&t.id, &t.name, &mut out);
        plain(&t.id, &t.blurb, &mut out);
        for (s, _) in &t.skills {
            if !skills.contains(s.as_str()) {
                out.push(format!("trait `{}` boosts unknown skill `{s}`", t.id));
            }
        }
        if t.start != t.earn.is_empty() {
            out.push(format!(
                "trait `{}`: a starting trait has no `earn`, an earned one has",
                t.id
            ));
        }
    }
    if f.traits.iter().filter(|t| t.start).count() < 3 {
        out.push("at least three starting traits are needed so the choice means something".into());
    }
    out
});

file_table!(ItemsFile, |f: &ItemsFile| {
    let mut out = Vec::new();
    dupes("item", f.item.iter().map(|i| i.id.as_str()), &mut out);
    for i in &f.item {
        plain(&i.id, &i.name, &mut out);
        if i.price < 0 {
            out.push(format!("item `{}` has a negative price", i.id));
        }
    }
    out
});

file_table!(SchemesFile, |f: &SchemesFile| {
    let mut out = Vec::new();
    dupes("target", f.target.iter().map(|t| t.id.as_str()), &mut out);
    dupes("method", f.method.iter().map(|t| t.id.as_str()), &mut out);
    dupes("twist", f.twist.iter().map(|t| t.id.as_str()), &mut out);
    dupes("getaway", f.getaway.iter().map(|t| t.id.as_str()), &mut out);
    for t in &f.twist {
        plain(&t.id, &t.name, &mut out);
        plain(&t.id, &t.blurb, &mut out);
    }
    for g in &f.getaway {
        plain(&g.id, &g.name, &mut out);
        plain(&g.id, &g.blurb, &mut out);
    }
    for t in &f.target {
        plain(&t.id, &t.name, &mut out);
        if !["staff", "prefect", "clique", "school"].contains(&t.kind.as_str()) {
            out.push(format!("target `{}` has unknown kind `{}`", t.id, t.kind));
        }
    }
    for m in &f.method {
        plain(&m.id, &m.name, &mut out);
        plain(&m.id, &m.blurb, &mut out);
        plain(&m.id, &m.story, &mut out);
        if !["absent", "present", "any"].contains(&m.present.as_str()) {
            out.push(format!(
                "method `{}`: `present` must be absent, present or any",
                m.id
            ));
        }
        if m.skills.len() != 3 {
            out.push(format!(
                "method `{}` needs three skills (sneak, act, escape)",
                m.id
            ));
        }
        if m.big != m.effect.is_some() {
            out.push(format!(
                "method `{}`: only big schemes have an effect, and all do",
                m.id
            ));
        }
        if !(1..=3).contains(&m.severity)
            || m.legend <= 0
            || m.heat < 0
            || !(0..=3).contains(&m.scout)
        {
            out.push(format!("method `{}` has numbers out of range", m.id));
        }
        for k in &m.kinds {
            if !["staff", "prefect", "clique", "school"].contains(&k.as_str()) {
                out.push(format!("method `{}` has unknown kind `{k}`", m.id));
            }
        }
    }
    for l in &f.line {
        plain("line", &l.text, &mut out);
    }
    for m in &f.mishap {
        plain("mishap", &m.text, &mut out);
    }
    for phase in ["sneak", "act", "escape"] {
        if !f.line.iter().any(|l| l.phase == phase) || !f.mishap.iter().any(|m| m.phase == phase) {
            out.push(format!(
                "phase `{phase}` needs at least one line and one mishap"
            ));
        }
    }
    for s in &f.synergy {
        plain("synergy", &s.text, &mut out);
    }
    out
});

file_table!(WorldFile, |f: &WorldFile| {
    let mut out = Vec::new();
    dupes("effect", f.effect.iter().map(|e| e.id.as_str()), &mut out);
    dupes("ending", f.ending.iter().map(|e| e.id.as_str()), &mut out);
    for e in &f.effect {
        plain(&e.id, &e.name, &mut out);
        plain(&e.id, &e.headline, &mut out);
        if e.days < 0 {
            out.push(format!("effect `{}` has negative days", e.id));
        }
    }
    for e in &f.ending {
        plain(&e.id, &e.name, &mut out);
        plain(&e.id, &e.text, &mut out);
    }
    if f.ending.last().is_none_or(|e| e.id != "scraped_through") {
        out.push("the last ending must be the catch-all `scraped_through`".into());
    }
    out
});

file_table!(LessonsFile, |f: &LessonsFile| {
    let mut out = Vec::new();
    dupes(
        "lesson",
        f.lesson.iter().map(|l| l.subject.as_str()),
        &mut out,
    );
    for l in &f.lesson {
        if ![
            "sequence", "balance", "vocab", "timeline", "pattern", "relay",
        ]
        .contains(&l.kind.as_str())
        {
            out.push(format!(
                "lesson `{}` has unknown kind `{}`",
                l.subject, l.kind
            ));
        }
        plain("lesson", &l.intro, &mut out);
    }
    if f.word.len() < 8 {
        out.push("vocabulary needs at least eight words so a puzzle has wrong answers".into());
    }
    if f.event.len() < 8 {
        out.push("the timeline needs at least eight events".into());
    }
    dupes("word", f.word.iter().map(|w| w.term.as_str()), &mut out);
    dupes("event", f.event.iter().map(|w| w.name.as_str()), &mut out);
    for w in &f.word {
        plain("word", &w.term, &mut out);
        plain("word", &w.meaning, &mut out);
    }
    for e in &f.event {
        plain("event", &e.name, &mut out);
    }
    if f.motif.is_empty() {
        out.push("patterns need at least one motif".into());
    }
    for m in &f.motif {
        if m.glyphs.len() < 4
            || m.glyphs
                .iter()
                .any(|g| g.chars().count() != 1 || !plain_text(g) || g.trim().is_empty())
        {
            out.push("a motif needs four or more distinct one-character glyphs".into());
        }
    }
    out
});

file_table!(ClubsFile, |f: &ClubsFile| {
    let mut out = Vec::new();
    dupes("club", f.club.iter().map(|c| c.id.as_str()), &mut out);
    for c in &f.club {
        plain(&c.id, &c.name, &mut out);
        plain(&c.id, &c.blurb, &mut out);
        plain(&c.id, &c.event, &mut out);
        if c.ranks.len() < 3
            || c.ranks.first().map(|r| r.1) != Some(0)
            || c.ranks.windows(2).any(|w| w[0].1 >= w[1].1)
        {
            out.push(format!(
                "club `{}`: ranks must be three or more and start at merit 0 and rise",
                c.id
            ));
        }
        for (name, _) in &c.ranks {
            plain(&c.id, name, &mut out);
        }
        if !["free", "after", "evening", "night", "lunch"].contains(&c.kind.as_str()) {
            out.push(format!(
                "club `{}` meets in unknown slot kind `{}`",
                c.id, c.kind
            ));
        }
        if c.days.iter().any(|d| *d > 6) {
            out.push(format!("club `{}` meets on a weekday over 6", c.id));
        }
        if c.event_legend < 0 || c.members.is_empty() {
            out.push(format!(
                "club `{}` needs members and a non-negative event bonus",
                c.id
            ));
        }
    }
    out
});

file_table!(TalksFile, |f: &TalksFile| {
    let mut out = Vec::new();
    dupes("talk", f.talk.iter().map(|t| t.id.as_str()), &mut out);
    for t in &f.talk {
        plain(&t.id, &t.title, &mut out);
        if t.node.is_empty() || !t.node.iter().any(|n| n.id == t.first) {
            out.push(format!("talk `{}` has no first node `{}`", t.id, t.first));
        }
        if !t.once && t.cooldown < 1 {
            out.push(format!(
                "talk `{}` repeats, so it needs a cooldown of a day or more",
                t.id
            ));
        }
        if t.arc.is_some() != t.step.is_some() {
            out.push(format!("talk `{}`: `arc` and `step` go together", t.id));
        }
        dupes(
            &format!("node of talk `{}`", t.id),
            t.node.iter().map(|n| n.id.as_str()),
            &mut out,
        );
        for n in &t.node {
            plain(&t.id, &n.text, &mut out);
            if !n.choice.iter().any(|c| c.needs.is_empty()) {
                out.push(format!(
                    "talk `{}` node `{}` needs a way out that asks for nothing",
                    t.id, n.id
                ));
            }
            for c in &n.choice {
                plain(&t.id, &c.label, &mut out);
                plain(&t.id, &c.say, &mut out);
                if !c.next.is_empty() && !t.node.iter().any(|x| x.id == c.next) {
                    out.push(format!(
                        "talk `{}` node `{}` leads to unknown node `{}`",
                        t.id, n.id, c.next
                    ));
                }
            }
        }
        // Every node must be reachable from the first, or it is dead text.
        let mut seen: HashSet<&str> = HashSet::new();
        let mut stack = vec![t.first.as_str()];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(n) = t.node.iter().find(|n| n.id == id) {
                stack.extend(
                    n.choice
                        .iter()
                        .filter(|c| !c.next.is_empty())
                        .map(|c| c.next.as_str()),
                );
            }
        }
        for n in &t.node {
            if !seen.contains(n.id.as_str()) {
                out.push(format!(
                    "talk `{}` node `{}` can never be reached",
                    t.id, n.id
                ));
            }
        }
    }
    out
});

file_table!(ArcsFile, |f: &ArcsFile| {
    let mut out = Vec::new();
    dupes("arc", f.arc.iter().map(|a| a.id.as_str()), &mut out);
    for a in &f.arc {
        plain(&a.id, &a.name, &mut out);
        plain(&a.id, &a.blurb, &mut out);
        dupes(
            &format!("step of arc `{}`", a.id),
            a.step.iter().map(|s| s.id.as_str()),
            &mut out,
        );
        dupes(
            &format!("ending of arc `{}`", a.id),
            a.ending.iter().map(|s| s.id.as_str()),
            &mut out,
        );
        if !a.step.iter().any(|s| s.id == a.first) {
            out.push(format!(
                "arc `{}` starts at unknown step `{}`",
                a.id, a.first
            ));
        }
        if a.ending.len() < 2 {
            out.push(format!(
                "arc `{}` needs at least two endings so the choices matter",
                a.id
            ));
        }
        if a.start.is_empty() {
            out.push(format!("arc `{}` needs a start condition", a.id));
        }
        for s in &a.step {
            plain(&a.id, &s.text, &mut out);
        }
        for e in &a.ending {
            plain(&a.id, &e.name, &mut out);
            plain(&a.id, &e.text, &mut out);
        }
    }
    out
});

/// Everything the engine reads, loaded and cross-checked.
#[derive(Debug, Clone)]
pub struct Content {
    /// The tunable numbers.
    pub rules: Rules,
    /// File levels.
    pub tiers: Vec<Tier>,
    /// File entry kinds.
    pub file_kinds: Vec<FileKind>,
    /// Periods in a day.
    pub periods: Vec<Period>,
    /// Timetables by weekday.
    pub days: Vec<DaySpec>,
    /// Subjects.
    pub subjects: Vec<Subject>,
    /// Places.
    pub places: Vec<PlaceSpec>,
    /// Cliques.
    pub cliques: Vec<Clique>,
    /// Characters.
    pub npcs: Vec<Npc>,
    /// Skills.
    pub skills: Vec<Skill>,
    /// Traits.
    pub traits: Vec<TraitSpec>,
    /// Items.
    pub items: Vec<Item>,
    /// Targets.
    pub targets: Vec<Target>,
    /// Methods.
    pub methods: Vec<Method>,
    /// Twists.
    pub twists: Vec<Twist>,
    /// Synergies.
    pub synergies: Vec<Synergy>,
    /// Getaways.
    pub getaways: Vec<Getaway>,
    /// Good-phase lines.
    pub lines: Vec<PhaseLine>,
    /// Mishaps.
    pub mishaps: Vec<Mishap>,
    /// Scouting grants.
    pub grants: Vec<Grant>,
    /// World effects.
    pub effects: Vec<Effect>,
    /// Endings.
    pub endings: Vec<Ending>,
    /// Which minigame each subject uses.
    pub lessons: Vec<LessonSpec>,
    /// The school's vocabulary.
    pub words: Vec<Word>,
    /// The school's history.
    pub events: Vec<HistoryEvent>,
    /// Pattern glyphs.
    pub motifs: Vec<Motif>,
    /// Clubs.
    pub clubs: Vec<ClubSpec>,
    /// Conversations.
    pub talks: Vec<Talk>,
    /// Storylines.
    pub arcs: Vec<ArcSpec>,
    place_index: HashMap<String, usize>,
    npc_index: HashMap<String, usize>,
}

/// The embedded source of each table, in one place so tests can break them on purpose.
pub struct Sources<'a> {
    /// `rules.toml`
    pub rules: &'a str,
    /// `timetable.toml`
    pub timetable: &'a str,
    /// `places.toml`
    pub places: &'a str,
    /// `cast.toml`
    pub cast: &'a str,
    /// `traits.toml`
    pub traits: &'a str,
    /// `items.toml`
    pub items: &'a str,
    /// `schemes.toml`
    pub schemes: &'a str,
    /// `world.toml`
    pub world: &'a str,
    /// `lessons.toml`
    pub lessons: &'a str,
    /// `clubs.toml`
    pub clubs: &'a str,
    /// `talks.toml`
    pub talks: &'a str,
    /// `arcs.toml`
    pub arcs: &'a str,
}

/// The tables shipped in this crate.
pub const EMBEDDED: Sources<'static> = Sources {
    rules: include_str!("../content/rules.toml"),
    timetable: include_str!("../content/timetable.toml"),
    places: include_str!("../content/places.toml"),
    cast: include_str!("../content/cast.toml"),
    traits: include_str!("../content/traits.toml"),
    items: include_str!("../content/items.toml"),
    schemes: include_str!("../content/schemes.toml"),
    world: include_str!("../content/world.toml"),
    lessons: include_str!("../content/lessons.toml"),
    clubs: include_str!("../content/clubs.toml"),
    talks: include_str!("../content/talks.toml"),
    arcs: include_str!("../content/arcs.toml"),
};

impl Content {
    /// Load and check the embedded tables.
    pub fn embedded() -> Result<Self, ContentError> {
        Self::from_sources(&EMBEDDED)
    }

    /// Load and check tables from `src`.
    pub fn from_sources(src: &Sources<'_>) -> Result<Self, ContentError> {
        let rules: RulesFile = content::load("rules", src.rules)?;
        let tt: TimetableFile = content::load("timetable", src.timetable)?;
        let places: PlacesFile = content::load("places", src.places)?;
        let cast: CastFile = content::load("cast", src.cast)?;
        let traits: TraitsFile = content::load("traits", src.traits)?;
        let items: ItemsFile = content::load("items", src.items)?;
        let schemes: SchemesFile = content::load("schemes", src.schemes)?;
        let world: WorldFile = content::load("world", src.world)?;
        let lessons: LessonsFile = content::load("lessons", src.lessons)?;
        let clubs: ClubsFile = content::load("clubs", src.clubs)?;
        let talks: TalksFile = content::load("talks", src.talks)?;
        let arcs: ArcsFile = content::load("arcs", src.arcs)?;
        let place_index = places
            .place
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id.clone(), i))
            .collect();
        let npc_index = cast
            .npc
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id.clone(), i))
            .collect();
        let all = Self {
            rules: rules.rules,
            tiers: rules.tier,
            file_kinds: rules.file_kind,
            periods: tt.period,
            days: tt.day,
            subjects: tt.subject,
            places: places.place,
            cliques: cast.clique,
            npcs: cast.npc,
            skills: traits.skill,
            traits: traits.traits,
            items: items.item,
            targets: schemes.target,
            methods: schemes.method,
            twists: schemes.twist,
            synergies: schemes.synergy,
            getaways: schemes.getaway,
            lines: schemes.line,
            mishaps: schemes.mishap,
            grants: schemes.grant,
            effects: world.effect,
            endings: world.ending,
            lessons: lessons.lesson,
            words: lessons.word,
            events: lessons.event,
            motifs: lessons.motif,
            clubs: clubs.club,
            talks: talks.talk,
            arcs: arcs.arc,
            place_index,
            npc_index,
        };
        let problems = all.problems();
        if problems.is_empty() {
            Ok(all)
        } else {
            Err(ContentError::Invalid {
                name: "hallpass",
                problems,
            })
        }
    }

    /// Cross-table problems: every reference must resolve, and the tone rules hold.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let place = |id: &str| self.place_index.contains_key(id);
        let npc = |id: &str| self.npc_index.contains_key(id);
        let skill = |id: &str| self.skills.iter().any(|s| s.id == id);
        let item = |id: &str| self.items.iter().any(|s| s.id == id);
        let clique = |id: &str| self.cliques.iter().any(|s| s.id == id);
        let effect = |id: &str| self.effects.iter().any(|s| s.id == id);
        if !place(&self.rules.start_place) {
            out.push("start_place is not a place".into());
        }
        for s in &self.subjects {
            if !place(&s.room) || !npc(&s.teacher) {
                out.push(format!("subject `{}` has an unknown room or teacher", s.id));
            }
        }
        for n in &self.npcs {
            if !place(&n.home) {
                out.push(format!("npc `{}` has an unknown home", n.id));
            }
            for r in &n.schedule {
                if !place(&r.place) {
                    out.push(format!(
                        "npc `{}` schedule names unknown place `{}`",
                        n.id, r.place
                    ));
                }
                if r.days.iter().any(|d| *d > 6) {
                    out.push(format!("npc `{}` schedule has a weekday over 6", n.id));
                }
                if !self.when_ok(&r.when) {
                    out.push(format!(
                        "npc `{}` schedule has unknown `when` `{}`",
                        n.id, r.when
                    ));
                }
            }
            if n.role == "student" && n.clique.is_none() {
                out.push(format!("student `{}` needs a clique", n.id));
            }
        }
        for t in &self.traits {
            if t.start && t.skills.iter().any(|(_, v)| *v < 0) {
                out.push(format!("trait `{}` is a penalty, not a trait", t.id));
            }
        }
        for i in &self.items {
            if !self
                .places
                .iter()
                .any(|p| p.shop.as_deref() == Some(i.shop.as_str()))
            {
                out.push(format!("item `{}` is sold in a shop no place has", i.id));
            }
        }
        for t in &self.targets {
            // Tone: mischief against authority and rivals. The Strays are allies, never targets,
            // and a target is never a person who can be typed in.
            if t.clique.as_deref() == Some("strays") {
                out.push(format!(
                    "target `{}` is a Stray; the Strays are allies, never targets",
                    t.id
                ));
            }
            match t.kind.as_str() {
                "staff" | "prefect" => match t.npc.as_deref().and_then(|n| self.npc(n)) {
                    Some(n) if n.prankable && n.is_authority() => {}
                    _ => out.push(format!(
                        "target `{}` must be a prankable staff member or prefect",
                        t.id
                    )),
                },
                "clique" if !t.clique.as_deref().is_some_and(clique) => {
                    out.push(format!("target `{}` names no clique", t.id));
                }
                _ => {}
            }
            if t.place.as_deref().is_some_and(|p| !place(p)) {
                out.push(format!("target `{}` has an unknown place", t.id));
            }
        }
        for m in &self.methods {
            if !place(&m.place) {
                out.push(format!("method `{}` has an unknown place", m.id));
            }
            for n in &m.needs {
                if !item(n) {
                    out.push(format!("method `{}` needs unknown item `{n}`", m.id));
                }
            }
            for s in &m.skills {
                if !skill(s) {
                    out.push(format!("method `{}` uses unknown skill `{s}`", m.id));
                }
            }
            for o in &m.only {
                if !self.targets.iter().any(|t| t.id == *o) {
                    out.push(format!("method `{}` names unknown target `{o}`", m.id));
                }
            }
            if m.effect.as_deref().is_some_and(|e| !effect(e)) {
                out.push(format!("method `{}` names an unknown effect", m.id));
            }
            for r in m
                .roles
                .iter()
                .chain(self.twists.iter().flat_map(|t| &t.roles))
            {
                if !["lookout", "distraction", "cover", "gadget", "keys"].contains(&r.as_str()) {
                    out.push(format!("unknown role `{r}`"));
                }
            }
            // A method must be usable on at least one target, or it is dead content.
            if !self.targets.iter().any(|t| self.method_fits(m, t)) {
                out.push(format!("method `{}` fits no target", m.id));
            }
            if !self.any_ally_for(&m.roles) {
                out.push(format!(
                    "method `{}` needs a role no character offers",
                    m.id
                ));
            }
        }
        for t in &self.twists {
            if !self.any_ally_for(&t.roles) {
                out.push(format!("twist `{}` needs a role no character offers", t.id));
            }
        }
        for g in &self.getaways {
            if !self.any_ally_for(&g.roles) {
                out.push(format!(
                    "getaway `{}` needs a role no character offers",
                    g.id
                ));
            }
        }
        if !self
            .getaways
            .first()
            .is_some_and(|g| g.id == "slip_away" && g.requires.is_empty() && g.roles.is_empty())
        {
            out.push("the first getaway must be an unconditional `slip_away`".into());
        }
        for s in &self.synergies {
            if !self.twists.iter().any(|t| t.id == s.twist)
                || s.methods
                    .iter()
                    .any(|m| !self.methods.iter().any(|x| x.id == *m))
            {
                out.push("a synergy names an unknown twist or method".into());
            }
        }
        for g in &self.grants {
            if g.place != "*" && !place(&g.place) {
                out.push(format!("grant names unknown place `{}`", g.place));
            }
        }
        for e in &self.effects {
            for p in &e.closes {
                if !place(p) {
                    out.push(format!("effect `{}` closes unknown place `{p}`", e.id));
                }
            }
            for (n, p) in &e.moves {
                if !npc(n) || !place(p) {
                    out.push(format!(
                        "effect `{}` moves an unknown character or place",
                        e.id
                    ));
                }
            }
        }
        for p in &self.places {
            if p.shop.is_some() && p.hours.is_empty() {
                out.push(format!("shop place `{}` is never open", p.id));
            }
        }
        out.extend(self.life_problems());
        out
    }

    fn when_ok(&self, when: &str) -> bool {
        when == "*"
            || when == "class"
            || ["chapel", "lunch", "free", "after", "evening", "night"].contains(&when)
            || when
                .strip_prefix("class:")
                .is_some_and(|s| self.subjects.iter().any(|x| x.id == s))
    }

    fn any_ally_for(&self, roles: &[String]) -> bool {
        roles.iter().all(|r| {
            self.npcs
                .iter()
                .any(|n| n.ally.iter().any(|a| a.role == *r))
        })
    }

    /// Whether `method` can be pointed at `target`.
    #[must_use]
    pub fn method_fits(&self, method: &Method, target: &Target) -> bool {
        method.kinds.contains(&target.kind)
            && (method.only.is_empty() || method.only.contains(&target.id))
    }

    /// A place by id.
    #[must_use]
    pub fn place(&self, id: &str) -> Option<&PlaceSpec> {
        self.place_index.get(id).and_then(|i| self.places.get(*i))
    }

    /// A character by id.
    #[must_use]
    pub fn npc(&self, id: &str) -> Option<&Npc> {
        self.npc_index.get(id).and_then(|i| self.npcs.get(*i))
    }

    /// A target by id.
    #[must_use]
    pub fn target(&self, id: &str) -> Option<&Target> {
        self.targets.iter().find(|t| t.id == id)
    }

    /// A method by id.
    #[must_use]
    pub fn method(&self, id: &str) -> Option<&Method> {
        self.methods.iter().find(|t| t.id == id)
    }

    /// A twist by id.
    #[must_use]
    pub fn twist(&self, id: &str) -> Option<&Twist> {
        self.twists.iter().find(|t| t.id == id)
    }

    /// A getaway by id.
    #[must_use]
    pub fn getaway(&self, id: &str) -> Option<&Getaway> {
        self.getaways.iter().find(|t| t.id == id)
    }

    /// An item by id.
    #[must_use]
    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|t| t.id == id)
    }

    /// A trait by id.
    #[must_use]
    pub fn trait_spec(&self, id: &str) -> Option<&TraitSpec> {
        self.traits.iter().find(|t| t.id == id)
    }

    /// An effect by id.
    #[must_use]
    pub fn effect(&self, id: &str) -> Option<&Effect> {
        self.effects.iter().find(|t| t.id == id)
    }

    /// A clique by id.
    #[must_use]
    pub fn clique(&self, id: &str) -> Option<&Clique> {
        self.cliques.iter().find(|t| t.id == id)
    }

    /// A subject by id.
    #[must_use]
    pub fn subject(&self, id: &str) -> Option<&Subject> {
        self.subjects.iter().find(|t| t.id == id)
    }

    /// The place a target's deed happens in for `method`.
    #[must_use]
    pub fn act_place<'a>(&'a self, method: &'a Method, target: &'a Target) -> &'a str {
        target.place.as_deref().unwrap_or(&method.place)
    }

    /// The text of a file kind.
    #[must_use]
    pub fn file_kind_text(&self, id: &str) -> &str {
        self.file_kinds
            .iter()
            .find(|k| k.id == id)
            .map_or("a matter of record", |k| k.text.as_str())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn the_embedded_content_loads_and_is_consistent() {
        let content = Content::embedded().unwrap();
        assert_eq!(content.problems(), Vec::<String>::new());
        assert_eq!(content.periods.len(), 9);
    }
}
