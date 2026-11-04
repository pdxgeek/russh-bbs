//! The player's state and the shared world, with their binary forms.
//!
//! These are plain data. The rules that change them live in [`crate::game`] and
//! [`crate::scheme`]; the formats are pinned by tests, and a reader refuses bytes that are not
//! exactly what the writer wrote.

use crate::wire::{Reader, Wire, Writer, wire_struct};

/// A small map: id to value, kept as a vector so the encoding is stable.
pub type Map<T> = Vec<(String, T)>;

/// The most entries a map-like list may hold. A guard against runaway growth, well under the
/// framework's save cap.
pub const LIST_CAP: usize = 48;

/// Read a value from a map.
#[must_use]
pub fn map_get<'a, T>(map: &'a Map<T>, id: &str) -> Option<&'a T> {
    map.iter().find(|(k, _)| k == id).map(|(_, v)| v)
}

/// Set a value in a map.
pub fn map_set<T>(map: &mut Map<T>, id: &str, value: T) {
    match map.iter_mut().find(|(k, _)| k == id) {
        Some(slot) => slot.1 = value,
        None => map.push((id.to_owned(), value)),
    }
}

/// Clamp into `u8`.
#[must_use]
pub fn to_u8(v: i32) -> u8 {
    u8::try_from(v.clamp(0, 255)).unwrap_or(u8::MAX)
}

/// Clamp into `i16`.
#[must_use]
pub fn to_i16(v: i32) -> i16 {
    i16::try_from(v.clamp(i32::from(i16::MIN), i32::from(i16::MAX))).unwrap_or(0)
}

/// Clamp into `u32` (never negative).
#[must_use]
pub fn to_u32(v: i64) -> u32 {
    u32::try_from(v.clamp(0, i64::from(u32::MAX))).unwrap_or(0)
}

/// The most flags (things learned, story marks) a character keeps. Well under the save cap.
pub const KNOW_CAP: usize = 160;

/// One line on the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// The day it was written.
    pub day: i64,
    /// Its kind (a `file_kind` id).
    pub kind: String,
    /// Remaining severity points.
    pub sev: u8,
}
wire_struct!(FileEntry { day, kind, sev });

/// A staff member's wariness of the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wary {
    /// Level, 0 to 6.
    pub level: u8,
    /// The last day it rose.
    pub last: i64,
}
wire_struct!(Wary { level, last });

/// A scheme in the planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    /// Its number, unique for this player.
    pub id: u16,
    /// The target id.
    pub target: String,
    /// The method id.
    pub method: String,
    /// Twist ids.
    pub twists: Vec<String>,
    /// The getaway id.
    pub getaway: String,
    /// Scouting level, 0 to 3.
    pub scout: u8,
    /// Recruited allies: (character, role).
    pub allies: Vec<(String, String)>,
    /// When it will be done: (day, slot).
    pub when: Option<(i64, u8)>,
    /// The day it was drafted.
    pub created: i64,
}
wire_struct!(Scheme {
    id,
    target,
    method,
    twists,
    getaway,
    scout,
    allies,
    when,
    created
});

/// A scheme being run, one phase at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The scheme.
    pub scheme: u16,
    /// 0 sneak, 1 act, 2 escape.
    pub phase: u8,
    /// Phases that went wrong but not fatally.
    pub flubs: u8,
    /// Heat the mishaps added so far.
    pub mishap_heat: u8,
    /// Whether a cover story already saved the run.
    pub covered: bool,
}
wire_struct!(Run {
    scheme,
    phase,
    flubs,
    mishap_heat,
    covered
});

/// A chain of pranks that build on each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Campaign {
    /// The target the chain is aimed at.
    pub theme: String,
    /// Schemes pulled off in the chain.
    pub links: u8,
    /// The day of the last one.
    pub last_day: i64,
}
wire_struct!(Campaign {
    theme,
    links,
    last_day
});

/// How a term ended for a character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermResult {
    /// The term number.
    pub term: u32,
    /// The ending id.
    pub ending: String,
    /// Legend at the end.
    pub legend: u32,
    /// File points at the end.
    pub file: u8,
    /// Average grade at the end.
    pub grades: u8,
}
wire_struct!(TermResult {
    term,
    ending,
    legend,
    file,
    grades
});

