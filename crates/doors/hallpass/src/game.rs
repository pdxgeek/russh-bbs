//! The engine: a `Game` is a character plus a snapshot of the shared world, and every change is an
//! [`Action`] that returns [`Event`]s. It reads time only from the day it is told and randomness
//! only from the `DoorRng` it is given, so a recorded run replays exactly.
//!
//! This file holds time, movement, the school day, the file and the term. Schemes are in
//! [`crate::scheme`].

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use russh_bbs_door::DoorRng;

use crate::cond::{Cond, Stat};
use crate::content::{Content, Npc};
use crate::state::{
    FileEntry, Player, Status, TermResult, Wary, World, map_get, map_set, to_i16, to_u8, to_u32,
};
use crate::world::WorldDelta;

/// Why an action was refused. The text is shown to the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub String);

impl Refused {
    pub(crate) fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The three phases of a scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Getting there unseen.
    Sneak,
    /// Doing the deed.
    Act,
    /// Getting away.
    Escape,
}

impl Phase {
    /// The phase at index `i`.
    #[must_use]
    pub const fn from_index(i: u8) -> Option<Self> {
        match i {
            0 => Some(Self::Sneak),
            1 => Some(Self::Act),
            2 => Some(Self::Escape),
            _ => None,
        }
    }

    /// Its content name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sneak => "sneak",
            Self::Act => "act",
            Self::Escape => "escape",
        }
    }
}

/// How a phase went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseResult {
    /// Clean.
    Clean,
    /// Went wrong, but you carried on.
    Flub,
    /// A cover story saved you from being caught.
    Covered,
    /// Caught.
    Busted,
}

/// What happened. Typed so tests can check outcomes; [`Event::describe`] gives the words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A line of narration.
    Say(String),
    /// A phase of a scheme was resolved.
    Phase {
        /// Which.
        phase: Phase,
        /// The chance in percent.
        chance: i32,
        /// The roll, 1 to 100.
        roll: i32,
        /// How it went.
        result: PhaseResult,
    },
    /// A scheme was pulled off.
    Pulled {
        /// The scheme.
        scheme: u16,
        /// Legend earned.
        legend: u32,
        /// Whether it was clean.
        clean: bool,
    },
    /// A scheme ended with the player caught.
    Busted {
        /// The scheme.
        scheme: u16,
        /// The phase.
        phase: Phase,
    },
    /// A pulled-off scheme was traced back to the player.
    Pinned {
        /// The scheme.
        scheme: u16,
    },
    /// A line was added to the file.
    FileEntry {
        /// The kind id.
        kind: String,
        /// Severity.
        sev: u8,
    },
    /// A trait was earned.
    TraitEarned(String),
    /// The school changed for everyone.
    WorldChange(String),
    /// The player was expelled.
    Expelled,
    /// The term ended.
    TermEnded(String),
    /// A scheme was timetabled.
    Scheduled {
        /// The scheme.
        scheme: u16,
        /// The day.
        day: i64,
        /// The period.
        slot: u8,
    },
}

impl Event {
    /// The words for an event.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Say(text) => text.clone(),
            Self::Phase {
                phase,
                chance,
                roll,
                result,
            } => {
                let what = match result {
                    PhaseResult::Clean => "clean",
                    PhaseResult::Flub => "a slip",
                    PhaseResult::Covered => "saved by a cover story",
                    PhaseResult::Busted => "caught",
                };
                format!("[{}] {chance}% - rolled {roll}: {what}.", phase.name())
            }
            Self::Pulled { legend, clean, .. } => {
                format!(
                    "Pulled off{}: +{legend} legend.",
                    if *clean { " cleanly" } else { ", messily" }
                )
            }
            Self::Busted { phase, .. } => format!("Caught during the {}.", phase.name()),
            Self::Pinned { .. } => "Somebody put two and two together.".to_owned(),
            Self::FileEntry { sev, .. } => format!("A line goes on your file (severity {sev})."),
            Self::TraitEarned(id) => format!("You have earned a reputation: {id}."),
            Self::WorldChange(id) => format!("The school has changed: {id}."),
            Self::Expelled => "Expelled.".to_owned(),
            Self::TermEnded(id) => format!("The term is over: {id}."),
            Self::Scheduled { day, slot, .. } => format!(
                "Set for {}, period {}.",
                [
                    "Monday",
                    "Tuesday",
                    "Wednesday",
                    "Thursday",
                    "Friday",
                    "Saturday",
                    "Sunday"
                ][Game::weekday(*day)],
                slot + 1
            ),
        }
    }
}

/// Everything a player can ask the game to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Pick a starting trait and begin.
    Enrol(String),
    /// Walk to another place.
    Go(String),
    /// Go to class (or serve detention) where you are.
    Attend,
    /// Study a subject in a place that allows it.
    Study(String),
    /// Hang about and listen.
    Loiter,
    /// Rest and keep your head down.
    Rest,
    /// Buy an item from the shop here.
    Buy(String),
    /// Start a scheme.
    Draft {
        /// The target id.
        target: String,
        /// The method id.
        method: String,
    },
    /// Put a twist on a scheme.
    AddTwist(u16, String),
    /// Take a twist off.
    RemoveTwist(u16, String),
    /// Choose how to get away.
    SetGetaway(u16, String),
    /// Throw a scheme away.
    Drop(u16),
    /// Watch the place for a period.
    Scout(u16),
    /// Ask someone for help.
    Recruit {
        /// The scheme.
        scheme: u16,
        /// The character.
        npc: String,
        /// The role.
        role: String,
    },
    /// Fix the time.
    Schedule {
        /// The scheme.
        scheme: u16,
        /// The day.
        day: i64,
        /// The period.
        slot: u8,
    },
    /// Begin the run.
    BeginRun(u16),
    /// Resolve the next phase, pushing with nerve.
    RunPhase {
        /// Nerve spent.
        push: u8,
    },
    /// Go to bed: end the day.
    Sleep,
    /// Answer the puzzle now up in a lesson with the option at this index.
    Answer(u8),
    /// Walk out of the lesson, or step out of the conversation, in progress.
    Abandon,
    /// Start a conversation with someone here.
    Talk(String),
    /// Say the choice at this index in the conversation under way.
    Choose(u8),
    /// Join a club at its sign-up place.
    Join(String),
    /// Leave a club (remembered for the term).
    Quit(String),
    /// Go to a meeting of one of your clubs, here and now.
    Meet,
    /// Play a structured scheme on another pupil, from the fixed list (H4).
    Plot {
        /// The plot template id.
        kind: String,
        /// The target's handle, from the roster.
        to: String,
    },
    /// Let a rumour drop, from a template (H4).
    Spread {
        /// The rumour template id.
        rumour: String,
        /// A character id, a clique id, or `self`.
        about: String,
    },
    /// Put your name on the election ballot (H4).
    Nominate,
    /// Work the corridors for your campaign (H4).
    Canvass,
    /// Vote for a name on the ballot (H4).
    Vote(String),
    /// Opt out of (or back into) player-versus-player play (H4).
    OptOut(bool),
    /// Search the place for something the east wing's mystery left (H4).
    Search,
}

