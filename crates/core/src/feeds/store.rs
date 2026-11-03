//! The storage trait for feeds: persistence only. The rules (who may add a feed, what an address
//! may be, what an item becomes, when a feed is due) are in [`super::service`], [`super::url`],
//! [`super::parse`] and [`super::poll`]; the backends only read and write rows.

use std::future::Future;

use crate::forums::BoardId;
use crate::sanitize::Sanitized;
use crate::store::{AccountId, StoreError, UnixSeconds};

use super::ids::FeedId;
use super::item::NewItem;
use super::url::FeedUrl;

/// How the last attempt to read a feed ended. A closed set: a remote server's words are never
/// stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::EnumString)]
pub enum Outcome {
    /// Fetched, parsed and stored.
    #[strum(serialize = "ok")]
    Ok,
    /// The SSRF guard or the address rules refused it.
    #[strum(serialize = "refused")]
    Refused,
    /// The connection or the server failed.
    #[strum(serialize = "failed")]
    Failed,
    /// The answer was not a usable feed.
    #[strum(serialize = "invalid")]
    Invalid,
    /// The answer was larger than the cap.
    #[strum(serialize = "too-large")]
    TooLarge,
    /// The fetch ran out of time.
    #[strum(serialize = "timed-out")]
    TimedOut,
}

impl Outcome {
    /// The stored name.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.into()
    }

    /// Parse a stored name.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        text.parse().ok()
    }
}

/// A feed as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedRow {
    /// The feed.
    pub id: FeedId,
    /// The read-only board its items appear on.
    pub board: BoardId,
    /// The board's name.
    pub board_name: String,
    /// The address (a secret for some feeds: it is never logged, only shown to staff).
    pub url: String,
    /// Whether plain `http` was allowed by the sysop for this feed.
    pub allow_http: bool,
    /// Seconds between fetches when it is healthy.
    pub interval_secs: u32,
    /// When it was last tried.
    pub last_attempt_at: Option<UnixSeconds>,
    /// When it last worked.
    pub last_success_at: Option<UnixSeconds>,
    /// How the last try ended.
    pub last_outcome: Option<Outcome>,
    /// Failed tries in a row.
    pub failures: u32,
    /// Whether the board may be read as RSS by outside readers.
    pub public: bool,
}

/// A feed to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFeed {
    /// The address.
    pub url: FeedUrl,
    /// The board's name.
    pub name: Sanitized,
    /// The board's description.
    pub description: Sanitized,
    /// Whether plain `http` is allowed for it.
    pub allow_http: bool,
    /// Seconds between fetches.
    pub interval_secs: u32,
}

/// What adding a feed did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddFeed {
    /// Added: the feed and its new board.
    Added(FeedId, BoardId),
    /// A board with that name exists.
    NameTaken,
    /// That address is already subscribed.
    UrlTaken,
    /// The board is at its limit of feeds.
    TooMany,
}

/// What storing a fetch's items did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ingested {
    /// Items that became threads.
    pub added: u32,
    /// Items already seen (same id or same content).
    pub skipped: u32,
}

/// Feeds, their items, and which boards are public.
pub trait FeedStore: Send + Sync {
    /// Every feed, oldest first.
    fn feeds(&self) -> impl Future<Output = Result<Vec<FeedRow>, StoreError>> + Send;

    /// The feed on `board`, or `None`.
    fn feed_of_board(
        &self,
        board: BoardId,
    ) -> impl Future<Output = Result<Option<FeedRow>, StoreError>> + Send;

    /// Add a feed and its read-only board in one transaction, unless the name or address is
    /// taken or `max_feeds` exist. Audited in the same transaction (`feed.added`, by `actor`;
    /// the log never holds the address).
    fn add_feed(
        &self,
        new: &NewFeed,
        actor: AccountId,
        now: UnixSeconds,
        max_feeds: u32,
    ) -> impl Future<Output = Result<AddFeed, StoreError>> + Send;

    /// Remove a feed and its board with every thread on it. Returns the board, or `None` when
    /// there is no such feed. Audited in the same transaction (`feed.removed`, by `actor`).
    fn remove_feed(
        &self,
        feed: FeedId,
        actor: AccountId,
    ) -> impl Future<Output = Result<Option<BoardId>, StoreError>> + Send;

    /// Store the items of a successful fetch and record the success, in one transaction. An item
    /// whose key or content hash was already seen is skipped; the feed's board keeps its newest
    /// `keep_threads` threads. `None` when the feed is gone (removed while it was being read).
    fn ingest(
        &self,
        feed: FeedId,
        items: &[NewItem],
        now: UnixSeconds,
        keep_threads: u32,
    ) -> impl Future<Output = Result<Option<Ingested>, StoreError>> + Send;

    /// Record a failed attempt: the time, the closed outcome, one more failure in a row.
    fn record_failure(
        &self,
        feed: FeedId,
        outcome: Outcome,
        now: UnixSeconds,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Mark a board as readable (or not) as RSS by outside readers. `false` when there is no
    /// such board, or it is restricted. A change is audited in the same transaction
    /// (`board.published` / `board.unpublished`); setting what already holds writes nothing.
    fn set_public(
        &self,
        board: BoardId,
        public: bool,
        actor: AccountId,
        now: UnixSeconds,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// The boards marked public.
    fn public_boards(&self) -> impl Future<Output = Result<Vec<BoardId>, StoreError>> + Send;
}

/// The empty directory has no feeds: reads find nothing, writes are refused.
impl FeedStore for crate::access::NoMembers {
    fn feeds(&self) -> impl Future<Output = Result<Vec<FeedRow>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn feed_of_board(
        &self,
        _board: BoardId,
    ) -> impl Future<Output = Result<Option<FeedRow>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn add_feed(
        &self,
        _new: &NewFeed,
        _actor: AccountId,
        _now: UnixSeconds,
        _max_feeds: u32,
    ) -> impl Future<Output = Result<AddFeed, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn remove_feed(
        &self,
        _feed: FeedId,
        _actor: AccountId,
    ) -> impl Future<Output = Result<Option<BoardId>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn ingest(
        &self,
        _feed: FeedId,
        _items: &[NewItem],
        _now: UnixSeconds,
        _keep_threads: u32,
    ) -> impl Future<Output = Result<Option<Ingested>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn record_failure(
        &self,
        _feed: FeedId,
        _outcome: Outcome,
        _now: UnixSeconds,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Ok(()))
    }

    fn set_public(
        &self,
        _board: BoardId,
        _public: bool,
        _actor: AccountId,
        _now: UnixSeconds,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(false))
    }

    fn public_boards(&self) -> impl Future<Output = Result<Vec<BoardId>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }
}
