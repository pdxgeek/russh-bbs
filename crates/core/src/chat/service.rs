//! Room listing and staff room management. Every function takes the session's snapshot and
//! checks the permission itself (D33, D55), so a front end cannot skip the check.
//!
//! **Rooms are access resources.** The list a member sees is exactly the rooms they hold
//! `chat.join` on ([`Snapshot::reach`]). A room they cannot join is not listed, and asking for
//! it by id gets [`RoomError::NotFound`], the same answer as for an id that never existed.

use super::room::{MAX_ROOMS, Room, RoomId};
use super::store::{CreateRoomError, RoomStore};
use crate::access::{Granted, Permission, ResourceKind, Slug, Snapshot};
use crate::store::{AccountId, StoreError};

/// Why a room action did not happen.
#[derive(Debug, thiserror::Error)]
pub enum RoomError {
    /// No such room, or the session may not reach it. Deliberately one answer.
    #[error("no such room")]
    NotFound,
    /// The session does not hold the permission for a staff action.
    #[error("not permitted")]
    Denied,
    /// An open room already has the name.
    #[error("a room with that name is open")]
    NameTaken,
    /// The room cap is reached.
    #[error("too many rooms")]
    TooMany,
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// The open rooms the session may join, by name. Empty for a session with no account.
pub async fn joinable_rooms<S: RoomStore>(
    store: &S,
    access: &Snapshot,
) -> Result<Vec<Room>, StoreError> {
    let reach = access.reach(Permission::ChatJoin, ResourceKind::Room);
    let mut rooms = store.open_rooms().await?;
    rooms.retain(|room| {
        room.id
            .resource()
            .is_some_and(|resource| reach.contains(resource.id))
    });
    Ok(rooms)
}

/// Check that the session may join `id` and that the room is open. The proof it returns is
/// what the hub's join takes.
pub async fn require_join<S: RoomStore>(
    store: &S,
    access: &Snapshot,
    id: RoomId,
) -> Result<(Granted, Room), RoomError> {
    let resource = id.resource().ok_or(RoomError::NotFound)?;
    let granted = access
        .require(Permission::ChatJoin, Some(resource))
        .map_err(|_| RoomError::NotFound)?;
    let room = store
        .open_rooms()
        .await?
        .into_iter()
        .find(|room| room.id == id)
        .ok_or(RoomError::NotFound)?;
    Ok((granted, room))
}

/// Open a room (`chat.manage_rooms`, global). Staff only; audited by the store.
pub async fn create_room<S: RoomStore>(
    store: &S,
    access: &Snapshot,
    name: &Slug,
) -> Result<Room, RoomError> {
    let (granted, actor) = manage(access)?;
    debug_assert_eq!(granted.permission(), Permission::ChatManageRooms);
    store
        .create_room(name, actor, MAX_ROOMS)
        .await
        .map_err(|error| match error {
            CreateRoomError::NameTaken => RoomError::NameTaken,
            CreateRoomError::TooMany => RoomError::TooMany,
            CreateRoomError::Store(error) => RoomError::Store(error),
        })
}

/// Close a room (`chat.manage_rooms`, global). `Ok(false)` when it was not open. The caller
/// then tells the hub to put the members out.
pub async fn close_room<S: RoomStore>(
    store: &S,
    access: &Snapshot,
    id: RoomId,
) -> Result<bool, RoomError> {
    let (_granted, actor) = manage(access)?;
    Ok(store.close_room(id, actor).await?)
}

fn manage(access: &Snapshot) -> Result<(Granted, AccountId), RoomError> {
    let granted = access
        .require(Permission::ChatManageRooms, None)
        .map_err(|_| RoomError::Denied)?;
    let actor = granted.account().ok_or(RoomError::Denied)?;
    Ok((granted, actor))
}
