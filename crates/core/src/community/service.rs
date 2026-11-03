//! The rules a front end calls: one [`execute`] for every request a community screen makes.
//!
//! Each request names the permission it needs and checks it with `Snapshot::require` before the
//! store is touched. Text is sanitized on the way in ([`LineText`], [`NewPoll`]) and again on
//! the way out, so a row edited by hand cannot reach a screen unchecked. The store's own
//! transactions decide the races (the rate limit, the cap, one vote each, who may close).

use crate::access::{Permission, PermissionCheck, Snapshot};
use crate::sanitize::{SingleLine, USERNAME_MAX_CHARS};
use crate::store::{AccountId, StoreError, UnixSeconds};

use super::keys;
use super::store::{
    AddLine, CloseOutcome, CommunityStore, CreatePoll, PollRules, RemoveOutcome, StoredLine,
    StoredPoll, VoteOutcome, visibility_of,
};
use super::types::{
    CALLERS_SHOWN, DESCRIPTION_CHARS, EntryId, LineKind, LineText, NewPoll, OPTION_MAX, POLL_PAGE,
    PollError, PollId, QUESTION_MAX, ResultsVisibility, TextError,
};

/// Polls the board keeps in all. Staff remove one to make room.
pub const MAX_POLLS: usize = 90;
/// Polls one member may make in a rolling day.
pub const POLLS_PER_DAY: usize = 3;

/// A one-line result shown over a screen: a string-table key and whether it is a problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    /// Whether it is a problem.
    pub error: bool,
    /// The string-table key of the words.
    pub key: &'static str,
}

impl Note {
    const fn info(key: &'static str) -> Self {
        Self { error: false, key }
    }

    const fn problem(key: &'static str) -> Self {
        Self { error: true, key }
    }
}

/// A one-liner or wall line, sanitized for a screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The row id (staff remove by it).
    pub id: EntryId,
    /// The author's handle.
    pub author: String,
    /// The text.
    pub text: String,
    /// When it was added.
    pub at: UnixSeconds,
}

/// A member in last callers, sanitized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerRow {
    /// The handle.
    pub handle: String,
    /// When their latest session began.
    pub at: UnixSeconds,
}

/// A row of the poll list, sanitized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollRow {
    /// The poll.
    pub id: PollId,
    /// The question.
    pub question: String,
    /// Whether it is closed.
    pub closed: bool,
    /// Votes cast.
    pub votes: u64,
}

/// A poll as one viewer may see it. `counts` is `None` while the poll's setting hides them from
/// this viewer: the counts are not even read from the store then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollDetail {
    /// The poll.
    pub id: PollId,
    /// The question.
    pub question: String,
    /// The description (may be empty).
    pub description: String,
    /// The creator's handle.
    pub creator: String,
    /// The options in voting order.
    pub options: Vec<String>,
    /// The option the viewer voted for.
    pub my_vote: Option<usize>,
    /// Whether the poll is closed.
    pub closed: bool,
    /// When counts show.
    pub visibility: ResultsVisibility,
    /// Votes per option, when this viewer may see them.
    pub counts: Option<Vec<u64>>,
    /// What the viewer may do with it now.
    pub can: PollActions,
}

/// What a viewer may do with a poll, for the hints a screen shows. The keys still ask the core,
/// which checks again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollActions {
    /// Vote: holds `polls.vote`, has not voted, and the poll is open.
    pub vote: bool,
    /// Close: the creator, or staff, and the poll is open.
    pub close: bool,
    /// Remove: staff.
    pub remove: bool,
}

