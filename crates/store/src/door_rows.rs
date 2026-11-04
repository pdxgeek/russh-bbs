//! Backend-independent mapping for the door tables: stored numbers to core's types, written once
//! (D24). Each backend runs its own SQL and hands the raw columns here.

use russh_bbs_core::doors::{
    DoorStoreError, MAX_SAVE_BYTES, MAX_WORLD_BYTES, Revision, SaveWrite, StoredSave, StoredWorld,
    WorldWrite,
};
use russh_bbs_core::limits::Day;
use russh_bbs_core::store::{StoreError, UnixSeconds};

fn corrupt(what: &str) -> StoreError {
    StoreError::Corrupt(format!("a stored door {what} is out of range"))
}

/// A stored count as a `u32`.
pub(crate) fn small(raw: i64, what: &str) -> Result<u32, StoreError> {
    u32::try_from(raw).map_err(|_| corrupt(what))
}

/// A stored revision.
pub(crate) fn revision(raw: i64) -> Result<Revision, StoreError> {
    u64::try_from(raw)
        .map(Revision::from_raw)
        .map_err(|_| corrupt("revision"))
}

/// The raw revision to store after `expected`: one more.
pub(crate) fn next(expected: Revision) -> Result<i64, DoorStoreError> {
    i64::try_from(expected.as_raw().saturating_add(1))
        .map_err(|_| DoorStoreError::Store(corrupt("revision")))
}

/// Refuse a save over its cap before the database is asked.
pub(crate) fn check_save(write: &SaveWrite<'_>) -> Result<(), DoorStoreError> {
    if write.bytes.len() > MAX_SAVE_BYTES {
        return Err(DoorStoreError::TooLarge {
            cap: MAX_SAVE_BYTES,
        });
    }
    Ok(())
}

/// Refuse a world over its cap before the database is asked.
pub(crate) fn check_world(write: &WorldWrite<'_>) -> Result<(), DoorStoreError> {
    if write.bytes.len() > MAX_WORLD_BYTES {
        return Err(DoorStoreError::TooLarge {
            cap: MAX_WORLD_BYTES,
        });
    }
    Ok(())
}

/// One `door_save` row.
pub(crate) struct SaveRow {
    pub(crate) season: i64,
    pub(crate) version: i64,
    pub(crate) revision: i64,
    pub(crate) data: Vec<u8>,
    pub(crate) updated_at: i64,
}

impl SaveRow {
    pub(crate) fn into_save(self) -> Result<StoredSave, StoreError> {
        Ok(StoredSave {
            season: small(self.season, "season")?,
            version: small(self.version, "version")?,
            revision: revision(self.revision)?,
            bytes: self.data,
            updated_at: UnixSeconds::from_raw(self.updated_at),
        })
    }
}

/// One `door_state` row's world columns. A revision of zero means no world was ever written.
pub(crate) struct WorldRow {
    pub(crate) world_revision: i64,
    pub(crate) world_version: i64,
    pub(crate) world: Vec<u8>,
    pub(crate) last_tick_day: Option<i64>,
}

impl WorldRow {
    pub(crate) fn into_world(self) -> Result<Option<StoredWorld>, StoreError> {
        if self.world_revision == 0 {
            return Ok(None);
        }
        Ok(Some(StoredWorld {
            version: small(self.world_version, "world version")?,
            revision: revision(self.world_revision)?,
            bytes: self.world,
            last_tick: self.last_tick_day.map(Day::from_raw),
        }))
    }
}
