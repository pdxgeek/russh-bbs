//! The example door: a coin toss with a daily allowance (M4-D0, D98).
//!
//! It exists to show the framework working end to end and to be small enough to read in one
//! sitting. It is not a game design. It has:
//!
//! - a pure rules engine ([`engine`]) with a seeded RNG and no I/O;
//! - a versioned save ([`save`]) with a forward-only migration from version 1;
//! - a shared world (total tosses, the day count) changed by sessions and by the daily tick;
//! - content as data ([`content`]): flavor lines in an embedded TOML table with a schema test;
//! - a player-typed nickname that goes through the framework's reserved-name rule;
//! - a text-first screen that doubles as its look-around view.
//!
//! The crate depends on the framework and nothing else, and `tests/sandbox.rs` proves it
//! contains no filesystem, network, process, thread, clock or printing API.

#![forbid(unsafe_code)]

pub mod content;
pub mod engine;
pub mod save;
mod session;
pub mod world;

use std::sync::Arc;

use russh_bbs_door::tick::{TickCtx, TickError};
use russh_bbs_door::{
    BlobKind, Door, DoorInfo, DoorRng, DoorSession, InvalidDoorId, MigrateError, Save as SaveInput,
    Start,
};

use crate::content::Phrases;
use crate::world::World;

/// The door's id: the key of its saves and world on every board.
pub const DOOR_ID: &str = "coin-toss";

/// The current save format version.
pub const SAVE_VERSION: u32 = 2;

/// The current world format version.
pub const WORLD_VERSION: u32 = 1;

/// The example door.
pub struct CoinToss {
    info: DoorInfo,
    phrases: Phrases,
}

impl CoinToss {
    /// The door, with its embedded content loaded and validated.
    ///
    /// # Errors
    /// Only if [`DOOR_ID`] were not a valid id, which a test rules out; the board treats it as
    /// a start-up error.
    pub fn new() -> Result<Self, InvalidDoorId> {
        Ok(Self {
            info: DoorInfo::new(
                DOOR_ID,
                "Coin Toss",
                "Call it in the air. Five tosses a day.",
                SAVE_VERSION,
                WORLD_VERSION,
            )?,
            phrases: Phrases::embedded(),
        })
    }

    /// The door as the board registers it.
    ///
    /// # Errors
    /// See [`CoinToss::new`].
    pub fn shared() -> Result<Arc<dyn Door>, InvalidDoorId> {
        Ok(Arc::new(Self::new()?))
    }
}

impl Door for CoinToss {
    fn info(&self) -> &DoorInfo {
        &self.info
    }

    fn open(&self, start: Start<'_>, _rng: &mut DoorRng) -> Box<dyn DoorSession> {
        let state = match start.save {
            SaveInput::Fresh => engine::State::new(start.today.as_raw(), &start.env.handle),
            SaveInput::Loaded { bytes, .. } => match save::decode(bytes) {
                Some(mut state) => {
                    state.new_day(start.today.as_raw());
                    state
                }
                // Bytes this version cannot read: start fresh rather than guess at them.
                None => engine::State::new(start.today.as_raw(), &start.env.handle),
            },
        };
        let world = start.world.and_then(World::decode).unwrap_or_default();
        Box::new(session::Session::new(state, world, self.phrases.clone()))
    }

    fn migrate(&self, kind: BlobKind, from: u32, bytes: &[u8]) -> Result<Vec<u8>, MigrateError> {
        match (kind, from) {
            (BlobKind::Save, 1) => save::migrate_v1(bytes).ok_or(MigrateError::Corrupt(1)),
            _ => Err(MigrateError::Unsupported(from)),
        }
    }

    fn has_daily_tick(&self) -> bool {
        true
    }

    fn daily_tick(&self, tick: &mut TickCtx<'_>) -> Result<(), TickError> {
        let mut world = tick.world().and_then(World::decode).unwrap_or_default();
        world.end_day();
        tick.set_world(world.encode());
        Ok(())
    }
}