/// Where a character is in their story.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Has not picked a trait yet.
    New,
    /// In school.
    Active,
    /// Sent home; the term is over for this character.
    Expelled,
    /// The term ended; waiting for the next.
    Finished,
}

impl Wire for Status {
    fn put(&self, w: &mut Writer) {
        match self {
            Self::New => 0u8,
            Self::Active => 1,
            Self::Expelled => 2,
            Self::Finished => 3,
        }
        .put(w);
    }
    fn get(r: &mut Reader<'_>) -> Option<Self> {
        match u8::get(r)? {
            0 => Some(Self::New),
            1 => Some(Self::Active),
            2 => Some(Self::Expelled),
            3 => Some(Self::Finished),
            _ => None,
        }
    }
}


// ---------------------------------------------------------------- school life (H2)

/// A club a character belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership {
    /// The club id.
    pub club: String,
    /// Rank, from 1.
    pub rank: u8,
    /// Merit earned at meetings and in club business.
    pub merit: u16,
    /// The last day a meeting was attended.
    pub last_meet: i64,
}
wire_struct!(Membership {
    club,
    rank,
    merit,
    last_meet
});

/// Where a character is in a storyline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcProgress {
    /// The storyline id.
    pub id: String,
    /// The step it is at.
    pub step: String,
    /// The day that step began.
    pub since: i64,
    /// The ending reached, once it is over.
    pub ended: Option<String>,
}
wire_struct!(ArcProgress {
    id,
    step,
    since,
    ended
});

/// A lesson under way: the minigame of the period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lesson {
    /// The subject.
    pub subject: String,
    /// Where the puzzles come from: round `n` is a pure function of `(seed, n, level)`.
    pub seed: u64,
    /// The round now up, from 0.
    pub round: u8,
    /// Rounds answered right so far.
    pub right: u8,
    /// How hard the puzzles are, 0 to 2, fixed when the lesson began.
    pub level: u8,
    /// How many wrong answers are struck out for the player, fixed when the lesson began.
    pub hints: u8,
}
wire_struct!(Lesson {
    subject,
    seed,
    round,
    right,
    level,
    hints
});

/// A conversation under way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    /// The conversation id.
    pub talk: String,
    /// The node now up.
    pub node: String,
    /// Choices made so far.
    pub chosen: u8,
}
wire_struct!(Scene { talk, node, chosen });

/// What H2 adds to a character. Appended to the save in version 2; a version 1 save gets the
/// empty value (see `save::migrate`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchoolLife {
    /// Clubs joined.
    pub clubs: Vec<Membership>,
    /// Friendship (positive) or rivalry (negative) by character, -100 to 100.
    pub bonds: Map<i16>,
    /// Storylines begun.
    pub arcs: Vec<ArcProgress>,
    /// The lesson being taken, if any.
    pub lesson: Option<Lesson>,
    /// The conversation being had, if any.
    pub scene: Option<Scene>,
    /// The day each conversation was last had.
    pub talked: Map<i64>,
    /// The helpers of the latest scheme, whom a headmistress may ask about.
    pub last_allies: Vec<String>,
    /// Lessons finished with every answer right.
    pub perfect: u16,
    /// Lessons taken.
    pub lessons: u16,
}
wire_struct!(SchoolLife {
    clubs,
    bonds,
    arcs,
    lesson,
    scene,
    talked,
    last_allies,
    perfect,
    lessons
});

impl SchoolLife {
    /// Friendship with a character.
    #[must_use]
    pub fn bond_with(&self, npc: &str) -> i32 {
        map_get(&self.bonds, npc).map_or(0, |v| i32::from(*v))
    }

    /// Change a friendship.
    pub fn add_bond(&mut self, npc: &str, delta: i32) {
        let now = self.bond_with(npc) + delta;
        map_set(&mut self.bonds, npc, to_i16(now.clamp(-100, 100)));
    }

    /// Rank in a club, 0 when not a member.
    #[must_use]
    pub fn rank_in(&self, club: &str) -> i32 {
        self.clubs
            .iter()
            .find(|m| m.club == club)
            .map_or(0, |m| i32::from(m.rank))
    }

    /// A storyline's progress.
    #[must_use]
    pub fn arc(&self, id: &str) -> Option<&ArcProgress> {
        self.arcs.iter().find(|a| a.id == id)
    }
}

