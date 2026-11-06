//! The story tables (M4-E4): dungeon events that appear in ordinary rooms, and the three
//! questlines of the ember-priests' chain. Both are data; the engine knows a closed set of goals,
//! gains and needs and a table combines them but cannot invent one.

use russh_bbs_door::content::{Table, plain_text};
use serde::Deserialize;

use crate::content::{
    Gain, RoomKind, RoomOption, check_ids, check_option, check_text, id_ok, text_ok,
};

/// The dungeon events table.
pub const EVENTS_TOML: &str = include_str!("../content/events.toml");
/// The questlines table.
pub const QUESTS_TOML: &str = include_str!("../content/quests.toml");

/// A dungeon event: a short scene that overlays an ordinary room on some days for some levels.
/// Its options are room options (so they have needs, rolls and gains), appended to the room's.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventDef {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// The bands it can appear in.
    pub bands: Vec<u32>,
    /// The kinds of room it can overlay (corridor, vault, forge, flooded or shrine).
    pub kinds: Vec<RoomKind>,
    /// What the hero sees, added to the room's own text.
    pub text: String,
    /// What can be done (1 to 3 options).
    pub option: Vec<RoomOption>,
}

/// The events.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Events {
    /// The events.
    pub event: Vec<EventDef>,
}

/// What a quest step asks of the hero. Every goal reads the hero's own tallies, so a step can be
/// finished before it is read (the chain then catches up in one go).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Goal {
    /// Have been this deep.
    Reach {
        /// The level.
        depth: u8,
    },
    /// Have killed this many of one monster.
    Slay {
        /// The monster id.
        monster: String,
        /// How many.
        count: u16,
    },
    /// Have walked into this many rooms of one kind.
    Visit {
        /// The kind.
        room: RoomKind,
        /// How many.
        count: u16,
    },
    /// Have slain the guardian of a band.
    Guardian {
        /// The band, 1 to 5.
        band: u8,
    },
    /// Have turned in this many contracts.
    Contracts {
        /// How many.
        count: u16,
    },
    /// Have given this much coin to the town's works.
    Give {
        /// How much.
        coin: u32,
    },
    /// Be this level.
    Level {
        /// The level.
        level: u8,
    },
    /// Have carried this many fallen heroes home.
    Rescues {
        /// How many.
        count: u16,
    },
    /// Have won this many duels in the ring.
    Duels {
        /// How many.
        wins: u16,
    },
}

/// A choice that closes a questline: it sets a flag on the hero for good.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    /// Unique within the step.
    pub id: String,
    /// The menu line.
    pub label: String,
    /// What the hero reads after choosing.
    pub text: String,
    /// The flag it sets (a room option or the Furnace's verdict may need it).
    pub flag: String,
}

/// One step of a questline.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Unique within the quest.
    pub id: String,
    /// The task, as the journal prints it.
    pub text: String,
    /// What finishes it.
    pub goal: Goal,
    /// What the hero reads when it is done.
    pub done_text: String,
    /// Paid when it is done (coin, salvage, xp, a relic, a recipe or a core only).
    #[serde(default)]
    pub reward: Vec<Gain>,
    /// A choice to make before the next step (the last step of a line usually has one).
    #[serde(default)]
    pub choose: Vec<Choice>,
}

/// A questline of the chain.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quest {
    /// Unique id.
    pub id: String,
    /// Shown name.
    pub name: String,
    /// The premise.
    pub text: String,
    /// The level at which the line is first offered.
    pub level: u8,
    /// The steps, in order.
    pub step: Vec<Step>,
}

/// The questlines.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quests {
    /// The lines.
    pub quest: Vec<Quest>,
}

fn reward_problem(g: &Gain) -> bool {
    !matches!(
        g,
        Gain::Coin { .. }
            | Gain::Salvage { .. }
            | Gain::Xp { .. }
            | Gain::Relic
            | Gain::Recipe { .. }
            | Gain::Core { .. }
    )
}

