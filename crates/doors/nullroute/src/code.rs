//! Coding (M4-N2): the module editor at the workbench.
//!
//! Fragments found on runs combine into a new script: pick an **effect**, optionally a **target
//! type** (tunes it to sentries, tracers, walls or wardens) and optionally a **modifier**. The
//! build's numbers are the sum of its fragments', and it must fit the editor's **memory** and
//! **noise** budgets: a quiet build costs memory, a powerful one costs noise, and the greedy ones do
//! not fit at all without something lean beside them. Coding burns the fragments and
//! [`CODE_CYCLES`] cycles. The same recipe is the same script to everyone, so a corporation learns
//! it as one signature (see [`crate::learn`]).

use crate::content::{Content, Effect, Family, FragKind, ScriptDef, codable};

/// The most memory a coded script may weigh.
pub const MEMORY_BUDGET: u32 = 6;

/// The most noise a coded script may make.
pub const NOISE_BUDGET: u32 = 10;

/// Cycles one coding session costs.
pub const CODE_CYCLES: u32 = 4;

/// Coded scripts a character can hold.
pub const CODED_MAX: usize = 16;

/// Script numbers from here up are coded scripts: `CODED_BASE + slot`.
pub const CODED_BASE: u8 = 128;

/// Fragments a character can carry.
pub const FRAGMENTS_MAX: usize = 40;

/// A recipe: fragment numbers (0 for none in the target and modifier slots).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Build {
    /// The effect fragment.
    pub effect: u8,
    /// The target fragment, or 0.
    pub target: u8,
    /// The modifier fragment, or 0.
    pub modifier: u8,
}

/// The numbers a build comes to, before the budgets are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    /// Power, 1 to 9.
    pub power: u32,
    /// Memory, 1 to 8.
    pub memory: u32,
    /// Noise.
    pub noise: u32,
    /// Cycles, 1 to 3.
    pub cycles: u32,
    /// Uses a run (0 is unlimited).
    pub charges: u32,
}

const fn family_of(effect: Effect) -> Family {
    match effect {
        Effect::Scan | Effect::Map => Family::Probe,
        Effect::Crack | Effect::Spoof | Effect::Overflow => Family::Break,
        Effect::Mask | Effect::Decoy | Effect::Reroute => Family::Hide,
        Effect::Jam | Effect::Fork | Effect::Purge => Family::Fight,
        _ => Family::Utility,
    }
}

/// What an effect costs and gives before fragments: (cycles, charges).
const fn effect_base(effect: Effect) -> (u32, u32) {
    match effect {
        Effect::Crack | Effect::Spoof | Effect::Overflow | Effect::Reroute | Effect::Purge => (
            2,
            if matches!(effect, Effect::Reroute) {
                2
            } else {
                0
            },
        ),
        Effect::Mask => (1, 2),
        Effect::Decoy | Effect::Fork => (1, 1),
        _ => (1, 0),
    }
}

fn add(base: u32, delta: i32) -> u32 {
    u32::try_from((i64::from(base) + i64::from(delta)).max(0)).unwrap_or(0)
}

/// The identity a corporation learns: all three fragments packed (the effect is never 0, so this
/// never collides with a stock script's number, which is under 1024).
#[must_use]
pub const fn signature(build: Build) -> u16 {
    ((build.effect as u16) << 10) | ((build.target as u16) << 5) | build.modifier as u16
}

/// The numbers of a build.
///
/// # Errors
/// If a fragment is missing, of the wrong kind, or the effect cannot be coded.
pub fn stats(content: &Content, build: Build) -> Result<Stats, String> {
    let effect = content
        .fragment(build.effect)
        .filter(|f| f.kind == FragKind::Effect)
        .ok_or("Pick an effect fragment.")?;
    let target = if build.target == 0 {
        None
    } else {
        Some(
            content
                .fragment(build.target)
                .filter(|f| f.kind == FragKind::Target)
                .ok_or("That is not a target fragment.")?,
        )
    };
    let modifier = if build.modifier == 0 {
        None
    } else {
        Some(
            content
                .fragment(build.modifier)
                .filter(|f| f.kind == FragKind::Modifier)
                .ok_or("That is not a modifier fragment.")?,
        )
    };
    let fx = effect
        .effect
        .filter(|e| codable(*e))
        .ok_or("Not codable.")?;
    let parts = [Some(effect), target, modifier];
    let sum = |pick: fn(&crate::content::FragmentDef) -> i32| -> i32 {
        parts.iter().flatten().map(|f| pick(f)).sum()
    };
    let (cycles, charges) = effect_base(fx);
    Ok(Stats {
        power: add(0, sum(|f| f.power)).clamp(1, 9),
        memory: add(0, sum(|f| f.memory)).clamp(1, 8),
        noise: add(0, sum(|f| f.noise)).min(30),
        cycles: add(cycles, modifier.map_or(0, |m| m.cycles)).clamp(1, 3),
        charges: modifier
            .filter(|m| m.charges > 0)
            .map_or(charges, |m| m.charges),
    })
}

