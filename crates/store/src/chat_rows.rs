//! Backend-independent mapping for the `chat_room` table, written once (D24).

use russh_bbs_core::access::Slug;
use russh_bbs_core::chat::{Room, RoomId};
use russh_bbs_core::store::{StoreError, UnixSeconds};

/// A `chat_room` row as the store hands it over. A name the code could not have written is
/// corrupt data, not a room.
pub(crate) fn room(id: i64, name: &str, created_at: i64) -> Result<Room, StoreError> {
    Ok(Room {
        id: RoomId::from_raw(id),
        name: Slug::parse(name)
            .map_err(|_| StoreError::Corrupt("a stored chat room name is not valid".into()))?,
        created_at: UnixSeconds::from_raw(created_at),
    })
}

/// The cap as a number the database can compare with.
pub(crate) fn cap(max_rooms: usize) -> i64 {
    i64::try_from(max_rooms).unwrap_or(i64::MAX)
}
