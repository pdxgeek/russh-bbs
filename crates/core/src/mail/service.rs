//! What a signed-in session can do with mail. Every function takes the session's [`Snapshot`]
//! (loaded for this action) and checks it first; then text; then the limit; then the write.
//!
//! **Sealing happens here.** Text is sanitized (the shared sanitizer, through [`super::text`]),
//! sealed with the active key ([`super::seal`]) and only then handed to the store, and a row read
//! back is opened and sanitized again before it is shown. A row that does not open (damaged,
//! moved to another mailbox, sealed under a key the board no longer holds) is an
//! unreadable entry the member can delete; it never takes the screen down.
//!
//! **No staff reader.** Nothing here returns another member's mail. The only staff-facing
//! functions are [`reports`] and [`close_report`], and they return what a recipient chose to
//! send: a copy, labelled as one.

use crate::access::{Permission, Snapshot};
use crate::drafts::{Body, QuoteLimits, quote};
use crate::limits::{Day, LimitKind, LimitRefusal, LimitStore, UsageOutcome, load_effective};
use crate::sanitize::{MultiLine, Sanitized, SingleLine};
use crate::store::{AccountId, StoreError, UnixSeconds};

use super::MailError;
use super::ids::{MessageId, ReportId};
use super::key::{KeyId, MailKeys};
use super::seal::{Field, SealError, open, seal};
use super::store::{MailRow, MailStore, MailWrite, NewMail, NewReport, ReportRow, ReportWrite};
use super::text::{BODY_MAX_CHARS, SUBJECT_MAX_CHARS, check_body, prepare_subject};

/// Messages on an inbox page, and reports on a report page.
pub const PAGE_SIZE: u32 = 12;

/// The most messages one mailbox holds. At the cap a new message to it is refused with
/// [`MailError::InboxFull`] (the sender is told; nothing is dropped and no old mail is deleted
/// for the recipient): the recipient deletes mail to make room.
pub const INBOX_CAP: u64 = 200;

/// The most open reports one member may have at a time.
pub const MAX_OPEN_REPORTS: u64 = 20;

/// How many times a send picks a new id after a collision.
const ID_ATTEMPTS: u32 = 4;

fn account_of(snapshot: &Snapshot) -> Result<AccountId, MailError> {
    snapshot.account().ok_or(MailError::NotSignedIn)
}

fn require(snapshot: &Snapshot, permission: Permission) -> Result<AccountId, MailError> {
    let account = account_of(snapshot)?;
    snapshot
        .require(permission, None)
        .map_err(|_| MailError::NotPermitted(permission))?;
    Ok(account)
}

fn clean(text: &str, max: usize) -> String {
    SingleLine::new(max).sanitize(text).into_string()
}

fn handle(text: Option<&String>) -> Option<String> {
    text.map(|h| clean(h, 64))
}

/// One line of the inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxEntry {
    /// The message.
    pub id: MessageId,
    /// Who sent it, or `None` when the account is gone.
    pub from: Option<String>,
    /// When it was sent.
    pub at: UnixSeconds,
    /// Whether it has not been opened.
    pub unread: bool,
    /// The subject, or `None` when the message cannot be opened (an error entry).
    pub subject: Option<String>,
}

/// One page of the inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxPage {
    /// The messages, newest first.
    pub entries: Vec<InboxEntry>,
    /// This page, from zero.
    pub page: u32,
    /// Whether a later page exists.
    pub more: bool,
    /// Messages in the mailbox.
    pub total: u64,
    /// Unopened messages in the mailbox.
    pub unread: u64,
    /// Whether the session may write mail.
    pub can_send: bool,
}

/// A message opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageView {
    /// The message.
    pub id: MessageId,
    /// Who sent it, or `None` when the account is gone.
    pub from: Option<String>,
    /// When it was sent.
    pub at: UnixSeconds,
    /// The subject, cleaned again.
    pub subject: String,
    /// The text, cleaned again.
    pub body: Sanitized,
    /// Whether the session may reply (it may send mail and the sender still has an account).
    pub can_reply: bool,
    /// Whether the session may report it.
    pub can_report: bool,
}

impl MessageView {
    /// The message quoted for a reply: each line prefixed, at most two deep ([`quote`]). A
    /// quote mark is plain text anyone can type (D80).
    #[must_use]
    pub fn quoted(&self) -> Sanitized {
        let attribution = self
            .from
            .as_deref()
            .map(|a| SingleLine::new(64).sanitize(a));
        quote(attribution.as_ref(), &self.body, QuoteLimits::default())
    }

    /// The subject for a reply: `Re: ` added once, cut to the subject limit.
    #[must_use]
    pub fn reply_subject(&self) -> String {
        let subject = if self.subject.starts_with("Re: ") {
            self.subject.clone()
        } else {
            format!("Re: {}", self.subject)
        };
        clean(&subject, SUBJECT_MAX_CHARS)
    }
}

