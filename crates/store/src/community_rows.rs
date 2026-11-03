//! Backend-independent mapping for the community tables (M3-J): what a stored row means,
//! written once (D24). Each backend runs its own SQL and hands the values here.

use russh_bbs_core::community::{
    EntryId, LineKind, PollId, StoredCaller, StoredLine, StoredPoll, StoredPollRow,
};
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

/// Seconds in the window the per-hour line limit counts over.
pub(crate) const HOUR: i64 = 3_600;
/// Seconds in the window the per-day poll limit counts over.
pub(crate) const DAY: i64 = 86_400;

pub(crate) fn line(id: i64, author: String, text: String, at: i64) -> StoredLine {
    StoredLine {
        id: EntryId::from_raw(id),
        author,
        text,
        at: UnixSeconds::from_raw(at),
    }
}

pub(crate) fn caller(handle: String, at: i64) -> StoredCaller {
    StoredCaller {
        handle,
        at: UnixSeconds::from_raw(at),
    }
}

pub(crate) fn poll_row(
    id: i64,
    question: String,
    closed: bool,
    votes: i64,
) -> Result<StoredPollRow, StoreError> {
    Ok(StoredPollRow {
        id: PollId::from_raw(id),
        question,
        closed,
        votes: u64::try_from(votes)
            .map_err(|_| StoreError::Corrupt("a poll has a negative vote count".into()))?,
    })
}

/// The pieces of a `poll` row and what was read beside it.
pub(crate) struct PollParts {
    pub id: i64,
    pub question: String,
    pub description: String,
    pub creator: i64,
    pub creator_handle: String,
    pub visibility: String,
    pub closed_at: Option<i64>,
    pub created_at: i64,
    pub options: Vec<String>,
    pub my_vote: Option<i64>,
}

pub(crate) fn poll(parts: PollParts) -> Result<StoredPoll, StoreError> {
    let my_vote = parts
        .my_vote
        .map(|p| {
            usize::try_from(p)
                .map_err(|_| StoreError::Corrupt("a vote names a negative option".into()))
        })
        .transpose()?;
    Ok(StoredPoll {
        id: PollId::from_raw(parts.id),
        question: parts.question,
        description: parts.description,
        creator: AccountId::from_raw(parts.creator),
        creator_handle: parts.creator_handle,
        visibility: parts.visibility,
        closed_at: parts.closed_at.map(UnixSeconds::from_raw),
        created_at: UnixSeconds::from_raw(parts.created_at),
        options: parts.options,
        my_vote,
    })
}

/// Votes per option from `(position, count)` rows.
pub(crate) fn tally(rows: impl IntoIterator<Item = (i64, i64)>) -> Result<Vec<u64>, StoreError> {
    let mut out: Vec<u64> = Vec::new();
    for (position, count) in rows {
        let at = usize::try_from(position)
            .map_err(|_| StoreError::Corrupt("a vote names a negative option".into()))?;
        let count = u64::try_from(count)
            .map_err(|_| StoreError::Corrupt("a poll has a negative vote count".into()))?;
        if out.len() <= at {
            out.resize(at + 1, 0);
        }
        out[at] = count;
    }
    Ok(out)
}

/// A count or position as a number the database can bind.
pub(crate) fn num(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// The kind of a line, as the database stores it.
pub(crate) const fn kind_name(kind: LineKind) -> &'static str {
    kind.name()
}
