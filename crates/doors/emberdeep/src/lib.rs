//! Emberdeep (M4-E1): a fantasy delving door. One hero, one town, a dungeon that is the same for
//! everyone and deeper each season. This crate is the **engine** slice: the hero, the seasonal
//! shared dungeon, turn-based combat with telegraphed enemies, the torch-turn economy, the
//! nightly tick, versioned saves and a minimal playable text screen. Town, crafting, the full UI,
//! parties, duels and the content pass are later slices (see `docs/doors/emberdeep.md`).
//!
//! **The RPG rule.** Every part of the hero changes what happens: callings, skill nodes, traits,
//! scars and wounds all fold into one derived sheet ([`hero`]); rooms offer options by the hero's
//! capability tags and attributes, so two builds see different choices in the same room, and the
//! results (a lift or a rope line) change the shared world for everyone.
//!
//! **Determinism.** All time comes from the board's clock (as a day number) and all randomness
//! from the hero's own `DoorRng`, whose state is part of the save; the dungeon is a pure function
//! of the season, epoch and depth. A recorded run therefore replays exactly.

#![forbid(unsafe_code)]

mod codec;
mod combat;
pub mod content;
pub mod content_story;
pub mod content_town;
mod dungeon;
mod endings;
mod game;
mod hero;
mod rules;
mod save;
mod session;
#[doc(hidden)]
pub mod showcase;
mod social;
mod story;
mod town;
mod world;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use russh_bbs_door::tick::{TickCtx, TickError};
use russh_bbs_door::{
    BlobKind, Door, DoorInfo, DoorRng, DoorSession, InvalidDoorId, MigrateError, Start,
};

use crate::content::Content;
use crate::world::{WORLD_FORMAT, World};

pub use crate::save::{SAVE_VERSION, Summary, Whereabouts, summarize};
pub use crate::world::{TownSummary, WorldSummary, summarize_town, summarize_world};

/// The door's id: the key of its saves and world on every board.
pub const DOOR_ID: &str = "emberdeep";

/// The current world format version.
pub const WORLD_VERSION: u32 = WORLD_FORMAT;

/// The Emberdeep door.
pub struct Emberdeep {
    info: DoorInfo,
    content: Option<Arc<Content>>,
}

impl Emberdeep {
    /// The door, with its embedded content loaded and validated. If the content were broken (a
    /// test rules that out) the door still registers and tells the player it is sealed.
    ///
    /// # Errors
    /// Only if [`DOOR_ID`] were not a valid id, which a test rules out.
    pub fn new() -> Result<Self, InvalidDoorId> {
        Ok(Self {
            info: DoorInfo::new(
                DOOR_ID,
                "Emberdeep",
                "Delve the old forges under Cindermere. How deep will you go today?",
                SAVE_VERSION,
                WORLD_VERSION,
            )?,
            content: Content::embedded().ok().map(Arc::new),
        })
    }

    /// The door as the board registers it.
    ///
    /// # Errors
    /// See [`Emberdeep::new`].
    pub fn shared() -> Result<Arc<dyn Door>, InvalidDoorId> {
        Ok(Arc::new(Self::new()?))
    }

    /// The loaded content (for the balance harness and tests).
    #[must_use]
    pub fn content(&self) -> Option<&Arc<Content>> {
        self.content.as_ref()
    }
}

impl Door for Emberdeep {
    fn info(&self) -> &DoorInfo {
        &self.info
    }

    fn open(&self, start: Start<'_>, rng: &mut DoorRng) -> Box<dyn DoorSession> {
        Box::new(session::Session::new(
            self.content.as_ref(),
            &start,
            rng.next_u64(),
        ))
    }

    fn migrate(&self, kind: BlobKind, from: u32, bytes: &[u8]) -> Result<Vec<u8>, MigrateError> {
        // Format 1 (M4-E1) becomes format 2 (the town slice) by appending an empty town section, and
        // format 2 becomes format 3 (M4-E4) by appending an empty story and social section;
        // a blob that does not read as format 1 is refused, never guessed at.
        match (kind, from) {
            (BlobKind::Save, 1) => save::upgrade_v1(bytes).ok_or(MigrateError::Corrupt(1)),
            (BlobKind::World, 1) => world::upgrade_v1(bytes).ok_or(MigrateError::Corrupt(1)),
            (BlobKind::Save, 2) => save::upgrade_v2(bytes).ok_or(MigrateError::Corrupt(2)),
            (BlobKind::World, 2) => world::upgrade_v2(bytes).ok_or(MigrateError::Corrupt(2)),
            _ => Err(MigrateError::Unsupported(from)),
        }
    }

    fn has_daily_tick(&self) -> bool {
        true
    }

    fn daily_tick(&self, tick: &mut TickCtx<'_>) -> Result<(), TickError> {
        let day = tick.day().as_raw();
        // The tick does not know the season, so it keeps whatever season the stored world has
        // (a new season's first session starts a fresh world, see `World::for_season`).
        let mut world = tick
            .world()
            .and_then(World::decode)
            .unwrap_or_else(|| World::new(1));
        match &self.content {
            Some(content) => world.tick_full(day, content),
            None => world.tick(day),
        }
        tick.set_world(world.encode());
        Ok(())
    }
}