/// A character in a school, and the rules that change them.
#[derive(Debug, Clone)]
pub struct Game {
    /// The content tables.
    pub content: Arc<Content>,
    /// The character.
    pub player: Player,
    /// The shared world as this session sees it.
    pub world: World,
    /// World changes the session still has to send to the board.
    pub pending: Vec<WorldDelta>,
    /// The roster entry last sent, so an unchanged one is not sent again.
    pub last_pupil: Option<crate::commons::Pupil>,
}

/// What a period was spent on, for the end-of-period checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Spent {
    Attended,
    Detention,
    Other,
}

impl Game {
    /// A game for `player` in `world`.
    #[must_use]
    pub fn new(content: Arc<Content>, player: Player, world: World) -> Self {
        Self {
            content,
            player,
            world,
            pending: Vec::new(),
            last_pupil: None,
        }
    }

    /// Start a visit: load or create the character, move to the current term and day.
    #[must_use]
    pub fn open(
        content: &Arc<Content>,
        name: &str,
        save: Option<Player>,
        world: Option<&[u8]>,
        today: i64,
    ) -> (Self, Vec<Event>) {
        let world = World::from_bytes(world, today);
        let rules = &content.rules;
        let mut player =
            save.unwrap_or_else(|| Player::new(name, world.term, today, &rules.start_place));
        let mut events = Vec::new();
        let mut game = Self {
            content: Arc::clone(content),
            player: Player::new(name, world.term, today, &rules.start_place),
            world,
            pending: Vec::new(),
            last_pupil: None,
        };
        if player.term < game.world.term {
            // A new term has begun since this character last played: close the old one's book.
            game.player = player;
            if game.player.status != Status::New {
                events.extend(game.finish_term());
            }
            player = Player::new(name, game.world.term, today, &rules.start_place);
            player.history = std::mem::take(&mut game.player.history);
            // What follows a pupil from term to term: the opt-out, the awards, the post read.
            let old = std::mem::take(&mut game.player.social);
            player.social.opt_out = old.opt_out;
            player.social.awards = old.awards;
            player.social.seen_plot = old.seen_plot;
        }
        name.clone_into(&mut player.name);
        game.player = player;
        events.extend(game.sync_day(today));
        events.extend(game.social_day());
        (game, events)
    }

    // ------------------------------------------------------------ time

    /// The weekday of `day`, 0 = Monday.
    #[must_use]
    pub fn weekday(day: i64) -> usize {
        usize::try_from((day + 3).rem_euclid(7)).unwrap_or(0)
    }

    /// The number of periods in a school day.
    #[must_use]
    pub fn periods(&self) -> usize {
        self.content.periods.len()
    }

    /// The activity of `slot` on `day`, e.g. `class:maths` or `lunch`.
    #[must_use]
    pub fn activity(&self, day: i64, slot: usize) -> &str {
        self.content
            .days
            .iter()
            .find(|d| usize::from(d.weekday) == Self::weekday(day))
            .and_then(|d| d.slots.get(slot))
            .map_or("free", String::as_str)
    }

    /// The kind of a slot: its activity without the subject.
    #[must_use]
    pub fn kind(&self, day: i64, slot: usize) -> &str {
        self.activity(day, slot).split(':').next().unwrap_or("free")
    }

    /// The current period index, or `None` once the day is over.
    #[must_use]
    pub fn current_slot(&self) -> Option<usize> {
        let slot = usize::from(self.player.slot);
        (slot < self.periods()).then_some(slot)
    }

    /// The day of the term.
    #[must_use]
    pub fn term_day(&self) -> i64 {
        self.world.term_day(self.player.day)
    }

    /// Whether the term has run out of days.
    #[must_use]
    pub fn term_over(&self) -> bool {
        self.term_day() >= self.content.rules.term_days
    }

    /// Whether the character can still act.
    #[must_use]
    pub fn playing(&self) -> bool {
        self.player.status == Status::Active && !self.term_over()
    }

