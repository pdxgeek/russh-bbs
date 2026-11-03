//! What a signed-in session can do with forums. Every function takes the session's
//! [`Snapshot`] (loaded for this action) and checks it, in this order: the board is reachable
//! (otherwise [`ForumError::NotFound`], the same answer as for a board that does not exist), the
//! permission is held (otherwise [`ForumError::NotPermitted`]), then the limit, then the write.

use crate::access::{
    AccessError, AccessStore, Granting, NewGrant, Permission, Scope, Snapshot, Subject,
};
use crate::audit::Actor;
use crate::drafts::{Body, QuoteLimits, quote};
use crate::limits::{LimitKind, LimitRefusal, LimitStore, load_effective};
use crate::sanitize::{Sanitized, SingleLine};
use crate::store::{AccountId, StoreError, UnixSeconds};

use super::ids::{BoardId, PostId, ThreadId};
use super::rules;
use super::search::{Hit, SearchQuery, hit};
use super::store::{
    BoardRow, ForumStore, NewBoard, NewPost, NewThread, PostCap, PostRow, PostWrite, ThreadRow,
};
use super::text::{TextError, check_body, prepare_description, prepare_name, prepare_subject};

/// Threads on a page, and hits on a search page.
pub const PAGE_SIZE: u32 = 12;

/// Seconds in the `posts_per_hour` window.
const HOUR: i64 = 3_600;

/// Why a forum action did not happen.
#[derive(Debug, thiserror::Error)]
pub enum ForumError {
    /// No such board, thread or post, **or** the session may not see it: one answer for both,
    /// so it reveals nothing.
    #[error("not found")]
    NotFound,
    /// The session can see the thing but may not do this to it.
    #[error("you may not do that")]
    NotPermitted(Permission),
    /// The board shows a feed: nobody posts or replies on it (M3-G, D89).
    #[error("this board is read-only")]
    ReadOnly,
    /// The session is not a signed-in account.
    #[error("not signed in")]
    NotSignedIn,
    /// A limit is used up.
    #[error("{0}")]
    Limit(LimitRefusal),
    /// The typed text was refused.
    #[error(transparent)]
    Text(#[from] TextError),
    /// A board with that name exists.
    #[error("a board with that name already exists")]
    NameTaken,
    /// No active member has that handle.
    #[error("no such member")]
    UnknownMember,
    /// Storage failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// What a feed board shows where a member's handle would be (M3-G): its items have no author
/// account. The colon-free word cannot be a member's handle only because the board is a feed
/// board; it is shown for those boards alone.
pub const FEED_AUTHOR: &str = "feed";

fn clean(text: &str, max: usize) -> String {
    SingleLine::new(max).sanitize(text).into_string()
}

/// A board in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardSummary {
    /// The board.
    pub id: BoardId,
    /// Its name.
    pub name: String,
    /// What it is for.
    pub description: String,
    /// Whether it needs a grant scoped to it (D83).
    pub restricted: bool,
    /// Whether it shows a feed's items and nobody can post on it (M3-G, D89).
    pub feed: bool,
    /// Threads on it.
    pub threads: u64,
}

impl BoardSummary {
    fn of(row: &BoardRow) -> Self {
        Self {
            id: row.id,
            name: clean(&row.name, 40),
            description: clean(&row.description, 160),
            restricted: row.restricted,
            feed: row.feed,
            threads: row.threads,
        }
    }
}

/// A thread in a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadSummary {
    /// The thread.
    pub id: ThreadId,
    /// The board's name.
    pub board_name: String,
    /// The subject.
    pub subject: String,
    /// Who started it.
    pub author: Option<String>,
    /// Posts in it.
    pub posts: u64,
    /// When it last changed.
    pub last_post_at: UnixSeconds,
}

impl ThreadSummary {
    fn of(row: &ThreadRow) -> Self {
        Self {
            id: row.id,
            board_name: clean(&row.board_name, 40),
            subject: clean(&row.subject, 120),
            author: row.author.as_deref().map(|a| clean(a, 64)),
            posts: row.posts,
            last_post_at: row.last_post_at,
        }
    }
}

/// One page of a board's threads.
// The flags are independent facts the screen reads (a later page exists; what the session may
// do here), not states of one machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadPage {
    /// The board.
    pub board: BoardSummary,
    /// The threads, newest activity first.
    pub threads: Vec<ThreadSummary>,
    /// This page, from zero.
    pub page: u32,
    /// Whether a later page exists.
    pub more: bool,
    /// Whether the session may start a thread here.
    pub can_post: bool,
    /// Whether the session manages boards (may allow members to a restricted one).
    pub can_manage: bool,
    /// Whether the session manages feeds (`feeds.manage`, M3-G): may remove this board's feed
    /// and make the board public as RSS.
    pub can_feeds: bool,
}

