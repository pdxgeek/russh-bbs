//! Script degradation (M4-N2): a corporation learns the scripts used against it too often.
//!
//! The world counts, per corporation and per script identity, how many runs used the script
//! successfully. At [`WARY_AT`] the corporation recognises the signature: the script is weaker and
//! noisier there. At [`LEARNED_AT`] it has a patch: the script **does not work there at all**, and
//! the deck screen says so before you jack in. The counts fall every night ([`DECAY_PER_NIGHT`]) and
//! a new season starts them from nothing, so a script recovers with time.
//!
//! Two things soften it, and both are choices: a **polymorphic** build is learned half as fast
//! (`slow`), and the starter scripts are *commodity*: everyone has them, so a corporation gets wary
//! of them but never patches them (a green rig is never shut out of tier 1).

use crate::content::ScriptDef;

/// Uses after which a corporation is wary of a script.
pub const WARY_AT: u32 = 8;

/// Uses after which a corporation has patched a script.
pub const LEARNED_AT: u32 = 16;

/// Uses forgotten each night.
pub const DECAY_PER_NIGHT: u32 = 2;

/// The most rows the world keeps.
pub const ROWS_MAX: usize = 96;

/// How well a corporation knows a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// Not known.
    Fresh,
    /// Recognised: power down 1, noise up half.
    Wary,
    /// Patched: it does nothing here.
    Patched,
}

/// The stage of `script` after `uses` runs against a corporation.
#[must_use]
pub fn stage(script: &ScriptDef, uses: u32) -> Stage {
    let uses = if script.slow { uses / 2 } else { uses };
    if uses >= LEARNED_AT && !commodity(script) {
        Stage::Patched
    } else if uses >= WARY_AT {
        Stage::Wary
    } else {
        Stage::Fresh
    }
}

/// A script everybody has: the starters.
#[must_use]
pub const fn commodity(script: &ScriptDef) -> bool {
    script.starter
}
