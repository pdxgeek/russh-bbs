//! Story arcs and endings (M4-L4): six arcs of linked scenes, each with branches that close
//! doors and several endings. The scenes are ordinary transit events (`content/arcs.toml`)
//! gated by the captain's place in the arc; this file holds the state a save keeps.

use crate::codec::{Reader, Writer};

/// How many arcs there are: one per faction and a mystery about why the gates failed.
pub const ARC_COUNT: usize = 6;
/// "No ending yet" in a save.
pub const NO_ENDING: u8 = 255;

/// Where a captain is in one arc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArcState {
    /// Steps finished (the next scene is step `step + 1`).
    pub step: u8,
    /// Branch bits set by choices.
    pub branches: u16,
    /// The real day the last step happened (steps are spaced out).
    pub day: i64,
    /// The ending reached, or [`NO_ENDING`].
    pub ending: u8,
}

impl Default for ArcState {
    fn default() -> Self {
        Self {
            step: 0,
            branches: 0,
            day: 0,
            ending: NO_ENDING,
        }
    }
}

impl ArcState {
    /// Whether the arc is over.
    #[must_use]
    pub const fn finished(&self) -> bool {
        self.ending != NO_ENDING
    }

    /// Write to a save.
    pub fn write(&self, w: &mut Writer) {
        w.u8(self.step);
        w.u16(self.branches);
        w.i64(self.day);
        w.u8(self.ending);
    }

    /// Read from a save.
    pub fn read(r: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            step: r.u8()?,
            branches: r.u16()?,
            day: r.i64()?,
            ending: r.u8()?,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// The arc tables: `content/arcs.toml`.
// ---------------------------------------------------------------------------------------------

use russh_bbs_door::content::Table;
use serde::Deserialize;

use crate::content::{Content, Faction, text_ok};
use crate::transit::{Effect, EventDef, Req};

/// Most branch names an arc can use (they live in a `u16`).
pub const MAX_BRANCHES: usize = 16;
/// Most endings one arc can have.
pub const MAX_ENDINGS: usize = 6;
/// Season endings are numbered from here; arc endings are `arc * 8 + n`.
pub const SEASON_ENDING_BASE: u8 = 100;

/// One way an arc can end.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndingDef {
    /// Stable id.
    pub id: String,
    /// Heading.
    pub title: String,
    /// What the captain reads when it happens.
    pub text: String,
    /// Whether reaching this ending opens (or seals) the gates for the whole frontier.
    #[serde(default)]
    pub gates: bool,
}

/// A story arc: a chain of scenes with branches and endings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArcDef {
    /// Stable id.
    pub id: String,
    /// Heading.
    pub title: String,
    /// The faction whose arc this is, if any.
    #[serde(default)]
    pub faction: Option<Faction>,
    /// A line for the journal before the arc has begun.
    pub hint: String,
    /// A line for the journal once it has.
    pub blurb: String,
    /// Names of the choices that close doors later.
    #[serde(default)]
    pub branches: Vec<String>,
    /// The endings.
    pub ending: Vec<EndingDef>,
}

/// How a season looks to a captain who never finished an arc: one is picked at the season's end.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeasonEnding {
    /// Stable id: `tycoon`, `solvent`, `pirate_king`, `folk_hero`, `cartographer`, `bankrupt`
    /// or `drifter`.
    pub id: String,
    /// Heading.
    pub title: String,
    /// The line for the Hall and the next captain.
    pub text: String,
}

/// `arcs.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArcTable {
    /// The arcs.
    pub arc: Vec<ArcDef>,
    /// Endings for a season that no arc finished.
    pub season_ending: Vec<SeasonEnding>,
    /// The scenes: ordinary transit events with `arc` and `step`.
    pub event: Vec<EventDef>,
}

/// The ids a season ending must have, in the order the code picks them.
pub const SEASON_ENDING_IDS: [&str; 7] = [
    "tycoon",
    "solvent",
    "pirate_king",
    "folk_hero",
    "cartographer",
    "bankrupt",
    "drifter",
];

