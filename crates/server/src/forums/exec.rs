//! Requests the forum screens make of the store, and the executor that runs them.

use std::time::Duration;

use russh_bbs_core::access::{AccessStore, Permission, Snapshot};
use russh_bbs_core::drafts::{
    Body, ContextKey, DraftError, DraftStore, SaveOutcome, discard_draft, save_draft,
};
use russh_bbs_core::forums::{
    self, BoardId, BoardSummary, ForumError, ForumStore, Gone, PostId, Posted, SearchPage,
    ThreadId, ThreadPage, ThreadView, body_limits,
};
use russh_bbs_core::limits::{Clock, LimitStore, SystemClock};
use russh_bbs_core::sanitize::{Sanitized, SingleLine};
use tokio::time::timeout;
use tracing::warn;

use super::strings;
use crate::ui::editor::{DraftSave, Editor, EditorRequest};

/// How long one forum request may take. A slow store must not hang the screen.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// What a member is writing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Purpose {
    /// A new thread on a board; the subject is already typed (sanitized again by core).
    Thread {
        board: BoardId,
        board_name: String,
        subject: String,
    },
    /// A reply in a thread, optionally quoting the post being read.
    Reply { thread: ThreadId, subject: String },
}

impl Purpose {
    /// The draft context key, built from ids only (never from member text).
    pub(crate) fn context(&self) -> Option<ContextKey> {
        let key = match self {
            Self::Thread { board, .. } => format!("forum:new:{board}"),
            Self::Reply { thread, .. } => format!("forum:reply:{thread}"),
        };
        ContextKey::parse(&key).ok()
    }

    fn title(&self) -> Sanitized {
        let text = match self {
            Self::Thread {
                board_name,
                subject,
                ..
            } => format!("{board_name}: {subject}"),
            Self::Reply { subject, .. } => format!("Reply: {subject}"),
        };
        SingleLine::new(60).sanitize(&text)
    }
}

/// The board list and what the member may do from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoardList {
    pub(crate) boards: Vec<BoardSummary>,
    /// Whether the session holds `forum.manage` (may create boards).
    pub(crate) can_manage: bool,
}

/// A request a screen is waiting on.
#[derive(Debug)]
pub(crate) enum Request {
    Boards,
    Threads {
        board: BoardId,
        page: u32,
    },
    Post {
        thread: ThreadId,
        index: u64,
    },
    Search {
        query: String,
        page: u32,
    },
    /// Open the editor, resuming a saved draft for the purpose's context.
    Compose {
        purpose: Purpose,
        quote: Option<Sanitized>,
    },
    SaveDraft(DraftSave),
    Submit {
        purpose: Purpose,
        body: Body,
    },
    Remove(PostId),
    CreateBoard {
        name: String,
        description: String,
        restricted: bool,
    },
    Allow {
        board: BoardId,
        handle: String,
    },
}

/// Why a request did not work, as words for the member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fail {
    pub(crate) text: String,
    /// The thing is not there (or not visible): the screen goes back to a list.
    pub(crate) gone: bool,
    /// The text the member wrote was kept as a draft.
    pub(crate) kept_draft: bool,
}

impl Fail {
    fn of(error: &ForumError) -> Self {
        let (text, gone) = match error {
            ForumError::NotFound => (strings::GONE.to_owned(), true),
            ForumError::NotPermitted(_) => (strings::DENIED.to_owned(), false),
            ForumError::NotSignedIn => (strings::NOT_SIGNED_IN.to_owned(), false),
            ForumError::Limit(refusal) => (format!("{refusal} {}", strings::ROLLING_HOUR), false),
            ForumError::Text(error) => (capitalize(&error.to_string()), false),
            ForumError::NameTaken => (strings::NAME_TAKEN.to_owned(), false),
            ForumError::UnknownMember => (strings::UNKNOWN_MEMBER.to_owned(), false),
            ForumError::Store(error) => {
                warn!(%error, "a forum request failed in the store");
                (strings::FAILED.to_owned(), false)
            }
        };
        Self {
            text,
            gone,
            kept_draft: false,
        }
    }

    fn failed() -> Self {
        Self {
            text: strings::FAILED.to_owned(),
            gone: false,
            kept_draft: false,
        }
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect::<String>() + "."
    })
}

/// The answer to a [`Request`].
#[derive(Debug)]
pub(crate) enum Response {
    Boards(Result<BoardList, Fail>),
    Threads(Result<ThreadPage, Fail>),
    Post(Result<ThreadView, Fail>),
    Search(Result<SearchPage, Fail>),
    Compose(Result<Box<Editor>, Fail>),
    Draft(Result<SaveOutcome, DraftError>),
    Posted(Result<Posted, Fail>),
    Removed(Result<Gone, Fail>),
    Created(Result<BoardId, Fail>),
    Allowed(Result<(), Fail>),
}

fn fail<T>(error: &ForumError) -> Result<T, Fail> {
    Err(Fail::of(error))
}

fn lift<T>(result: Result<T, ForumError>) -> Result<T, Fail> {
    result.map_err(|error| Fail::of(&error))
}

impl Request {
    /// The answer for this request when it fails outright (a timeout): a plain failure of the
    /// kind the screen is waiting for.
    fn failure(&self) -> Response {
        let failed = Fail::failed();
        match self {
            Self::Boards => Response::Boards(Err(failed)),
            Self::Threads { .. } => Response::Threads(Err(failed)),
            Self::Post { .. } => Response::Post(Err(failed)),
            Self::Search { .. } => Response::Search(Err(failed)),
            Self::Compose { .. } => Response::Compose(Err(failed)),
            Self::SaveDraft(_) => Response::Draft(Err(DraftError::Store(
                russh_bbs_core::store::StoreError::Corrupt("timed out".into()),
            ))),
            Self::Submit { .. } => Response::Posted(Err(failed)),
            Self::Remove(_) => Response::Removed(Err(failed)),
            Self::CreateBoard { .. } => Response::Created(Err(failed)),
            Self::Allow { .. } => Response::Allowed(Err(failed)),
        }
    }
}