/// What a community screen asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Show a list of lines.
    Lines(LineKind),
    /// Add a line (typed text, not yet sanitized).
    Post(LineKind, String),
    /// Remove a line (staff).
    Remove(LineKind, EntryId),
    /// Show last callers.
    Callers,
    /// Show a page of polls starting at `offset`.
    Polls {
        /// How many polls to skip.
        offset: usize,
    },
    /// Open one poll.
    Poll(PollId),
    /// Make a poll from typed text.
    Create {
        /// The question.
        question: String,
        /// The options as typed (blank ones are dropped).
        options: Vec<String>,
        /// The description from the editor (may be empty).
        description: String,
        /// When counts show.
        visibility: ResultsVisibility,
    },
    /// Vote for option `option` (zero-based).
    Vote(PollId, usize),
    /// Close a poll.
    Close(PollId),
    /// Remove a poll (staff).
    RemovePoll(PollId),
}

/// What a request came to. Every variant that shows content carries it whole, so a screen never
/// has to merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// A list of lines (after loading, posting or removing), with what happened.
    Lines {
        /// Which list.
        kind: LineKind,
        /// Newest first.
        entries: Vec<Entry>,
        /// What happened to the request, if anything is worth saying.
        note: Option<Note>,
    },
    /// Last callers.
    Callers(Vec<CallerRow>),
    /// A page of the poll list.
    Polls {
        /// Where the page starts.
        offset: usize,
        /// The rows.
        rows: Vec<PollRow>,
        /// Polls in all.
        total: usize,
        /// What happened to the request, if anything is worth saying.
        note: Option<Note>,
    },
    /// One poll.
    Poll {
        /// The poll as this viewer sees it.
        detail: Box<PollDetail>,
        /// What happened to the request, if anything is worth saying.
        note: Option<Note>,
    },
    /// The form was refused (the text stays on screen).
    Refused(Note),
    /// The session lacks the permission. Same words for every request.
    NotPermitted,
    /// The store failed or timed out; nothing is claimed about a write.
    Failed,
}

fn clean(text: &str, max: usize) -> String {
    SingleLine::new(max).sanitize(text).into_string()
}

fn handle(text: &str) -> String {
    clean(text, USERNAME_MAX_CHARS)
}

fn entry(line: &StoredLine, kind: LineKind) -> Entry {
    Entry {
        id: line.id,
        author: handle(&line.author),
        text: clean(&line.text, kind.max_chars()),
        at: line.at,
    }
}

async fn lines<S: CommunityStore>(store: &S, kind: LineKind) -> Result<Vec<Entry>, StoreError> {
    let rows = store.recent_lines(kind, kind.keep()).await?;
    Ok(rows.iter().map(|row| entry(row, kind)).collect())
}

/// The account of a session that is signed in. A waiting-room snapshot has none and holds none
/// of these permissions, so this is only a second guard.
fn me(access: &Snapshot) -> Option<AccountId> {
    access.account()
}

/// Record that `account`'s member session began (last callers). The caller is the point where a
/// member session starts: never the waiting room, never a founder-claim session.
///
/// # Errors
/// [`StoreError`] when storage fails; the session goes on without the row.
pub async fn record_sign_in<S: CommunityStore>(
    store: &S,
    account: AccountId,
    now: UnixSeconds,
) -> Result<(), StoreError> {
    store.record_sign_in(account, now).await
}

/// Run `request` for a session holding `access`. `now` is the time the rate limits count from.
pub async fn execute<S: CommunityStore>(
    store: &S,
    access: &Snapshot,
    now: UnixSeconds,
    request: Request,
) -> Response {
    match run(store, access, now, request).await {
        Ok(response) => response,
        Err(Failure::NotPermitted) => Response::NotPermitted,
        Err(Failure::Store(error)) => {
            tracing::warn!(%error, "community request failed");
            Response::Failed
        }
    }
}

enum Failure {
    NotPermitted,
    Store(StoreError),
}

