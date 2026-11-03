//! Internal mail, stage 1 (M3-I, D78, D90): member to member, sealed at rest.
//!
//! * **At rest.** A subject and a body are each sealed with XChaCha20-Poly1305 under a
//!   server-held key ([`MailKeys`]); the associated data names the field, the message id, the
//!   recipient's account id and the key id ([`seal`]). The store holds sealed bytes and
//!   metadata (sender, recipient, time, opened); it never holds a key or a plaintext
//!   subject or body. The key file is the server's (`mail::key_file`), next to the host key.
//! * **No staff reader.** No function, permission, screen, export or audit field returns
//!   another member's subject or text. Staff see only reports: the copy a recipient chose to
//!   send ([`service::report`]).
//! * **Honest wording.** Mail is encrypted at rest and **not** end-to-end encrypted: the
//!   server holds the key and decrypts to show mail. The one explanation lives in the welcome
//!   message and the help screen (D78 (7)); mail screens carry no notice, and nothing here
//!   calls mail private or secure.
//! * **Limits.** `mail_per_day` after the permission, an inbox cap of [`INBOX_CAP`] messages,
//!   bounded subject and body ([`text`]).
//!
//! Rules live here; `russh-bbs-store` persists sealed rows and `russh-bbs-server::mail` draws
//! the screens.

mod ids;
mod key;
pub mod seal;
pub mod service;
mod store;
mod text;

pub use ids::{MessageId, ReportId};
pub use key::{FILE_BYTES, KEY_BYTES, KeyId, MailKey, MailKeys};
pub use seal::{Field, SealError};
pub use service::{
    INBOX_CAP, InboxEntry, InboxPage, KeyCheckError, MAX_OPEN_REPORTS, MessageView, PAGE_SIZE,
    ReportView, Sent, check_recipient, close_report, delete, inbox, read, reply, report, reports,
    send, verify_keys,
};
pub use store::{MailRow, MailStore, MailWrite, NewMail, NewReport, ReportRow, ReportWrite};
pub use text::{
    BODY_MAX_CHARS, BODY_MAX_LINES, MailTextError, SUBJECT_MAX_CHARS, body_limits, check_body,
    prepare_subject,
};

use crate::access::Permission;
use crate::limits::LimitRefusal;
use crate::store::StoreError;

/// Why a mail action did not happen.
#[derive(Debug, thiserror::Error)]
pub enum MailError {
    /// No such message in your mailbox (the same answer for one that does not exist and one in
    /// someone else's mailbox).
    #[error("not found")]
    NotFound,
    /// The session may not do this.
    #[error("you may not do that")]
    NotPermitted(Permission),
    /// The session is not a signed-in account.
    #[error("not signed in")]
    NotSignedIn,
    /// A limit is used up.
    #[error("{0}")]
    Limit(LimitRefusal),
    /// The typed text was refused.
    #[error(transparent)]
    Text(#[from] MailTextError),
    /// No active member has that handle (one answer for unknown and not active).
    #[error("no such member")]
    UnknownRecipient,
    /// The recipient's mailbox is full.
    #[error("that mailbox is full")]
    InboxFull,
    /// The message could not be opened (damaged, moved, or sealed under a key not held).
    #[error("this message cannot be opened")]
    Unreadable,
    /// This message was already reported by this member.
    #[error("already reported")]
    AlreadyReported,
    /// The member has too many open reports.
    #[error("too many open reports")]
    TooManyReports,
    /// The operating system's random source failed.
    #[error("no random source")]
    Random,
    /// A key file is not usable. The text never holds key bytes.
    #[error("{0}")]
    KeyFile(&'static str),
    /// Sealing failed.
    #[error(transparent)]
    Seal(#[from] SealError),
    /// Storage failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

#[cfg(test)]
mod tests;