    /// Bring the character to `today`: apply the days that passed, then start the morning.
    pub fn sync_day(&mut self, today: i64) -> Vec<Event> {
        let mut events = Vec::new();
        if self.player.status != Status::Active || today <= self.player.day {
            if self.player.status == Status::New {
                self.player.day = today;
            }
            return events;
        }
        let missed = today - self.player.day;
        let rules = self.content.rules.clone();
        let steps = missed.min(rules.catch_up_days);
        for i in 0..steps {
            let day = today - steps + i + 1;
            self.decay_one_day(day);
        }
        self.player.day = today;
        self.player.slot = 0;
        self.player.attended = false;
        self.player.slipped = false;
        self.player.moves = to_u8(i32::try_from(rules.moves_per_slot).unwrap_or(3));
        self.player.place.clone_from(&rules.start_place);
        self.player.nerve = self.player.nerve_max;
        // One detention a day falls due; new ones given today wait for tomorrow.
        self.player.due = self.player.detention.min(1);
        // A lesson or a conversation left open overnight is over.
        if self.player.life.lesson.take().is_some() {
            events.push(Event::Say(
                "You left a lesson unfinished; the class moved on without you.".into(),
            ));
        }
        self.player.life.scene = None;
        // A run that was not finished before the day ended is abandoned.
        if let Some(run) = self.player.run.take() {
            self.player.schemes.retain(|s| s.id != run.scheme);
            events.push(Event::Say(
                "You left a scheme half done overnight; it has gone cold.".into(),
            ));
        }
        let lapsed: Vec<u16> = self
            .player
            .schemes
            .iter()
            .filter(|s| s.when.is_some_and(|(d, _)| d < today))
            .map(|s| s.id)
            .collect();
        for id in lapsed {
            if let Some(s) = self.player.schemes.iter_mut().find(|s| s.id == id) {
                s.when = None;
            }
            events.push(Event::Say(format!(
                "Scheme #{id} missed its moment and needs a new time."
            )));
        }
        self.allowance(steps.min(3));
        events.extend(self.weekly_report(today));
        events.extend(self.check_traits());
        events.extend(self.check_arcs());
        if self.term_over() {
            events.extend(self.finish_term());
            return events;
        }
        if self.player.summons {
            events.extend(self.report_to_headmistress());
        }
        events.push(Event::Say(format!(
            "Morning bell: day {} of {}.",
            self.term_day() + 1,
            rules.term_days
        )));
        events
    }

    /// Friday's report: good marks are noticed and poor ones go on the file.
    fn weekly_report(&mut self, today: i64) -> Vec<Event> {
        if Self::weekday(today) != 4 {
            return Vec::new();
        }
        let rules = self.content.rules.clone();
        let avg = self.average_grade();
        let mut events = Vec::new();
        if avg >= rules.honour_roll {
            self.player.cool = to_i16(i32::from(self.player.cool) + 2);
            let teachers: Vec<String> = self
                .content
                .subjects
                .iter()
                .map(|s| s.teacher.clone())
                .collect();
            for t in teachers {
                self.player.life.add_bond(&t, 1);
            }
            events.push(Event::Say(
                "Friday's report puts you on the honour roll. The staff think better of you."
                    .into(),
            ));
        } else if self
            .player
            .grades
            .iter()
            .any(|(_, g)| i32::from(*g) < rules.poor_grade)
        {
            events.push(Event::Say(
                "Friday's report has a poor mark on it, and a teacher's initials.".into(),
            ));
            events.extend(self.add_file("poor_report", 1));
        }
        events
    }

    fn allowance(&mut self, days: i64) {
        let rules = &self.content.rules;
        let mut per_day = rules.allowance;
        if self.player.letter {
            per_day /= 2;
        }
        if self.player.privileges_until > self.player.day {
            per_day = 0;
        }
        self.player.cash += per_day * i32::try_from(days).unwrap_or(0);
    }

    fn decay_one_day(&mut self, day: i64) {
        let rules = self.content.rules.clone();
        let low = self.player.heat > 0 && self.player.run.is_none();
        let _ = low;
        self.player.heat = to_u8(i32::from(self.player.heat) - rules.heat_decay);
        // Marks slip when they are not kept up.
        if day.rem_euclid(rules.grade_decay_days.max(1)) == 0 {
            for (_, g) in &mut self.player.grades {
                if *g > 30 {
                    *g -= 1;
                }
            }
        }
        // Notoriety fades slowly, but never below a trait's floor.
        if day.rem_euclid(rules.notoriety_decay_every.max(1)) == 0 {
            self.player.notoriety = to_u8(i32::from(self.player.notoriety) - 1);
        }
        let floor = self.notoriety_floor();
        self.player.notoriety = self.player.notoriety.max(to_u8(floor));
        for (_, w) in &mut self.player.wary {
            if w.level > 0 && day - w.last >= rules.wary_decay_days {
                w.level -= 1;
                w.last = day;
            }
        }
        self.player.wary.retain(|(_, w)| w.level > 0);
        // A week of good conduct strikes a point off the file.
        if self.player.file.is_empty() {
            self.player.clean_days = 0;
        } else {
            self.player.clean_days = self.player.clean_days.saturating_add(1);
        }
        let need = if self.has_clean_trait() {
            rules.clean_days_pet
        } else {
            rules.clean_days
        };
        if i32::from(self.player.clean_days) >= need && !self.player.file.is_empty() {
            self.player.clean_days = 0;
            if let Some(first) = self.player.file.first_mut() {
                first.sev = first.sev.saturating_sub(1);
            }
            self.player.file.retain(|e| e.sev > 0);
        }
        self.refresh_tiers();
        if self.player.final_warning && self.player.file_points() < rules.file_full {
            self.player.final_warning = false;
        }
        self.player
            .ally_used
            .retain(|(_, d)| day - d <= rules.ally_cooldown_days);
    }

    fn has_clean_trait(&self) -> bool {
        self.player
            .traits
            .iter()
            .filter_map(|t| self.content.trait_spec(t))
            .any(|t| t.clean_days)
    }

    fn notoriety_floor(&self) -> i32 {
        self.player
            .traits
            .iter()
            .filter_map(|t| self.content.trait_spec(t))
            .map(|t| t.notoriety_floor)
            .max()
            .unwrap_or(0)
    }

    /// Clear the bit of every file level the file has fallen below, so crossing it again counts.
    fn refresh_tiers(&mut self) {
        let points = self.player.file_points();
        for (i, tier) in self.content.tiers.iter().enumerate() {
            if points < tier.at {
                self.player.tiers_hit &= !(1u8 << i.min(7));
            }
        }
    }

    // ------------------------------------------------------------ conditions and numbers

    /// A skill's level: from experience, plus trait bonuses.
    #[must_use]
    pub fn skill_level(&self, skill: &str) -> i32 {
        let xp = i32::from(map_get(&self.player.xp, skill).copied().unwrap_or(0));
        let from_xp = self
            .content
            .rules
            .skill_levels
            .iter()
            .filter(|need| xp >= **need)
            .count()
            .saturating_sub(1);
        let bonus: i32 = self
            .player
            .traits
            .iter()
            .filter_map(|t| self.content.trait_spec(t))
            .flat_map(|t| t.skills.iter())
            .filter(|(s, _)| s == skill)
            .map(|(_, v)| *v)
            .sum();
        i32::try_from(from_xp).unwrap_or(0) + bonus
    }

