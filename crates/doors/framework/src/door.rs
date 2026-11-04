//! The `Door` trait and what crosses it.
//!
//! The board hands a session to a door with [`Door::open`] and the door hands it back by
//! answering [`Flow::Exit`]. Between those two, the board owns everything outside the door's
//! rectangle: the title bar, the status row, the security markers, the transport's footer, and
//! the logoff keys.

use russh_bbs_core::doors::{DoorId, InvalidDoorId};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::limits::Day;
use russh_bbs_core::terminal_settings::{LayoutId, TerminalSettings};

use crate::canvas::Canvas;
use crate::ctx::Ctx;
use crate::key::DoorKey;
use crate::rng::DoorRng;
use crate::tick::{TickCtx, TickError};

/// Which transport carries the session. **Information only**: a door must behave the same on
/// both (D94). The board has already taken what differs (the telnet footer row is not in the
/// door's area), so a door has no reason to branch on this; the field exists so a test can
/// prove it does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportKind {
    /// SSH.
    Ssh,
    /// Telnet (D27, D94).
    Telnet,
}

/// What a door knows about the member's terminal.
#[derive(Debug, Clone)]
pub struct DoorEnv {
    /// The transport. See [`TransportKind`].
    pub transport: TransportKind,
    /// The member's settings: color, theme, size overrides.
    pub settings: TerminalSettings,
    /// The icon tier in force (D45): a door that draws symbols draws the ASCII ones at
    /// [`Tier::Ascii`].
    pub tier: Tier,
    /// The layout in force for this window.
    pub layout: LayoutId,
    /// The member's handle, already sanitized.
    pub handle: String,
}

impl DoorEnv {
    /// An environment for tests: ssh, default settings, the ASCII tier, the classic layout.
    #[must_use]
    pub fn for_tests(handle: &str) -> Self {
        Self {
            transport: TransportKind::Ssh,
            settings: TerminalSettings::default(),
            tier: Tier::Ascii,
            layout: LayoutId::Classic,
            handle: handle.to_owned(),
        }
    }
}

/// Which blob a [`Door::migrate`] call is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobKind {
    /// A player's save.
    Save,
    /// The door's shared world.
    World,
}

/// Why a stored blob could not be brought up to date.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MigrateError {
    /// The door has no migration for this version (a bug in the door).
    #[error("no migration from version {0}")]
    Unsupported(u32),
    /// The bytes are not what that version wrote.
    #[error("the stored data is not valid for version {0}")]
    Corrupt(u32),
}

/// A door's fixed facts.
#[derive(Debug, Clone)]
pub struct DoorInfo {
    /// The door's id: the key of everything stored for it. Never change it once boards exist.
    pub id: DoorId,
    /// The name on the door list and the title bar.
    pub title: &'static str,
    /// One line for the door list.
    pub blurb: &'static str,
    /// The format version of a player's save as this code writes it (first version: 1).
    pub save_version: u32,
    /// The format version of the shared world (first version: 1; unused without a world).
    pub world_version: u32,
}

impl DoorInfo {
    /// Fixed facts for a door whose id is `id` (see [`DoorId::parse`]).
    pub fn new(
        id: &str,
        title: &'static str,
        blurb: &'static str,
        save_version: u32,
        world_version: u32,
    ) -> Result<Self, InvalidDoorId> {
        Ok(Self {
            id: DoorId::parse(id)?,
            title,
            blurb,
            save_version,
            world_version,
        })
    }
}

/// The player's save as the board gives it to [`Door::open`].
#[derive(Debug, Clone, Copy)]
pub enum Save<'a> {
    /// A first visit, or the save was reset by staff.
    Fresh,
    /// A stored save, already migrated to [`DoorInfo::save_version`].
    Loaded {
        /// The bytes the door wrote.
        bytes: &'a [u8],
        /// The season it was written in. Compare with [`Start::season`]: after staff end a
        /// season the two differ, and the door decides what that means.
        season: u32,
    },
}

/// Everything [`Door::open`] gets.
pub struct Start<'a> {
    /// The player's save.
    pub save: Save<'a>,
    /// The shared world as stored (migrated), if there is one.
    pub world: Option<&'a [u8]>,
    /// The member's terminal.
    pub env: &'a DoorEnv,
    /// Today, by the board's clock.
    pub today: Day,
    /// The door's current season.
    pub season: u32,
}

/// What a door answers after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Stay in the door.
    Stay,
    /// Leave the door; the board takes the session back.
    Exit,
}

/// One player's visit to a door.
pub trait DoorSession: Send {
    /// Handle a key. Save by calling [`Ctx::request_save`]; the board writes it after this
    /// returns, also when this returns [`Flow::Exit`].
    fn key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow;

    /// Draw the whole screen into `canvas`. Called after every batch of keys and when the
    /// window changes, so it must not depend on how many times it was called.
    fn render(&self, canvas: &mut Canvas<'_>, env: &DoorEnv);

    /// The same screen as plain lines of text, for a screen reader or a very small window
    /// (`docs/doors/README.md`: every map or board screen has a text "look around" view). The
    /// board sanitizes it like everything else.
    fn look_around(&self, env: &DoorEnv) -> Vec<String>;
}

/// A door game.
pub trait Door: Send + Sync + 'static {
    /// The door's fixed facts.
    fn info(&self) -> &DoorInfo;

    /// Start a visit. This is pure: it builds the session from the start data (and may draw from
    /// the session's seeded `rng`) and does no I/O.
    fn open(&self, start: Start<'_>, rng: &mut DoorRng) -> Box<dyn DoorSession>;

    /// Bring a stored blob forward one version: `bytes` is version `from`, the result must be
    /// version `from + 1`. The board calls it until the blob reaches the current version.
    /// Migrations are forward-only: a blob newer than this code is never loaded or overwritten.
    fn migrate(&self, kind: BlobKind, from: u32, bytes: &[u8]) -> Result<Vec<u8>, MigrateError> {
        let _ = (kind, bytes);
        Err(MigrateError::Unsupported(from))
    }

    /// Whether the door has a daily tick (the board then registers a scheduler job for it).
    fn has_daily_tick(&self) -> bool {
        false
    }

    /// Apply one day to the shared world. Called once per missed day, oldest first, with a cap
    /// (see the `tick` module). It must be a pure function of the world, the day and the tick's
    /// seeded RNG.
    fn daily_tick(&self, tick: &mut TickCtx<'_>) -> Result<(), TickError> {
        let _ = tick;
        Ok(())
    }
}