impl Table for Events {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids(
            "event",
            self.event.iter().map(|e| e.id.as_str()),
            &mut problems,
        );
        for e in &self.event {
            check_text("event", &e.id, &e.text, &mut problems);
            if e.name.trim().is_empty() || !plain_text(&e.name) {
                problems.push(format!("[event] `{}`: bad name", e.id));
            }
            if e.bands.is_empty() || e.bands.iter().any(|b| !(1..=5).contains(b)) {
                problems.push(format!("[event] `{}`: bands must be listed, 1 to 5", e.id));
            }
            if e.kinds.is_empty()
                || e.kinds.iter().any(|k| {
                    !matches!(
                        k,
                        RoomKind::Corridor
                            | RoomKind::Vault
                            | RoomKind::Forge
                            | RoomKind::Flooded
                            | RoomKind::Shrine
                    )
                })
            {
                problems.push(format!(
                    "[event] `{}`: kinds must be corridor, vault, forge, flooded or shrine",
                    e.id
                ));
            }
            if e.option.is_empty() || e.option.len() > 3 {
                problems.push(format!("[event] `{}`: 1 to 3 options", e.id));
            }
            check_ids(
                &format!("event {} option", e.id),
                e.option.iter().map(|o| o.id.as_str()),
                &mut problems,
            );
            for o in &e.option {
                let who = format!("[event] `{}` option `{}`", e.id, o.id);
                check_option(&who, o, &mut problems);
                if o.ok
                    .iter()
                    .chain(&o.fail)
                    .any(|g| matches!(g, Gain::Fight | Gain::Clear | Gain::Shortcut { .. }))
                {
                    problems.push(format!(
                        "{who}: an event has no pack to fight or clear, and no shaft"
                    ));
                }
            }
        }
        problems
    }
}

impl Table for Quests {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        check_ids(
            "quest",
            self.quest.iter().map(|q| q.id.as_str()),
            &mut problems,
        );
        for q in &self.quest {
            check_text("quest", &q.id, &q.text, &mut problems);
            if q.name.trim().is_empty() || !plain_text(&q.name) {
                problems.push(format!("[quest] `{}`: bad name", q.id));
            }
            if !(1..=20).contains(&q.level) {
                problems.push(format!("[quest] `{}`: level must be 1 to 20", q.id));
            }
            if q.step.len() < 5 || q.step.len() > 14 {
                problems.push(format!("[quest] `{}`: 5 to 14 steps", q.id));
            }
            check_ids(
                &format!("quest {} step", q.id),
                q.step.iter().map(|s| s.id.as_str()),
                &mut problems,
            );
            for (i, s) in q.step.iter().enumerate() {
                let who = format!("[quest] `{}` step `{}`", q.id, s.id);
                check_text("quest step", &s.id, &s.text, &mut problems);
                if !text_ok(&s.done_text) {
                    problems.push(format!("{who}: done_text must be plain text"));
                }
                if s.reward.iter().any(reward_problem) {
                    problems.push(format!(
                        "{who}: a reward is coin, salvage, xp, a relic, a recipe or a core"
                    ));
                }
                if !s.choose.is_empty() && s.choose.len() != 2 && s.choose.len() != 3 {
                    problems.push(format!("{who}: a choice offers two or three ways"));
                }
                for c in &s.choose {
                    if !id_ok(&c.id) || !id_ok(&c.flag) || !text_ok(&c.text) {
                        problems.push(format!("{who}: choice `{}` is malformed", c.id));
                    }
                    if c.label.trim().is_empty()
                        || c.label.chars().count() > 50
                        || !plain_text(&c.label)
                    {
                        problems.push(format!("{who}: choice `{}` needs a short label", c.id));
                    }
                }
                if i + 1 == q.step.len() && s.choose.is_empty() {
                    problems.push(format!("{who}: the last step of a line makes a choice"));
                }
                match &s.goal {
                    Goal::Reach { depth } if !(1..=25).contains(depth) => {
                        problems.push(format!("{who}: depth must be 1 to 25"));
                    }
                    Goal::Guardian { band } if !(1..=5).contains(band) => {
                        problems.push(format!("{who}: band must be 1 to 5"));
                    }
                    Goal::Level { level } if !(2..=20).contains(level) => {
                        problems.push(format!("{who}: level must be 2 to 20"));
                    }
                    Goal::Slay { count, .. }
                    | Goal::Visit { count, .. }
                    | Goal::Contracts { count }
                    | Goal::Rescues { count }
                        if *count == 0 || *count > 60 =>
                    {
                        problems.push(format!("{who}: count must be 1 to 60"));
                    }
                    Goal::Duels { wins } if *wins == 0 || *wins > 30 => {
                        problems.push(format!("{who}: wins must be 1 to 30"));
                    }
                    Goal::Give { coin } if *coin == 0 => {
                        problems.push(format!("{who}: give something"));
                    }
                    _ => {}
                }
            }
        }
        problems
    }
}
