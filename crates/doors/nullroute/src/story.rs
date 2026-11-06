//! The story as data (M4-N4): the fixers' arcs, their endings, the season's endings, the
//! characters' epilogues, and the lines the social layer says (`arcs.toml`, `seasons.toml`,
//! `social.toml`).
//!
//! An **arc** is a chain of jobs from one fixer. Each **step** is a job (a corporation, a tier,
//! an objective) with a short brief; when the job is done the step offers **choices**, and a
//! choice sends the character to another step (a branch) or to an **ending**. A choice can be
//! gated by the character's build and history (a [`Cond`]), sets or clears flags, and moves
//! reputation, marks, heat and standing, so a betrayal really does close a fixer's door. Steps
//! only move forward (`next` is always a later step), which makes every arc finite and lets a
//! test walk all of them.

use russh_bbs_door::content::{Table, plain_text};
use serde::Deserialize;

use crate::cond::Cond;
use crate::content::{Content, Effect};
use crate::world::Contract;

/// The embedded arcs.
pub const ARCS_TOML: &str = include_str!("../content/arcs.toml");
/// The embedded season endings and epilogues.
pub const SEASONS_TOML: &str = include_str!("../content/seasons.toml");
/// The embedded lines of the social layer.
pub const SOCIAL_TOML: &str = include_str!("../content/social.toml");

/// The contract template number that marks an arc step (never a real template).
pub const ARC_TMPL: u8 = 251;
/// Arc contract ids start here: `ARC_ID + arc * 256 + step`.
pub const ARC_ID: u32 = 0x4000_0000;

/// The most story flags.
pub const FLAGS_MAX: usize = 32;

/// One fixer's arc.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArcDef {
    /// Stable id (0 to 4).
    pub num: u8,
    /// The fixer who gives it.
    pub fixer: u8,
    /// Title.
    pub title: String,
    /// One line for the arcs screen.
    pub blurb: String,
    /// The first step's id.
    pub first: u8,
    /// What the character needs before the arc will start.
    #[serde(default)]
    pub open: String,
}

/// What a choice does to the character.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceDef {
    /// What the player reads.
    pub label: String,
    /// The step it leads to (0 when it ends the arc).
    #[serde(default)]
    pub next: u8,
    /// The ending it reaches (0 when it leads on).
    #[serde(default)]
    pub ending: u8,
    /// What the character needs for it to be offered as open.
    #[serde(default)]
    pub gate: String,
    /// Flags to set.
    #[serde(default)]
    pub set: Vec<String>,
    /// Flags to clear.
    #[serde(default)]
    pub clear: Vec<String>,
    /// Reputation gained (or lost).
    #[serde(default)]
    pub rep: i32,
    /// Marks gained (or lost).
    #[serde(default)]
    pub marks: i32,
    /// Heat gained (or lost).
    #[serde(default)]
    pub heat: i32,
    /// `[fixer, change]` pairs.
    #[serde(default)]
    pub standing: Vec<[i8; 2]>,
    /// A line the player reads after choosing.
    #[serde(default)]
    pub note: String,
}

/// One job in an arc.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepDef {
    /// The arc.
    pub arc: u8,
    /// The step's id within the arc (1-based, unique).
    pub id: u8,
    /// Title.
    pub title: String,
    /// The target corporation.
    pub corp: u8,
    /// Tier 1 to 3.
    pub tier: u8,
    /// Objective node code.
    pub target: String,
    /// Effect needed on it.
    pub effect: Effect,
    /// Pay.
    pub reward: u32,
    /// Heat the job adds.
    pub heat: u32,
    /// What the character needs for the step to open.
    #[serde(default)]
    pub open: String,
    /// The brief.
    pub brief: String,
    /// What the fixer says when it is done.
    pub done: String,
    /// Fragments (by number) the fixer hands over when the job is done: how a story gives a
    /// character the code it cannot count on finding.
    #[serde(default)]
    pub give: Vec<u8>,
    /// The choices after it.
    #[serde(default)]
    pub choice: Vec<ChoiceDef>,
}

/// How an arc ends for the character.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndingDef {
    /// Stable id (1 to 31: a bit in the save).
    pub num: u8,
    /// The arc it ends.
    pub arc: u8,
    /// Title.
    pub title: String,
    /// The text.
    pub text: String,
}