/// One post to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostView {
    /// The post.
    pub id: PostId,
    /// The author's handle, or `None` when the account is gone.
    pub author: Option<String>,
    /// When it was written.
    pub at: UnixSeconds,
    /// The text, cleaned again.
    pub body: Sanitized,
}

impl PostView {
    fn of(row: &PostRow) -> Self {
        Self {
            id: row.id,
            author: row.author.as_deref().map(|a| clean(a, 64)),
            at: row.created_at,
            body: crate::sanitize::MultiLine::new(super::text::BODY_MAX_CHARS).sanitize(&row.body),
        }
    }

    /// The post quoted for a reply: each line prefixed, already-quoted lines kept at most two
    /// deep ([`quote`]). Quote marks are plain text anyone can type (D80).
    #[must_use]
    pub fn quoted(&self) -> Sanitized {
        let who = self.author.as_deref().map(|a| clean(a, 64));
        let attribution = who.map(|w| SingleLine::new(64).sanitize(&w));
        quote(attribution.as_ref(), &self.body, QuoteLimits::default())
    }
}

/// A thread opened at one post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadView {
    /// The thread.
    pub thread: ThreadSummary,
    /// The board.
    pub board: BoardId,
    /// Which post is shown, from zero.
    pub index: u64,
    /// Posts in the thread.
    pub total: u64,
    /// The post.
    pub post: PostView,
    /// Whether the session may reply.
    pub can_reply: bool,
    /// Whether the session may remove posts here.
    pub can_moderate: bool,
}

/// What a successful post is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Posted {
    /// The thread it is in.
    pub thread: ThreadId,
    /// The new post.
    pub post: PostId,
}

/// One page of search results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPage {
    /// The hits, best first.
    pub hits: Vec<Hit>,
    /// This page, from zero.
    pub page: u32,
    /// Whether a later page exists.
    pub more: bool,
    /// Whether the query held any searchable word.
    pub searched: bool,
}

async fn load_board<S: ForumStore>(store: &S, id: BoardId) -> Result<BoardRow, ForumError> {
    store.board(id).await?.ok_or(ForumError::NotFound)
}

/// The board, if the session may read it: the one gate for opening anything inside a board.
async fn readable_board<S: ForumStore>(
    store: &S,
    snapshot: &Snapshot,
    id: BoardId,
) -> Result<BoardRow, ForumError> {
    let board = load_board(store, id).await?;
    if rules::can(snapshot, Permission::ForumRead, &board) {
        Ok(board)
    } else {
        Err(ForumError::NotFound)
    }
}

fn account_of(snapshot: &Snapshot) -> Result<AccountId, ForumError> {
    snapshot.account().ok_or(ForumError::NotSignedIn)
}

fn require_on(
    snapshot: &Snapshot,
    permission: Permission,
    board: &BoardRow,
) -> Result<AccountId, ForumError> {
    let account = account_of(snapshot)?;
    if rules::can(snapshot, permission, board) {
        Ok(account)
    } else {
        Err(ForumError::NotPermitted(permission))
    }
}

/// The boards the session may list.
///
/// # Errors
/// [`ForumError::Store`] when storage fails.
pub async fn boards<S: ForumStore>(
    store: &S,
    snapshot: &Snapshot,
) -> Result<Vec<BoardSummary>, ForumError> {
    let all = store.boards().await?;
    Ok(rules::reachable(snapshot, Permission::ForumList, &all)
        .into_iter()
        .map(BoardSummary::of)
        .collect())
}

/// A page of a board's threads.
///
/// # Errors
/// [`ForumError::NotFound`] for a board that is absent or not readable by the session.
pub async fn threads<S: ForumStore>(
    store: &S,
    snapshot: &Snapshot,
    board: BoardId,
    page: u32,
) -> Result<ThreadPage, ForumError> {
    let row = readable_board(store, snapshot, board).await?;
    let offset = page.saturating_mul(PAGE_SIZE);
    let mut rows = store.threads(board, PAGE_SIZE + 1, offset).await?;
    let more = rows.len() > PAGE_SIZE as usize;
    rows.truncate(PAGE_SIZE as usize);
    Ok(ThreadPage {
        board: BoardSummary::of(&row),
        threads: rows
            .iter()
            .map(|r| {
                let mut t = ThreadSummary::of(r);
                if row.feed && t.author.is_none() {
                    t.author = Some(FEED_AUTHOR.to_owned());
                }
                t
            })
            .collect(),
        page,
        more,
        can_post: rules::can(snapshot, Permission::ForumPost, &row),
        can_manage: snapshot.resolve(Permission::ForumManage, None).is_granted(),
        can_feeds: snapshot.resolve(Permission::FeedsManage, None).is_granted(),
    })
}