impl Table for ArcTable {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        crate::content::duplicates(self.arc.iter().map(|a| a.id.as_str()), "arc", &mut p);
        if self.arc.len() != ARC_COUNT {
            p.push(format!("exactly {ARC_COUNT} arcs"));
        }
        for a in &self.arc {
            let id = &a.id;
            if !crate::content::ident_ok(id)
                || !text_ok(&a.title, 32)
                || !text_ok(&a.hint, 120)
                || !text_ok(&a.blurb, 200)
            {
                p.push(format!("arc `{id}`: id, title <= 32, hint <= 120, blurb <= 200"));
            }
            if a.branches.len() > MAX_BRANCHES {
                p.push(format!("arc `{id}`: at most {MAX_BRANCHES} branches"));
            }
            crate::content::duplicates(a.branches.iter().map(String::as_str), "branch", &mut p);
            if a.ending.is_empty() || a.ending.len() > MAX_ENDINGS {
                p.push(format!("arc `{id}`: 1 to {MAX_ENDINGS} endings"));
            }
            crate::content::duplicates(a.ending.iter().map(|e| e.id.as_str()), "ending", &mut p);
            for e in &a.ending {
                if !crate::content::ident_ok(&e.id) || !text_ok(&e.title, 32) || !text_ok(&e.text, 300) {
                    p.push(format!("arc `{id}` ending `{}`: id, title <= 32, text <= 300", e.id));
                }
            }
        }
        for want in SEASON_ENDING_IDS {
            if !self.season_ending.iter().any(|e| e.id == want) {
                p.push(format!("season ending `{want}` is missing"));
            }
        }
        for e in &self.season_ending {
            if !text_ok(&e.title, 32) || !text_ok(&e.text, 200) {
                p.push(format!("season ending `{}`: title <= 32, text <= 200", e.id));
            }
        }
        for e in &self.event {
            if e.family != "arc" || e.arc.is_empty() || e.step == 0 {
                p.push(format!("scene `{}`: family `arc`, with an arc and a step", e.id));
            }
        }
        p
    }
}

fn walk_effects<'a>(e: &'a EventDef) -> impl Iterator<Item = &'a Effect> {
    e.options
        .iter()
        .flat_map(|o| o.outcomes.iter())
        .flat_map(|out| out.effects.iter())
}

fn reqs_of<'a>(e: &'a EventDef) -> impl Iterator<Item = &'a Req> {
    e.when
        .iter()
        .chain(e.mods.iter().map(|m| &m.req))
        .chain(e.options.iter().flat_map(|o| o.req.iter()))
        .chain(
            e.options
                .iter()
                .flat_map(|o| o.outcomes.iter())
                .flat_map(|out| out.bonus.iter().map(|b| &b.req)),
        )
}

/// Checks that need all the tables: steps contiguous, no dead end, no dead branch or ending.
pub(crate) fn link_problems(content: &Content, table: &ArcTable) -> Vec<String> {
    let mut p = Vec::new();
    for (ai, arc) in table.arc.iter().enumerate() {
        let id = &arc.id;
        let mut steps: Vec<&EventDef> = table.event.iter().filter(|e| &e.arc == id).collect();
        steps.sort_by_key(|e| e.step);
        let n = steps.len();
        if !(6..=10).contains(&n) {
            p.push(format!("arc `{id}`: 6 to 10 steps, found {n}"));
        }
        for (i, e) in steps.iter().enumerate() {
            if usize::from(e.step) != i + 1 {
                p.push(format!("arc `{id}`: steps must run 1, 2, 3 ... (found {})", e.step));
            }
            let gated = e.when.iter().any(|r| {
                matches!(r, Req::ArcAt { arc, step } if arc == id && *step == e.step)
            });
            if !gated {
                p.push(format!("scene `{}`: `when` needs arc_at for its own step", e.id));
            }
            let last = usize::from(e.step) == n;
            for o in &e.options {
                for out in &o.outcomes {
                    let advances = out.effects.iter().any(
                        |f| matches!(f, Effect::ArcStep { arc } if arc == id),
                    );
                    let ends = out.effects.iter().any(
                        |f| matches!(f, Effect::Ending { arc, .. } if arc == id),
                    );
                    if last && !ends {
                        p.push(format!("scene `{}`: every outcome of the last step ends the arc", e.id));
                    }
                    if !last && !(advances || ends) {
                        p.push(format!("scene `{}`: an outcome neither advances nor ends the arc", e.id));
                    }
                    if advances && ends {
                        p.push(format!("scene `{}`: an outcome both advances and ends", e.id));
                    }
                }
            }
        }
        // Every branch is set somewhere and tested somewhere; every ending is reachable.
        for b in &arc.branches {
            let set = table.event.iter().any(|e| {
                walk_effects(e).any(|f| matches!(f, Effect::ArcBranch { arc, branch } if arc == id && branch == b))
            });
            let tested = table.event.iter().any(|e| {
                reqs_of(e).any(|r| req_tests_branch(r, id, b))
            });
            if !set {
                p.push(format!("arc `{id}`: branch `{b}` is never set"));
            }
            if !tested {
                p.push(format!("arc `{id}`: branch `{b}` never closes or opens anything"));
            }
        }
        for en in &arc.ending {
            let reached = table.event.iter().any(|e| {
                walk_effects(e).any(|f| matches!(f, Effect::Ending { arc, ending } if arc == id && ending == &en.id))
            });
            if !reached {
                p.push(format!("arc `{id}`: ending `{}` can never happen", en.id));
            }
        }
        let _ = ai;
    }
    for e in &table.event {
        if content.arc_ix(&e.arc).is_none() {
            p.push(format!("scene `{}`: unknown arc `{}`", e.id, e.arc));
        }
        for r in reqs_of(e) {
            arc_req_problems(content, &e.id, r, &mut p);
        }
        for f in walk_effects(e) {
            match f {
                Effect::ArcStep { arc } if content.arc_ix(arc).is_none() => {
                    p.push(format!("scene `{}`: unknown arc `{arc}`", e.id));
                }
                Effect::ArcBranch { arc, branch } if content.arc_branch_bit(arc, branch).is_none() => {
                    p.push(format!("scene `{}`: unknown branch `{arc}/{branch}`", e.id));
                }
                Effect::Ending { arc, ending } if content.arc_ending_ix(arc, ending).is_none() => {
                    p.push(format!("scene `{}`: unknown ending `{arc}/{ending}`", e.id));
                }
                _ => {}
            }
        }
    }
    p
}