/// The arcs table.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoryTable {
    /// Story flag names; the index is the bit.
    #[serde(default)]
    pub flags: Vec<String>,
    /// The arcs.
    #[serde(default)]
    pub arc: Vec<ArcDef>,
    /// The steps.
    #[serde(default)]
    pub step: Vec<StepDef>,
    /// The endings.
    #[serde(default)]
    pub ending: Vec<EndingDef>,
}

/// A season ending, or a character's epilogue.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerdictDef {
    /// Stable id.
    pub num: u8,
    /// Title.
    pub title: String,
    /// The text.
    pub text: String,
    /// The condition, tried in file order; the last one should be empty (the default).
    #[serde(default)]
    pub rule: String,
}

/// A line the social layer says.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineDef {
    /// The number in [`crate::social::Lines`].
    pub num: u8,
    /// The text, with `{a}` (the other player) and `{n}` (a number).
    pub text: String,
}

/// The season endings and epilogues.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeasonsTable {
    /// How a season can end for the board.
    #[serde(default)]
    pub season_ending: Vec<VerdictDef>,
    /// How a character's season can end.
    #[serde(default)]
    pub epilogue: Vec<VerdictDef>,
}

/// The social lines.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinesTable {
    /// The lines.
    #[serde(default)]
    pub line: Vec<LineDef>,
}

fn text_problem(what: &str, text: &str, max: usize, problems: &mut Vec<String>) {
    if text.trim().is_empty() || text.chars().count() > max || !plain_text(text) {
        problems.push(format!(
            "{what}: must be plain text of 1 to {max} characters"
        ));
    }
}

