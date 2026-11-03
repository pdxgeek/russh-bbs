//! The storage trait for forums: persistence only. Rules (access, text, limits, what a search
//! may see) are in [`super::service`], [`super::rules`] and [`super::search`]; the backends only
//! read and write rows.

use std::future::Future;

use crate::drafts::Body;
use crate::sanitize::Sanitized;
use crate::store::{AccountId, StoreError, UnixSeconds};

use super::ids::{BoardId, PostId, ThreadId};

/// A board as stored. Text is what the sanitizer let through; core cleans it again on read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardRow {
    /// The board.
    pub id: BoardId,
    /// Its name.
    pub name: String,
    /// What it is for; may be empty.
    pub description: String,
    /// Whether a global holding of `forum.*` is not enough to reach it (D83).
    pub restricted: bool,
    /// Threads on it.
    pub threads: u64,
}

/// A board to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBoard {
    /// Its name; unique ignoring case.
    pub name: Sanitized,
    /// What it is for.
    pub description: Sanitized,
    /// Whether it needs a grant scoped to it.
    pub restricted: bool,
}

/// A thread as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadRow {
    /// The thread.
    pub id: ThreadId,
    /// The board it is on.
    pub board: BoardId,
    /// The board's name, for headings.
    pub board_name: String,
    /// The subject.
    pub subject: String,
    /// Who started it (their handle), or `None` when the account is gone.
    pub author: Option<String>,
    /// Posts in it, the opening one included.
    pub posts: u64,
    /// When the last post was made.
    pub last_post_at: UnixSeconds,
}

/// A post as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostRow {
    /// The post.
    pub id: PostId,
    /// Its thread.
    pub thread: ThreadId,
    /// Its board.
    pub board: BoardId,
    /// Who wrote it, or `None` when the account is gone.
    pub author_id: Option<AccountId>,
    /// Their handle.
    pub author: Option<String>,
    /// The text.
    pub body: String,
    /// When it was made.
    pub created_at: UnixSeconds,
}

/// A thread to start: its subject and opening post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewThread {
    /// The board.
    pub board: BoardId,
    /// The subject.
    pub subject: Sanitized,
    /// The opening post.
    pub body: Body,
    /// The author.
    pub author: AccountId,
    /// When.
    pub now: UnixSeconds,
}

/// A reply to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPost {
    /// The thread.
    pub thread: ThreadId,
    /// The text.
    pub body: Body,
    /// The author.
    pub author: AccountId,
    /// When.
    pub now: UnixSeconds,
}

/// `posts_per_hour`, as the write needs it: the write itself counts the author's posts made at
/// or after `since` and refuses to add one past `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostCap {
    /// The start of the rolling window.
    pub since: UnixSeconds,
    /// The most posts allowed in it; `None` is unlimited.
    pub max: Option<u64>,
}

/// What a post write did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostWrite<T> {
    /// Written.
    Written(T),
    /// The author has used the window's posts; nothing was written.
    Full,
    /// The board or thread is gone (removed while the member typed).
    NoParent,
}

/// What removing a post did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Removed {
    /// The board it was on.
    pub board: BoardId,
    /// The thread it was in.
    pub thread: ThreadId,
    /// Who wrote it.
    pub author: Option<AccountId>,
    /// Whether it was the opening post, so the whole thread went with it.
    pub thread_removed: bool,
}

/// A search hit as stored, before core cuts a snippet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawHit {
    /// The matching post.
    pub post: PostId,
    /// Its thread.
    pub thread: ThreadId,
    /// Its board.
    pub board: BoardId,
    /// The board's name.
    pub board_name: String,
    /// The thread's subject.
    pub subject: String,
    /// The post's author's handle.
    pub author: Option<String>,
    /// The post's text.
    pub body: String,
    /// How many posts of its thread come before it (its place when the thread is read).
    pub index: u64,
    /// When it was made.
    pub created_at: UnixSeconds,
}

/// Boards, threads, posts and search.
pub trait ForumStore: Send + Sync {
    /// Every board, oldest first, with its thread count. The caller filters by access.
    fn boards(&self) -> impl Future<Output = Result<Vec<BoardRow>, StoreError>> + Send;

    /// One board, or `None`.
    fn board(
        &self,
        id: BoardId,
    ) -> impl Future<Output = Result<Option<BoardRow>, StoreError>> + Send;

