//! Requests the mail screens make of the store, and the executor that runs them.

use std::time::Duration;

use russh_bbs_core::access::{Permission, Snapshot};
use russh_bbs_core::drafts::{Body, DraftError, DraftStore, SaveOutcome};
use russh_bbs_core::limits::{Clock, LimitStore, SystemClock};
use russh_bbs_core::mail::{
    self, InboxPage, MailError, MailKeys, MailStore, MessageId, MessageView, ReportId, Sent,
    body_limits,
};
use russh_bbs_core::sanitize::{Sanitized, SingleLine};
use tokio::time::timeout;
use tracing::warn;

use super::strings;
use crate::ui::editor::{DraftSave, Editor, EditorRequest};

/// How long one mail request may take. A slow store must not hang the screen.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// What a member is writing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Purpose {
    /// A new message to a handle; the subject is already typed (cleaned again by core).
    New { to: String, subject: String },
    /// A reply to a message in the mailbox, to its sender.
    Reply { id: MessageId, subject: String },
}

impl Purpose {
    fn title(&self) -> Sanitized {
        let text = match self {
            Self::New { to, subject } => format!("To {to}: {subject}"),
            Self::Reply { subject, .. } => format!("Reply: {subject}"),
        };
        SingleLine::new(60).sanitize(&text)
    }
}

/// A request a screen is waiting on.
#[derive(Debug)]
pub(crate) enum Request {
    Inbox {
        page: u32,
    },
    Open(MessageId),
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
    Delete(MessageId),
    /// Send the staff the copy the member just agreed to send.
    Report {
        id: MessageId,
        subject: String,
        body: Body,
    },
}

/// Why a request did not work, as words for the member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fail {
    pub(crate) text: String,
    /// The message is not there: the screen goes back to the inbox.
    pub(crate) gone: bool,
}

impl Fail {
    fn of(error: &MailError) -> Self {
        let (text, gone) = match error {
            MailError::NotFound => (strings::GONE.to_owned(), true),
            MailError::NotPermitted(_) => (strings::DENIED.to_owned(), false),
            MailError::NotSignedIn => (strings::NOT_SIGNED_IN.to_owned(), false),
            MailError::Limit(refusal) => (refusal.lines().join(" "), false),
            MailError::Text(error) => (capitalize(&error.to_string()), false),
            MailError::UnknownRecipient => (strings::UNKNOWN_MEMBER.to_owned(), false),
            MailError::InboxFull => (strings::MAILBOX_FULL.to_owned(), false),
            MailError::Unreadable => (strings::UNREADABLE.to_owned(), false),
            MailError::AlreadyReported => (strings::ALREADY_REPORTED.to_owned(), false),
            MailError::TooManyReports => (strings::TOO_MANY_REPORTS.to_owned(), false),
            MailError::Random | MailError::KeyFile(_) | MailError::Seal(_) => {
                warn!("a mail request failed in sealing");
                (strings::FAILED.to_owned(), false)
            }
            MailError::Store(error) => {
                warn!(%error, "a mail request failed in the store");
                (strings::FAILED.to_owned(), false)
            }
        };
        Self { text, gone }
    }

    fn failed() -> Self {
        Self {
            text: strings::FAILED.to_owned(),
            gone: false,
        }
    }

