//! Hallpass (M4-H1, M4-H2): a school-mischief door.
//!
//! You transfer mid-year into Greywater Academy and make mischief against the staff, the prefects
//! and the rival cliques, staying one step ahead of the headmistress's file. This slice is the
//! deterministic engine: the timetable and the school map, character schedules, stats and traits,
//! schemes (target, combinable plan, setup, the sneak / act / escape run, heat, the file, legend,
//! staff who learn, campaigns, and big schemes that change the school for everyone), the term
//! clock and the day tick, and a minimal text screen. M4-H2 adds class minigames ([`lesson`]), clubs
//! ([`club`]), conversations, friendships and storylines ([`talk`], with [`effect`]s). The full UI,
//! the social layer and the mystery are later slices; the seams are noted in `docs/doors/hallpass.md`.
//!
//! Layout: [`content`] (data tables), [`cond`] (the condition language), [`state`] and [`wire`]
//! (state and its binary form), [`game`] and [`scheme`] (the engine), [`world`] (the shared school
//! and the tick), [`options`] (legal actions), [`session`] (the screen).

#![forbid(unsafe_code)]
// Rules tables and screens read better as long straight-line functions than as many tiny ones.
#![allow(clippy::too_many_lines)]

mod check;
pub mod club;
pub mod cond;
pub mod content;
pub mod effect;
pub mod game;
pub mod lesson;
pub mod options;
pub mod save;
pub mod scheme;
mod session;
pub mod state;
pub mod talk;
pub mod wire;
pub mod world;

use std::sync::Arc;

use russh_bbs_door::tick::{TickCtx, TickError};
use russh_bbs_door::{
    BlobKind, Door, DoorInfo, DoorRng, DoorSession, InvalidDoorId, MigrateError, Save as SaveInput,
    Start,
};

use crate::content::Content;
use crate::game::Game;

/// The door's id: the key of its saves and world on every board.
pub const DOOR_ID: &str = "hallpass";

/// The current save format version.
pub const SAVE_VERSION: u32 = 2;

/// The current world format version.
pub const WORLD_VERSION: u32 = 1;

/// Why the door could not be built.
#[derive(Debug)]
pub enum BuildError {
    /// The id is not a valid door id.
    Id(InvalidDoorId),
    /// The embedded content failed its checks.
    Content(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Id(_) => f.write_str("hallpass has an invalid door id"),
            Self::Content(message) => write!(f, "hallpass content is invalid: {message}"),
        }
    }
}

impl std::error::Error for BuildError {}

/// The Hallpass door.
pub struct Hallpass {
    info: DoorInfo,
    content: Arc<Content>,
}

impl Hallpass {
    /// The door, with its embedded content loaded and checked.
    ///
    /// # Errors
    /// If the id is invalid or the embedded content fails its checks; tests rule both out.
    pub fn new() -> Result<Self, BuildError> {
        let content = Content::embedded().map_err(|e| BuildError::Content(e.to_string()))?;
        Self::with_content(content)
    }

    /// The door with other content (tests and writers' previews).
    ///
    /// # Errors
    /// If the id is invalid.
    pub fn with_content(content: Content) -> Result<Self, BuildError> {
        Ok(Self {
            info: DoorInfo::new(
                DOOR_ID,
                "Hallpass",
                "Mischief at Greywater Academy. Pull schemes, dodge the file.",
                SAVE_VERSION,
                WORLD_VERSION,
            )
            .map_err(BuildError::Id)?,
            content: Arc::new(content),
        })
    }

    /// The door as the board registers it.
    ///
    /// # Errors
    /// See [`Hallpass::new`].
    pub fn shared() -> Result<Arc<dyn Door>, BuildError> {
        Ok(Arc::new(Self::new()?))
    }

    /// The content tables.
    #[must_use]
    pub fn content(&self) -> &Arc<Content> {
        &self.content
    }
}

impl Door for Hallpass {
    fn info(&self) -> &DoorInfo {
        &self.info
    }

    fn open(&self, start: Start<'_>, _rng: &mut DoorRng) -> Box<dyn DoorSession> {
        let saved = match start.save {
            // A save from before staff ended the season, or one this version cannot read, starts a new character.
            SaveInput::Loaded { bytes, season } if season == start.season => {
                save::decode_player(bytes)
            }
            SaveInput::Loaded { .. } | SaveInput::Fresh => None,
        };
        let (game, opening) = Game::open(
            &self.content,
            &start.env.handle,
            saved,
            start.world,
            start.today.as_raw(),
        );
        Box::new(session::Session::new(game, opening))
    }

    fn migrate(&self, kind: BlobKind, from: u32, bytes: &[u8]) -> Result<Vec<u8>, MigrateError> {
        save::migrate(kind, from, bytes)
    }

    fn has_daily_tick(&self) -> bool {
        true
    }

    fn daily_tick(&self, tick: &mut TickCtx<'_>) -> Result<(), TickError> {
        let day = tick.day().as_raw();
        let mut world = match tick.world() {
            Some(stored) => wire::decode(stored)
                .ok_or_else(|| TickError::Door("the stored world is damaged".to_owned()))?,
            None => state::World::new(day),
        };
        world::tick_day(&mut world, &self.content, day, tick.rng());
        tick.set_world(world.to_bytes());
        Ok(())
    }
}