/// A message that was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sent {
    /// Its id in the recipient's mailbox.
    pub id: MessageId,
}

fn open_subject(keys: &MailKeys, row: &MailRow) -> Result<String, SealError> {
    open(
        keys,
        row.key_id,
        Field::Subject,
        row.id,
        row.recipient,
        &row.subject,
    )
}

fn open_body(keys: &MailKeys, row: &MailRow) -> Result<String, SealError> {
    open(
        keys,
        row.key_id,
        Field::Body,
        row.id,
        row.recipient,
        &row.body,
    )
}

/// Your mailbox, one page.
///
/// # Errors
/// [`MailError`]: not permitted or storage. A message that does not open is an entry with no
/// subject, not an error.
pub async fn inbox<S: MailStore>(
    store: &S,
    keys: &MailKeys,
    snapshot: &Snapshot,
    page: u32,
) -> Result<InboxPage, MailError> {
    let account = require(snapshot, Permission::MailRead)?;
    let offset = page.saturating_mul(PAGE_SIZE);
    let mut rows = store.inbox(account, PAGE_SIZE + 1, offset).await?;
    let more = rows.len() > PAGE_SIZE as usize;
    rows.truncate(PAGE_SIZE as usize);
    let (total, unread) = store.inbox_counts(account).await?;
    let entries = rows
        .iter()
        .map(|row| InboxEntry {
            id: row.id,
            from: handle(row.sender.as_ref()),
            at: row.sent_at,
            unread: !row.read,
            subject: open_subject(keys, row)
                .ok()
                .map(|s| clean(&s, SUBJECT_MAX_CHARS)),
        })
        .collect();
    Ok(InboxPage {
        entries,
        page,
        more,
        total,
        unread,
        can_send: snapshot.resolve(Permission::MailSend, None).is_granted(),
    })
}

/// Open one of your messages and mark it read.
///
/// # Errors
/// [`MailError`]: not permitted, [`MailError::NotFound`] (no such message **in your mailbox**),
/// [`MailError::Unreadable`] (it does not open) or storage.
pub async fn read<S: MailStore>(
    store: &S,
    keys: &MailKeys,
    snapshot: &Snapshot,
    now: UnixSeconds,
    id: MessageId,
) -> Result<MessageView, MailError> {
    let account = require(snapshot, Permission::MailRead)?;
    let row = store
        .message(account, id)
        .await?
        .ok_or(MailError::NotFound)?;
    let subject = open_subject(keys, &row).map_err(|_| MailError::Unreadable)?;
    let body = open_body(keys, &row).map_err(|_| MailError::Unreadable)?;
    if !row.read {
        store.mark_read(account, id, now).await?;
    }
    Ok(MessageView {
        id,
        from: handle(row.sender.as_ref()),
        at: row.sent_at,
        subject: clean(&subject, SUBJECT_MAX_CHARS),
        body: MultiLine::new(BODY_MAX_CHARS).sanitize(&body),
        can_reply: row.sender_id.is_some()
            && snapshot.resolve(Permission::MailSend, None).is_granted(),
        can_report: snapshot.resolve(Permission::MailReport, None).is_granted(),
    })
}

/// Delete one of your messages. Nothing is audited.
///
/// # Errors
/// [`MailError`]: not permitted, [`MailError::NotFound`] or storage.
pub async fn delete<S: MailStore>(
    store: &S,
    snapshot: &Snapshot,
    id: MessageId,
) -> Result<(), MailError> {
    let account = require(snapshot, Permission::MailRead)?;
    if store.delete_mail(account, id).await? {
        Ok(())
    } else {
        Err(MailError::NotFound)
    }
}

/// Send a message to the member with `to` as their handle.
///
/// Unknown handles and members who are not active get **one** answer,
/// [`MailError::UnknownRecipient`] (D60: every member can see the members list, so the answer
/// tells a sender nothing the list does not). The permission is checked first, then the text,
/// then the recipient, then `mail_per_day`, then the write.
///
/// # Errors
/// [`MailError`]: not permitted, refused text, unknown recipient, a full inbox, the daily
/// limit, or storage.
pub async fn send<S: MailStore + LimitStore>(
    store: &S,
    keys: &MailKeys,
    snapshot: &Snapshot,
    now: UnixSeconds,
    to: &str,
    subject: &str,
    body: &Body,
) -> Result<Sent, MailError> {
    let sender = require(snapshot, Permission::MailSend)?;
    let subject = prepare_subject(subject)?;
    let body = check_body(body)?;
    let to = to.trim();
    if to.is_empty() || to.chars().count() > 64 {
        return Err(MailError::UnknownRecipient);
    }
    let recipient = store
        .mail_recipient(to)
        .await?
        .ok_or(MailError::UnknownRecipient)?;
    deliver(
        store, keys, snapshot, now, sender, recipient, &subject, &body,
    )
    .await
}

