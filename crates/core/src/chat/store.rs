//! The storage trait for room definitions. Nothing said in a room is ever stored (D78): this
//! holds the names of rooms and nothing else.

use std::future::Future;

use super::room::{Room, RoomId};
use crate::access::Slug;
use crate::store::{AccountId, StoreError};

/// Why a room could not be made.
#[derive(Debug, thiserror::Error)]
pub enum CreateRoomError {
    /// An open room already has this name.
    #[error("a room with that name is open")]
    NameTaken,
    /// [`super::MAX_ROOMS`] rooms are open.
    #[error("the board already has the most rooms it allows")]
    TooMany,
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Room definitions.
pub trait RoomStore: Send + Sync {
    /// Open a room named `name` for `actor`, atomically with its audit row. The count against
    /// `max_rooms` and the insert are one statement, so two creations cannot pass the cap.
    fn create_room(
        &self,
        name: &Slug,
        actor: AccountId,
        max_rooms: usize,
    ) -> impl Future<Output = Result<Room, CreateRoomError>> + Send;

    /// Close a room, atomically with its audit row. `true` when it was open. The id is never
    /// reused and grants that name it stay meaningless rather than pointing at a new room.
    fn close_room(
        &self,
        id: RoomId,
        actor: AccountId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// The open rooms, by name.
    fn open_rooms(&self) -> impl Future<Output = Result<Vec<Room>, StoreError>> + Send;
}

/// The empty directory has no rooms and cannot make any.
impl RoomStore for crate::access::NoMembers {
    fn create_room(
        &self,
        _name: &Slug,
        _actor: AccountId,
        _max_rooms: usize,
    ) -> impl Future<Output = Result<Room, CreateRoomError>> + Send {
        std::future::ready(Err(CreateRoomError::Store(StoreError::AccountNotFound)))
    }

    fn close_room(
        &self,
        _id: RoomId,
        _actor: AccountId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(false))
    }

    fn open_rooms(&self) -> impl Future<Output = Result<Vec<Room>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }
}
