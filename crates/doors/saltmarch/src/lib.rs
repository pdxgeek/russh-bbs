//! Saltmarch: an original clan game on a tidal salt marsh (M4-S1, the engine slice).
//!
//! The sea took the old coast. Each player leads a clan on a scrap of high ground in a marsh
//! that floods and drains twice a day; the tide is both the clock and the map. This crate is
//! the deterministic engine: the tide, the seasonal map, the clan and its crew, the hold and
//! the economy, with a minimal text screen so the door can be opened, replayed and simulated.
//! Expeditions and combat, the real screens, raids and the market's social side are later
//! slices (`docs/doors/saltmarch.md`).
//!
//! How the pieces fit:
//!
//! | Rule (design doc) | Where |
//! |---|---|
//! | The tide table, spring/neap, storms | [`tide`] |
//! | The seasonal map from a seed | [`map`] |
//! | Crew: trades, attributes, skills, traits, injuries, aging, bonds | [`crew`], [`clan`] |
//! | The hold: plots, three tracks, upkeep, the sluice | [`hold`] |
//! | Resources, recipes, the Tidecourt's prices | [`types`], [`world`], [`engine`] |
//! | Tide-turns, the nightly rules | [`engine`] |
//! | Content as data | [`content`] and `content/*.toml` |
//!
//! **Time.** The only clock is `Ctx::now` (through `Ctx::today`); the only randomness is the
//! framework's `DoorRng`. The clan's own nights (production, upkeep, healing, aging) are
//! computed from its saved state the next time it is opened, from an RNG seeded by the clan
//! and the day, so they do not depend on when the player looks. The shared world (prices) is
//! changed by the framework's daily tick.
#![forbid(unsafe_code)]

pub mod board;
pub mod clan;
pub mod codec;
pub mod combat;
pub mod content;
pub mod crew;
pub mod engine;
pub mod event;
pub mod hold;
pub mod lore;
pub mod map;
mod session;
pub mod social;
pub mod story;
pub mod tide;
pub mod trip;
pub mod types;
pub mod world;

use std::fmt;
use std::sync::Arc;

use russh_bbs_door::content::ContentError;
use russh_bbs_door::tick::{TickCtx, TickError};
use russh_bbs_door::{
    BlobKind, Door, DoorInfo, DoorRng, DoorSession, InvalidDoorId, MigrateError, Save, Start,
};

use crate::clan::{Clan, Settings};
use crate::content::Catalog;
use crate::session::{Session, Setup};
use crate::world::World;

/// The door's id: the key of its saves and world on every board. Never change it.
pub const DOOR_ID: &str = "saltmarch";

/// The current save format version.
pub const SAVE_VERSION: u32 = 4;

/// The current world format version.
pub const WORLD_VERSION: u32 = 4;