/// Run `request` for the session holding `access`. A timeout is a plain failure: nothing is
/// claimed about a write beyond what the store reported.
pub(crate) async fn run<S>(store: &S, access: &Snapshot, request: Request) -> Response
where
    S: ForumStore + LimitStore + AccessStore + DraftStore,
{
    let on_timeout = request.failure();
    if let Ok(response) = timeout(REQUEST_TIMEOUT, execute(store, access, request)).await {
        response
    } else {
        warn!("a forum request timed out");
        on_timeout
    }
}

async fn execute<S>(store: &S, access: &Snapshot, request: Request) -> Response
where
    S: ForumStore + LimitStore + AccessStore + DraftStore,
{
    let now = SystemClock.now();
    match request {
        Request::Boards => Response::Boards(match forums::boards(store, access).await {
            Ok(boards) => Ok(BoardList {
                boards,
                can_manage: access.resolve(Permission::ForumManage, None).is_granted(),
            }),
            Err(error) => fail(&error),
        }),
        Request::Threads { board, page } => {
            Response::Threads(lift(forums::threads(store, access, board, page).await))
        }
        Request::Post { thread, index } => Response::Post(lift(
            forums::thread_post(store, access, thread, index).await,
        )),
        Request::Search { query, page } => {
            Response::Search(lift(forums::search(store, access, &query, page).await))
        }
        Request::Compose { purpose, quote } => {
            Response::Compose(compose(store, access, &purpose, quote).await)
        }
        Request::SaveDraft(save) => match access.account() {
            Some(account) => Response::Draft(save.persist(store, account, now).await),
            None => Response::Draft(Err(DraftError::Store(
                russh_bbs_core::store::StoreError::AccountNotFound,
            ))),
        },
        Request::Submit { purpose, body } => {
            Response::Posted(submit(store, access, now, &purpose, &body).await)
        }
        Request::Remove(post) => {
            Response::Removed(lift(forums::remove_post(store, access, post).await))
        }
        Request::CreateBoard {
            name,
            description,
            restricted,
        } => Response::Created(lift(
            forums::create_board(store, access, now, &name, &description, restricted).await,
        )),
        Request::Allow { board, handle } => Response::Allowed(lift(
            forums::allow_member(store, access, board, &handle).await,
        )),
    }
}

/// Check the member may write here, then open the editor (resuming a draft).
async fn compose<S>(
    store: &S,
    access: &Snapshot,
    purpose: &Purpose,
    quote: Option<Sanitized>,
) -> Result<Box<Editor>, Fail>
where
    S: ForumStore + DraftStore,
{
    let account = access
        .account()
        .ok_or_else(|| Fail::of(&ForumError::NotSignedIn))?;
    // The same visibility and permission the post itself will need, before the editor opens.
    match purpose {
        Purpose::Thread { board, .. } => {
            let page = forums::threads(store, access, *board, 0)
                .await
                .map_err(|e| Fail::of(&e))?;
            if !page.can_post {
                return fail(&ForumError::NotPermitted(Permission::ForumPost));
            }
        }
        Purpose::Reply { thread, .. } => {
            let view = forums::thread_post(store, access, *thread, 0)
                .await
                .map_err(|e| Fail::of(&e))?;
            if !view.can_reply {
                return fail(&ForumError::NotPermitted(Permission::ForumReply));
            }
        }
    }
    let mut request = EditorRequest::new(purpose.title()).limits(body_limits());
    if let Some(context) = purpose.context() {
        request = request.context(context);
    }
    if let Some(quote) = quote {
        request = request.initial(quote);
    }
    Editor::open_with_store(store, account, request)
        .await
        .map(Box::new)
        .map_err(|error| {
            warn!(%error, "the forum editor could not be opened");
            Fail::failed()
        })
}

/// Write the post, and only then drop the draft. A refused or failed write keeps the text as a
/// draft so nothing the member wrote is lost.
async fn submit<S>(
    store: &S,
    access: &Snapshot,
    now: russh_bbs_core::store::UnixSeconds,
    purpose: &Purpose,
    body: &Body,
) -> Result<Posted, Fail>
where
    S: ForumStore + LimitStore + DraftStore,
{
    let posted = match purpose {
        Purpose::Thread { board, subject, .. } => {
            forums::post_thread(store, access, now, *board, subject, body).await
        }
        Purpose::Reply { thread, .. } => {
            forums::post_reply(store, access, now, *thread, body).await
        }
    };
    let context = purpose.context();
    match (posted, access.account(), context) {
        (Ok(posted), Some(account), Some(context)) => {
            if let Err(error) = discard_draft(store, account, &context).await {
                warn!(%error, "a posted draft could not be discarded");
            }
            Ok(posted)
        }
        (Ok(posted), _, _) => Ok(posted),
        (Err(error), account, context) => {
            let mut failure = Fail::of(&error);
            if let (Some(account), Some(context)) = (account, context) {
                let kept = save_draft(store, account, &context, body.as_str(), body_limits(), now)
                    .await
                    .is_ok();
                failure.kept_draft = kept;
            }
            Err(failure)
        }
    }
}
