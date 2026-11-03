//! The mail screens' words. Fixed chrome text, not member text. **No privacy notice here**:
//! the one D78 (7) explanation is in the welcome message and the help screen only, and nothing
//! on a mail screen says "private", "secure" or "encrypted".

pub(super) const TITLE_INBOX: &str = "Mail";
pub(super) const TITLE_TO: &str = "New message";
pub(super) const TITLE_SUBJECT: &str = "New message";

pub(super) const NO_MAIL: &str = "Your mailbox is empty.";
pub(super) const CANNOT_OPEN: &str = "(this message cannot be opened)";
pub(super) const GONE_MEMBER: &str = "(gone)";

pub(super) const HELP_INBOX: &str =
    "Up/Down + Enter read   1-9 jump   c write   d delete   n/b next/back page   Esc back";
pub(super) const HELP_INPUT: &str = "Enter to continue   Esc to cancel";
pub(super) const HELP_READER: &str =
    "Space next page   b page back   r reply   c reply with quote   d delete   x report   Esc back";
pub(super) const HELP_READER_NO_REPLY: &str =
    "Space next page   b page back   d delete   x report   Esc back";
pub(super) const CONFIRM_DELETE: &str =
    "Delete this message? y to delete, any other key to keep it.";
pub(super) const CONFIRM_REPORT: &str = "Send the staff a copy of this message's subject and text, and who it is from? \
     The message stays in your mailbox. y to send the copy, any other key to cancel.";

pub(super) const PROMPT_TO: &str = "Handle of the member to write to:";
pub(super) const PROMPT_SUBJECT: &str = "Subject:";

pub(super) const OK_SENT: &str = "Sent.";
pub(super) const OK_DELETED: &str = "Deleted.";
pub(super) const OK_REPORTED: &str = "The staff have a copy of that message.";
pub(super) const OK_DRAFT: &str = "Draft saved.";
pub(super) const KEPT_UNSENT: &str =
    "Your text is kept until you leave mail; write the message again to continue it.";
pub(super) const FAILED: &str = "That did not work. Try again in a moment.";
pub(super) const GONE: &str = "That message is not available.";
pub(super) const DENIED: &str = "You may not do that.";
pub(super) const NOT_SIGNED_IN: &str = "You are not signed in.";
pub(super) const UNKNOWN_MEMBER: &str = "No active member has that handle.";
pub(super) const MAILBOX_FULL: &str = "That mailbox is full. Nothing was sent.";
pub(super) const UNREADABLE: &str = "This message cannot be opened. You can delete it with d.";
pub(super) const ALREADY_REPORTED: &str = "You already reported this message.";
pub(super) const TOO_MANY_REPORTS: &str = "You have too many reports waiting for staff.";
pub(super) const UNAVAILABLE: &str = "Mail is not available on this board right now.";
