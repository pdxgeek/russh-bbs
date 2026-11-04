//! Long Haul: a space trade and exploration door (M4-L1: the engine).
//!
//! You captain a tramp freighter with a debt. Every jump burns fuel and drive-days, the lender
//! wants a payment each week, and the best profits sit on the longest, riskiest routes. This
//! crate is the deterministic engine plus a minimal playable text screen; crew, events and
//! combat (L2), the full UI (L3) and the social layer (L4) are later slices and have seams here.
//!
//! * [`frontier`]: about 120 systems, generated from the season number alone.
//! * [`market`], [`world`]: the board-wide economy. Stock moves with every captain's trades and
//!   the nightly tick; prices follow stock inside a fixed band; news spreads a hop a night.
//! * [`ship`], [`engine`], [`trade`]: hulls, modules, wear, fuel, jumps, the loan.
//! * [`player`]: a captain's save.
//! * [`content`]: the tables under `content/`.

#![forbid(unsafe_code)]

pub mod codec;
pub mod content;
pub mod engine;
pub mod events;
pub mod frontier;
pub mod market;
pub mod player;
mod session;
pub mod ship;
pub mod skills;
pub mod trade;
pub mod world;

use std::sync::{Arc, Mutex, PoisonError};

use russh_bbs_door::content::ContentError;
use russh_bbs_door::tick::{TickCtx, TickError};
use russh_bbs_door::{
    BlobKind, Door, DoorInfo, DoorRng, DoorSession, InvalidDoorId, MigrateError, Save, Start,
};

use crate::content::Content;
use crate::frontier::{Frontier, GEN_VERSION};
use crate::player::Player;
use crate::world::World;

/// The door's id. It never changes: saves and the world are filed under it.
pub const DOOR_ID: &str = "long-haul";
/// The player save's version (see [`player::SAVE_FORMAT`] for the format byte inside it).
pub const SAVE_VERSION: u32 = 1;
/// The shared world's version.
pub const WORLD_VERSION: u32 = 1;

/// Why the door could not be built.
#[derive(Debug)]
pub enum BuildError {
    /// The door id is not valid (a bug the tests rule out).
    Id(InvalidDoorId),
    /// An embedded content table is bad (the schema test rules this out).
    Content(ContentError),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Id(e) => write!(f, "long haul: {e}"),
            Self::Content(e) => write!(f, "long haul: {e}"),
        }
    }
}

impl std::error::Error for BuildError {}

/// The Long Haul door.
pub struct LongHaul {
    info: DoorInfo,
    content: Arc<Content>,
    frontiers: Mutex<Vec<Arc<Frontier>>>,
}

impl LongHaul {
    /// Build the door from the embedded content.
    ///
    /// # Errors
    /// An invalid id or content table; both are ruled out by tests.
    pub fn new() -> Result<Self, BuildError> {
        Ok(Self {
            info: DoorInfo::new(
                DOOR_ID,
                "Long Haul",
                "A tramp freighter, a loan and a failing frontier.",
                SAVE_VERSION,
                WORLD_VERSION,
            )
            .map_err(BuildError::Id)?,
            content: Arc::new(Content::embedded().map_err(BuildError::Content)?),
            frontiers: Mutex::new(Vec::new()),
        })
    }

    /// The door, shared, for the board's registry.
    ///
    /// # Errors
    /// See [`LongHaul::new`].
    pub fn shared() -> Result<Arc<dyn Door>, BuildError> {
        Ok(Arc::new(Self::new()?))
    }

    /// The content tables.
    #[must_use]
    pub fn content(&self) -> &Arc<Content> {
        &self.content
    }

    /// The frontier for a season (generated once and kept; the last few seasons stay cached).
    #[must_use]
    pub fn frontier(&self, season: u32) -> Arc<Frontier> {
        let mut cache = self
            .frontiers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = cache.iter().find(|f| f.season == season) {
            return Arc::clone(found);
        }
        let made = Arc::new(Frontier::generate(&self.content, season));
        if cache.len() >= 3 {
            cache.remove(0);
        }
        cache.push(Arc::clone(&made));
        made
    }
}

impl Door for LongHaul {
    fn info(&self) -> &DoorInfo {
        &self.info
    }

    fn open(&self, start: Start<'_>, rng: &mut DoorRng) -> Box<dyn DoorSession> {
        let frontier = self.frontier(start.season);
        let today = start.today.as_raw();
        let world = start
            .world
            .and_then(|bytes| World::decode(bytes, &frontier))
            .filter(|w| w.season == start.season && w.gen_version == GEN_VERSION)
            .unwrap_or_else(|| World::fresh(&self.content, &frontier, today));
        let player = match start.save {
            Save::Fresh => session::Start::New,
            // A save from an earlier season is a past captain: this season starts a new one.
            Save::Loaded { season, .. } if season != start.season => session::Start::New,
            Save::Loaded { bytes, .. } => match Player::decode(bytes, &self.content, &frontier) {
                Some(player) => session::Start::Captain(Box::new(player)),
                None => session::Start::Unreadable,
            },
        };
        Box::new(session::Session::new(
            Arc::clone(&self.content),
            frontier,
            world,
            player,
            &start.env.handle,
            today,
            rng,
        ))
    }

    fn migrate(&self, _kind: BlobKind, from: u32, _bytes: &[u8]) -> Result<Vec<u8>, MigrateError> {
        // Version 1 is the first: there is nothing older to read. When a version 2 exists, add
        // its `from == 1` step here and keep it for good (D98 (3): saves are forward-only).
        Err(MigrateError::Unsupported(from))
    }

    fn has_daily_tick(&self) -> bool {
        true
    }

    fn daily_tick(&self, tick: &mut TickCtx<'_>) -> Result<(), TickError> {
        let day = tick.day().as_raw();
        let season = tick.world().map_or(Some(1), World::peek_season);
        let season =
            season.ok_or_else(|| TickError::Door("the world blob is unreadable".into()))?;
        let frontier = self.frontier(season);
        let mut world = match tick.world() {
            None => World::fresh(&self.content, &frontier, day),
            Some(bytes) => World::decode(bytes, &frontier)
                .ok_or_else(|| TickError::Door("the world does not fit this frontier".into()))?,
        };
        world.tick_day(&self.content, &frontier, day, tick.rng());
        tick.set_world(world.encode());
        Ok(())
    }
}
