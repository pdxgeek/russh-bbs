//! Backend-independent mapping for the feed tables: what a stored row means, written once (D24).

use russh_bbs_core::feeds::{FeedId, FeedRow, Outcome};
use russh_bbs_core::forums::BoardId;
use russh_bbs_core::store::{StoreError, UnixSeconds};

/// Everything one `feed` row holds, as the two backends read it.
pub(crate) struct Raw {
    pub id: i64,
    pub board_id: i64,
    pub board_name: String,
    pub url: String,
    pub allow_http: bool,
    pub interval_secs: i64,
    pub last_attempt_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_outcome: Option<String>,
    pub failures: i64,
    pub public: bool,
}

pub(crate) fn feed(raw: Raw) -> Result<FeedRow, StoreError> {
    let last_outcome = raw
        .last_outcome
        .map(|name| {
            Outcome::parse(&name)
                .ok_or_else(|| StoreError::Corrupt("a feed has an unknown outcome".into()))
        })
        .transpose()?;
    Ok(FeedRow {
        id: FeedId::from_raw(raw.id),
        board: BoardId::from_raw(raw.board_id),
        board_name: raw.board_name,
        url: raw.url,
        allow_http: raw.allow_http,
        interval_secs: u32::try_from(raw.interval_secs)
            .map_err(|_| StoreError::Corrupt("a feed interval is out of range".into()))?,
        last_attempt_at: raw.last_attempt_at.map(UnixSeconds::from_raw),
        last_success_at: raw.last_success_at.map(UnixSeconds::from_raw),
        last_outcome,
        failures: u32::try_from(raw.failures).unwrap_or(u32::MAX),
        public: raw.public,
    })
}

/// Most `feed_item` rows kept per feed (the memory of what was already seen).
pub(crate) const KEEP_SEEN: i64 = 5_000;
