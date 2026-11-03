//! Reading the feeds that are due. The fetcher is a trait ([`Fetch`]) so the rules here are tested
//! without a network, and the server's guarded fetcher is tested on its own.

use std::future::Future;

use tracing::{info, warn};

use crate::store::{StoreError, UnixSeconds};

use super::parse::parse;
use super::store::{FeedRow, FeedStore, Outcome};
use super::url::FeedUrl;

/// Most threads a feed's board keeps; older ones are removed as new items arrive.
pub const KEEP_THREADS: u32 = 200;
/// Most items stored from one fetch.
pub const MAX_NEW_PER_FETCH: usize = 50;
/// The longest a failing feed waits between tries, whatever its interval.
const MAX_BACKOFF_SECS: i64 = 604_800;

/// Why a fetch did not give a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// The guard refused the address (or a redirect's), or a rule on the URL did.
    #[error("refused")]
    Refused,
    /// The answer was larger than the cap.
    #[error("too large")]
    TooLarge,
    /// The fetch ran out of time.
    #[error("timed out")]
    TimedOut,
    /// The connection or the server failed.
    #[error("failed")]
    Failed,
}

impl FetchError {
    fn outcome(self) -> Outcome {
        match self {
            Self::Refused => Outcome::Refused,
            Self::TooLarge => Outcome::TooLarge,
            Self::TimedOut => Outcome::TimedOut,
            Self::Failed => Outcome::Failed,
        }
    }
}

/// Something that can fetch the body of a feed. The server implements it with the SSRF guard;
/// nothing in core opens a socket.
pub trait Fetch: Send + Sync {
    /// Fetch `url` (plain `http` only when `allow_http`). The body is at most the fetcher's cap.
    fn get(
        &self,
        url: &FeedUrl,
        allow_http: bool,
    ) -> impl Future<Output = Result<Vec<u8>, FetchError>> + Send;
}

/// What a run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PollReport {
    /// Feeds tried.
    pub tried: u32,
    /// Items stored.
    pub added: u32,
    /// Feeds whose try failed.
    pub failed: u32,
}

/// Whether `feed` is due at `now`. A feed that keeps failing waits twice as long each time, up
/// to 64 times its interval and never more than a week, so a dead address is not hammered.
#[must_use]
pub fn is_due(feed: &FeedRow, now: UnixSeconds) -> bool {
    let Some(last) = feed.last_attempt_at else {
        return true;
    };
    let factor = 1_i64 << feed.failures.min(6);
    let wait = i64::from(feed.interval_secs)
        .saturating_mul(factor)
        .min(MAX_BACKOFF_SECS.max(i64::from(feed.interval_secs)));
    now.as_raw() >= last.as_raw().saturating_add(wait)
}

/// Read one feed now: fetch, parse, store, and record how it went. Never logs the address.
///
/// # Errors
/// [`StoreError`] when storage fails (a bad feed is an [`Outcome`], not an error).
pub async fn poll_feed<S: FeedStore, F: Fetch>(
    store: &S,
    fetch: &F,
    feed: &FeedRow,
    now: UnixSeconds,
) -> Result<(Outcome, u32), StoreError> {
    let fail = |outcome: Outcome| async move {
        store.record_failure(feed.id, outcome, now).await?;
        warn!(feed = %feed.id, outcome = outcome.name(), "feed not read");
        Ok::<_, StoreError>((outcome, 0))
    };
    let Ok(url) = FeedUrl::parse(&feed.url, feed.allow_http) else {
        return fail(Outcome::Refused).await;
    };
    let bytes = match fetch.get(&url, feed.allow_http).await {
        Ok(bytes) => bytes,
        Err(error) => return fail(error.outcome()).await,
    };
    // Parsing is CPU work on up to the cap's worth of hostile bytes; keep it off the reactor.
    let parsed = tokio::task::spawn_blocking(move || parse(&bytes, now)).await;
    let Ok(Ok(parsed)) = parsed else {
        return fail(Outcome::Invalid).await;
    };
    let mut items = parsed.items;
    if items.len() > MAX_NEW_PER_FETCH {
        items.drain(..items.len() - MAX_NEW_PER_FETCH);
    }
    match store.ingest(feed.id, &items, now, KEEP_THREADS).await? {
        Some(done) => {
            info!(feed = %feed.id, added = done.added, skipped = done.skipped, "feed read");
            Ok((Outcome::Ok, done.added))
        }
        // Removed while it was being read: nothing to record.
        None => Ok((Outcome::Ok, 0)),
    }
}

/// Read the feeds that are due, one after another, at most `max_feeds` of them.
///
/// # Errors
/// [`StoreError`] when the list of feeds cannot be read or a write fails.
pub async fn poll_due<S: FeedStore, F: Fetch>(
    store: &S,
    fetch: &F,
    now: UnixSeconds,
    max_feeds: usize,
) -> Result<PollReport, StoreError> {
    let mut report = PollReport::default();
    for feed in store
        .feeds()
        .await?
        .into_iter()
        .filter(|f| is_due(f, now))
        .take(max_feeds)
    {
        let (outcome, added) = poll_feed(store, fetch, &feed, now).await?;
        report.tried += 1;
        report.added += added;
        if outcome != Outcome::Ok {
            report.failed += 1;
        }
    }
    Ok(report)
}