    /// Average grade.
    #[must_use]
    pub fn average_grade(&self) -> i32 {
        let total: i32 = self.player.grades.iter().map(|(_, g)| i32::from(*g)).sum();
        let n = i32::try_from(self.player.grades.len()).unwrap_or(1).max(1);
        total / n
    }

    /// Whether the character has a capability from a trait.
    #[must_use]
    pub fn has_tag(&self, tag: &str) -> bool {
        self.player
            .traits
            .iter()
            .filter_map(|t| self.content.trait_spec(t))
            .any(|t| t.tags.iter().any(|x| x == tag))
    }

    /// The number a stat names.
    #[must_use]
    pub fn stat(&self, stat: &Stat) -> i32 {
        let p = &self.player;
        match stat {
            Stat::Legend => i32::try_from(p.legend).unwrap_or(i32::MAX),
            Stat::Notoriety => i32::from(p.notoriety),
            Stat::Heat => i32::from(p.heat),
            Stat::File => p.file_points(),
            Stat::Cool => i32::from(p.cool),
            Stat::Nerve => i32::from(p.nerve),
            Stat::Cash => p.cash,
            Stat::Grades => self.average_grade(),
            Stat::Pulled => i32::from(p.pulled),
            Stat::Caught => i32::from(p.caught),
            Stat::Skill(id) => self.skill_level(id),
            Stat::Standing(id) => p.standing_with(id),
            Stat::Wary(id) => p.wary_of(id),
            Stat::Grade(id) => map_get(&p.grades, id).map_or(0, |g| i32::from(*g)),
            Stat::Bond(id) => p.life.bond_with(id),
            Stat::Rank(id) => p.life.rank_in(id),
            Stat::Day => i32::try_from(self.term_day()).unwrap_or(i32::MAX),
            Stat::Clues => i32::try_from(
                self.player
                    .knowledge
                    .iter()
                    .filter(|k| k.starts_with("clue_"))
                    .count(),
            )
            .unwrap_or(0),
        }
    }

    /// Whether a condition holds for this character in this school.
    #[must_use]
    pub fn holds(&self, cond: &Cond) -> bool {
        match cond {
            Cond::Tag(t) => self.has_tag(t),
            Cond::Trait(t) => self.player.traits.iter().any(|x| x == t),
            Cond::Item(i) => self.player.count(i) > 0,
            Cond::Know(k) => self.player.knows(k),
            Cond::World(w) => self.world.has(w),
            Cond::Arc { arc, step } => self
                .player
                .life
                .arc(arc)
                .is_some_and(|a| a.ended.is_none() && a.step == *step),
            Cond::Ended { arc, ending } => self
                .player
                .life
                .arc(arc)
                .is_some_and(|a| a.ended.as_deref() == Some(ending.as_str())),
            Cond::Expelled => self.player.status == Status::Expelled,
            Cond::Not(inner) => !self.holds(inner),
            Cond::Cmp { stat, op, value } => op.test(self.stat(stat), *value),
        }
    }

    /// Whether all of `conds` hold.
    #[must_use]
    pub fn all_hold(&self, conds: &[Cond]) -> bool {
        conds.iter().all(|c| self.holds(c))
    }

    // ------------------------------------------------------------ places and people

    /// Where a character is on `day` in `slot`.
    #[must_use]
    pub fn npc_place<'a>(&'a self, npc: &'a Npc, day: i64, slot: usize) -> &'a str {
        if let Some(moved) = self
            .content
            .effects
            .iter()
            .filter(|e| self.world.has(&e.id))
            .flat_map(|e| e.moves.iter())
            .find(|(n, _)| *n == npc.id)
        {
            return moved.1.as_str();
        }
        let activity = self.activity(day, slot);
        let kind = self.kind(day, slot);
        let weekday = u8::try_from(Self::weekday(day)).unwrap_or(0);
        npc.schedule
            .iter()
            .find(|r| {
                (r.when == "*" || r.when == activity || r.when == kind)
                    && (r.days.is_empty() || r.days.contains(&weekday))
            })
            .map_or(npc.home.as_str(), |r| r.place.as_str())
    }

    /// Characters at `place` on `day` in `slot`.
    #[must_use]
    pub fn npcs_at(&self, place: &str, day: i64, slot: usize) -> Vec<&Npc> {
        self.content
            .npcs
            .iter()
            .filter(|n| self.npc_place(n, day, slot) == place)
            .collect()
    }

    /// Staff and prefects at `place`.
    #[must_use]
    pub fn witnesses_at(&self, place: &str, day: i64, slot: usize) -> i32 {
        i32::try_from(
            self.npcs_at(place, day, slot)
                .iter()
                .filter(|n| n.is_authority())
                .count(),
        )
        .unwrap_or(0)
    }

    /// The patrol level at a place: the places prefects and staff keep returning to.
    #[must_use]
    pub fn patrol_level(&self, place: &str) -> i32 {
        let base = self.world.patrol_heat(place) / self.content.rules.patrol_threshold.max(1);
        let tag = i32::from(
            self.content
                .place(place)
                .is_some_and(|p| p.tags.iter().any(|t| t == "patrolled")),
        );
        let moved: i32 = self
            .content
            .effects
            .iter()
            .filter(|e| self.world.has(&e.id))
            .map(|e| e.patrol_mod)
            .sum();
        (base + tag + moved).max(0)
    }

    /// Whether a place is open to the character on `day` in `slot`: its hours, no world effect
    /// closing it, and the lock if it has one.
    #[must_use]
    pub fn place_open(&self, place: &str, day: i64, slot: usize) -> bool {
        let Some(spec) = self.content.place(place) else {
            return false;
        };
        if !spec.hours.iter().any(|h| h == self.kind(day, slot)) {
            return false;
        }
        if self
            .content
            .effects
            .iter()
            .any(|e| self.world.has(&e.id) && e.closes.iter().any(|c| c == place))
        {
            return false;
        }
        spec.lock.is_none() || spec.unlock.iter().any(|c| self.holds(c))
    }