/// One player's character.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // plain saved state: each flag is its own fact
pub struct Player {
    /// The handle (already sanitized by the board).
    pub name: String,
    /// The term this character is in.
    pub term: u32,
    /// The last school day processed.
    pub day: i64,
    /// The current period (the number of periods in a day means the day is over).
    pub slot: u8,
    /// Where they are.
    pub place: String,
    /// Where the story is.
    pub status: Status,
    /// Trait ids.
    pub traits: Vec<String>,
    /// Skill experience.
    pub xp: Map<u16>,
    /// Grades by subject.
    pub grades: Map<u8>,
    /// Standing by clique, -100 to 100.
    pub standing: Map<i16>,
    /// Nerve at its best.
    pub nerve_max: u8,
    /// Nerve left today.
    pub nerve: u8,
    /// Cool.
    pub cool: i16,
    /// Cash.
    pub cash: i32,
    /// Notoriety, 0 to 100.
    pub notoriety: u8,
    /// Heat, 0 to 100.
    pub heat: u8,
    /// Legend: the term's score.
    pub legend: u32,
    /// The file.
    pub file: Vec<FileEntry>,
    /// Days since the last file entry.
    pub clean_days: u8,
    /// File levels already acted on (a bit per tier).
    pub tiers_hit: u8,
    /// Detentions owed.
    pub detention: u8,
    /// Whether a detention must be served today.
    pub due: u8,
    /// Privileges are lost until this day.
    pub privileges_until: i64,
    /// A letter has gone home.
    pub letter: bool,
    /// The headmistress wants to see them tomorrow.
    pub summons: bool,
    /// Wariness by staff member.
    pub wary: Map<Wary>,
    /// Items carried.
    pub inventory: Map<u8>,
    /// Schemes in the planner.
    pub schemes: Vec<Scheme>,
    /// A scheme in progress.
    pub run: Option<Run>,
    /// The campaign in progress.
    pub campaign: Option<Campaign>,
    /// Things learned.
    pub knowledge: Vec<String>,
    /// The day each ally was last asked.
    pub ally_used: Map<i64>,
    /// How many times each (target, method) has been done.
    pub done: Vec<(String, u8)>,
    /// Schemes pulled off.
    pub pulled: u16,
    /// Times caught in the act.
    pub caught: u16,
    /// Times traced back.
    pub pinned: u16,
    /// Big schemes pulled off.
    pub big: u16,
    /// Whether today's class was attended.
    pub attended: bool,
    /// Whether they are out after lights-out this period.
    pub slipped: bool,
    /// Corridors left to walk this period.
    pub moves: u8,
    /// Whether the file was full and a warning given.
    pub final_warning: bool,
    /// The next scheme number.
    pub next_scheme: u16,
    /// The latest news, newest last.
    pub news: Vec<String>,
    /// How past terms ended.
    pub history: Vec<TermResult>,
    /// Classes, clubs, friendships and storylines (save version 2).
    pub life: SchoolLife,
}

wire_struct!(Player {
    name,
    term,
    day,
    slot,
    place,
    status,
    traits,
    xp,
    grades,
    standing,
    nerve_max,
    nerve,
    cool,
    cash,
    notoriety,
    heat,
    legend,
    file,
    clean_days,
    tiers_hit,
    detention,
    due,
    privileges_until,
    letter,
    summons,
    wary,
    inventory,
    schemes,
    run,
    campaign,
    knowledge,
    ally_used,
    done,
    pulled,
    caught,
    pinned,
    big,
    attended,
    slipped,
    moves,
    final_warning,
    next_scheme,
    news,
    history,
    life
});

