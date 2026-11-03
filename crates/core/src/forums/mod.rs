//! Forums: boards, threads, posts and full-text search (M3-E, D83).
//!
//! * [`ForumStore`] is persistence only (both backends, one shared test suite). Every rule is
//!   here, and a front end calls the functions in [`service`] with the session's [`Snapshot`],
//!   never the store directly.
//! * **Boards are access resources** (`ResourceKind::Forum`). A list asks
//!   [`Snapshot::reach`](crate::access::Snapshot::reach); by-id access answers
//!   [`ForumError::NotFound`] the same for a board that does not exist and one the reader may
//!   not see, so the answer reveals nothing. A **restricted** board is not reached through a
//!   global holding of `forum.*`; it needs a grant scoped to that board (an allow entry or a
//!   scoped role), or `forum.manage` (D83). See [`rules`].
//! * **Search** ([`search`]) is SQLite FTS5 or Postgres full-text behind one trait method. The
//!   query is parsed into plain word terms in core ([`SearchQuery`]), so no backend ever sees
//!   the member's FTS syntax, and the board ids the reader may read are part of the query
//!   itself: a hit, a title or a count from another board is never fetched.
//! * **Text.** Subjects, board names and descriptions go through the shared single-line
//!   sanitizer and bodies through [`Body`](crate::drafts::Body) (the shared multi-line one);
//!   everything read back is cleaned again.
//! * **Limits.** `posts_per_hour` is checked after the permission and counted from the post rows
//!   inside the write ([`PostCap`]), so two posts at once cannot both pass.
//!
//! Nothing here calls a post, a board or a search "private": the server reads all of it (D3,
//! D78).

mod ids;
pub mod rules;
pub mod search;
pub mod service;
mod store;
mod text;

pub use ids::{BoardId, PostId, ThreadId};
pub use search::{Hit, MAX_QUERY_CHARS, MAX_TERM_CHARS, MAX_TERMS, SearchQuery};
pub use service::{
    BoardSummary, ForumError, Gone, PAGE_SIZE, PostView, Posted, SearchPage, ThreadPage,
    ThreadSummary, ThreadView, allow_member, boards, create_board, post_reply, post_thread,
    remove_post, search, thread_post, threads,
};
pub use store::{
    BoardRow, ForumStore, NewBoard, NewPost, NewThread, PostCap, PostRow, PostWrite, RawHit,
    Removed, ThreadRow,
};
pub use text::{
    BODY_MAX_CHARS, BODY_MAX_LINES, DESCRIPTION_MAX_CHARS, NAME_MAX_CHARS, SUBJECT_MAX_CHARS,
    TextError, body_limits, prepare_description, prepare_name, prepare_subject,
};

#[cfg(test)]
mod tests;
