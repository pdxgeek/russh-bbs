//! Public chat rooms, presence and whispers (M3-F, D84).
//!
//! The rules live here; the server crate wraps [`Chat`] in a lock, wakes the sessions it names
//! and draws [`ChatScreen`].
//!
//! * [`Room`] / [`RoomStore`]: room *definitions* only, in the store. Rooms are access
//!   resources ([`service`]): the list is `reach(chat.join, room)`, an unreachable room is
//!   "no such room", and staff open and close rooms with `chat.manage_rooms` (audited).
//! * [`Chat`]: the bounded in-memory hub state (sessions, rooms, scrollback, queues, send
//!   limit). **Chat lines and whispers are never written to the store, the audit log or the
//!   logs (D78).** No staff screen, permission or export shows whispers. They are "whispers",
//!   never "private".
//! * [`ChatScreen`]: keys in, [`ChatRequest`]s out.
//!
//! What this is not: it does not call anything private, and no screen carries a privacy
//! notice (D78). The server can read everything said; the product's one explanation lives in
//! the welcome message and help screen.

mod event;
mod room;
mod screen;
mod service;
mod state;
mod store;
mod text;

pub use event::{Event, Line};
pub use room::{MAX_ROOMS, Room, RoomId};
pub use screen::{
    ChatEffect, ChatKey, ChatNotice, ChatPage, ChatRequest, ChatResponse, ChatScreen,
    INPUT_MAX_CHARS, ListPage, RoomPage, Row, TRANSCRIPT_ROWS, strings,
};
pub use service::{RoomError, close_room, create_room, joinable_rooms, require_join};
pub use state::{Chat, ChatLimits, Entered, JoinError, SayError, SessionId, Touched, WhisperError};
pub use store::{CreateRoomError, RoomStore};
pub use text::{MAX_LINE_CHARS, clean_line, same_handle};

#[cfg(test)]
mod tests;