/// A short plain name for a build, such as `crack-warden-piercing`.
#[must_use]
pub fn name(content: &Content, build: Build) -> String {
    let word = |n: u8| content.fragment(n).map(|f| f.name.as_str());
    let fx = content
        .fragment(build.effect)
        .and_then(|f| f.effect)
        .map_or("?", effect_word);
    let target = content
        .fragment(build.target)
        .and_then(|f| f.target)
        .map_or("any", |t| match t {
            crate::content::CmKind::Sentry => "sentry",
            crate::content::CmKind::Tracer => "tracer",
            crate::content::CmKind::Wall => "wall",
            crate::content::CmKind::Warden => "warden",
        });
    let modifier = word(build.modifier).unwrap_or("plain");
    format!("{fx}-{target}-{modifier}")
}

const fn effect_word(effect: Effect) -> &'static str {
    match effect {
        Effect::Scan => "scan",
        Effect::Map => "map",
        Effect::Crack => "crack",
        Effect::Spoof => "spoof",
        Effect::Overflow => "overflow",
        Effect::Mask => "mask",
        Effect::Decoy => "decoy",
        Effect::Reroute => "reroute",
        Effect::Jam => "jam",
        Effect::Fork => "fork",
        Effect::Purge => "purge",
        Effect::Copy => "copy",
        Effect::Plant => "plant",
        Effect::Shred => "shred",
        Effect::Cutfeed => "cutfeed",
        Effect::Wipelogs => "wipelogs",
    }
}

/// Build the script a recipe makes, numbered `num`.
///
/// # Errors
/// If the recipe is not valid or does not fit the budgets.
pub fn assemble(content: &Content, build: Build, num: u8) -> Result<ScriptDef, String> {
    let s = stats(content, build)?;
    if s.memory > MEMORY_BUDGET {
        return Err(format!(
            "Over the memory budget: {} of {MEMORY_BUDGET}.",
            s.memory
        ));
    }
    if s.noise > NOISE_BUDGET {
        return Err(format!(
            "Over the noise budget: {} of {NOISE_BUDGET}.",
            s.noise
        ));
    }
    let fx = content
        .fragment(build.effect)
        .and_then(|f| f.effect)
        .ok_or("Pick an effect fragment.")?;
    let target = content.fragment(build.target).and_then(|f| f.target);
    let modifier = content.fragment(build.modifier);
    Ok(ScriptDef {
        num,
        name: name(content, build),
        family: family_of(fx),
        effect: fx,
        power: s.power,
        memory: s.memory,
        noise: s.noise,
        cycles: s.cycles,
        charges: s.charges,
        price: 0,
        min_rep: 0,
        min_skill: 0,
        starter: false,
        blurb: "Coded at your workbench.".to_owned(),
        target,
        pierce: modifier.map_or(0, |m| m.pierce),
        slow: modifier.is_some_and(|m| m.slow),
        sig: signature(build),
    })
}

/// The best build `have` can make that fits the budgets, by a plain rule: it prefers a warden
/// target, then the most power, then the least noise. `None` if there is no effect fragment.
/// Used by the headless player and the tests; a person chooses by hand.
#[must_use]
pub fn best_build(content: &Content, have: &[u8]) -> Option<Build> {
    let of = |kind: FragKind| -> Vec<u8> {
        let mut v: Vec<u8> = have
            .iter()
            .copied()
            .filter(|n| content.fragment(*n).is_some_and(|f| f.kind == kind))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let (effects, mut targets, mut modifiers) = (
        of(FragKind::Effect),
        of(FragKind::Target),
        of(FragKind::Modifier),
    );
    targets.insert(0, 0);
    modifiers.insert(0, 0);
    let mut best: Option<(Build, (bool, u32, std::cmp::Reverse<u32>))> = None;
    for &effect in &effects {
        for &target in &targets {
            for &modifier in &modifiers {
                let build = Build {
                    effect,
                    target,
                    modifier,
                };
                let Ok(def) = assemble(content, build, CODED_BASE) else {
                    continue;
                };
                let score = (
                    def.target == Some(crate::content::CmKind::Warden),
                    def.power + def.pierce,
                    std::cmp::Reverse(def.noise),
                );
                if best.is_none_or(|(_, s)| score > s) {
                    best = Some((build, score));
                }
            }
        }
    }
    best.map(|(b, _)| b)
}
