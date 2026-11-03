//! Rooms as the store holds them, and the bounds that keep a room list small.

use std::fmt;

use crate::access::{ResourceId, ResourceKind, ResourceRef, Slug};
use crate::store::UnixSeconds;

/// Most open rooms at once. A room is a resource with its own policy, so the list stays short
/// enough to read on one screen and to hold in memory.
pub const MAX_ROOMS: usize = 32;

/// Store-assigned identifier of a room. It is also the room's resource id in the access
/// tables (`ResourceKind::Room`), so a grant "on room 3" and the room row share one number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RoomId(i64);

impl RoomId {
    /// Wrap a raw id. Call this only where a row is mapped (the store).
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// The raw id, for a query.
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }

    /// The access resource this room is. `None` only for a non-positive id, which the store
    /// never assigns.
    #[must_use]
    pub const fn resource(self) -> Option<ResourceRef> {
        match ResourceId::new(self.0) {
            Some(id) => Some(ResourceRef {
                kind: ResourceKind::Room,
                id,
            }),
            None => None,
        }
    }

    /// The room an access resource id names.
    #[must_use]
    pub const fn from_resource(id: ResourceId) -> Self {
        Self(id.as_raw())
    }
}

impl fmt::Display for RoomId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An open room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    /// Store-assigned id.
    pub id: RoomId,
    /// The room's name: 1 to 32 of `a-z`, `0-9`, `-`. Chosen by staff, so it is chrome-safe by
    /// construction (the type admits nothing else).
    pub name: Slug,
    /// When staff opened it (or the migration seeded it).
    pub created_at: UnixSeconds,
}