impl From<StoreError> for Failure {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

fn need(access: &Snapshot, permission: Permission) -> Result<(), Failure> {
    access
        .require(permission, None)
        .map(|_| ())
        .map_err(|_| Failure::NotPermitted)
}

fn line_refusal(error: TextError) -> Note {
    match error {
        TextError::Blank => Note::problem(keys::NOTE_BLANK),
        TextError::TooLong { .. } => Note::problem(keys::NOTE_TOO_LONG),
    }
}

fn poll_refusal(error: &PollError) -> Note {
    Note::problem(match error {
        PollError::QuestionBlank => keys::NOTE_POLL_QUESTION_BLANK,
        PollError::QuestionTooLong => keys::NOTE_POLL_QUESTION_LONG,
        PollError::TooFewOptions => keys::NOTE_POLL_TOO_FEW,
        PollError::TooManyOptions => keys::NOTE_POLL_TOO_MANY,
        PollError::OptionTooLong => keys::NOTE_POLL_OPTION_LONG,
        PollError::DuplicateOption => keys::NOTE_POLL_DUPLICATE,
        PollError::Description(_) => keys::NOTE_POLL_DESCRIPTION,
    })
}

async fn polls_page<S: CommunityStore>(
    store: &S,
    offset: usize,
    note: Option<Note>,
) -> Result<Response, Failure> {
    let (rows, total) = store.list_polls(offset, POLL_PAGE).await?;
    let rows = rows
        .into_iter()
        .map(|row| PollRow {
            id: row.id,
            question: clean(&row.question, QUESTION_MAX),
            closed: row.closed,
            votes: row.votes,
        })
        .collect();
    Ok(Response::Polls {
        offset,
        rows,
        total,
        note,
    })
}

async fn poll_detail<S: CommunityStore>(
    store: &S,
    access: &Snapshot,
    viewer: AccountId,
    id: PollId,
    note: Option<Note>,
) -> Result<Response, Failure> {
    let Some(poll) = store.poll(id, viewer).await? else {
        return Ok(Response::Refused(Note::problem(keys::NOTE_POLL_GONE)));
    };
    let visibility = visibility_of(&poll.visibility)?;
    let StoredPoll {
        question,
        description,
        creator,
        creator_handle,
        closed_at,
        options,
        my_vote,
        ..
    } = poll;
    let closed = closed_at.is_some();
    // The counts are read only when this viewer may see them.
    let counts = if visibility.shows(my_vote.is_some(), closed) {
        let mut counts = store.tally(id).await?;
        counts.resize(options.len(), 0);
        Some(counts)
    } else {
        None
    };
    let staff = access.allows(Permission::CommunityModerate);
    Ok(Response::Poll {
        detail: Box::new(PollDetail {
            id,
            question: clean(&question, QUESTION_MAX),
            description: crate::sanitize::MultiLine::new(DESCRIPTION_CHARS)
                .sanitize(&description)
                .into_string(),
            creator: handle(&creator_handle),
            options: options.iter().map(|o| clean(o, OPTION_MAX)).collect(),
            my_vote,
            closed,
            visibility,
            counts,
            can: PollActions {
                vote: !closed && my_vote.is_none() && access.allows(Permission::PollsVote),
                close: !closed && (creator == viewer || staff),
                remove: staff,
            },
        }),
        note,
    })
}

#[allow(clippy::too_many_lines)]
async fn run<S: CommunityStore>(
    store: &S,
    access: &Snapshot,
    now: UnixSeconds,
    request: Request,
) -> Result<Response, Failure> {
    let who = me(access).ok_or(Failure::NotPermitted)?;
    match request {
        Request::Lines(kind) => {
            need(access, Permission::CommunityView)?;
            Ok(Response::Lines {
                kind,
                entries: lines(store, kind).await?,
                note: None,
            })
        }
        Request::Post(kind, raw) => {
            need(access, kind.post_permission())?;
            let text = match LineText::parse(kind, &raw) {
                Ok(text) => text,
                Err(error) => return Ok(Response::Refused(line_refusal(error))),
            };
            let note = match store.add_line(who, kind, &text, now).await? {
                AddLine::Added(_) => Note::info(keys::NOTE_POSTED),
                AddLine::RateLimited { .. } => Note::problem(keys::NOTE_RATE_LIMITED),
            };
            Ok(Response::Lines {
                kind,
                entries: lines(store, kind).await?,
                note: Some(note),
            })
        }
        Request::Remove(kind, id) => {
            need(access, Permission::CommunityModerate)?;
            let note = match store.remove_line(who, kind, id).await? {
                RemoveOutcome::Removed => Note::info(keys::NOTE_REMOVED),
                RemoveOutcome::NotFound => Note::problem(keys::NOTE_NOT_FOUND),
            };
            Ok(Response::Lines {
                kind,
                entries: lines(store, kind).await?,
                note: Some(note),
            })
        }
        Request::Callers => {
            need(access, Permission::CommunityView)?;
            let rows = store.last_callers(CALLERS_SHOWN).await?;
            Ok(Response::Callers(
                rows.into_iter()
                    .map(|row| CallerRow {
                        handle: handle(&row.handle),
                        at: row.at,
                    })
                    .collect(),
            ))
        }
        Request::Polls { offset } => {
            need(access, Permission::CommunityView)?;
            polls_page(store, offset, None).await
        }
        Request::Poll(id) => {
            need(access, Permission::CommunityView)?;
            poll_detail(store, access, who, id, None).await
        }
        Request::Create {
            question,
            options,
            description,
            visibility,
        } => {
            need(access, Permission::PollsCreate)?;
            let poll = match NewPoll::build(&question, &options, &description, visibility) {
                Ok(poll) => poll,
                Err(error) => return Ok(Response::Refused(poll_refusal(&error))),
            };
            let rules = PollRules {
                max_polls: MAX_POLLS,
                per_day: POLLS_PER_DAY,
            };
            match store.create_poll(who, &poll, rules, now).await? {
                CreatePoll::Created(id) => {
                    poll_detail(
                        store,
                        access,
                        who,
                        id,
                        Some(Note::info(keys::NOTE_POLL_CREATED)),
                    )
                    .await
                }
                CreatePoll::RateLimited { .. } => Ok(Response::Refused(Note::problem(
                    keys::NOTE_POLL_RATE_LIMITED,
                ))),
                CreatePoll::BoardFull => Ok(Response::Refused(Note::problem(keys::NOTE_POLL_FULL))),
            }
        }
        Request::Vote(id, option) => {
            need(access, Permission::PollsVote)?;
            let note = match store.vote(id, who, option, now).await? {
                VoteOutcome::Voted => Note::info(keys::NOTE_VOTED),
                VoteOutcome::AlreadyVoted => Note::problem(keys::NOTE_ALREADY_VOTED),
                VoteOutcome::Closed => Note::problem(keys::NOTE_POLL_CLOSED),
                VoteOutcome::NotFound => Note::problem(keys::NOTE_POLL_GONE),
            };
            poll_detail(store, access, who, id, Some(note)).await
        }
        Request::Close(id) => {
            need(access, Permission::CommunityView)?;
            let staff = access.allows(Permission::CommunityModerate);
            let note = match store.close_poll(who, id, staff, now).await? {
                CloseOutcome::Done => Note::info(keys::NOTE_POLL_CLOSED_NOW),
                CloseOutcome::AlreadyClosed => Note::problem(keys::NOTE_POLL_CLOSED),
                CloseOutcome::NotAllowed => Note::problem(keys::NOTE_POLL_NOT_YOURS),
                CloseOutcome::NotFound => Note::problem(keys::NOTE_POLL_GONE),
            };
            poll_detail(store, access, who, id, Some(note)).await
        }
        Request::RemovePoll(id) => {
            need(access, Permission::CommunityModerate)?;
            let note = match store.remove_poll(who, id).await? {
                CloseOutcome::Done => Note::info(keys::NOTE_POLL_REMOVED),
                CloseOutcome::NotFound | CloseOutcome::AlreadyClosed => {
                    Note::problem(keys::NOTE_POLL_GONE)
                }
                CloseOutcome::NotAllowed => Note::problem(keys::NOTE_POLL_NOT_YOURS),
            };
            polls_page(store, 0, Some(note)).await
        }
    }
}
