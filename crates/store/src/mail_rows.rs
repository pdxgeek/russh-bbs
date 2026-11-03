//! Backend-independent mapping for the mail tables: what a stored row means, written once (D24).
//! Each backend runs its own SQL and hands the values here. Subject and body arrive as the
//! sealed bytes they are stored as; nothing here can open them.

use russh_bbs_core::mail::{KeyId, MailRow, MessageId, ReportId, ReportRow};
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

pub(crate) fn key_id(value: i64) -> Result<KeyId, StoreError> {
    u32::try_from(value)
        .ok()
        .filter(|v| *v > 0)
        .map(KeyId::from_raw)
        .ok_or_else(|| StoreError::Corrupt("a mail key id is out of range".into()))
}

pub(crate) struct RawMail {
    pub(crate) id: i64,
    pub(crate) recipient_id: i64,
    pub(crate) sender_id: Option<i64>,
    pub(crate) sender: Option<String>,
    pub(crate) sent_at: i64,
    pub(crate) read_at: Option<i64>,
    pub(crate) key_id: i64,
    pub(crate) subject: Vec<u8>,
    pub(crate) body: Vec<u8>,
}

pub(crate) fn mail(raw: RawMail) -> Result<MailRow, StoreError> {
    Ok(MailRow {
        id: MessageId::from_raw(raw.id),
        recipient: AccountId::from_raw(raw.recipient_id),
        sender_id: raw.sender_id.map(AccountId::from_raw),
        sender: raw.sender,
        sent_at: UnixSeconds::from_raw(raw.sent_at),
        read: raw.read_at.is_some(),
        key_id: key_id(raw.key_id)?,
        subject: raw.subject,
        body: raw.body,
    })
}

pub(crate) struct RawReport {
    pub(crate) id: i64,
    pub(crate) reporter: Option<String>,
    pub(crate) sender: Option<String>,
    pub(crate) message_id: i64,
    pub(crate) reported_at: i64,
    pub(crate) copy_subject: String,
    pub(crate) copy_body: String,
    pub(crate) closed_at: Option<i64>,
}

pub(crate) fn report(raw: RawReport) -> ReportRow {
    ReportRow {
        id: ReportId::from_raw(raw.id),
        reporter: raw.reporter,
        sender: raw.sender,
        message: MessageId::from_raw(raw.message_id),
        at: UnixSeconds::from_raw(raw.reported_at),
        copy_subject: raw.copy_subject,
        copy_body: raw.copy_body,
        open: raw.closed_at.is_none(),
    }
}

pub(crate) fn count(value: i64, what: &str) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Corrupt(format!("{what} is negative")))
}
