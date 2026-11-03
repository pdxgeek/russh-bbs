//! The storage trait for mail: persistence of **sealed** rows only. The store never sees a key
//! or a plaintext subject or body; rules (permissions, limits, sealing, the inbox cap) are in
//! [`super::service`].

use std::future::Future;

use super::ids::{MessageId, ReportId};
use super::key::KeyId;
use crate::store::{AccountId, StoreError, UnixSeconds};

/// A message as stored. Subject and body are sealed bytes; the rest is metadata the server
/// can see (who sent it, when, whether it was opened).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailRow {
    /// The message.
    pub id: MessageId,
    /// Whose mailbox it is in.
    pub recipient: AccountId,
    /// Who sent it, or `None` when that account is gone.
    pub sender_id: Option<AccountId>,
    /// The sender's handle, or `None` when the account is gone.
    pub sender: Option<String>,
    /// When it was sent.
    pub sent_at: UnixSeconds,
    /// Whether the recipient has opened it.
    pub read: bool,
    /// The key that sealed it.
    pub key_id: KeyId,
    /// The sealed subject.
    pub subject: Vec<u8>,
    /// The sealed body.
    pub body: Vec<u8>,
}

/// A message to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMail {
    /// The message's id, chosen by core (it is part of what the bytes are sealed against).
    pub id: MessageId,
    /// Whose mailbox.
    pub recipient: AccountId,
    /// Who sent it.
    pub sender: AccountId,
    /// When.
    pub sent_at: UnixSeconds,
    /// The key that sealed it.
    pub key_id: KeyId,
    /// The sealed subject.
    pub subject: Vec<u8>,
    /// The sealed body.
    pub body: Vec<u8>,
}

/// What a mail write did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailWrite {
    /// Stored.
    Written,
    /// The mailbox already holds the most messages it may; nothing was written.
    Full,
    /// A message with that id exists (a one-in-2^62 collision): choose another id.
    IdTaken,
    /// The recipient is gone or not an active member.
    RecipientGone,
}

/// A report to store: what the reporter chose to send to staff, labelled as a copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewReport {
    /// Who reported (the recipient).
    pub reporter: AccountId,
    /// The message's sender, from the stored row (not from what the reporter typed).
    pub sender: Option<AccountId>,
    /// The message reported.
    pub message: MessageId,
    /// When.
    pub at: UnixSeconds,
    /// The copy of the subject the reporter sent.
    pub copy_subject: String,
    /// The copy of the body the reporter sent.
    pub copy_body: String,
}

/// What a report write did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportWrite {
    /// Stored (and audited in the same transaction).
    Written(ReportId),
    /// This reporter already reported this message.
    Duplicate,
    /// The reporter has too many open reports.
    TooMany,
}

/// A report as staff see it: a copy, with ids and handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRow {
    /// The report.
    pub id: ReportId,
    /// Who sent it (their handle), or `None` when the account is gone.
    pub reporter: Option<String>,
    /// Who sent the reported message (their handle), or `None` when unknown or gone.
    pub sender: Option<String>,
    /// The message reported.
    pub message: MessageId,
    /// When it was reported.
    pub at: UnixSeconds,
    /// The copy of the subject.
    pub copy_subject: String,
    /// The copy of the body.
    pub copy_body: String,
    /// Whether staff have not yet closed it.
    pub open: bool,
}

/// Sealed mail and reports.
pub trait MailStore: Send + Sync {
    /// The active member whose handle is `handle`, ignoring case.
    fn mail_recipient(
        &self,
        handle: &str,
    ) -> impl Future<Output = Result<Option<AccountId>, StoreError>> + Send;