impl Table for StoryTable {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        if self.flags.len() > FLAGS_MAX {
            p.push(format!("more than {FLAGS_MAX} flags"));
        }
        for (i, f) in self.flags.iter().enumerate() {
            if f.is_empty() || !f.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
                p.push(format!(
                    "flag `{f}`: lowercase letters and underscores only"
                ));
            }
            if self.flags[..i].contains(f) {
                p.push(format!("flag `{f}` is listed twice"));
            }
        }
        let cond = |what: &str, text: &str, p: &mut Vec<String>| {
            if let Err(e) = Cond::parse(text, &self.flags) {
                p.push(format!("{what}: condition `{text}`: {e}"));
            }
        };
        for (i, a) in self.arc.iter().enumerate() {
            let what = format!("arc {}", a.num);
            if self.arc[..i].iter().any(|b| b.num == a.num) {
                p.push(format!("{what} is used twice"));
            }
            text_problem(&format!("{what} title"), &a.title, 40, &mut p);
            text_problem(&format!("{what} blurb"), &a.blurb, 100, &mut p);
            cond(&what, &a.open, &mut p);
            let steps: Vec<&StepDef> = self.step.iter().filter(|s| s.arc == a.num).collect();
            if !steps.iter().any(|s| s.id == a.first) {
                p.push(format!("{what}: first step {} does not exist", a.first));
            }
            if !self.ending.iter().any(|e| e.arc == a.num) {
                p.push(format!("{what} has no ending"));
            }
        }
        for s in &self.step {
            let what = format!("arc {} step {}", s.arc, s.id);
            if !self.arc.iter().any(|a| a.num == s.arc) {
                p.push(format!("{what}: no such arc"));
            }
            if s.id == 0
                || self
                    .step
                    .iter()
                    .filter(|t| t.arc == s.arc && t.id == s.id)
                    .count()
                    > 1
            {
                p.push(format!("{what}: ids are 1 up and unique within an arc"));
            }
            if !(1..=3).contains(&s.tier) || s.reward == 0 || s.reward > 2_000 {
                p.push(format!("{what}: tier 1 to 3 and a reward of 1 to 2000"));
            }
            if !matches!(s.effect, Effect::Copy | Effect::Plant | Effect::Shred) {
                p.push(format!(
                    "{what}: the objective effect must be copy, plant or shred"
                ));
            }
            if !crate::content::objective_possible(&s.target, s.effect) {
                p.push(format!(
                    "{what}: {:?} cannot be done at a {} node",
                    s.effect, s.target
                ));
            }
            if s.target.len() != 2 || !s.target.bytes().all(|b| b.is_ascii_uppercase()) {
                p.push(format!("{what}: target is a two-letter node code"));
            }
            text_problem(&format!("{what} title"), &s.title, 40, &mut p);
            text_problem(&format!("{what} brief"), &s.brief, 220, &mut p);
            text_problem(&format!("{what} done"), &s.done, 220, &mut p);
            cond(&what, &s.open, &mut p);
            if s.give.len() > 3 || s.give.iter().any(|n| *n == 0 || *n > 31) {
                p.push(format!(
                    "{what}: `give` is up to three fragment numbers, 1 to 31"
                ));
            }
            if s.choice.is_empty() {
                p.push(format!("{what}: needs at least one choice"));
            }
            if !s.choice.iter().any(|c| c.gate.is_empty()) {
                p.push(format!(
                    "{what}: needs a choice with no gate, so nobody is stuck"
                ));
            }
            for (k, c) in s.choice.iter().enumerate() {
                let what = format!("{what} choice {}", k + 1);
                text_problem(&format!("{what} label"), &c.label, 56, &mut p);
                if !c.note.is_empty() {
                    text_problem(&format!("{what} note"), &c.note, 160, &mut p);
                }
                cond(&what, &c.gate, &mut p);
                if (c.next == 0) == (c.ending == 0) {
                    p.push(format!("{what}: exactly one of `next` and `ending`"));
                }
                if c.next != 0 {
                    if c.next <= s.id {
                        p.push(format!("{what}: `next` must be a later step"));
                    }
                    if !self.step.iter().any(|t| t.arc == s.arc && t.id == c.next) {
                        p.push(format!("{what}: step {} does not exist", c.next));
                    }
                }
                if c.ending != 0
                    && !self
                        .ending
                        .iter()
                        .any(|e| e.num == c.ending && e.arc == s.arc)
                {
                    p.push(format!(
                        "{what}: ending {} is not one of this arc's",
                        c.ending
                    ));
                }
                for f in c.set.iter().chain(&c.clear) {
                    if !self.flags.contains(f) {
                        p.push(format!("{what}: no flag `{f}`"));
                    }
                }
                if c.standing.iter().any(|[f, d]| {
                    *f < 0
                        || usize::from(f.unsigned_abs()) >= crate::player::FIXERS_MAX
                        || d.abs() > 5
                }) {
                    p.push(format!(
                        "{what}: standing is [fixer, change] with a change of at most 5"
                    ));
                }
                if c.rep.abs() > 40 || c.marks.abs() > 1_000 || c.heat.abs() > 40 {
                    p.push(format!("{what}: effects out of range"));
                }
            }
        }
        for (i, e) in self.ending.iter().enumerate() {
            let what = format!("ending {}", e.num);
            if e.num == 0 || e.num > 31 || self.ending[..i].iter().any(|o| o.num == e.num) {
                p.push(format!("{what}: ids are 1 to 31 and unique"));
            }
            if !self.arc.iter().any(|a| a.num == e.arc) {
                p.push(format!("{what}: no such arc"));
            }
            text_problem(&format!("{what} title"), &e.title, 40, &mut p);
            text_problem(&format!("{what} text"), &e.text, 300, &mut p);
        }
        p
    }
}

fn verdict_problems(what: &str, list: &[VerdictDef], p: &mut Vec<String>) {
    if list.is_empty() {
        p.push(format!("{what}: none"));
    }
    for (i, v) in list.iter().enumerate() {
        if list[..i].iter().any(|o| o.num == v.num) {
            p.push(format!("{what} {}: used twice", v.num));
        }
        text_problem(&format!("{what} {} title", v.num), &v.title, 40, p);
        text_problem(&format!("{what} {} text", v.num), &v.text, 300, p);
        if let Err(e) = Cond::parse(&v.rule, &[]) {
            p.push(format!("{what} {}: rule `{}`: {e}", v.num, v.rule));
        }
    }
    if list.last().is_some_and(|v| !v.rule.is_empty()) {
        p.push(format!(
            "{what}: the last one must have no rule (the default)"
        ));
    }
}

impl Table for SeasonsTable {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        verdict_problems("season ending", &self.season_ending, &mut p);
        verdict_problems("epilogue", &self.epilogue, &mut p);
        p
    }
}