    /// Places reachable this period by walking through open places.
    #[must_use]
    pub fn reachable(&self) -> Vec<String> {
        self.routes().into_iter().map(|(place, _)| place).collect()
    }

    /// Places reachable this period, with the corridors each walk costs.
    #[must_use]
    pub fn routes(&self) -> Vec<(String, u8)> {
        let Some(slot) = self.current_slot() else {
            return Vec::new();
        };
        let day = self.player.day;
        let night = self.kind(day, slot) == "night";
        let mut seen: HashSet<&str> = HashSet::new();
        let mut queue: VecDeque<(&str, usize)> = VecDeque::new();
        seen.insert(self.player.place.as_str());
        queue.push_back((self.player.place.as_str(), 0));
        let mut out = Vec::new();
        while let Some((here, hops)) = queue.pop_front() {
            if hops >= usize::from(self.player.moves) {
                continue;
            }
            for next in self.neighbours(here) {
                if seen.contains(next) {
                    continue;
                }
                // After lights-out the corridors are walked in the dark: any unlocked way through is
                // possible, but only some places can be stood in (see `place_open`).
                let passable = self.place_open(next, day, slot)
                    || (night && self.content.place(next).is_some_and(|p| p.lock.is_none()));
                if !passable {
                    continue;
                }
                seen.insert(next);
                if self.place_open(next, day, slot) {
                    out.push((next.to_owned(), to_u8(i32::try_from(hops + 1).unwrap_or(0))));
                }
                queue.push_back((next, hops + 1));
            }
        }
        out.sort();
        out
    }

    /// Places next to `place`, in either direction.
    fn neighbours(&self, place: &str) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        if let Some(spec) = self.content.place(place) {
            out.extend(spec.exits.iter().map(String::as_str));
        }
        out.extend(
            self.content
                .places
                .iter()
                .filter(|p| p.exits.iter().any(|e| e == place))
                .map(|p| p.id.as_str()),
        );
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Whether `a` and `b` are the same place or next to each other.
    #[must_use]
    pub fn near(&self, a: &str, b: &str) -> bool {
        a == b || self.neighbours(a).contains(&b)
    }

    /// The description of a place now.
    #[must_use]
    pub fn describe_place(&self, place: &str) -> String {
        let Some(spec) = self.content.place(place) else {
            return String::new();
        };
        let kind = self
            .current_slot()
            .map_or("night", |s| self.kind(self.player.day, s));
        spec.change
            .iter()
            .find(|c| c.kinds.iter().any(|k| k == kind))
            .map_or(spec.look.as_str(), |c| c.text.as_str())
            .to_owned()
    }

    // ------------------------------------------------------------ the school day

    /// Finish the current period, run the end-of-period checks, and move to the next.
    pub(crate) fn end_slot(&mut self, spent: Spent, rng: &mut DoorRng) -> Vec<Event> {
        let mut events = Vec::new();
        let Some(slot) = self.current_slot() else {
            return events;
        };
        let day = self.player.day;
        let kind = self.kind(day, slot).to_owned();
        if kind == "class" && spent != Spent::Attended {
            events.extend(self.skipped_class(slot, rng));
        }
        if kind == "after" && self.player.due > 0 {
            self.player.due = 0;
            if spent == Spent::Detention {
                self.player.detention = self.player.detention.saturating_sub(1);
                events.push(Event::Say("You serve your detention in silence.".into()));
            } else {
                events.extend(self.add_file("skipped_detention", 2));
            }
        }
        self.player.attended = false;
        self.player.slipped = false;
        self.player.moves = to_u8(i32::try_from(self.content.rules.moves_per_slot).unwrap_or(3));
        self.player.slot += 1;
        if self.current_slot().is_some() {
            self.tidy_position();
        }
        events
    }

    /// If the place has closed for the new period, the character is shown out.
    fn tidy_position(&mut self) {
        let Some(slot) = self.current_slot() else {
            return;
        };
        let day = self.player.day;
        if self.place_open(&self.player.place, day, slot) {
            return;
        }
        let fallback = self
            .neighbours(&self.player.place)
            .into_iter()
            .find(|p| self.place_open(p, day, slot))
            .map_or_else(|| self.content.rules.start_place.clone(), str::to_owned);
        self.player.place = fallback;
    }

    fn skipped_class(&mut self, slot: usize, rng: &mut DoorRng) -> Vec<Event> {
        let day = self.player.day;
        let subject = self
            .activity(day, slot)
            .strip_prefix("class:")
            .unwrap_or("")
            .to_owned();
        let teacher = self.content.subject(&subject).map(|s| s.teacher.clone());
        let wary = teacher.as_deref().map_or(0, |t| self.player.wary_of(t));
        let rules = &self.content.rules;
        let teacher_bond = teacher
            .as_deref()
            .map_or(0, |t| self.player.life.bond_with(t));
        let grade_now = i32::from(map_get(&self.player.grades, &subject).copied().unwrap_or(0));
        let chance = (rules.skip_notice_base - (grade_now - 55) / 6 - teacher_bond / 4
            + wary * 8
            + i32::from(self.player.heat) / 4
            + i32::from(self.player.notoriety) / 4
            - i32::from(self.player.cool) / 8
            - if self.has_tag("trusted") { 10 } else { 0 })
        .clamp(3, 90);
        let noticed = i32::try_from(rng.below(100)).unwrap_or(0) < chance;
        let grade = map_get(&self.player.grades, &subject).copied().unwrap_or(0);
        map_set(
            &mut self.player.grades,
            &subject,
            to_u8(i32::from(grade) - 2),
        );
        if noticed {
            self.player.detention = self.player.detention.saturating_add(1);
            self.player.notoriety = to_u8(i32::from(self.player.notoriety) + 1);
            vec![Event::Say(format!(
                "Your empty seat in {subject} was noticed: a detention."
            ))]
        } else {
            vec![Event::Say("Nobody noticed you were not in class.".into())]
        }
    }

    // ------------------------------------------------------------ the file

