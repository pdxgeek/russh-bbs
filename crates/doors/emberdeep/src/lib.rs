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
mod dungeon;
mod game;
mod hero;
mod rules;
mod save;
mod session;
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

    fn migrate(&self, _kind: BlobKind, from: u32, _bytes: &[u8]) -> Result<Vec<u8>, MigrateError> {
        // Format 1 is the first. When a version 2 exists, this is where 1 becomes 2.
        Err(MigrateError::Unsupported(from))
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
        world.tick(day);
        tick.set_world(world.encode());
        Ok(())
    }
}
