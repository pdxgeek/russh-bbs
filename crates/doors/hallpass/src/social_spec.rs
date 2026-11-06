//! The social and mystery tables (M4-H4): structured player schemes, template rumours, the school
//! election, the end-of-term awards, and the east wing's clues and verdicts.
//!
//! Everything a player can aim at another player is a row here: a template with a name, a cost,
//! a counterplay and a small, bounded effect. There is no free text and no typed target. The
//! checks below are the tone rules, and `tests/content.rs` breaks each on purpose.

use serde::Deserialize;

use crate::cond::Cond;
use crate::content::Content;
use crate::effect::{Eff, Gauge};
use russh_bbs_door::content::{Table, plain_text};

fn one() -> i32 {
    1
}

/// The tunable numbers of the social layer.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocialRules {
    /// Plots a pupil may send in a day.
    pub plots_per_day: usize,
    /// Days before the same pupil may be the target of the same sender again.
    pub pair_days: i64,
    /// Days a target is left alone after a mischief plot reached them.
    pub protect_days: i64,
    /// Days a plot or its outcome waits before the school forgets it.
    pub plot_days: i64,
    /// Days a rumour goes round.
    pub rumour_days: i64,
    /// Rumours a pupil may start in a day.
    pub rumours_per_day: u8,
    /// The first day (of the term) a pupil may stand and canvass.
    pub nominate_from: i64,
    /// The first day votes are taken.
    pub vote_from: i64,
    /// The day the votes are counted.
    pub tally_day: i64,
    /// Times a pupil may canvass in a day.
    pub canvass_per_day: u8,
    /// What a vote is worth beside canvassing.
    pub vote_weight: i32,
    /// What it takes to stand.
    pub nominate_needs: Vec<Cond>,
    /// Legend for the winner of the election.
    pub winner_legend: i32,
    /// The earned trait the winner takes.
    pub winner_trait: String,
    /// The world effect a pupil winning brings about.
    pub player_win_effect: String,
    /// Cash each award brings.
    pub award_cash: i32,
}

/// A structured scheme one pupil may play on another, from a fixed list.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlotSpec {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// One line.
    pub blurb: String,
    /// `mischief` (a harmless inconvenience) or `kind` (a favour).
    pub kind: String,
    /// Nerve it costs the sender.
    pub nerve: i32,
    /// Cash it costs the sender.
    #[serde(default)]
    pub cash: i32,
    /// What the sender needs (a build, an item, a skill).
    #[serde(default)]
    pub needs: Vec<Cond>,
    /// What lets the target see it coming: any one holding blunts it.
    #[serde(default)]
    pub counter: Vec<Cond>,
    /// What the target is told when they saw it coming (`{from}`).
    pub countered: String,
    /// What the target is told when it lands (`{from}`).
    pub seen: String,
    /// What it does to the target: gauges only, small.
    pub hit: Vec<Eff>,
    /// What the sender gains when it lands.
    #[serde(default)]
    pub reward: Vec<Eff>,
    /// What the sender loses when it is seen through.
    #[serde(default)]
    pub fizzle: Vec<Eff>,
}

/// A rumour template.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RumourSpec {
    /// Its id.
    pub id: String,
    /// Its name, for the menu.
    pub name: String,
    /// The line the school hears (`{about}`).
    pub text: String,
    /// What it is about: `npc`, `clique` or `self`.
    pub about: String,
    /// `praise`, `gossip` or `tall`.
    pub kind: String,
    /// What it takes to be able to start it.
    #[serde(default)]
    pub needs: Vec<Cond>,
    /// Standing with the clique it is about (or the character's clique).
    #[serde(default)]
    pub stand: i32,
    /// Friendship with the character it is about.
    #[serde(default)]
    pub bond: i32,
    /// Cool.
    #[serde(default)]
    pub cool: i32,
    /// Legend.
    #[serde(default)]
    pub legend: i32,
    /// Heat with the staff.
    #[serde(default)]
    pub heat: i32,
    /// Patrol heat it adds where the character is usually found.
    #[serde(default)]
    pub patrol: i32,
    /// Backing it gives (or takes from) a candidate in the election.
    #[serde(default)]
    pub backing: i32,
}