    /// Write a line on the file, and carry out what that costs.
    pub fn add_file(&mut self, kind: &str, sev: u8) -> Vec<Event> {
        let mut events = Vec::new();
        let rules = self.content.rules.clone();
        if self.player.file_points() >= rules.file_full && self.player.status == Status::Active {
            // The file was already full: this is the end.
            events.push(Event::FileEntry {
                kind: kind.to_owned(),
                sev,
            });
            events.extend(self.expel());
            return events;
        }
        let day = self.player.day;
        self.player.file.push(FileEntry {
            day,
            kind: kind.to_owned(),
            sev,
        });
        if self.player.file.len() > crate::state::LIST_CAP {
            self.player.file.remove(0);
        }
        self.player.clean_days = 0;
        self.player.notoriety = to_u8(i32::from(self.player.notoriety) + i32::from(sev));
        let text = self.content.file_kind_text(kind).to_owned();
        events.push(Event::FileEntry {
            kind: kind.to_owned(),
            sev,
        });
        events.push(Event::Say(format!("On your file: {text}.")));
        let points = self.player.file_points();
        for (i, tier) in self.content.tiers.clone().iter().enumerate() {
            let bit = 1u8 << i.min(7);
            if points >= tier.at && self.player.tiers_hit & bit == 0 {
                self.player.tiers_hit |= bit;
                events.push(Event::Say(tier.text.clone()));
                match tier.effect.as_str() {
                    "detention" => {
                        self.player.detention =
                            self.player.detention.saturating_add(to_u8(tier.amount));
                    }
                    "privileges" => self.player.privileges_until = day + i64::from(tier.amount),
                    "letter" => {
                        self.player.letter = true;
                        self.player.cool = to_i16(i32::from(self.player.cool) - 5);
                    }
                    _ => self.player.summons = true,
                }
            }
        }
        if points >= rules.file_full && !self.player.final_warning {
            self.player.final_warning = true;
            events.push(Event::Say("Your file is full. One more line, or the end of term like this, and you are expelled.".into()));
        }
        events
    }

    /// Dame Vane sees you.
    fn report_to_headmistress(&mut self) -> Vec<Event> {
        self.player.summons = false;
        self.player.detention = self.player.detention.saturating_add(2);
        self.bump_wary("vane", 1);
        self.player.slot = 1;
        vec![Event::Say(
            "You spend first period in the headmistress's office, being looked at. Two more detentions, and she remembers your face.".into(),
        )]
    }

    /// Raise a staff member's wariness.
    pub fn bump_wary(&mut self, npc: &str, by: i32) {
        let day = self.player.day;
        let level = to_u8((self.player.wary_of(npc) + by).min(6));
        map_set(&mut self.player.wary, npc, Wary { level, last: day });
    }

    fn expel(&mut self) -> Vec<Event> {
        self.player.status = Status::Expelled;
        self.player.run = None;
        self.player.schemes.clear();
        let mut events = vec![Event::Expelled];
        events.extend(self.record_term("expelled"));
        events
    }

    // ------------------------------------------------------------ traits

    /// Earn any trait whose conditions now hold.
    pub fn check_traits(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let candidates: Vec<String> = self
            .content
            .traits
            .iter()
            .filter(|t| !t.start && !self.player.traits.contains(&t.id) && self.all_hold(&t.earn))
            .map(|t| t.id.clone())
            .collect();
        for id in candidates {
            self.player.traits.push(id.clone());
            if id == "known_troublemaker" {
                self.player.add_standing("the_bench", 10);
                self.player.add_standing("lamplighters", -10);
            }
            events.push(Event::TraitEarned(id));
        }
        events
    }

    // ------------------------------------------------------------ the term

    /// End the term for this character.
    pub fn finish_term(&mut self) -> Vec<Event> {
        if self.player.status == Status::Finished || self.player.status == Status::Expelled {
            return Vec::new();
        }
        let full = self.player.file_points() >= self.content.rules.file_full;
        if full {
            return self.expel();
        }
        let finales = self.club_finales();
        let ending = self
            .content
            .endings
            .iter()
            .find(|e| self.all_hold(&e.requires))
            .map_or_else(|| "scraped_through".to_owned(), |e| e.id.clone());
        self.player.status = Status::Finished;
        self.player.run = None;
        let mut events = finales;
        events.extend(self.record_term(&ending));
        events.push(Event::TermEnded(ending));
        events
    }

    fn record_term(&mut self, ending: &str) -> Vec<Event> {
        let result = TermResult {
            term: self.player.term,
            ending: ending.to_owned(),
            legend: self.player.legend,
            file: to_u8(self.player.file_points()),
            grades: to_u8(self.average_grade()),
        };
        self.player.history.push(result);
        if self.player.history.len() > 6 {
            self.player.history.remove(0);
        }
        let text = self
            .content
            .endings
            .iter()
            .find(|e| e.id == ending)
            .map(|e| e.text.clone());
        text.map(|t| vec![Event::Say(t)]).unwrap_or_default()
    }

    /// The ending of the latest finished term, if any.
    #[must_use]
    pub fn latest_result(&self) -> Option<&TermResult> {
        self.player.history.last()
    }

    // ------------------------------------------------------------ plain actions

    /// Do something. Returns the events, or why not.
    pub fn apply(&mut self, action: &Action, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        if let Action::Enrol(id) = action {
            return self.enrol(id);
        }
        if !self.playing() {
            return Err(Refused::new("You are not in school right now."));
        }
        if self.player.run.is_some() && !matches!(action, Action::RunPhase { .. }) {
            return Err(Refused::new("A scheme is under way: finish it first."));
        }
        if self.player.life.lesson.is_some()
            && !matches!(action, Action::Answer(_) | Action::Abandon)
        {
            return Err(Refused::new("You are in the middle of a lesson."));
        }
        if self.player.life.scene.is_some()
            && !matches!(action, Action::Choose(_) | Action::Abandon)
        {
            return Err(Refused::new("You are in the middle of a conversation."));
        }
        let mut result = self.dispatch(action, rng);
        if let Ok(events) = &mut result {
            events.extend(self.check_arcs());
        }
        result
    }