/// Open a thread at post `index` (from zero; past the end shows the last post).
///
/// # Errors
/// [`ForumError::NotFound`] for a thread that is absent or on a board the session cannot read.
pub async fn thread_post<S: ForumStore>(
    store: &S,
    snapshot: &Snapshot,
    thread: ThreadId,
    index: u64,
) -> Result<ThreadView, ForumError> {
    let row = store.thread(thread).await?.ok_or(ForumError::NotFound)?;
    let board = readable_board(store, snapshot, row.board).await?;
    let total = row.posts.max(1);
    let index = index.min(total - 1);
    let post = store
        .post_at(thread, index)
        .await?
        .ok_or(ForumError::NotFound)?;
    let mut summary = ThreadSummary::of(&row);
    let mut view = PostView::of(&post);
    if board.feed {
        summary.author.get_or_insert_with(|| FEED_AUTHOR.to_owned());
        view.author.get_or_insert_with(|| FEED_AUTHOR.to_owned());
    }
    Ok(ThreadView {
        thread: summary,
        board: board.id,
        index,
        total,
        post: view,
        can_reply: rules::can(snapshot, Permission::ForumReply, &board),
        can_moderate: rules::can(snapshot, Permission::ForumModerate, &board),
    })
}

async fn post_cap<S: LimitStore>(
    store: &S,
    snapshot: &Snapshot,
    account: AccountId,
    now: UnixSeconds,
) -> Result<PostCap, ForumError> {
    let limits = load_effective(store, account, snapshot).await?;
    Ok(PostCap {
        since: UnixSeconds::from_raw(now.as_raw().saturating_sub(HOUR)),
        max: limits.get(LimitKind::PostsPerHour).cap(),
    })
}

fn refused(cap: PostCap) -> ForumError {
    ForumError::Limit(LimitRefusal {
        kind: LimitKind::PostsPerHour,
        limit: cap.max.unwrap_or(0),
        // A rolling hour has no fixed reset time; the screen says so in words.
        resets_in: None,
    })
}

/// Start a thread on `board`.
///
/// # Errors
/// [`ForumError`]: not found, not permitted, a limit, refused text, or storage.
pub async fn post_thread<S: ForumStore + LimitStore>(
    store: &S,
    snapshot: &Snapshot,
    now: UnixSeconds,
    board: BoardId,
    subject: &str,
    body: &Body,
) -> Result<Posted, ForumError> {
    let row = readable_board(store, snapshot, board).await?;
    if row.feed {
        return Err(ForumError::ReadOnly);
    }
    let author = require_on(snapshot, Permission::ForumPost, &row)?;
    let subject = prepare_subject(subject)?;
    let body = check_body(body)?;
    let cap = post_cap(store, snapshot, author, now).await?;
    let new = NewThread {
        board,
        subject,
        body,
        author,
        now,
    };
    match store.add_thread(&new, cap).await? {
        PostWrite::Written((thread, post)) => Ok(Posted { thread, post }),
        PostWrite::Full => Err(refused(cap)),
        PostWrite::NoParent => Err(ForumError::NotFound),
    }
}

/// Reply in `thread`.
///
/// # Errors
/// [`ForumError`]: not found, not permitted, a limit, refused text, or storage.
pub async fn post_reply<S: ForumStore + LimitStore>(
    store: &S,
    snapshot: &Snapshot,
    now: UnixSeconds,
    thread: ThreadId,
    body: &Body,
) -> Result<Posted, ForumError> {
    let row = store.thread(thread).await?.ok_or(ForumError::NotFound)?;
    let board = readable_board(store, snapshot, row.board).await?;
    if board.feed {
        return Err(ForumError::ReadOnly);
    }
    let author = require_on(snapshot, Permission::ForumReply, &board)?;
    let body = check_body(body)?;
    let cap = post_cap(store, snapshot, author, now).await?;
    let new = NewPost {
        thread,
        body,
        author,
        now,
    };
    match store.add_reply(&new, cap).await? {
        PostWrite::Written(post) => Ok(Posted { thread, post }),
        PostWrite::Full => Err(refused(cap)),
        PostWrite::NoParent => Err(ForumError::NotFound),
    }
}

/// What removing a post did, for the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gone {
    /// The board it was on.
    pub board: BoardId,
    /// Whether the whole thread went (it was the opening post).
    pub thread_removed: bool,
    /// The thread, when it still exists.
    pub thread: ThreadId,
}

/// Remove a post (staff: `forum.moderate` on its board). Removing the opening post removes the
/// thread. The removal is audited in the same transaction, by id only.
///
/// # Errors
/// [`ForumError`]: not found, not permitted, or storage.
pub async fn remove_post<S: ForumStore>(
    store: &S,
    snapshot: &Snapshot,
    post: PostId,
) -> Result<Gone, ForumError> {
    let row = store.post(post).await?.ok_or(ForumError::NotFound)?;
    let board = readable_board(store, snapshot, row.board).await?;
    let actor = require_on(snapshot, Permission::ForumModerate, &board)?;
    let removed = store
        .remove_post(post, actor)
        .await?
        .ok_or(ForumError::NotFound)?;
    Ok(Gone {
        board: removed.board,
        thread_removed: removed.thread_removed,
        thread: removed.thread,
    })
}