    /// Mail is not set up on this board (the session has no key).
    pub(crate) fn unavailable() -> Self {
        Self {
            text: strings::UNAVAILABLE.to_owned(),
            gone: false,
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
    Inbox(Result<InboxPage, Fail>),
    Message(Result<MessageView, Fail>),
    Compose(Result<Box<Editor>, Fail>),
    Draft(Result<SaveOutcome, DraftError>),
    Sent(Result<Sent, Fail>),
    Deleted(Result<(), Fail>),
    Reported(Result<ReportId, Fail>),
}

fn lift<T>(result: Result<T, MailError>) -> Result<T, Fail> {
    result.map_err(|error| Fail::of(&error))
}

impl Request {
    /// The answer for this request when it fails outright (a timeout, or no key).
    pub(crate) fn failure(&self, failed: Fail) -> Response {
        match self {
            Self::Inbox { .. } => Response::Inbox(Err(failed)),
            Self::Open(_) => Response::Message(Err(failed)),
            Self::Compose { .. } => Response::Compose(Err(failed)),
            Self::SaveDraft(_) => Response::Draft(Err(DraftError::Store(
                russh_bbs_core::store::StoreError::Corrupt("timed out".into()),
            ))),
            Self::Submit { .. } => Response::Sent(Err(failed)),
            Self::Delete(_) => Response::Deleted(Err(failed)),
            Self::Report { .. } => Response::Reported(Err(failed)),
        }
    }
}

/// Run `request` for the session holding `access`. A timeout is a plain failure: nothing is
/// claimed about a write beyond what the store reported. `keys` is `None` when the board has no
/// mail key (a test board): every request then fails with the honest "not available".
pub(crate) async fn run<S>(
    store: &S,
    keys: Option<&MailKeys>,
    access: &Snapshot,
    request: Request,
) -> Response
where
    S: MailStore + LimitStore + DraftStore,
{
    let Some(keys) = keys else {
        // A draft save needs no key; everything else does.
        if let Request::SaveDraft(save) = request {
            return save_only(store, access, save).await;
        }
        return request.failure(Fail::unavailable());
    };
    let on_timeout = request.failure(Fail::failed());
    if let Ok(response) = timeout(REQUEST_TIMEOUT, execute(store, keys, access, request)).await {
        response
    } else {
        warn!("a mail request timed out");
        on_timeout
    }
}

async fn save_only<S: DraftStore>(store: &S, access: &Snapshot, save: DraftSave) -> Response {
    let now = SystemClock.now();
    match access.account() {
        Some(account) => Response::Draft(save.persist(store, account, now).await),
        None => Response::Draft(Err(DraftError::Store(
            russh_bbs_core::store::StoreError::AccountNotFound,
        ))),
    }
}

async fn execute<S>(store: &S, keys: &MailKeys, access: &Snapshot, request: Request) -> Response
where
    S: MailStore + LimitStore + DraftStore,
{
    let now = SystemClock.now();
    match request {
        Request::Inbox { page } => {
            Response::Inbox(lift(mail::inbox(store, keys, access, page).await))
        }
        Request::Open(id) => {
            Response::Message(lift(mail::read(store, keys, access, now, id).await))
        }
        Request::Compose { purpose, quote } => {
            Response::Compose(compose(store, access, &purpose, quote).await)
        }
        Request::SaveDraft(save) => save_only(store, access, save).await,
        Request::Submit { purpose, body } => {
            Response::Sent(submit(store, keys, access, now, &purpose, &body).await)
        }
        Request::Delete(id) => Response::Deleted(lift(mail::delete(store, access, id).await)),
        Request::Report { id, subject, body } => Response::Reported(lift(
            mail::report(store, access, now, id, &subject, &body).await,
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
    S: MailStore + DraftStore,
{
    let account = access
        .account()
        .ok_or_else(|| Fail::of(&MailError::NotSignedIn))?;
    if !access.resolve(Permission::MailSend, None).is_granted() {
        return Err(Fail::of(&MailError::NotPermitted(Permission::MailSend)));
    }
    if let Purpose::New { to, .. } = purpose {
        // A wrong handle is caught before anything is typed.
        lift(mail::check_recipient(store, access, to).await)?;
    }
    // No draft context: a mail draft would sit in the `draft` table readable, which D78 (5)
    // forbids, so the editor offers no save-draft for mail (D90). The screen keeps an unsent
    // message in memory until the member leaves mail.
    let mut request = EditorRequest::new(purpose.title()).limits(body_limits());
    if let Some(quote) = quote {
        request = request.initial(quote);
    }
    Editor::open_with_store(store, account, request)
        .await
        .map(Box::new)
        .map_err(|error| {
            warn!(%error, "the mail editor could not be opened");
            Fail::failed()
        })
}

/// Send the message.
async fn submit<S>(
    store: &S,
    keys: &MailKeys,
    access: &Snapshot,
    now: russh_bbs_core::store::UnixSeconds,
    purpose: &Purpose,
    body: &Body,
) -> Result<Sent, Fail>
where
    S: MailStore + LimitStore,
{
    let sent = match purpose {
        Purpose::New { to, subject } => {
            mail::send(store, keys, access, now, to, subject, body).await
        }
        Purpose::Reply { id, subject } => {
            mail::reply(store, keys, access, now, *id, subject, body).await
        }
    };
    lift(sent)
}