/// Why the door could not be built.
#[derive(Debug)]
pub enum SetupError {
    /// [`DOOR_ID`] is not a valid id (a test rules this out).
    Id(InvalidDoorId),
    /// A content table is bad (a schema test rules this out).
    Content(ContentError),
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Id(e) => write!(f, "bad door id: {e}"),
            Self::Content(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SetupError {}

/// The door.
pub struct Saltmarch {
    info: DoorInfo,
    cat: Arc<Catalog>,
    settings: Settings,
}

impl Saltmarch {
    /// The door with the default settings (30 tide-turns a day).
    ///
    /// # Errors
    /// See [`SetupError`].
    pub fn new() -> Result<Self, SetupError> {
        Self::with_settings(Settings::default())
    }

    /// The door with the sysop's settings.
    ///
    /// # Errors
    /// See [`SetupError`].
    pub fn with_settings(settings: Settings) -> Result<Self, SetupError> {
        Ok(Self {
            info: DoorInfo::new(
                DOOR_ID,
                "Saltmarch",
                "Lead a clan on a tidal salt marsh. The water is the clock.",
                SAVE_VERSION,
                WORLD_VERSION,
            )
            .map_err(SetupError::Id)?,
            cat: Arc::new(Catalog::embedded().map_err(SetupError::Content)?),
            settings,
        })
    }

    /// The door as the board registers it.
    ///
    /// # Errors
    /// See [`SetupError`].
    pub fn shared() -> Result<Arc<dyn Door>, SetupError> {
        Ok(Arc::new(Self::new()?))
    }

    /// The loaded content.
    #[must_use]
    pub fn catalog(&self) -> &Catalog {
        &self.cat
    }
}

impl Door for Saltmarch {
    fn info(&self) -> &DoorInfo {
        &self.info
    }

    fn open(&self, start: Start<'_>, rng: &mut DoorRng) -> Box<dyn DoorSession> {
        let today = start.today.as_raw();
        let mut unreadable = false;
        // An empty blob is "no world yet" (the daily tick can write one before any player).
        let stored = match start.world {
            Some(bytes) if !bytes.is_empty() => {
                let decoded = World::decode(bytes);
                unreadable |= decoded.is_none();
                decoded
            }
            _ => None,
        };
        // A world left over from an earlier season is kept only to write up that season: the
        // new world carries its book, and each clan writes where it stood in its own hall.
        let (old_world, current) = match stored {
            Some(w) if w.season == start.season => (None, Some(w)),
            other => (other, None),
        };
        let (world, world_stored) = match current {
            Some(w) => (w, true),
            None => (
                World::successor(old_world.as_ref(), start.season, rng.next_u64(), today),
                false,
            ),
        };
        let clan = match start.save {
            Save::Fresh => None,
            Save::Loaded { bytes, .. } => {
                let decoded = Clan::decode(bytes);
                unreadable |= decoded.is_none();
                decoded
            }
        };
        // A clan from an earlier season starts the new one with only its hall.
        let mut rolled = false;
        let clan = clan.map(|c| {
            if c.season == start.season {
                return c;
            }
            rolled = true;
            let line = social::hall::season_line(&c, old_world.as_ref());
            let map = crate::map::Map::generate(world.seed, &self.cat.pois);
            let site = Clan::site_for(&c.name, map.sites.len());
            let heirloom = social::legacy::heirloom_for(&c, old_world.as_ref(), today);
            let mut next = c.next_season(&self.cat, &map, start.season, site, today, rng);
            social::legacy::start(&mut next, &self.cat, &map, world.legacy, heirloom);
            next.hall.push(clan::HallEntry {
                season: c.season,
                text: line,
            });
            if let Some(h) = heirloom {
                next.hall.push(clan::HallEntry {
                    season: c.season,
                    text: format!(
                        "Carried into the new season: {} ({}).",
                        h.name(),
                        h.effect()
                    ),
                });
            }
            if next.hall.len() > clan::HALL_CAP {
                next.hall.remove(0);
            }
            next
        });
        Box::new(Session::new(Setup {
            cat: Arc::clone(&self.cat),
            settings: self.settings,
            world,
            world_stored,
            clan,
            today,
            unreadable,
            rolled,
            default_name: start.env.handle.clone(),
        }))
    }

    fn migrate(&self, kind: BlobKind, from: u32, bytes: &[u8]) -> Result<Vec<u8>, MigrateError> {
        match (kind, from) {
            // Version 2 added the expedition counters, the clan's marks and the trip.
            (BlobKind::Save, 1) => {
                let upgraded = Clan::upgrade_v1(bytes);
                // Version 2 is not readable by this code; check it as it will be once the next
                // step has run.
                Clan::decode(&Clan::upgrade_v3(&Clan::upgrade_v2(&upgraded)))
                    .ok_or(MigrateError::Corrupt(1))?;
                Ok(upgraded)
            }
            // Version 3 added the social state: the clan's pending dispatches, inbox and counters.
            (BlobKind::Save, 2) => {
                let upgraded = Clan::upgrade_v2(bytes);
                Clan::decode(&Clan::upgrade_v3(&upgraded)).ok_or(MigrateError::Corrupt(2))?;
                Ok(upgraded)
            }
            // Version 4 added the story: standing, the contract in hand, the heirloom, the outbox.
            (BlobKind::Save, 3) => {
                let upgraded = Clan::upgrade_v3(bytes);
                Clan::decode(&upgraded).ok_or(MigrateError::Corrupt(3))?;
                Ok(upgraded)
            }
            // Version 2 added the league scores and the price history.
            (BlobKind::World, 1) => World::upgrade_v1(bytes).ok_or(MigrateError::Corrupt(1)),
            // Version 3 added the posted defences, the records and the social state.
            (BlobKind::World, 2) => World::upgrade_v2(bytes).ok_or(MigrateError::Corrupt(2)),
            // Version 4 added the causeways, the quests' changes to the marsh, the legacy and
            // what each clan has done that the ending reads.
            (BlobKind::World, 3) => World::upgrade_v3(bytes).ok_or(MigrateError::Corrupt(3)),
            _ => Err(MigrateError::Unsupported(from)),
        }
    }

    fn has_daily_tick(&self) -> bool {
        true
    }

    fn daily_tick(&self, tick: &mut TickCtx<'_>) -> Result<(), TickError> {
        let day = tick.day().as_raw();
        let Some(mut world) = tick
            .world()
            .filter(|b| !b.is_empty())
            .and_then(World::decode)
        else {
            // No world yet (or one this version cannot read): nothing to advance. The first
            // player creates the season's world; a tick must never invent a seed.
            return Ok(());
        };
        world.end_day(day, tick.rng());
        // Raids sent before today are settled against what the holds posted; the exchange's
        // old orders lapse and its night's prices are kept; kept pacts earn standing.
        let tide = tide::Tide::new(world.seed);
        social::raid::resolve_all(&mut world, &self.cat, &tide, day, tick.rng());
        social::market::tick(&mut world, day);
        world.pacts_tick(day);
        tick.set_world(world.encode());
        Ok(())
    }
}