/// A character who runs in the school election.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runner {
    /// The character.
    pub npc: String,
    /// What they stand for.
    pub platform: String,
    /// Their first backing.
    pub base: i32,
    /// Backing they add each day of the term after the nominations open (a character's campaign).
    #[serde(default)]
    pub grow: i32,
    /// World effects that keep them out of the race.
    #[serde(default)]
    pub withdrawn_by: Vec<String>,
    /// World effects that add to (or take from) their backing.
    #[serde(default)]
    pub swing: Vec<(String, i32)>,
    /// The world effect their win brings about.
    pub win: String,
}

/// An end-of-term award.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwardSpec {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// What the school says of the winner.
    pub text: String,
    /// What is scored: `legend`, `grades`, `pulled`, `clues`, `merit`, `friends` or `clean`.
    pub stat: String,
    /// The least score that wins anything.
    #[serde(default = "one")]
    pub min: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SocialFile {
    rules: SocialRules,
    plot: Vec<PlotSpec>,
    rumour: Vec<RumourSpec>,
    runner: Vec<Runner>,
    award: Vec<AwardSpec>,
}

/// The social tables.
#[derive(Debug, Clone)]
pub struct SocialSpec {
    /// The numbers.
    pub rules: SocialRules,
    /// Player schemes.
    pub plots: Vec<PlotSpec>,
    /// Rumour templates.
    pub rumours: Vec<RumourSpec>,
    /// The characters who run.
    pub runners: Vec<Runner>,
    /// The awards.
    pub awards: Vec<AwardSpec>,
}

/// The mystery's own numbers.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MysteryRules {
    /// The storyline that carries it.
    pub arc: String,
    /// The day of the term the east wing's secret comes out.
    pub finale_day: i64,
    /// The paper's line when nobody settled it.
    pub unsolved: String,
    /// Chance per risk point, in percent, of being noticed when searching.
    pub risk_pct: i32,
}

/// A thing to be found by searching a place.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClueSpec {
    /// Its id. Finding it sets the flag `clue_ID`.
    pub id: String,
    /// The act of the mystery, 1 to 4.
    pub act: u8,
    /// Where it is found.
    pub place: String,
    /// The slot kinds it can be found in.
    pub kinds: Vec<String>,
    /// What it takes (a build, an item, a bond).
    #[serde(default)]
    pub needs: Vec<Cond>,
    /// Clues that must have been found first.
    #[serde(default)]
    pub after: Vec<String>,
    /// What the search turns up.
    pub text: String,
    /// What finding it does.
    #[serde(default)]
    pub effects: Vec<Eff>,
    /// How risky the search is, 0 to 3.
    #[serde(default)]
    pub risk: u8,
}

/// A way the secret can come out, chosen by whom the pupils trusted.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerdictSpec {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// What it means.
    pub blurb: String,
    /// The world effect it brings about for everyone.
    pub effect: String,
    /// Which wins a tie: the lowest.
    pub order: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MysteryFile {
    mystery: MysteryRules,
    clue: Vec<ClueSpec>,
    verdict: Vec<VerdictSpec>,
}

/// The mystery tables.
#[derive(Debug, Clone)]
pub struct MysterySpec {
    /// The numbers.
    pub rules: MysteryRules,
    /// The clues.
    pub clues: Vec<ClueSpec>,
    /// The verdicts.
    pub verdicts: Vec<VerdictSpec>,
}

