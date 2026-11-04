//! The door store trait and the types it passes.

use std::fmt;
use std::future::Future;

use crate::access::Slug;
use crate::audit::AuditEvent;
use crate::limits::Day;
use crate::store::{AccountId, StoreError, UnixSeconds};

/// The most bytes one player's save may hold.
pub const MAX_SAVE_BYTES: usize = 64 * 1024;

/// The most bytes one door's shared world may hold.
pub const MAX_WORLD_BYTES: usize = 256 * 1024;

/// The longest door id, so `door-<id>` is always a valid scheduler job name (32 characters).
pub const MAX_DOOR_ID_CHARS: usize = 24;

/// Why text is not a [`DoorId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a door id is 1 to 24 characters of a-z, 0-9 and -")]
pub struct InvalidDoorId;

/// A door's id: a [`Slug`] of at most [`MAX_DOOR_ID_CHARS`] characters, fixed by the door's code
/// and used as the key of everything the board stores for it. Never typed by a player.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DoorId(Slug);

impl DoorId {
    /// Validate an id. Call this where a door is defined.
    pub fn parse(text: &str) -> Result<Self, InvalidDoorId> {
        if text.len() > MAX_DOOR_ID_CHARS {
            return Err(InvalidDoorId);
        }
        Slug::parse(text).map(Self).map_err(|_| InvalidDoorId)
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The scheduler job name for this door's daily tick.
    #[must_use]
    pub fn tick_job(&self) -> Slug {
        // `door-` plus at most 24 characters is at most 29, and both parts are slug characters.
        Slug::parse(&format!("door-{}", self.0.as_str())).unwrap_or(Slug::SYSOP)
    }
}

impl fmt::Display for DoorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How many times a row has been written. Zero means "no row yet"; the first write makes it 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Revision(u64);

impl Revision {
    /// "Nothing stored yet": what a first write expects.
    pub const NONE: Self = Self(0);

    /// From a stored number.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The number to store.
    #[must_use]
    pub const fn as_raw(self) -> u64 {
        self.0
    }
}

/// A player's stored save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSave {
    /// The door's season when it was written.
    pub season: u32,
    /// The game's own format version, for its forward-only migrations.
    pub version: u32,
    /// Which write this is; pass it back to replace the save.
    pub revision: Revision,
    /// The opaque bytes.
    pub bytes: Vec<u8>,
    /// When it was written, by the store's clock.
    pub updated_at: UnixSeconds,
}

/// A door's stored shared world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredWorld {
    /// The game's own format version.
    pub version: u32,
    /// Which write this is.
    pub revision: Revision,
    /// The opaque bytes.
    pub bytes: Vec<u8>,
    /// The last UTC day the daily tick has been applied for, if it ever ran.
    pub last_tick: Option<Day>,
}

/// A save to write. `bytes` is at most [`MAX_SAVE_BYTES`].
#[derive(Debug, Clone, Copy)]
pub struct SaveWrite<'a> {
    /// The revision read (or [`Revision::NONE`]); a different stored revision is a conflict.
    pub expected: Revision,
    /// The door's current season.
    pub season: u32,
    /// The game's format version.
    pub version: u32,
    /// The bytes.
    pub bytes: &'a [u8],
}

/// A world to write. `bytes` is at most [`MAX_WORLD_BYTES`].
#[derive(Debug, Clone, Copy)]
pub struct WorldWrite<'a> {
    /// The revision read (or [`Revision::NONE`]).
    pub expected: Revision,
    /// The game's format version.
    pub version: u32,
    /// The bytes.
    pub bytes: &'a [u8],
    /// The day a tick was just applied for, or `None` to leave the last tick as it is.
    pub tick: Option<Day>,
}

/// Why a door write was not made.
#[derive(Debug, thiserror::Error)]
pub enum DoorStoreError {
    /// The storage backend failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The row was written by someone else since it was read. Nothing changed.
    #[error("the stored door data changed since it was read")]
    Conflict,
    /// The blob is over its cap. Nothing changed.
    #[error("the door data is over its size cap of {cap} bytes")]
    TooLarge {
        /// The cap that applies.
        cap: usize,
    },
}

/// Door persistence. The framework's save API is the only caller outside tests.
pub trait DoorStore: Send + Sync {
    /// The player's save, if any.
    fn load_save(
        &self,
        door: &DoorId,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<StoredSave>, StoreError>> + Send;

    /// Write the player's save, refusing a stale revision ([`DoorStoreError::Conflict`]) or an
    /// over-cap blob ([`DoorStoreError::TooLarge`]) without changing anything. Returns the new
    /// revision.
    fn write_save(
        &self,
        door: &DoorId,
        account: AccountId,
        write: &SaveWrite<'_>,
    ) -> impl Future<Output = Result<Revision, DoorStoreError>> + Send;

    /// The door's shared world, if one was ever written.
    fn load_world(
        &self,
        door: &DoorId,
    ) -> impl Future<Output = Result<Option<StoredWorld>, StoreError>> + Send;

    /// Write the door's world (and, with `tick`, the day the tick was applied for) in one
    /// step, with the same refusals as [`DoorStore::write_save`].
    fn write_world(
        &self,
        door: &DoorId,
        write: &WorldWrite<'_>,
    ) -> impl Future<Output = Result<Revision, DoorStoreError>> + Send;

    /// The door's season: 1 until staff end one.
    fn season(&self, door: &DoorId) -> impl Future<Output = Result<u32, StoreError>> + Send;

    /// Whether staff barred this player from the door.
    fn is_barred(
        &self,
        door: &DoorId,
        account: AccountId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// The rank of the account's top role (0 for none), for the staff rank rule (D69).
    /// `None` for an account that does not exist.
    fn account_rank(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Option<u8>, StoreError>> + Send;

    /// Delete the player's save and write `audit`, in one transaction. `true` if there was a
    /// save; when there was none, nothing is written (not even the audit row).
    fn reset_player(
        &self,
        door: &DoorId,
        account: AccountId,
        audit: &AuditEvent,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// Add one to the door's season and write `audit`, in one transaction. Returns the new
    /// season.
    fn end_season(
        &self,
        door: &DoorId,
        audit: &AuditEvent,
    ) -> impl Future<Output = Result<u32, StoreError>> + Send;

    /// Bar the player and write `audit`. `true` if they were not barred already (otherwise
    /// nothing is written).
    fn bar_player(
        &self,
        door: &DoorId,
        account: AccountId,
        by: AccountId,
        audit: &AuditEvent,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// Lift a bar and write `audit`. `true` if the player was barred (otherwise nothing is
    /// written).
    fn unbar_player(
        &self,
        door: &DoorId,
        account: AccountId,
        audit: &AuditEvent,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;
}