impl Player {
    /// A new character who has not enrolled.
    #[must_use]
    pub fn new(name: &str, term: u32, day: i64, start_place: &str) -> Self {
        Self {
            name: name.to_owned(),
            term,
            day,
            slot: 0,
            place: start_place.to_owned(),
            status: Status::New,
            traits: Vec::new(),
            xp: Vec::new(),
            grades: Vec::new(),
            standing: Vec::new(),
            nerve_max: 0,
            nerve: 0,
            cool: 0,
            cash: 0,
            notoriety: 0,
            heat: 0,
            legend: 0,
            file: Vec::new(),
            clean_days: 0,
            tiers_hit: 0,
            detention: 0,
            due: 0,
            privileges_until: 0,
            letter: false,
            summons: false,
            wary: Vec::new(),
            inventory: Vec::new(),
            schemes: Vec::new(),
            run: None,
            campaign: None,
            knowledge: Vec::new(),
            ally_used: Vec::new(),
            done: Vec::new(),
            pulled: 0,
            caught: 0,
            pinned: 0,
            big: 0,
            attended: false,
            slipped: false,
            moves: 0,
            final_warning: false,
            next_scheme: 1,
            news: Vec::new(),
            history: Vec::new(),
            life: SchoolLife::default(),
        }
    }

    /// File points.
    #[must_use]
    pub fn file_points(&self) -> i32 {
        self.file.iter().map(|e| i32::from(e.sev)).sum()
    }

    /// Whether the player knows `what`.
    #[must_use]
    pub fn knows(&self, what: &str) -> bool {
        self.knowledge.iter().any(|k| k == what)
    }

    /// Learn something.
    pub fn learn(&mut self, what: &str) {
        if !self.knows(what) && self.knowledge.len() < KNOW_CAP {
            self.knowledge.push(what.to_owned());
        }
    }

    /// How many of an item is carried.
    #[must_use]
    pub fn count(&self, item: &str) -> u8 {
        map_get(&self.inventory, item).copied().unwrap_or(0)
    }

    /// A clique standing.
    #[must_use]
    pub fn standing_with(&self, clique: &str) -> i32 {
        map_get(&self.standing, clique).map_or(0, |v| i32::from(*v))
    }

    /// Change a clique standing.
    pub fn add_standing(&mut self, clique: &str, delta: i32) {
        let now = self.standing_with(clique) + delta;
        map_set(&mut self.standing, clique, to_i16(now.clamp(-100, 100)));
    }

    /// A staff member's wariness level.
    #[must_use]
    pub fn wary_of(&self, npc: &str) -> i32 {
        map_get(&self.wary, npc).map_or(0, |w| i32::from(w.level))
    }

    /// Keep the news short.
    pub fn say(&mut self, line: String) {
        self.news.push(line);
        if self.news.len() > 8 {
            self.news.remove(0);
        }
    }

    /// The scheme numbered `id`.
    #[must_use]
    pub fn scheme(&self, id: u16) -> Option<&Scheme> {
        self.schemes.iter().find(|s| s.id == id)
    }
}

// ---------------------------------------------------------------- the shared world

/// A big scheme's lasting effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Active {
    /// The effect id.
    pub id: String,
    /// The day it began.
    pub since: i64,
    /// The day it ends, if it does.
    pub until: Option<i64>,
    /// The player who did it (as the paper names them).
    pub by: String,
}
wire_struct!(Active {
    id,
    since,
    until,
    by
});

/// A line of the school paper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headline {
    /// The day it ran.
    pub day: i64,
    /// The words.
    pub text: String,
}
wire_struct!(Headline { day, text });

/// An entry in the hall of legend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HallEntry {
    /// The player's handle.
    pub name: String,
    /// Their legend this term.
    pub legend: u32,
}
wire_struct!(HallEntry { name, legend });

/// A past term's best.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YearEntry {
    /// The term.
    pub term: u32,
    /// The best player's handle.
    pub name: String,
    /// Their legend.
    pub legend: u32,
    /// How many big schemes changed the school that term.
    pub changes: u32,
}
wire_struct!(YearEntry {
    term,
    name,
    legend,
    changes
});

/// The school everyone shares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    /// The term number.
    pub term: u32,
    /// The first day of the term.
    pub term_start: i64,
    /// The last day the tick ran.
    pub last_day: i64,
    /// Effects in force.
    pub active: Vec<Active>,
    /// Patrol heat by place.
    pub patrol: Map<u8>,
    /// The paper, newest last.
    pub paper: Vec<Headline>,
    /// Legend this term, best first.
    pub hall: Vec<HallEntry>,
    /// Past terms.
    pub yearbook: Vec<YearEntry>,
    /// Big schemes this term.
    pub changes: u32,
}
wire_struct!(World {
    term,
    term_start,
    last_day,
    active,
    patrol,
    paper,
    hall,
    yearbook,
    changes
});

