//! Backend-independent mapping for the forum tables: what a stored row means, written once
//! (D24). Each backend runs its own SQL and hands the values here.

use russh_bbs_core::forums::{BoardId, BoardRow, PostId, PostRow, RawHit, ThreadId, ThreadRow};
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

fn count(value: i64, what: &str) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Corrupt(format!("{what} is negative")))
}

pub(crate) fn board(
    id: i64,
    name: String,
    description: String,
    restricted: bool,
    threads: i64,
) -> Result<BoardRow, StoreError> {
    Ok(BoardRow {
        id: BoardId::from_raw(id),
        name,
        description,
        restricted,
        threads: count(threads, "a board's thread count")?,
    })
}

pub(crate) fn thread(
    id: i64,
    board_id: i64,
    board_name: String,
    subject: String,
    author: Option<String>,
    posts: i64,
    last_post_at: i64,
) -> Result<ThreadRow, StoreError> {
    Ok(ThreadRow {
        id: ThreadId::from_raw(id),
        board: BoardId::from_raw(board_id),
        board_name,
        subject,
        author,
        posts: count(posts, "a thread's post count")?,
        last_post_at: UnixSeconds::from_raw(last_post_at),
    })
}

pub(crate) fn post(
    id: i64,
    thread_id: i64,
    board_id: i64,
    author_id: Option<i64>,
    author: Option<String>,
    body: String,
    created_at: i64,
) -> PostRow {
    PostRow {
        id: PostId::from_raw(id),
        thread: ThreadId::from_raw(thread_id),
        board: BoardId::from_raw(board_id),
        author_id: author_id.map(AccountId::from_raw),
        author,
        body,
        created_at: UnixSeconds::from_raw(created_at),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn hit(
    id: i64,
    thread_id: i64,
    board_id: i64,
    board_name: String,
    subject: String,
    author: Option<String>,
    body: String,
    index: i64,
    created_at: i64,
) -> Result<RawHit, StoreError> {
    Ok(RawHit {
        post: PostId::from_raw(id),
        thread: ThreadId::from_raw(thread_id),
        board: BoardId::from_raw(board_id),
        board_name,
        subject,
        author,
        body,
        index: count(index, "a post's place in its thread")?,
        created_at: UnixSeconds::from_raw(created_at),
    })
}

/// A number the database can compare with.
pub(crate) fn int(value: u32) -> i64 {
    i64::from(value)
}

/// The cap as a nullable number (`None` is unlimited).
pub(crate) fn cap(max: Option<u64>) -> Option<i64> {
    max.map(|n| i64::try_from(n).unwrap_or(i64::MAX))
}

/// The lower-cased name that makes two boards differ by more than case.
pub(crate) fn name_key(name: &str) -> String {
    name.to_lowercase()
}

/// `1,2,3` for a JSON array of board ids (SQLite binds it to `json_each`).
pub(crate) fn json_ids(boards: &[BoardId]) -> String {
    let list: Vec<String> = boards.iter().map(|b| b.as_raw().to_string()).collect();
    format!("[{}]", list.join(","))
}

/// The board ids as numbers (Postgres binds them as an array).
pub(crate) fn raw_ids(boards: &[BoardId]) -> Vec<i64> {
    boards.iter().map(|b| b.as_raw()).collect()
}

/// The terms as a SQLite FTS5 MATCH string: every term a quoted phrase, all must match. Terms
/// are runs of letters and digits (core's `SearchQuery`), so they hold no quote or operator;
/// the filter below drops anything else rather than trust that.
pub(crate) fn fts5_match(terms: &[String]) -> String {
    terms
        .iter()
        .filter(|t| !t.is_empty() && t.chars().all(char::is_alphanumeric))
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The terms as plain text for Postgres `plainto_tsquery`.
pub(crate) fn plain_terms(terms: &[String]) -> String {
    terms
        .iter()
        .filter(|t| !t.is_empty() && t.chars().all(char::is_alphanumeric))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}