impl Table for SocialFile {
    fn validate(&self) -> Vec<String> {
        let mut out = Vec::new();
        let r = &self.rules;
        if !(0 < r.nominate_from && r.nominate_from < r.vote_from && r.vote_from < r.tally_day) {
            out.push("election days must rise: nominate_from < vote_from < tally_day".into());
        }
        if r.plots_per_day == 0 || r.pair_days < 1 || r.protect_days < 1 || r.plot_days < 1 {
            out.push("plot limits must be positive".into());
        }
        if r.rumour_days < 1 || r.rumours_per_day == 0 || r.canvass_per_day == 0 {
            out.push("rumour and canvass limits must be positive".into());
        }
        let mut ids = std::collections::HashSet::new();
        for p in &self.plot {
            if !ids.insert(format!("plot {}", p.id)) {
                out.push(format!("plot id `{}` is used twice", p.id));
            }
            for t in [&p.name, &p.blurb, &p.countered, &p.seen] {
                if t.trim().is_empty() || !plain_text(t) {
                    out.push(format!("plot `{}` has empty or unplain text", p.id));
                }
            }
            if !p.seen.contains("{from}") || !p.countered.contains("{from}") {
                out.push(format!(
                    "plot `{}`: texts must name the sender with {{from}}",
                    p.id
                ));
            }
        }
        for u in &self.rumour {
            if !ids.insert(format!("rumour {}", u.id)) {
                out.push(format!("rumour id `{}` is used twice", u.id));
            }
            if u.name.trim().is_empty() || u.text.trim().is_empty() || !plain_text(&u.text) {
                out.push(format!("rumour `{}` has empty or unplain text", u.id));
            }
            if u.about != "self" && !u.text.contains("{about}") {
                out.push(format!(
                    "rumour `{}` must say who it is about with {{about}}",
                    u.id
                ));
            }
        }
        for a in &self.award {
            if !ids.insert(format!("award {}", a.id)) {
                out.push(format!("award id `{}` is used twice", a.id));
            }
            if ![
                "legend", "grades", "pulled", "clues", "merit", "friends", "clean",
            ]
            .contains(&a.stat.as_str())
            {
                out.push(format!("award `{}` scores unknown stat `{}`", a.id, a.stat));
            }
            if a.text.trim().is_empty() || !plain_text(&a.text) || !plain_text(&a.name) {
                out.push(format!("award `{}` has empty or unplain text", a.id));
            }
        }
        out
    }
}

impl Table for MysteryFile {
    fn validate(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut ids = std::collections::HashSet::new();
        for c in &self.clue {
            if !ids.insert(c.id.as_str()) {
                out.push(format!("clue id `{}` is used twice", c.id));
            }
            if !(1..=4).contains(&c.act) {
                out.push(format!("clue `{}` is in act {}, not 1 to 4", c.id, c.act));
            }
            if c.risk > 3 {
                out.push(format!("clue `{}` risk is over 3", c.id));
            }
            if c.text.trim().is_empty() || !plain_text(&c.text) {
                out.push(format!("clue `{}` has empty or unplain text", c.id));
            }
            if c.kinds.is_empty()
                || c.kinds.iter().any(|k| {
                    ![
                        "class", "chapel", "lunch", "free", "after", "evening", "night",
                    ]
                    .contains(&k.as_str())
                })
            {
                out.push(format!("clue `{}` has no valid slot kind", c.id));
            }
        }
        for v in &self.verdict {
            if v.name.trim().is_empty() || !plain_text(&v.blurb) {
                out.push(format!("verdict `{}` has empty or unplain text", v.id));
            }
        }
        let mut seen = std::collections::HashSet::new();
        for v in &self.verdict {
            if !seen.insert(v.id.as_str()) {
                out.push(format!("verdict id `{}` is used twice", v.id));
            }
        }
        out
    }
}

/// Load both tables from their sources.
///
/// # Errors
/// If a table does not parse or fails its own checks.
pub(crate) fn load(
    social: &str,
    mystery: &str,
) -> Result<(SocialSpec, MysterySpec), russh_bbs_door::content::ContentError> {
    let s: SocialFile = russh_bbs_door::content::load("social", social)?;
    let m: MysteryFile = russh_bbs_door::content::load("mystery", mystery)?;
    Ok((
        SocialSpec {
            rules: s.rules,
            plots: s.plot,
            rumours: s.rumour,
            runners: s.runner,
            awards: s.award,
        },
        MysterySpec {
            rules: m.mystery,
            clues: m.clue,
            verdicts: m.verdict,
        },
    ))
}

/// Which subjects a rumour template may name, as ids.
fn bounded(delta: i32, limit: i32) -> bool {
    delta.abs() <= limit
}

impl Content {
    /// A plot template.
    #[must_use]
    pub fn plot(&self, id: &str) -> Option<&PlotSpec> {
        self.social.plots.iter().find(|p| p.id == id)
    }

    /// A rumour template.
    #[must_use]
    pub fn rumour(&self, id: &str) -> Option<&RumourSpec> {
        self.social.rumours.iter().find(|p| p.id == id)
    }

