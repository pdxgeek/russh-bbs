//! What staff can do with feeds. Every function takes the session's [`Snapshot`] and checks it
//! first: `feeds.manage`, held globally (the sysop and co-sysop roles; a feed makes the server
//! fetch an address, so it is not a per-board permission).

use crate::access::{Permission, Snapshot};
use crate::forums::{BoardId, TextError, prepare_description, prepare_name};
use crate::sanitize::SingleLine;
use crate::store::{AccountId, StoreError, UnixSeconds};

use super::ids::FeedId;
use super::store::{AddFeed, FeedRow, FeedStore, NewFeed};
use super::url::{FeedUrl, UrlError};

/// Most feeds a board may subscribe to.
pub const MAX_FEEDS: u32 = 50;
/// Seconds between fetches of a healthy feed unless the sysop chose otherwise.
pub const DEFAULT_INTERVAL_SECS: u32 = 3_600;
/// Shortest interval a feed may have (also the database's `CHECK`).
pub const MIN_INTERVAL_SECS: u32 = 900;
/// Longest interval a feed may have (also the database's `CHECK`).
pub const MAX_INTERVAL_SECS: u32 = 604_800;

/// Why a feed action did not happen.
#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    /// The session may not manage feeds.
    #[error("you may not do that")]
    NotPermitted,
    /// The session is not a signed-in account.
    #[error("not signed in")]
    NotSignedIn,
    /// The address was refused.
    #[error(transparent)]
    Url(#[from] UrlError),
    /// The name was refused.
    #[error(transparent)]
    Text(#[from] TextError),
    /// The interval is outside the allowed range.
    #[error("the interval must be between 15 minutes and 7 days")]
    Interval,
    /// A board with that name exists.
    #[error("a board with that name already exists")]
    NameTaken,
    /// That address is already subscribed.
    #[error("that address is already subscribed")]
    UrlTaken,
    /// The limit of feeds is reached.
    #[error("the limit of {MAX_FEEDS} feeds is reached")]
    TooMany,
    /// No such feed or board.
    #[error("not found")]
    NotFound,
    /// A restricted board cannot be published.
    #[error("a restricted board cannot be public")]
    Restricted,
    /// Storage failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

fn manager(snapshot: &Snapshot) -> Result<AccountId, FeedError> {
    let account = snapshot.account().ok_or(FeedError::NotSignedIn)?;
    if snapshot.resolve(Permission::FeedsManage, None).is_granted() {
        Ok(account)
    } else {
        Err(FeedError::NotPermitted)
    }
}

/// What the screen shows after adding a feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Added {
    /// The feed.
    pub feed: FeedId,
    /// Its board.
    pub board: BoardId,
}

/// Subscribe the board to `url`, as a new read-only board called `name`. Nothing is fetched
/// here: the scheduler's job reads it on its next run, through the SSRF-guarded fetcher.
///
/// # Errors
/// [`FeedError`]: not permitted, a refused address or name, a taken name or address, the limit,
/// or storage.
pub async fn add_feed<S: FeedStore>(
    store: &S,
    snapshot: &Snapshot,
    now: UnixSeconds,
    url: &str,
    name: &str,
    allow_http: bool,
    interval_secs: Option<u32>,
) -> Result<Added, FeedError> {
    let actor = manager(snapshot)?;
    let url = FeedUrl::parse(url, allow_http)?;
    let name = prepare_name(name)?;
    let interval_secs = interval_secs.unwrap_or(DEFAULT_INTERVAL_SECS);
    if !(MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS).contains(&interval_secs) {
        return Err(FeedError::Interval);
    }
    let host = url.url().host_str().unwrap_or_default();
    let description = prepare_description(
        SingleLine::new(150)
            .sanitize(&format!("Feed from {host}"))
            .as_str(),
    )?;
    let new = NewFeed {
        url,
        name,
        description,
        allow_http,
        interval_secs,
    };
    match store.add_feed(&new, actor, now, MAX_FEEDS).await? {
        AddFeed::Added(feed, board) => Ok(Added { feed, board }),
        AddFeed::NameTaken => Err(FeedError::NameTaken),
        AddFeed::UrlTaken => Err(FeedError::UrlTaken),
        AddFeed::TooMany => Err(FeedError::TooMany),
    }
}

/// Remove a feed with its board and every thread on it.
///
/// # Errors
/// [`FeedError`]: not permitted, not found, or storage.
pub async fn remove_feed<S: FeedStore>(
    store: &S,
    snapshot: &Snapshot,
    feed: FeedId,
) -> Result<BoardId, FeedError> {
    let actor = manager(snapshot)?;
    store
        .remove_feed(feed, actor)
        .await?
        .ok_or(FeedError::NotFound)
}

/// Every feed, for the sysop's list.
///
/// # Errors
/// [`FeedError`]: not permitted, or storage.
pub async fn feeds<S: FeedStore>(
    store: &S,
    snapshot: &Snapshot,
) -> Result<Vec<FeedRow>, FeedError> {
    manager(snapshot)?;
    Ok(store.feeds().await?)
}

/// Let outside readers read `board` as RSS (or stop). A restricted board cannot be public.
///
/// # Errors
/// [`FeedError`]: not permitted, not found, restricted, or storage.
pub async fn publish_board<S: FeedStore + crate::forums::ForumStore>(
    store: &S,
    snapshot: &Snapshot,
    now: UnixSeconds,
    board: BoardId,
    public: bool,
) -> Result<(), FeedError> {
    let actor = manager(snapshot)?;
    let row = store.board(board).await?.ok_or(FeedError::NotFound)?;
    if public && row.restricted {
        return Err(FeedError::Restricted);
    }
    if store.set_public(board, public, actor, now).await? {
        Ok(())
    } else {
        Err(FeedError::NotFound)
    }
}
