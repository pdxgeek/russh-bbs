//! Nullroute: a hacking door (M4-N1, the engine; design in `docs/doors/nullroute.md`).
//!
//! You are a freelancer with a secondhand rig. Each day you take contracts from fixers, build a
//! deck of scripts that fits your rig's memory, jack in to a procedurally built corporate network
//! and work the node map toward the objective before the **trace** reaches 100.
//!
//! This slice is the deterministic engine and a minimal text screen:
//!
//! | Module | What it is |
//! |---|---|
//! | [`content`] | corporations, node types, scripts, contract templates, rig: TOML tables |
//! | [`net`] | network generation and reachability (the deck decides which nodes you can enter) |
//! | [`run`] | one run: the trace, the deck, moves, scripts, burn and jack-out |
//! | [`player`] | the character: rig, hideout, heat, skills, standing, catch-up of missed days |
//! | [`world`] | the season's memory (holes, alert, backdoors), the contract board, the nightly tick |
//! | [`save`] | the byte formats for the save and the world |
//!
//! The engine uses only the session RNG and the board's clock; the crate depends on the framework
//! and nothing else, and `tests/sandbox.rs` proves it.
//!
//! **Seams for the next slices.** N2 (countermeasures and coding) replaces the one-step patrol
//! rules in [`run::Run::act`] with encounters and adds script degradation (a per-corporation use
//! count beside [`world::NetState`]) and the module editor (the workbench flag in
//! [`player::Player`]). N3 replaces the text screen in `session`. N4 adds crews, honeypots at
//! hideouts, bounties and the fixer arcs on top of `Player::standing`.

#![forbid(unsafe_code)]
// The run and the screens are state machines and one-screen-per-view lists; splitting them by line
// count would scatter one rule across files. Run state is a set of independent flags.
#![allow(clippy::too_many_lines, clippy::struct_excessive_bools)]

pub mod autoplay;
pub mod cm;
pub mod cond;
pub mod code;
pub mod crew;
pub mod content;
pub mod learn;
pub mod net;
pub mod player;
pub mod run;
pub mod save;
mod session;
pub mod social;
pub mod story;
pub mod ties;
pub mod world;

use std::sync::Arc;

use russh_bbs_door::tick::{TickCtx, TickError};
use russh_bbs_door::{
    BlobKind, Door, DoorInfo, DoorRng, DoorSession, InvalidDoorId, MigrateError, Save as SaveInput,
    Start,
};

use crate::content::Content;
use crate::player::Player;

/// The door's id: the key of its saves and world on every board.
pub const DOOR_ID: &str = "nullroute";

/// The current save format version.
pub const SAVE_VERSION: u32 = 3;

/// The current world format version.
pub const WORLD_VERSION: u32 = 3;

/// The Nullroute door.
pub struct Nullroute {
    info: DoorInfo,
    content: Arc<Content>,
}

impl Nullroute {
    /// The door, with its embedded content loaded and validated.
    ///
    /// # Errors
    /// Only if [`DOOR_ID`] were not a valid id, which a test rules out.
    pub fn new() -> Result<Self, InvalidDoorId> {
        Ok(Self {
            info: DoorInfo::new(
                DOOR_ID,
                "Nullroute",
                "Hack corporate networks before the trace finds your rig.",
                SAVE_VERSION,
                WORLD_VERSION,
            )?,
            content: Arc::new(Content::embedded()),
        })
    }

    /// The door as the board registers it.
    ///
    /// # Errors
    /// See [`Nullroute::new`].
    pub fn shared() -> Result<Arc<dyn Door>, InvalidDoorId> {
        Ok(Arc::new(Self::new()?))
    }

    /// The loaded content.
    #[must_use]
    pub fn content(&self) -> &Content {
        &self.content
    }
}

impl Door for Nullroute {
    fn info(&self) -> &DoorInfo {
        &self.info
    }

    fn open(&self, start: Start<'_>, rng: &mut DoorRng) -> Box<dyn DoorSession> {
        let today = start.today.as_raw();
        let fresh = |rng: &mut DoorRng| {
            Player::new(
                &self.content,
                rng.next_u64(),
                &start.env.handle,
                start.season,
                today,
            )
        };
        let (mut player, saved) = match start.save {
            SaveInput::Fresh => (fresh(rng), Vec::new()),
            SaveInput::Loaded { bytes, .. } => match save::decode(bytes) {
                Some(p) => (p, bytes.to_vec()),
                // Bytes this version cannot read: start fresh rather than guess at them.
                None => (fresh(rng), Vec::new()),
            },
        };
        if player.season != start.season {
            player.new_season(&self.content, start.season, today);
        }
        let mut world = start.world.and_then(save::decode_world).unwrap_or_default();
        world.sync_season(&self.content, start.season);
        Box::new(session::Session::new(
            Arc::clone(&self.content),
            player,
            world,
            start.season,
            today,
            saved,
        ))
    }

    fn migrate(&self, kind: BlobKind, from: u32, bytes: &[u8]) -> Result<Vec<u8>, MigrateError> {
        // Version 2 (M4-N2) appends to version 1: fragments and coded scripts to a save, the season
        // and what the corporations have learned to the world. Version 1 is the oldest there is.
        let moved = match (kind, from) {
            (BlobKind::Save, 1) => save::migrate_save_v1(bytes),
            (BlobKind::World, 1) => save::migrate_world_v1(bytes),
            (BlobKind::Save, 2) => save::migrate_save_v2(bytes),
            (BlobKind::World, 2) => save::migrate_world_v2(bytes),
            _ => return Err(MigrateError::Unsupported(from)),
        };
        moved.ok_or(MigrateError::Corrupt(from))
    }

    fn has_daily_tick(&self) -> bool {
        true
    }

    fn daily_tick(&self, tick: &mut TickCtx<'_>) -> Result<(), TickError> {
        let mut world = tick
            .world()
            .and_then(save::decode_world)
            .unwrap_or_default();
        let day = tick.day().as_raw();
        world.nightly(&self.content, tick.rng(), day);
        tick.set_world(save::encode_world(&world));
        Ok(())
    }
}