/// Whether a message to `to` could be addressed now: the session may send mail and `to` is an
/// active member. Used before the editor opens, so a wrong handle is caught before anything
/// is typed. The same one answer for unknown and not active as [`send`].
///
/// # Errors
/// [`MailError::NotPermitted`], [`MailError::UnknownRecipient`] or storage.
pub async fn check_recipient<S: MailStore>(
    store: &S,
    snapshot: &Snapshot,
    to: &str,
) -> Result<(), MailError> {
    require(snapshot, Permission::MailSend)?;
    let to = to.trim();
    if to.is_empty() || to.chars().count() > 64 {
        return Err(MailError::UnknownRecipient);
    }
    store
        .mail_recipient(to)
        .await?
        .map(|_| ())
        .ok_or(MailError::UnknownRecipient)
}

/// Reply to a message in your mailbox: to its sender, by account (not by handle, so a handle
/// reused later cannot receive it).
///
/// # Errors
/// As [`send`], and [`MailError::NotFound`] when the message is not in your mailbox, or its
/// sender is gone or no longer active ([`MailError::UnknownRecipient`]).
pub async fn reply<S: MailStore + LimitStore>(
    store: &S,
    keys: &MailKeys,
    snapshot: &Snapshot,
    now: UnixSeconds,
    original: MessageId,
    subject: &str,
    body: &Body,
) -> Result<Sent, MailError> {
    let sender = require(snapshot, Permission::MailSend)?;
    let subject = prepare_subject(subject)?;
    let body = check_body(body)?;
    let row = store
        .message(sender, original)
        .await?
        .ok_or(MailError::NotFound)?;
    let recipient = row.sender_id.ok_or(MailError::UnknownRecipient)?;
    if !store.mail_account_active(recipient).await? {
        return Err(MailError::UnknownRecipient);
    }
    deliver(
        store, keys, snapshot, now, sender, recipient, &subject, &body,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn deliver<S: MailStore + LimitStore>(
    store: &S,
    keys: &MailKeys,
    snapshot: &Snapshot,
    now: UnixSeconds,
    sender: AccountId,
    recipient: AccountId,
    subject: &Sanitized,
    body: &Body,
) -> Result<Sent, MailError> {
    // A cheap look first, so a refusal for a full mailbox does not use up a count. The write
    // checks again inside its transaction; that is the one that decides.
    let (held, _) = store.inbox_counts(recipient).await?;
    if held >= INBOX_CAP {
        return Err(MailError::InboxFull);
    }
    let limits = load_effective(store, sender, snapshot).await?;
    let cap = limits.get(LimitKind::MailPerDay).cap();
    let day = Day::of(now);
    if let UsageOutcome::Full(_) = store
        .add_usage(sender, LimitKind::MailPerDay, day, 1, cap)
        .await?
    {
        let resets = day.end().as_raw().saturating_sub(now.as_raw());
        return Err(MailError::Limit(LimitRefusal {
            kind: LimitKind::MailPerDay,
            limit: cap.unwrap_or(0),
            resets_in: Some(u64::try_from(resets).unwrap_or(0)),
        }));
    }
    for _ in 0..ID_ATTEMPTS {
        let id = MessageId::random()?;
        let (key_id, subject_sealed) = seal(keys, Field::Subject, id, recipient, subject.as_str())?;
        let (_, body_sealed) = seal(keys, Field::Body, id, recipient, body.as_str())?;
        let new = NewMail {
            id,
            recipient,
            sender,
            sent_at: now,
            key_id,
            subject: subject_sealed,
            body: body_sealed,
        };
        match store.add_mail(&new, INBOX_CAP).await? {
            MailWrite::Written => return Ok(Sent { id }),
            MailWrite::Full => return Err(MailError::InboxFull),
            MailWrite::RecipientGone => return Err(MailError::UnknownRecipient),
            MailWrite::IdTaken => {}
        }
    }
    Err(MailError::Store(StoreError::Corrupt(
        "no free message id".into(),
    )))
}

/// Report one of your messages to staff, sending a copy of the text you give.
///
/// The recipient decides what goes: `copy_subject` and `copy_body` are what the reporter
/// submits (the screen shows them and asks first). They are sanitized like any mail text and
/// stored in clear as a report, a **copy that staff cannot verify against the sealed original**.
/// The sender is taken from the stored message. Audited by ids only (`mail.reported`).
///
/// # Errors
/// [`MailError`]: not permitted, [`MailError::NotFound`], refused text,
/// [`MailError::AlreadyReported`], [`MailError::TooManyReports`] or storage.
pub async fn report<S: MailStore>(
    store: &S,
    snapshot: &Snapshot,
    now: UnixSeconds,
    id: MessageId,
    copy_subject: &str,
    copy_body: &Body,
) -> Result<ReportId, MailError> {
    let reporter = require(snapshot, Permission::MailReport)?;
    let row = store
        .message(reporter, id)
        .await?
        .ok_or(MailError::NotFound)?;
    let copy_subject = prepare_subject(copy_subject)?;
    let copy_body = check_body(copy_body)?;
    let new = NewReport {
        reporter,
        sender: row.sender_id,
        message: id,
        at: now,
        copy_subject: copy_subject.into_string(),
        copy_body: copy_body.as_str().to_owned(),
    };
    match store.add_report(&new, MAX_OPEN_REPORTS).await? {
        ReportWrite::Written(id) => Ok(id),
        ReportWrite::Duplicate => Err(MailError::AlreadyReported),
        ReportWrite::TooMany => Err(MailError::TooManyReports),
    }
}

/// A report for staff, cleaned again. A copy: it says what the reporter sent, not what the
/// sealed message holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportView {
    /// The report.
    pub id: ReportId,
    /// Who reported, or `None` when the account is gone.
    pub reporter: Option<String>,
    /// Who sent the reported message, or `None` when unknown or gone.
    pub sender: Option<String>,
    /// The message reported.
    pub message: MessageId,
    /// When it was reported.
    pub at: UnixSeconds,
    /// The reporter's copy of the subject.
    pub copy_subject: String,
    /// The reporter's copy of the text.
    pub copy_body: Sanitized,
    /// Whether it is still open.
    pub open: bool,
}

impl ReportView {
    fn of(row: &ReportRow) -> Self {
        Self {
            id: row.id,
            reporter: handle(row.reporter.as_ref()),
            sender: handle(row.sender.as_ref()),
            message: row.message,
            at: row.at,
            copy_subject: clean(&row.copy_subject, SUBJECT_MAX_CHARS),
            copy_body: MultiLine::new(BODY_MAX_CHARS).sanitize(&row.copy_body),
            open: row.open,
        }
    }
}

/// Reports sent to staff, open first (staff: `mail.moderate`). There is no function that reads
/// a member's mail; this lists copies members chose to send.
///
/// # Errors
/// [`MailError`]: not permitted or storage.
pub async fn reports<S: MailStore>(
    store: &S,
    snapshot: &Snapshot,
    page: u32,
) -> Result<Vec<ReportView>, MailError> {
    require(snapshot, Permission::MailModerate)?;
    let rows = store
        .reports(PAGE_SIZE, page.saturating_mul(PAGE_SIZE))
        .await?;
    Ok(rows.iter().map(ReportView::of).collect())
}

/// Close a report (staff: `mail.moderate`), audited by id.
///
/// # Errors
/// [`MailError`]: not permitted, [`MailError::NotFound`] or storage.
pub async fn close_report<S: MailStore>(
    store: &S,
    snapshot: &Snapshot,
    id: ReportId,
) -> Result<(), MailError> {
    let actor = require(snapshot, Permission::MailModerate)?;
    if store.close_report(id, actor).await? {
        Ok(())
    } else {
        Err(MailError::NotFound)
    }
}

/// Why the board's keys do not match its stored mail. Start-up stops on any of these.
#[derive(Debug, thiserror::Error)]
pub enum KeyCheckError {
    /// Stored messages are sealed under keys the board does not hold.
    #[error("stored mail is sealed under key ids the key file does not hold: {0:?}")]
    Missing(Vec<u32>),
    /// The key file's key does not open the stored mail it claims to (a wrong key file).
    #[error(
        "the mail key does not open the stored mail; this is not the key file it was sealed with"
    )]
    DoesNotOpen,
    /// Storage failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// How many stored rows the boot check tries to open.
const SAMPLE: u32 = 5;

/// Check at start-up that `keys` can open the mail already stored: every key id in use is held,
/// and at least one of a few sampled rows opens (one damaged row does not stop boot; a wrong
/// key file fails them all).
///
/// # Errors
/// [`KeyCheckError`].
pub async fn verify_keys<S: MailStore>(store: &S, keys: &MailKeys) -> Result<(), KeyCheckError> {
    let in_use = store.key_ids_in_use().await?;
    let missing: Vec<u32> = in_use
        .iter()
        .filter(|id: &&KeyId| !keys.holds(**id))
        .map(|id| id.as_raw())
        .collect();
    if !missing.is_empty() {
        return Err(KeyCheckError::Missing(missing));
    }
    let sample = store.sample_mail(SAMPLE).await?;
    if !sample.is_empty() && sample.iter().all(|row| open_subject(keys, row).is_err()) {
        return Err(KeyCheckError::DoesNotOpen);
    }
    Ok(())
}
