//! What reaches a session from a room. Plain data; nothing here is written anywhere.

use crate::sanitize::{Sanitized, Username};

/// One public line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Who said it (a validated handle).
    pub from: Username,
    /// What they said (sanitized and capped).
    pub text: Sanitized,
}

/// Something for one session to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A line said in the room (including the session's own, so everyone sees one order).
    Line(Line),
    /// Someone came into the room.
    Joined(Username),
    /// Someone left, or their connection ended.
    Left(Username),
    /// A whisper to this session.
    Whisper {
        /// Who sent it.
        from: Username,
        /// What it says.
        text: Sanitized,
    },
    /// The echo of a whisper this session sent, so the sender sees it went.
    WhisperSent {
        /// Who it went to.
        to: Username,
        /// What it said.
        text: Sanitized,
    },
    /// This many events were dropped because the session was not keeping up.
    Missed(u32),
    /// Staff closed the room; the session is no longer in it.
    RoomClosed,
}
