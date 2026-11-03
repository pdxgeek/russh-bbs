//! RSS and Atom feeds as read-only boards, and RSS out (M3-G, D89).
//!
//! * **Model.** A feed is one source address bound to one board ([`FeedRow`]). The board is an
//!   ordinary forum board (an access resource, D83) with its `feed` flag set; the forum rules
//!   refuse posting and replying there ([`crate::forums::rules::can`] and
//!   [`crate::forums::ForumError::ReadOnly`]), so it is a rule in core, not a hidden key. Items
//!   become threads with no author account.
//! * **Untrusted input.** The address is validated ([`FeedUrl`]); the fetcher is a trait
//!   ([`Fetch`]) the server implements behind its SSRF guard; the document is checked before it is
//!   parsed ([`parse`]); every field passes through the shared sanitizer; HTML is reduced to text
//!   with the links listed ([`html`]); bodies are capped; duplicates are detected by id and by
//!   content hash ([`NewItem`]).
//! * **Polling.** [`poll_due`] reads the feeds that are due; the server registers it as one job
//!   with the scheduler (D79). A failing feed backs off ([`is_due`]); outcomes are a closed set
//!   ([`Outcome`]) and the address is never logged.
//! * **RSS out.** [`out`] renders a public board as RSS 2.0. A board is public only when the
//!   sysop marked it and it is not restricted; readability is decided on an anonymous snapshot by
//!   the same rule every board uses.
//!
//! Nothing here is private: the server reads every item (D3, D78).

mod ids;
mod item;
mod store;
mod url;

pub mod html;
pub mod out;
pub mod parse;
pub mod poll;
pub mod service;

pub use ids::FeedId;
pub use item::NewItem;
pub use parse::{ParseError, Parsed, parse};
pub use poll::{
    Fetch, FetchError, KEEP_THREADS, MAX_NEW_PER_FETCH, PollReport, is_due, poll_due, poll_feed,
};
pub use service::{
    Added, DEFAULT_INTERVAL_SECS, FeedError, MAX_FEEDS, MAX_INTERVAL_SECS, MIN_INTERVAL_SECS,
    add_feed, feeds, publish_board, remove_feed,
};
pub use store::{AddFeed, FeedRow, FeedStore, Ingested, NewFeed, Outcome};
pub use url::{FeedUrl, MAX_URL_BYTES, UrlError};

#[cfg(test)]
mod tests;