    /// A clue.
    #[must_use]
    pub fn clue(&self, id: &str) -> Option<&ClueSpec> {
        self.mystery.clues.iter().find(|c| c.id == id)
    }

    /// A verdict.
    #[must_use]
    pub fn verdict(&self, id: &str) -> Option<&VerdictSpec> {
        self.mystery.verdicts.iter().find(|v| v.id == id)
    }

    /// Who or what a rumour may be about: character ids, clique ids, or `self`.
    ///
    /// Gossip and tall tales may name only prankable staff and prefects and the rival cliques;
    /// never a Stray, and never an ordinary pupil. Praise may name anyone who is not a player.
    #[must_use]
    pub fn rumour_subjects(&self, spec: &RumourSpec) -> Vec<String> {
        match spec.about.as_str() {
            "self" => vec!["self".to_owned()],
            "clique" => self
                .cliques
                .iter()
                .filter(|c| spec.kind == "praise" || c.id != "strays")
                .map(|c| c.id.clone())
                .collect(),
            _ => self
                .npcs
                .iter()
                .filter(|n| {
                    if spec.kind == "praise" {
                        true
                    } else {
                        n.prankable && n.is_authority() && n.clique.as_deref() != Some("strays")
                    }
                })
                .map(|n| n.id.clone())
                .collect(),
        }
    }