    fn dispatch(&mut self, action: &Action, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        match action {
            Action::Answer(i) => self.answer(*i, rng),
            Action::Abandon => self.abandon(rng),
            Action::Talk(id) => self.start_talk(id),
            Action::Choose(i) => self.choose(*i, rng),
            Action::Join(id) => self.join_club(id),
            Action::Quit(id) => self.quit_club(id),
            Action::Meet => self.meet(rng),
            Action::Plot { kind, to } => self.send_plot(kind, to, rng),
            Action::Spread { rumour, about } => self.spread(rumour, about, rng),
            Action::Nominate => self.nominate(),
            Action::Canvass => self.canvass(rng),
            Action::Vote(who) => self.cast_vote(who),
            Action::OptOut(out) => Ok(self.set_opt_out(*out)),
            Action::Search => self.search(rng),
            Action::Enrol(_) => Err(Refused::new("You have already enrolled.")),
            Action::Go(place) => self.go(place, rng),
            Action::Attend => self.attend(rng),
            Action::Study(subject) => self.study(subject, rng),
            Action::Loiter => self.loiter(rng),
            Action::Rest => self.rest(rng),
            Action::Buy(item) => self.buy(item),
            Action::Sleep => self.sleep(rng),
            scheme_action => self.scheme_action(scheme_action, rng),
        }
    }

    fn enrol(&mut self, trait_id: &str) -> Result<Vec<Event>, Refused> {
        if self.player.status != Status::New {
            return Err(Refused::new("You have already enrolled."));
        }
        let spec = self
            .content
            .trait_spec(trait_id)
            .filter(|t| t.start)
            .cloned()
            .ok_or_else(|| Refused::new("That is not a way to start."))?;
        let rules = self.content.rules.clone();
        let p = &mut self.player;
        p.status = Status::Active;
        p.traits = vec![spec.id.clone()];
        p.nerve_max = to_u8(rules.start_nerve_max + spec.nerve);
        p.nerve = p.nerve_max;
        p.cool = to_i16(rules.start_cool + spec.cool);
        p.cash = rules.start_cash
            + i32::from(p.social.unpaid) * self.content.social.rules.award_cash;
        p.social.unpaid = 0;
        p.grades = self
            .content
            .subjects
            .iter()
            .map(|s| (s.id.clone(), to_u8(rules.start_grade)))
            .collect();
        for c in &self.content.cliques {
            p.standing.push((c.id.clone(), 0));
        }
        for (clique, v) in &spec.standing {
            p.add_standing(clique, *v);
        }
        p.place.clone_from(&rules.start_place);
        p.moves = to_u8(i32::try_from(rules.moves_per_slot).unwrap_or(3));
        Ok(vec![Event::Say(format!(
            "You step off the coach at Greywater Academy as a {}.",
            spec.name.to_lowercase()
        ))])
    }

    fn go(&mut self, place: &str, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        let slot = self
            .current_slot()
            .ok_or_else(|| Refused::new("Lights out: the day is over."))?;
        if place == self.player.place {
            return Err(Refused::new("You are already here."));
        }
        let Some(cost) = self
            .routes()
            .into_iter()
            .find(|(p, _)| p == place)
            .map(|(_, c)| c)
        else {
            return Err(Refused::new("You cannot get there from here this period."));
        };
        let day = self.player.day;
        if self.kind(day, slot) == "night" && !self.player.slipped {
            // After lights-out, leaving the dorm is a risk.
            if self.player.nerve == 0 {
                return Err(Refused::new(
                    "You do not have the nerve to slip out tonight.",
                ));
            }
            self.player.nerve -= 1;
            let chance = (self.content.rules.slip_out_base + 6 * self.skill_level("sneak")
                - 8 * self.patrol_level(place)
                - 6 * self.witnesses_at(&self.player.place, day, slot)
                - i32::from(self.player.heat) / 5)
                .clamp(5, 95);
            if i32::try_from(rng.below(100)).unwrap_or(0) >= chance {
                let mut events = vec![Event::Say("A torch beam finds you on the stairs. Back to bed, and a note to your housemaster.".into())];
                self.player
                    .place
                    .clone_from(&self.content.rules.start_place);
                self.player.detention = self.player.detention.saturating_add(1);
                events.extend(self.add_file("out_of_bounds", 1));
                events.extend(self.end_slot(Spent::Other, rng));
                return Ok(events);
            }
            self.player.slipped = true;
            self.gain_xp("sneak", 1);
        }
        place.clone_into(&mut self.player.place);
        self.player.moves = self.player.moves.saturating_sub(cost);
        let name = self
            .content
            .place(place)
            .map_or(place, |p| p.name.as_str())
            .to_owned();
        Ok(vec![Event::Say(format!("You go to {name}."))])
    }

    /// Grow a skill by use.
    pub(crate) fn gain_xp(&mut self, skill: &str, amount: i32) -> Option<String> {
        let before = self.skill_level(skill);
        let xp = i32::from(map_get(&self.player.xp, skill).copied().unwrap_or(0)) + amount;
        map_set(
            &mut self.player.xp,
            skill,
            u16::try_from(xp.clamp(0, i32::from(u16::MAX))).unwrap_or(0),
        );
        (self.skill_level(skill) > before).then(|| skill.to_owned())
    }

    fn attend(&mut self, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        let slot = self
            .current_slot()
            .ok_or_else(|| Refused::new("The day is over."))?;
        let day = self.player.day;
        let here = self.player.place.clone();
        let tags_detention = self
            .content
            .place(&here)
            .is_some_and(|p| p.tags.iter().any(|t| t == "detention"));
        if self.kind(day, slot) == "after" && self.player.due > 0 && tags_detention {
            return Ok(self.end_slot(Spent::Detention, rng));
        }
        let activity = self.activity(day, slot).to_owned();
        let Some(subject_id) = activity.strip_prefix("class:") else {
            return Err(Refused::new("There is no lesson this period."));
        };
        let subject = self
            .content
            .subject(subject_id)
            .cloned()
            .ok_or_else(|| Refused::new("There is no such lesson."))?;
        if subject.room != here {
            return Err(Refused::new(format!(
                "The lesson is in the {}.",
                self.content
                    .place(&subject.room)
                    .map_or("classroom", |p| p.name.as_str())
            )));
        }
        self.begin_lesson(subject_id, rng)
    }