/// Create a board (staff: `forum.manage`).
///
/// # Errors
/// [`ForumError`]: not permitted, refused text, a taken name, or storage.
pub async fn create_board<S: ForumStore>(
    store: &S,
    snapshot: &Snapshot,
    now: UnixSeconds,
    name: &str,
    description: &str,
    restricted: bool,
) -> Result<BoardId, ForumError> {
    let actor = account_of(snapshot)?;
    snapshot
        .require(Permission::ForumManage, None)
        .map_err(|_| ForumError::NotPermitted(Permission::ForumManage))?;
    let new = NewBoard {
        name: prepare_name(name)?,
        description: prepare_description(description)?,
        restricted,
    };
    store
        .create_board(&new, actor, now)
        .await?
        .ok_or(ForumError::NameTaken)
}

/// The permissions an allow entry on a board gives: to see it, read it, start threads, reply.
const ALLOWED: [Permission; 4] = [
    Permission::ForumList,
    Permission::ForumRead,
    Permission::ForumPost,
    Permission::ForumReply,
];

/// Let the member with `handle` into `board` (staff: `forum.manage`): four allow entries scoped
/// to the board, each audited as `permission.granted`. Already allowed is not an error.
///
/// # Errors
/// [`ForumError`]: not found, not permitted, no such member, or storage.
pub async fn allow_member<S: ForumStore + AccessStore>(
    store: &S,
    snapshot: &Snapshot,
    board: BoardId,
    handle: &str,
) -> Result<(), ForumError> {
    let actor = account_of(snapshot)?;
    snapshot
        .require(Permission::ForumManage, None)
        .map_err(|_| ForumError::NotPermitted(Permission::ForumManage))?;
    let row = load_board(store, board).await?;
    let resource = row.id.resource().ok_or(ForumError::NotFound)?;
    // Nobody hands out what they do not hold themselves (no escalation, as in the access
    // editing services).
    for permission in ALLOWED {
        if !snapshot.resolve(permission, Some(resource)).is_granted() {
            return Err(ForumError::NotPermitted(permission));
        }
    }
    let member = store
        .active_account_by_handle(handle.trim())
        .await?
        .ok_or(ForumError::UnknownMember)?;
    for permission in ALLOWED {
        let grant = NewGrant {
            subject: Subject::Account(member),
            granting: Granting::Permission(permission),
            scope: Scope::Resource(resource),
        };
        store
            .add_grant(&grant, Actor::Account(actor))
            .await
            .map_err(|error| match error {
                AccessError::Store(error) => ForumError::Store(error),
                AccessError::UnknownAccount => ForumError::UnknownMember,
                _ => ForumError::Store(StoreError::Corrupt("a grant was refused".into())),
            })?;
    }
    Ok(())
}

/// Search the posts the session may read. The query is parsed to plain words ([`SearchQuery`]);
/// the readable boards are part of the query, so a result, a title or a count from any other
/// board is never fetched.
///
/// # Errors
/// [`ForumError::Store`] when storage fails.
pub async fn search<S: ForumStore>(
    store: &S,
    snapshot: &Snapshot,
    raw: &str,
    page: u32,
) -> Result<SearchPage, ForumError> {
    let none = SearchPage {
        hits: Vec::new(),
        page,
        more: false,
        searched: false,
    };
    let Some(query) = SearchQuery::parse(raw) else {
        return Ok(none);
    };
    let all = store.boards().await?;
    let readable = rules::reachable_ids(snapshot, Permission::ForumRead, &all);
    if readable.is_empty() {
        return Ok(SearchPage {
            searched: true,
            ..none
        });
    }
    let offset = page.saturating_mul(PAGE_SIZE);
    let mut raws = store
        .search_posts(&readable, query.terms(), PAGE_SIZE + 1, offset)
        .await?;
    // The store was only given readable boards; this keeps a faulty backend from leaking.
    raws.retain(|raw| readable.contains(&raw.board));
    let more = raws.len() > PAGE_SIZE as usize;
    raws.truncate(PAGE_SIZE as usize);
    let feeds: Vec<BoardId> = all.iter().filter(|b| b.feed).map(|b| b.id).collect();
    Ok(SearchPage {
        hits: raws
            .iter()
            .map(|raw| {
                let mut h = hit(raw, &query);
                if feeds.contains(&raw.board) {
                    h.author.get_or_insert_with(|| FEED_AUTHOR.to_owned());
                }
                h
            })
            .collect(),
        page,
        more,
        searched: true,
    })
}
