//! Search queries and hits.
//!
//! A member's query is **never** passed to a database as syntax. [`SearchQuery::parse`] cuts it
//! into plain word terms (runs of letters and digits, lower-cased), keeps at most [`MAX_TERMS`]
//! of at most [`MAX_TERM_CHARS`] characters from at most [`MAX_QUERY_CHARS`] characters of input,
//! and the backends bind those words as quoted phrases (SQLite) or `plainto_tsquery` input
//! (Postgres). Quotes, `AND`/`OR`/`NOT`, `NEAR`, `*`, `:` and `-` are only separators, so an
//! operator typed by a member is just a word. Every term must match (AND).

use super::ids::{BoardId, PostId, ThreadId};
use super::store::RawHit;
use crate::sanitize::SingleLine;
use crate::store::UnixSeconds;

/// Input past this many characters is ignored.
pub const MAX_QUERY_CHARS: usize = 200;
/// Most terms kept.
pub const MAX_TERMS: usize = 8;
/// Longest term kept (longer words are cut).
pub const MAX_TERM_CHARS: usize = 40;
/// Longest snippet, in characters.
const SNIPPET_CHARS: usize = 100;

/// A parsed query: a non-empty list of plain word terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery(Vec<String>);

impl SearchQuery {
    /// Parse typed text. `None` when it holds no word (empty, only punctuation).
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let mut terms: Vec<String> = Vec::new();
        let mut word = String::new();
        let flush = |word: &mut String, terms: &mut Vec<String>| {
            let term = std::mem::take(word);
            if !term.is_empty() && terms.len() < MAX_TERMS && !terms.contains(&term) {
                terms.push(term);
            }
        };
        for c in raw.chars().take(MAX_QUERY_CHARS) {
            if c.is_alphanumeric() {
                if word.chars().count() < MAX_TERM_CHARS {
                    word.extend(c.to_lowercase());
                }
            } else {
                flush(&mut word, &mut terms);
            }
        }
        flush(&mut word, &mut terms);
        if terms.is_empty() {
            None
        } else {
            Some(Self(terms))
        }
    }

    /// The terms.
    #[must_use]
    pub fn terms(&self) -> &[String] {
        &self.0
    }
}

/// One search result, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The matching post.
    pub post: PostId,
    /// Its thread (open it with [`super::thread_post`]).
    pub thread: ThreadId,
    /// Its board.
    pub board: BoardId,
    /// The board's name.
    pub board_name: String,
    /// The thread's subject.
    pub subject: String,
    /// The author's handle.
    pub author: Option<String>,
    /// Its place in the thread, from zero (open the thread there).
    pub index: u64,
    /// A cut of the post around the first term found, one line.
    pub snippet: String,
    /// When it was written.
    pub at: UnixSeconds,
}

pub(crate) fn hit(raw: &RawHit, query: &SearchQuery) -> Hit {
    let clean = |text: &str, max| SingleLine::new(max).sanitize(text).into_string();
    Hit {
        post: raw.post,
        thread: raw.thread,
        board: raw.board,
        board_name: clean(&raw.board_name, 40),
        subject: clean(&raw.subject, 120),
        author: raw.author.as_deref().map(|a| clean(a, 64)),
        index: raw.index,
        snippet: snippet(&raw.body, query),
        at: raw.created_at,
    }
}

/// A one-line cut of `body` that starts a little before the first term found.
fn snippet(body: &str, query: &SearchQuery) -> String {
    let flat: Vec<char> = SingleLine::new(MAX_QUERY_CHARS * 80)
        .sanitize(body)
        .as_str()
        .chars()
        .collect();
    let lower: String = flat.iter().flat_map(|c| c.to_lowercase()).collect();
    let at = query
        .terms()
        .iter()
        .filter_map(|term| lower.find(term.as_str()))
        .min()
        .map_or(0, |byte| lower[..byte].chars().count());
    let start = at.saturating_sub(20).min(flat.len());
    let end = (start + SNIPPET_CHARS).min(flat.len());
    let mut out: String = flat[start..end].iter().collect();
    if start > 0 {
        out.insert_str(0, "...");
    }
    if end < flat.len() {
        out.push_str("...");
    }
    out
}