    /// Create a board. `None` when the name is taken (ignoring case). Audited in the same
    /// transaction (`board.created`, by `actor`).
    fn create_board(
        &self,
        new: &NewBoard,
        actor: AccountId,
        now: UnixSeconds,
    ) -> impl Future<Output = Result<Option<BoardId>, StoreError>> + Send;

    /// A board's threads, newest activity first.
    fn threads(
        &self,
        board: BoardId,
        limit: u32,
        offset: u32,
    ) -> impl Future<Output = Result<Vec<ThreadRow>, StoreError>> + Send;

    /// One thread, or `None`.
    fn thread(
        &self,
        id: ThreadId,
    ) -> impl Future<Output = Result<Option<ThreadRow>, StoreError>> + Send;

    /// The post at `index` (from zero, oldest first) of a thread, or `None`.
    fn post_at(
        &self,
        thread: ThreadId,
        index: u64,
    ) -> impl Future<Output = Result<Option<PostRow>, StoreError>> + Send;

    /// One post, or `None`.
    fn post(&self, id: PostId) -> impl Future<Output = Result<Option<PostRow>, StoreError>> + Send;

    /// Start a thread, unless `cap` says the author has used their posts.
    fn add_thread(
        &self,
        new: &NewThread,
        cap: PostCap,
    ) -> impl Future<Output = Result<PostWrite<(ThreadId, PostId)>, StoreError>> + Send;

    /// Reply in a thread, unless `cap` says the author has used their posts.
    fn add_reply(
        &self,
        new: &NewPost,
        cap: PostCap,
    ) -> impl Future<Output = Result<PostWrite<PostId>, StoreError>> + Send;

    /// Delete a post (and its thread when it was the opening post). `None` when there is no
    /// such post. Audited in the same transaction (`post.removed`, by `actor`; ids only, never
    /// the text).
    fn remove_post(
        &self,
        post: PostId,
        actor: AccountId,
    ) -> impl Future<Output = Result<Option<Removed>, StoreError>> + Send;

    /// Posts in `boards` that match every term, best match first. Only these boards are
    /// searched; an empty list finds nothing. `terms` are plain words (see
    /// [`super::SearchQuery`]).
    fn search_posts(
        &self,
        boards: &[BoardId],
        terms: &[String],
        limit: u32,
        offset: u32,
    ) -> impl Future<Output = Result<Vec<RawHit>, StoreError>> + Send;

    /// The active account whose handle is `handle`, ignoring case.
    fn active_account_by_handle(
        &self,
        handle: &str,
    ) -> impl Future<Output = Result<Option<AccountId>, StoreError>> + Send;
}

/// The empty directory has no boards and no accounts: reads find nothing, writes are refused.
impl ForumStore for crate::access::NoMembers {
    fn boards(&self) -> impl Future<Output = Result<Vec<BoardRow>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn board(
        &self,
        _id: BoardId,
    ) -> impl Future<Output = Result<Option<BoardRow>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn create_board(
        &self,
        _new: &NewBoard,
        _actor: AccountId,
        _now: UnixSeconds,
    ) -> impl Future<Output = Result<Option<BoardId>, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn threads(
        &self,
        _board: BoardId,
        _limit: u32,
        _offset: u32,
    ) -> impl Future<Output = Result<Vec<ThreadRow>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn thread(
        &self,
        _id: ThreadId,
    ) -> impl Future<Output = Result<Option<ThreadRow>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn post_at(
        &self,
        _thread: ThreadId,
        _index: u64,
    ) -> impl Future<Output = Result<Option<PostRow>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn post(
        &self,
        _id: PostId,
    ) -> impl Future<Output = Result<Option<PostRow>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn add_thread(
        &self,
        _new: &NewThread,
        _cap: PostCap,
    ) -> impl Future<Output = Result<PostWrite<(ThreadId, PostId)>, StoreError>> + Send {
        std::future::ready(Ok(PostWrite::NoParent))
    }

    fn add_reply(
        &self,
        _new: &NewPost,
        _cap: PostCap,
    ) -> impl Future<Output = Result<PostWrite<PostId>, StoreError>> + Send {
        std::future::ready(Ok(PostWrite::NoParent))
    }

    fn remove_post(
        &self,
        _post: PostId,
        _actor: AccountId,
    ) -> impl Future<Output = Result<Option<Removed>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn search_posts(
        &self,
        _boards: &[BoardId],
        _terms: &[String],
        _limit: u32,
        _offset: u32,
    ) -> impl Future<Output = Result<Vec<RawHit>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn active_account_by_handle(
        &self,
        _handle: &str,
    ) -> impl Future<Output = Result<Option<AccountId>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }
}