/// The names `{a}` and `{n}` are the only placeholders a line may use.
fn placeholders_ok(text: &str) -> bool {
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        let tail = &rest[i..];
        if !(tail.starts_with("{a}") || tail.starts_with("{n}")) {
            return false;
        }
        rest = &tail[3..];
    }
    !rest.contains('}')
}

impl Table for LinesTable {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        for (i, l) in self.line.iter().enumerate() {
            if self.line[..i].iter().any(|o| o.num == l.num) {
                p.push(format!("line {} is used twice", l.num));
            }
            let plain_part = l.text.replace("{a}", "A").replace("{n}", "1");
            text_problem(&format!("line {}", l.num), &plain_part, 110, &mut p);
            if !placeholders_ok(&l.text) {
                p.push(format!(
                    "line {}: only {{a}} and {{n}} are placeholders",
                    l.num
                ));
            }
        }
        p
    }
}

/// What a contract stands for: its title, verb, objective and pay rule.
#[derive(Debug, Clone)]
pub struct Job {
    /// The title.
    pub title: String,
    /// The verb.
    pub verb: String,
    /// The objective node code.
    pub target: String,
    /// The effect needed on it.
    pub effect: Effect,
    /// The heat it adds.
    pub heat: u32,
    /// The arc and step, if it is a story step.
    pub arc: Option<(u8, u8)>,
}

/// The arc and step a contract id stands for.
#[must_use]
pub const fn arc_of_id(id: u32) -> Option<(u8, u8)> {
    if id & 0xF000_0000 != ARC_ID {
        return None;
    }
    Some((((id >> 8) & 0xFF) as u8, (id & 0xFF) as u8))
}

/// The contract id of an arc step.
#[must_use]
pub const fn arc_id(arc: u8, step: u8) -> u32 {
    ARC_ID + ((arc as u32) << 8) + step as u32
}

impl Content {
    /// The job a contract stands for (a template, a story step or the finale).
    #[must_use]
    pub fn job(&self, c: &Contract) -> Option<Job> {
        if c.tmpl == ARC_TMPL {
            let (arc, step) = arc_of_id(c.id)?;
            let s = self.step(arc, step)?;
            return Some(Job {
                title: s.title.clone(),
                verb: verb_of(s.effect).to_owned(),
                target: s.target.clone(),
                effect: s.effect,
                heat: s.heat,
                arc: Some((arc, step)),
            });
        }
        if c.tmpl == crate::crew::FINALE_TMPL {
            return Some(Job {
                title: "The Null Vault".to_owned(),
                verb: "Heist".to_owned(),
                target: "AC".to_owned(),
                effect: Effect::Plant,
                heat: 20,
                arc: None,
            });
        }
        let t = self.contract(c.tmpl)?;
        Some(Job {
            title: t.title.clone(),
            verb: t.verb.clone(),
            target: t.target.clone(),
            effect: t.effect,
            heat: t.heat,
            arc: None,
        })
    }

    /// A step of an arc.
    #[must_use]
    pub fn step(&self, arc: u8, id: u8) -> Option<&StepDef> {
        self.story.step.iter().find(|s| s.arc == arc && s.id == id)
    }

    /// An arc.
    #[must_use]
    pub fn arc(&self, num: u8) -> Option<&ArcDef> {
        self.story.arc.iter().find(|a| a.num == num)
    }

    /// An arc's ending.
    #[must_use]
    pub fn ending(&self, num: u8) -> Option<&EndingDef> {
        self.story.ending.iter().find(|e| e.num == num)
    }

    /// A social line's text.
    #[must_use]
    pub fn line(&self, num: u8) -> &str {
        self.lines
            .line
            .iter()
            .find(|l| l.num == num)
            .map_or("", |l| l.text.as_str())
    }

    /// A social line with `{a}` and `{n}` filled in.
    #[must_use]
    pub fn say(&self, num: u8, a: &str, n: i64) -> String {
        self.line(num)
            .replace("{a}", a)
            .replace("{n}", &n.to_string())
    }
}

/// The verb for a story step by what it asks you to do.
#[must_use]
pub const fn verb_of(effect: Effect) -> &'static str {
    match effect {
        Effect::Copy => "Steal",
        Effect::Plant => "Plant",
        Effect::Shred => "Wipe",
        _ => "Job",
    }
}
