//! The waiting room: what a key the board does not know sees, and the limits that keep
//! strangers from flooding it.
//!
//! The rules live here and the SSH front end only draws them (AGENTS.md: business logic in
//! core). The pieces:
//!
//! * [`screens`]: which pages exist in each registration mode (D2, D23).
//! * [`WaitingRoom`]: the per-session state machine and the [`View`] it produces.
//! * [`WaitingRoomSource`]: where the mode and the sysop's banner and rules come from.
//! * [`limits`]: per-source rate limiting with bounded memory and an injectable clock.
//! * [`strings`]: every fixed string, kept out of rendering code so it can be themed later.
//!
//! **The no-leak rule.** Nothing here can name a member, a board or a statistic: the room is
//! built from a [`RoomConfig`] and never sees a store. The only text that is not fixed chrome is
//! the sysop's banner and rules and what the visitor types, and all of it is sanitized.

pub mod limits;
mod room;
pub mod screens;
mod source;
pub mod strings;

pub use limits::{
    Clock, KeyedLimiter, ManualClock, RatePolicy, SystemClock, Verdict, client_network,
};
pub use room::{
    BANNER_MAX_CHARS, CODE_MAX_CHARS, DEFAULT_MAX_SUBMISSIONS, FieldView, LINE_MAX_CHARS,
    MenuItemView, Notice, NoticeKind, RULES_MAX_CHARS, RoomEffect, RoomInput, View, WaitingRoom,
};
pub use screens::{MenuEntry, Page};
pub use source::{RoomConfig, WaitingRoomSource};

#[cfg(test)]
mod tests;