/// The most headlines kept.
pub const PAPER_CAP: usize = 8;
/// The most hall entries kept.
pub const HALL_CAP: usize = 8;
/// The most past terms kept.
pub const YEAR_CAP: usize = 6;

impl World {
    /// A new school whose first term starts on `day`.
    #[must_use]
    pub fn new(day: i64) -> Self {
        Self {
            term: 1,
            term_start: day,
            last_day: day - 1,
            active: Vec::new(),
            patrol: Vec::new(),
            paper: Vec::new(),
            hall: Vec::new(),
            yearbook: Vec::new(),
            changes: 0,
        }
    }

    /// Decode a stored world, or start one on `today`.
    #[must_use]
    pub fn from_bytes(bytes: Option<&[u8]>, today: i64) -> Self {
        bytes
            .and_then(crate::wire::decode)
            .unwrap_or_else(|| Self::new(today))
    }

    /// Encode.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        crate::wire::encode(self)
    }

    /// The day of the term, from 0.
    #[must_use]
    pub fn term_day(&self, today: i64) -> i64 {
        today - self.term_start
    }

    /// Whether an effect is in force.
    #[must_use]
    pub fn has(&self, effect: &str) -> bool {
        self.active.iter().any(|a| a.id == effect)
    }

    /// Patrol heat at a place.
    #[must_use]
    pub fn patrol_heat(&self, place: &str) -> i32 {
        map_get(&self.patrol, place).map_or(0, |v| i32::from(*v))
    }

    /// Add a headline.
    pub fn headline(&mut self, day: i64, text: String) {
        self.paper.push(Headline { day, text });
        if self.paper.len() > PAPER_CAP {
            self.paper.remove(0);
        }
    }

    /// Record a player's legend in the hall.
    pub fn hall_update(&mut self, name: &str, legend: u32) {
        match self.hall.iter_mut().find(|e| e.name == name) {
            Some(e) => e.legend = e.legend.max(legend),
            None => self.hall.push(HallEntry {
                name: name.to_owned(),
                legend,
            }),
        }
        self.hall
            .sort_by(|a, b| b.legend.cmp(&a.legend).then_with(|| a.name.cmp(&b.name)));
        self.hall.truncate(HALL_CAP);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::wire::{decode, encode};

    #[test]
    fn a_player_and_a_world_round_trip() {
        let mut p = Player::new("ann", 2, 20_000, "dorm");
        p.status = Status::Active;
        p.add_standing("strays", 7);
        p.learn("back_stairs");
        p.schemes.push(Scheme {
            id: 1,
            target: "marsh".into(),
            method: "salt_swap".into(),
            twists: vec!["timed".into()],
            getaway: "slip_away".into(),
            scout: 2,
            allies: vec![("sol".into(), "lookout".into())],
            when: Some((20_001, 5)),
            created: 20_000,
        });
        p.run = Some(Run {
            scheme: 1,
            phase: 1,
            flubs: 1,
            mishap_heat: 3,
            covered: true,
        });
        p.say("hello".into());
        assert_eq!(decode::<Player>(&encode(&p)), Some(p));
        let mut w = World::new(20_000);
        w.hall_update("ann", 10);
        w.headline(20_001, "x".into());
        w.active.push(Active {
            id: "deputy_moved".into(),
            since: 20_001,
            until: None,
            by: "ann".into(),
        });
        assert_eq!(World::from_bytes(Some(&w.to_bytes()), 0), w);
        assert_eq!(World::from_bytes(Some(&[1, 2, 3]), 5), World::new(5));
        assert_eq!(World::from_bytes(None, 5).term_day(9), 4);
    }

    #[test]
    fn the_hall_keeps_the_best_and_the_paper_stays_short() {
        let mut w = World::new(0);
        for i in 0..20u32 {
            w.hall_update(&format!("p{i}"), i);
            w.headline(i64::from(i), format!("h{i}"));
        }
        assert_eq!(w.hall.len(), HALL_CAP);
        assert_eq!(w.hall[0].name, "p19");
        assert_eq!(w.paper.len(), PAPER_CAP);
        w.hall_update("p19", 3);
        assert_eq!(w.hall[0].legend, 19, "legend never goes down in the hall");
    }
}