    /// Problems across the social and mystery tables.
    pub(crate) fn social_problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let sr = &self.social.rules;
        let term = self.rules.term_days;
        if sr.tally_day >= term || self.mystery.rules.finale_day >= term {
            out.push("the election and the finale must fall inside the term".into());
        }
        if self.effect(&sr.player_win_effect).is_none() {
            out.push("player_win_effect is not a world effect".into());
        }
        match self.trait_spec(&sr.winner_trait) {
            Some(t) if !t.start => {}
            _ => out.push("winner_trait must be an earned trait".into()),
        }
        self.cond_problems("election", &sr.nominate_needs, &mut out);
        for p in &self.social.plots {
            let what = format!("plot `{}`", p.id);
            self.cond_problems(&what, &p.needs, &mut out);
            self.cond_problems(&what, &p.counter, &mut out);
            if !["mischief", "kind"].contains(&p.kind.as_str()) {
                out.push(format!("{what}: kind must be mischief or kind"));
            }
            if p.nerve < 1 || p.nerve > 4 || p.cash < 0 || p.cash > 20 {
                out.push(format!("{what}: costs out of range"));
            }
            if p.kind == "mischief" && p.counter.is_empty() {
                out.push(format!("{what}: a mischief plot needs a counterplay"));
            }
            if p.hit.is_empty() {
                out.push(format!("{what}: it does nothing to its target"));
            }
            // Tone: what lands on a player is small, is only a gauge, and never reaches the file,
            // the world, a friendship or a standing.
            for e in &p.hit {
                match e {
                    Eff::Stat(Gauge::Cool | Gauge::Nerve, n) if bounded(*n, 3) => {
                        if p.kind == "mischief" && *n > 0 || p.kind == "kind" && *n < 0 {
                            out.push(format!("{what}: a {} plot has the wrong sign", p.kind));
                        }
                    }
                    Eff::Stat(Gauge::Cash, n) if bounded(*n, 6) => {
                        if p.kind == "mischief" && *n > 0 || p.kind == "kind" && *n < 0 {
                            out.push(format!("{what}: a {} plot has the wrong sign", p.kind));
                        }
                    }
                    _ => out.push(format!(
                        "{what}: what lands on a player may only be a small change of cool, nerve or cash"
                    )),
                }
            }
            for e in p.reward.iter().chain(&p.fizzle) {
                if matches!(
                    e,
                    Eff::World(_)
                        | Eff::Betray(_)
                        | Eff::Arc(..)
                        | Eff::ArcEnd(..)
                        | Eff::File(..)
                        | Eff::Verdict(_)
                ) {
                    out.push(format!(
                        "{what}: a plot's own reward or cost may not do that"
                    ));
                }
            }
        }
        for u in &self.social.rumours {
            let what = format!("rumour `{}`", u.id);
            self.cond_problems(&what, &u.needs, &mut out);
            if !["npc", "clique", "self"].contains(&u.about.as_str()) {
                out.push(format!("{what}: about must be npc, clique or self"));
            }
            if !["praise", "gossip", "tall"].contains(&u.kind.as_str()) {
                out.push(format!("{what}: kind must be praise, gossip or tall"));
            }
            if u.kind == "tall" && u.about != "self" {
                out.push(format!("{what}: a tall tale is told about yourself"));
            }
            if u.kind == "gossip" && u.about == "self" {
                out.push(format!("{what}: gossip is about someone else"));
            }
            if u.kind != "praise" && u.bond > 0 {
                out.push(format!("{what}: only praise wins a friend"));
            }
            if self.rumour_subjects(u).is_empty() {
                out.push(format!("{what} has nobody it may be about"));
            }
            if u.patrol < 0 || u.patrol > 3 || u.heat < 0 {
                out.push(format!("{what}: patrol and heat only go up, and not far"));
            }
            // Tone: a rumour never costs the Strays standing or friendship.
            if u.about != "self" && u.kind != "praise" && (u.stand < 0 || u.bond < 0) {
                for s in self.rumour_subjects(u) {
                    let stray = self
                        .npc(&s)
                        .is_some_and(|n| n.clique.as_deref() == Some("strays"))
                        || s == "strays";
                    if stray {
                        out.push(format!("{what} can be about a Stray and costs them"));
                    }
                }
            }
        }
        for r in &self.social.runners {
            let what = format!("runner `{}`", r.npc);
            match self.npc(&r.npc) {
                None => out.push(format!("{what} is not a character")),
                Some(n) if n.clique.as_deref() == Some("strays") => {
                    out.push(format!("{what} is a Stray; the Strays do not run"));
                }
                Some(_) => {}
            }
            for e in r
                .withdrawn_by
                .iter()
                .chain(r.swing.iter().map(|(e, _)| e))
                .chain(std::iter::once(&r.win))
            {
                if self.effect(e).is_none() {
                    out.push(format!("{what} names unknown world effect `{e}`"));
                }
            }
            if r.platform.trim().is_empty() || !plain_text(&r.platform) {
                out.push(format!("{what} has an empty platform"));
            }
        }
        if self.social.runners.len() < 2 {
            out.push("an election needs at least two characters who run".into());
        }
        out
    }

    /// Problems across the mystery.
    pub(crate) fn mystery_problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let m = &self.mystery;
        let Some(arc) = self.arc(&m.rules.arc) else {
            out.push("the mystery's storyline does not exist".into());
            return out;
        };
        for (i, c) in m.clues.iter().enumerate() {
            let what = format!("clue `{}`", c.id);
            match self.place(&c.place) {
                None => out.push(format!("{what} is at unknown place `{}`", c.place)),
                Some(p) if !c.kinds.iter().any(|k| p.hours.contains(k)) => {
                    out.push(format!("{what} can only be found when its place is shut"));
                }
                Some(_) => {}
            }
            self.cond_problems(&what, &c.needs, &mut out);
            self.eff_problems(&what, &c.effects, &mut out);
            for a in &c.after {
                if !m.clues.iter().take(i).any(|x| x.id == *a) {
                    out.push(format!(
                        "{what} comes after `{a}`, which is not an earlier clue"
                    ));
                }
            }
        }
        for act in 1..=4u8 {
            if !m.clues.iter().any(|c| c.act == act) {
                out.push(format!("act {act} of the mystery has no clue"));
            }
        }
        if m.verdicts.len() < 3 {
            out.push("the mystery needs at least three verdicts".into());
        }
        let casts: Vec<&str> = arc
            .ending
            .iter()
            .flat_map(|e| e.effects.iter())
            .filter_map(|e| match e {
                Eff::Verdict(v) => Some(v.as_str()),
                _ => None,
            })
            .collect();
        for v in &m.verdicts {
            if self.effect(&v.effect).is_none() {
                out.push(format!("verdict `{}` names unknown world effect", v.id));
            }
            if !casts.contains(&v.id.as_str()) {
                out.push(format!(
                    "verdict `{}` is cast by no ending: dead content",
                    v.id
                ));
            }
        }
        for c in casts {
            if self.verdict(c).is_none() {
                out.push(format!("an ending casts unknown verdict `{c}`"));
            }
        }
        out
    }
}