    fn study(&mut self, subject_id: &str, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        self.current_slot()
            .ok_or_else(|| Refused::new("The day is over."))?;
        let here = self
            .content
            .place(&self.player.place)
            .ok_or_else(|| Refused::new("You are nowhere."))?;
        if !here.tags.iter().any(|t| t == "study") {
            return Err(Refused::new("There is nowhere to study here."));
        }
        let name = self
            .content
            .subject(subject_id)
            .map(|s| s.name.clone())
            .ok_or_else(|| Refused::new("No such subject."))?;
        let bonus: i32 = self
            .player
            .traits
            .iter()
            .filter_map(|t| self.content.trait_spec(t))
            .map(|t| t.grade_bonus)
            .sum();
        let restricted = here.tags.iter().any(|t| t == "restricted");
        let gain = 3 + bonus + if restricted { 2 } else { 0 };
        let grade = i32::from(
            map_get(&self.player.grades, subject_id)
                .copied()
                .unwrap_or(0),
        );
        map_set(
            &mut self.player.grades,
            subject_id,
            to_u8((grade + gain).min(100)),
        );
        let mut events = vec![Event::Say(format!(
            "An hour of {name} under the green lamps (+{gain}).",
        ))];
        events.extend(self.end_slot(Spent::Other, rng));
        Ok(events)
    }

    fn loiter(&mut self, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        let slot = self
            .current_slot()
            .ok_or_else(|| Refused::new("The day is over."))?;
        let day = self.player.day;
        let others: Vec<(String, String, String)> = self
            .npcs_at(&self.player.place, day, slot)
            .iter()
            .filter(|n| !n.is_authority())
            .filter_map(|n| n.clique.clone().map(|c| (n.id.clone(), n.name.clone(), c)))
            .collect();
        let mut events = Vec::new();
        if others.is_empty() {
            events.push(Event::Say("You hang about. Nobody much is here.".into()));
        } else {
            let pick =
                usize::try_from(rng.below(u32::try_from(others.len()).unwrap_or(1))).unwrap_or(0);
            if let Some((id, name, clique)) = others.get(pick) {
                self.player.add_standing(clique, 1);
                events.push(Event::Say(format!(
                    "You pass the time with {name}. Word gets round that you are all right."
                )));
                events.extend(self.change_bond(id, 1));
            }
        }
        self.player.cool = to_i16(i32::from(self.player.cool) + 1);
        events.extend(self.end_slot(Spent::Other, rng));
        Ok(events)
    }

    fn rest(&mut self, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        self.current_slot()
            .ok_or_else(|| Refused::new("The day is over."))?;
        self.player.heat = to_u8(i32::from(self.player.heat) - 2);
        self.player.nerve = self
            .player
            .nerve
            .saturating_add(1)
            .min(self.player.nerve_max);
        let mut events = vec![Event::Say(
            "You keep your head down and your feet up.".into(),
        )];
        events.extend(self.end_slot(Spent::Other, rng));
        Ok(events)
    }

    fn sleep(&mut self, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        let slot = self
            .current_slot()
            .ok_or_else(|| Refused::new("The day is over."))?;
        if self.kind(self.player.day, slot) != "night" {
            return Err(Refused::new("It is not lights-out yet."));
        }
        let mut events = vec![Event::Say(
            "Lights out. The school settles around you.".into(),
        )];
        events.extend(self.end_slot(Spent::Other, rng));
        Ok(events)
    }

    fn buy(&mut self, item_id: &str) -> Result<Vec<Event>, Refused> {
        let slot = self
            .current_slot()
            .ok_or_else(|| Refused::new("The day is over."))?;
        let day = self.player.day;
        let item = self
            .content
            .item(item_id)
            .cloned()
            .ok_or_else(|| Refused::new("There is no such thing for sale."))?;
        let here = self
            .content
            .place(&self.player.place)
            .ok_or_else(|| Refused::new("You are nowhere."))?;
        if here.shop.as_deref() != Some(item.shop.as_str())
            || !self.place_open(&self.player.place, day, slot)
        {
            return Err(Refused::new("They do not sell that here."));
        }
        if self.player.privileges_until > day {
            return Err(Refused::new(
                "Your privileges are lost: the shop will not serve you.",
            ));
        }
        if !self.all_hold(&item.needs) {
            return Err(Refused::new(
                "They look at you and decide you are not the sort they sell that to yet.",
            ));
        }
        if self.player.cash < item.price {
            return Err(Refused::new("You cannot afford it."));
        }
        if self.player.count(item_id) >= 3 {
            return Err(Refused::new("You cannot carry more of those."));
        }
        self.player.cash -= item.price;
        let have = self.player.count(item_id);
        map_set(&mut self.player.inventory, item_id, have + 1);
        Ok(vec![Event::Say(format!(
            "You buy {} for {}.",
            item.name.to_lowercase(),
            item.price
        ))])
    }

    // ------------------------------------------------------------ the shared world

    /// Queue a change to the shared world and apply it to this session's snapshot too.
    pub(crate) fn push_world(&mut self, delta: WorldDelta) {
        delta.apply(&mut self.world, &self.content);
        self.pending.push(delta);
    }

    /// Take the world changes the board still has to apply.
    pub fn take_world_changes(&mut self) -> Vec<WorldDelta> {
        if self.player.status == Status::Active {
            let entry = self.pupil_entry();
            if self.last_pupil.as_ref() != Some(&entry) {
                self.pending.push(WorldDelta::Roster(entry.clone()));
                self.last_pupil = Some(entry);
            }
        }
        std::mem::take(&mut self.pending)
    }

    /// Whether the character sat at least one big scheme out; used by the sim.
    #[must_use]
    pub fn file_text(&self) -> Vec<String> {
        self.player
            .file
            .iter()
            .map(|e| {
                format!(
                    "{} (severity {})",
                    self.content.file_kind_text(&e.kind),
                    e.sev
                )
            })
            .collect()
    }

    /// Legend as a plain number.
    #[must_use]
    pub fn legend_gain(&mut self, amount: i64) -> u32 {
        let total = i64::from(self.player.legend) + amount;
        self.player.legend = to_u32(total);
        self.player.legend
    }
}