    /// Whether `account` is an active member.
    fn mail_account_active(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// A mailbox's messages, newest first. Only `recipient`'s rows are ever returned.
    fn inbox(
        &self,
        recipient: AccountId,
        limit: u32,
        offset: u32,
    ) -> impl Future<Output = Result<Vec<MailRow>, StoreError>> + Send;

    /// How many messages are in a mailbox, and how many of them are unopened.
    fn inbox_counts(
        &self,
        recipient: AccountId,
    ) -> impl Future<Output = Result<(u64, u64), StoreError>> + Send;

    /// One message **in `recipient`'s mailbox**, or `None` (the same answer for a message that
    /// does not exist and one in another mailbox).
    fn message(
        &self,
        recipient: AccountId,
        id: MessageId,
    ) -> impl Future<Output = Result<Option<MailRow>, StoreError>> + Send;

    /// Store a message unless the mailbox already holds `max_inbox` (counted in the same
    /// transaction, so two sends at once cannot both pass).
    fn add_mail(
        &self,
        new: &NewMail,
        max_inbox: u64,
    ) -> impl Future<Output = Result<MailWrite, StoreError>> + Send;

    /// Mark a message opened (the first time only).
    fn mark_read(
        &self,
        recipient: AccountId,
        id: MessageId,
        now: UnixSeconds,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Delete a message from `recipient`'s mailbox. `false` when there was none. Not audited:
    /// the log does not record who had mail or what they did with it.
    fn delete_mail(
        &self,
        recipient: AccountId,
        id: MessageId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// The distinct key ids that stored messages are sealed under (boot check, and how a key
    /// rotation knows it is finished).
    fn key_ids_in_use(&self) -> impl Future<Output = Result<Vec<KeyId>, StoreError>> + Send;

    /// A few stored rows, for the boot check that the key opens existing mail.
    fn sample_mail(
        &self,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<MailRow>, StoreError>> + Send;

    /// Store a report unless the reporter already reported the message or has `max_open` open
    /// reports (both counted in the transaction). Audited in the same transaction
    /// (`mail.reported`, ids only).
    fn add_report(
        &self,
        new: &NewReport,
        max_open: u64,
    ) -> impl Future<Output = Result<ReportWrite, StoreError>> + Send;

    /// Reports, open first then newest first.
    fn reports(
        &self,
        limit: u32,
        offset: u32,
    ) -> impl Future<Output = Result<Vec<ReportRow>, StoreError>> + Send;

    /// Close a report. `false` when there was no open one. Audited in the same transaction
    /// (`mail.report_closed`).
    fn close_report(
        &self,
        id: ReportId,
        actor: AccountId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;
}

/// The empty directory has no mail and no accounts: reads find nothing, writes are refused.
impl MailStore for crate::access::NoMembers {
    fn mail_recipient(
        &self,
        _handle: &str,
    ) -> impl Future<Output = Result<Option<AccountId>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn mail_account_active(
        &self,
        _account: AccountId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(false))
    }

    fn inbox(
        &self,
        _recipient: AccountId,
        _limit: u32,
        _offset: u32,
    ) -> impl Future<Output = Result<Vec<MailRow>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn inbox_counts(
        &self,
        _recipient: AccountId,
    ) -> impl Future<Output = Result<(u64, u64), StoreError>> + Send {
        std::future::ready(Ok((0, 0)))
    }

    fn message(
        &self,
        _recipient: AccountId,
        _id: MessageId,
    ) -> impl Future<Output = Result<Option<MailRow>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn add_mail(
        &self,
        _new: &NewMail,
        _max_inbox: u64,
    ) -> impl Future<Output = Result<MailWrite, StoreError>> + Send {
        std::future::ready(Ok(MailWrite::RecipientGone))
    }

    fn mark_read(
        &self,
        _recipient: AccountId,
        _id: MessageId,
        _now: UnixSeconds,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Ok(()))
    }

    fn delete_mail(
        &self,
        _recipient: AccountId,
        _id: MessageId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(false))
    }

    fn key_ids_in_use(&self) -> impl Future<Output = Result<Vec<KeyId>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn sample_mail(
        &self,
        _limit: u32,
    ) -> impl Future<Output = Result<Vec<MailRow>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn add_report(
        &self,
        _new: &NewReport,
        _max_open: u64,
    ) -> impl Future<Output = Result<ReportWrite, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn reports(
        &self,
        _limit: u32,
        _offset: u32,
    ) -> impl Future<Output = Result<Vec<ReportRow>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn close_report(
        &self,
        _id: ReportId,
        _actor: AccountId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(false))
    }
}