fn req_tests_branch(r: &Req, arc: &str, branch: &str) -> bool {
    match r {
        Req::ArcBranch { arc: a, branch: b } => a == arc && b == branch,
        Req::Not { req } => req_tests_branch(req, arc, branch),
        _ => false,
    }
}

fn arc_req_problems(content: &Content, id: &str, r: &Req, p: &mut Vec<String>) {
    match r {
        Req::ArcAt { arc, .. } | Req::ArcWait { arc, .. } if content.arc_ix(arc).is_none() => {
            p.push(format!("scene `{id}`: unknown arc `{arc}`"));
        }
        Req::ArcBranch { arc, branch } if content.arc_branch_bit(arc, branch).is_none() => {
            p.push(format!("scene `{id}`: unknown branch `{arc}/{branch}`"));
        }
        Req::Not { req } => arc_req_problems(content, id, req, p),
        _ => {}
    }
}

impl Content {
    /// An arc's index by id.
    #[must_use]
    pub fn arc_ix(&self, id: &str) -> Option<usize> {
        self.arcs.arc.iter().position(|a| a.id == id)
    }

    /// The bit of one of an arc's branches.
    #[must_use]
    pub fn arc_branch_bit(&self, arc: &str, branch: &str) -> Option<u16> {
        self.arcs
            .arc
            .iter()
            .find(|a| a.id == arc)?
            .branches
            .iter()
            .position(|b| b == branch)
            .map(|i| 1u16 << i)
    }

    /// An ending's number within its arc.
    #[must_use]
    pub fn arc_ending_ix(&self, arc: &str, ending: &str) -> Option<u8> {
        self.arcs
            .arc
            .iter()
            .find(|a| a.id == arc)?
            .ending
            .iter()
            .position(|e| e.id == ending)
            .and_then(|i| u8::try_from(i).ok())
    }

    /// The title and text of an ending number kept in a save or the world (arc endings are
    /// `arc * 8 + n`, season endings are `100 + n`).
    #[must_use]
    pub fn ending_text(&self, code: u8) -> Option<(&str, &str)> {
        if code >= SEASON_ENDING_BASE {
            let e = self.arcs.season_ending.get(usize::from(code - SEASON_ENDING_BASE))?;
            Some((&e.title, &e.text))
        } else {
            let arc = self.arcs.arc.get(usize::from(code / 8))?;
            let e = arc.ending.get(usize::from(code % 8))?;
            Some((&e.title, &e.text))
        }
    }

    /// The code of a season ending by id.
    #[must_use]
    pub fn season_ending_code(&self, id: &str) -> u8 {
        let n = self
            .arcs
            .season_ending
            .iter()
            .position(|e| e.id == id)
            .unwrap_or(0);
        SEASON_ENDING_BASE + u8::try_from(n).unwrap_or(0)
    }
}
