//! The small classic community features (M3-J, D86): one-liners, a graffiti wall, last callers
//! and polls. Each is a bounded list of short, sanitized text.
//!
//! * [`types`]: what an entry, a line of text and a poll are, and every bound in one place.
//! * [`CommunityStore`]: persistence only (both backends implement it). The rules that must be
//!   atomic live in the store's own transaction and are tested by the shared suite: the
//!   per-member rate limit, the cap that rolls old lines off, **one vote per member** (a unique
//!   key), who may close a poll, and the audit row of a staff action.
//! * [`execute`]: the rules a front end calls. It takes a request and the session's
//!   [`Snapshot`](crate::access::Snapshot) and calls `require` for the permission of the action,
//!   so a front end cannot skip a check. Text is sanitized with the shared sanitizer on the way
//!   in and again on the way out.
//! * [`CommunityScreen`]: the screens as a pure state machine that emits string-table keys and
//!   sanitized content, like the members screens.
//!
//! **Honesty.** Everything here is readable by every member who may see the feature and by the
//! sysop. Nothing is called private, secure or encrypted, and nothing shows a lock (D78). A poll
//! vote is stored with the voter's account (the store enforces one vote each), so the sysop can
//! tell who voted for what; results shown to members are counts only.

pub mod keys;
mod screen;
mod service;
mod store;
mod types;

pub use keys::TEXT_KEYS;
pub use screen::{CommunityScreen, EditorAsk, Section};
pub use service::{
    CallerRow, Entry, MAX_POLLS, Note, POLLS_PER_DAY, PollActions, PollDetail, PollRow, Request,
    Response, execute, record_sign_in,
};
pub use store::{
    AddLine, CloseOutcome, CommunityStore, CreatePoll, PollRules, RemoveOutcome, StoredCaller,
    StoredLine, StoredPoll, StoredPollRow, VoteOutcome, visibility_of,
};
pub use types::{
    CALLERS_SHOWN, DESCRIPTION_CHARS, DESCRIPTION_LINES, EntryId, LineKind, LineText, NewPoll,
    OPTION_MAX, OPTIONS_MAX, OPTIONS_MIN, POLL_PAGE, PollError, PollId, QUESTION_MAX,
    ResultsVisibility, TextError,
};

#[cfg(test)]
mod tests;
