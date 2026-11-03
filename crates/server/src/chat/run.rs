//! Carrying out the chat screen's requests against the hub and the room store, with the
//! checks core requires. Every request is checked against the snapshot loaded for this
//! action (D55): a permission taken away since the last key applies now.

use std::time::Duration;

use russh_bbs_core::access::{Permission, Snapshot};
use russh_bbs_core::chat::{
    ChatNotice, ChatRequest, ChatResponse, JoinError, RoomError, RoomStore, SayError, WhisperError,
    close_room, create_room, joinable_rooms, require_join,
};
use tokio::time::timeout;
use tracing::warn;

use super::ChatSession;

/// How long one room-store call may take. A slow store must not hang the screen; the screen
/// then says it did not work and changes nothing.
const STORE_TIMEOUT: Duration = Duration::from_secs(2);

impl ChatSession {
    /// Run `request` for a session holding `access`.
    pub(crate) async fn execute<S: RoomStore>(
        &self,
        store: &S,
        access: &Snapshot,
        request: ChatRequest,
    ) -> ChatResponse {
        match request {
            ChatRequest::List => self.rooms(store, access, None).await,
            ChatRequest::Join(id) => {
                let joined = timeout(STORE_TIMEOUT, require_join(store, access, id)).await;
                // A room the member may not reach, a closed room and one that never existed
                // are the same answer; a store failure is a failure.
                let (proof, room) = match joined {
                    Ok(Ok(pair)) => pair,
                    Ok(Err(RoomError::NotFound)) => {
                        return self.rooms(store, access, Some(ChatNotice::NotFound)).await;
                    }
                    _ => return ChatResponse::Refused(ChatNotice::Failed),
                };
                match self.join(&proof) {
                    Ok(entered) => ChatResponse::Joined { room, entered },
                    Err(JoinError::Full) => ChatResponse::Refused(ChatNotice::RoomFull),
                    Err(JoinError::Busy) => ChatResponse::Refused(ChatNotice::ChatFull),
                    Err(JoinError::NotFound) => ChatResponse::Refused(ChatNotice::NotFound),
                }
            }
            ChatRequest::Leave => {
                self.leave();
                self.rooms(store, access, None).await
            }
            ChatRequest::Who => match self.in_room(store, access, Permission::ChatJoin).await {
                Ok(_) => ChatResponse::Roster(self.roster()),
                Err(bounce) => bounce,
            },
            ChatRequest::Say(text) => {
                match self.in_room(store, access, Permission::ChatSpeak).await {
                    Ok(proof) => match self.say(&proof, text) {
                        Ok(()) => ChatResponse::Done,
                        Err(SayError::RateLimited { .. }) => {
                            ChatResponse::Refused(ChatNotice::SlowDown)
                        }
                        Err(SayError::NotInRoom) => ChatResponse::Refused(ChatNotice::NotFound),
                    },
                    Err(bounce) => bounce,
                }
            }
            ChatRequest::Whisper { to, text } => {
                match self.in_room(store, access, Permission::ChatWhisper).await {
                    Ok(proof) => match self.whisper(&proof, &to, text) {
                        Ok(()) => ChatResponse::Done,
                        Err(WhisperError::NotHere) => ChatResponse::Refused(ChatNotice::NotHere),
                        Err(WhisperError::RateLimited { .. }) => {
                            ChatResponse::Refused(ChatNotice::SlowDown)
                        }
                        Err(WhisperError::NotInRoom) => ChatResponse::Refused(ChatNotice::NotFound),
                    },
                    Err(bounce) => bounce,
                }
            }
            ChatRequest::Create(name) => {
                let made = timeout(STORE_TIMEOUT, create_room(store, access, &name)).await;
                let notice = match made {
                    Ok(Ok(_)) => ChatNotice::Created,
                    Ok(Err(RoomError::Denied)) => ChatNotice::NotPermitted,
                    Ok(Err(RoomError::NameTaken)) => ChatNotice::NameTaken,
                    Ok(Err(RoomError::TooMany)) => ChatNotice::TooManyRooms,
                    Ok(Err(RoomError::NotFound)) => ChatNotice::NotFound,
                    Ok(Err(RoomError::Store(error))) => {
                        warn!(%error, "opening a chat room failed in the store");
                        ChatNotice::Failed
                    }
                    Err(_) => {
                        warn!("opening a chat room timed out");
                        ChatNotice::Failed
                    }
                };
                self.rooms(store, access, Some(notice)).await
            }
            ChatRequest::Close(id) => {
                let closed = timeout(STORE_TIMEOUT, close_room(store, access, id)).await;
                let notice = match closed {
                    Ok(Ok(true)) => {
                        self.hub.room_closed(id);
                        ChatNotice::Closed
                    }
                    Ok(Ok(false)) => ChatNotice::NotFound,
                    Ok(Err(RoomError::Denied)) => ChatNotice::NotPermitted,
                    Ok(Err(RoomError::Store(error))) => {
                        warn!(%error, "closing a chat room failed in the store");
                        ChatNotice::Failed
                    }
                    Ok(Err(_)) => ChatNotice::NotFound,
                    Err(_) => {
                        warn!("closing a chat room timed out");
                        ChatNotice::Failed
                    }
                };
                self.rooms(store, access, Some(notice)).await
            }
        }
    }

    /// The joinable rooms, or a failure notice.
    async fn rooms<S: RoomStore>(
        &self,
        store: &S,
        access: &Snapshot,
        notice: Option<ChatNotice>,
    ) -> ChatResponse {
        match timeout(STORE_TIMEOUT, joinable_rooms(store, access)).await {
            Ok(Ok(rooms)) => ChatResponse::Rooms { rooms, notice },
            Ok(Err(error)) => {
                warn!(%error, "listing chat rooms failed in the store");
                ChatResponse::Refused(ChatNotice::Failed)
            }
            Err(_) => {
                warn!("listing chat rooms timed out");
                ChatResponse::Refused(ChatNotice::Failed)
            }
        }
    }

    /// The proof for an action in the room the session is in. Every in-room action re-checks
    /// `chat.join` on the room, so a member who lost the room is put out of it: they get the
    /// room list back instead of the action.
    async fn in_room<S: RoomStore>(
        &self,
        store: &S,
        access: &Snapshot,
        permission: Permission,
    ) -> Result<russh_bbs_core::access::Granted, ChatResponse> {
        let held = self.room().and_then(|room| {
            let resource = room.resource()?;
            access.require(Permission::ChatJoin, Some(resource)).ok()?;
            Some(resource)
        });
        let Some(resource) = held else {
            self.leave();
            return Err(self.rooms(store, access, Some(ChatNotice::NotFound)).await);
        };
        access
            .require(permission, Some(resource))
            .map_err(|_| ChatResponse::Refused(ChatNotice::NotPermitted))
    }
}
