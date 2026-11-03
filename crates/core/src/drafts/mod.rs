//! Saved drafts and quoting for the full-screen message editor (M3-B, D79).
//!
//! The editor widget (`russh-bbs-server::ui::editor`) is reusable by the forum, mail and poll
//! slices, so what is *not* about drawing lives here, once:
//!
//! * [`ContextKey`]: the opaque key a calling slice chooses to say *what* a draft is for
//!   (`forum:reply:1234`, `mail:new`). A member has at most one draft per key.
//! * [`BodyLimits`] and [`Body`]: the bounded size and line count of a message and the one place
//!   it is sanitized (the shared multi-line sanitizer; this module has no sanitizer of its own).
//! * [`quote`]: quoting already-sanitized text.
//! * [`DraftStore`] and the functions that use it ([`save_draft`], [`load_draft`],
//!   [`discard_draft`], [`list_drafts`]): at most [`MAX_DRAFTS_PER_MEMBER`] drafts per member.
//!
//! **What a draft is.** Text the member chose to keep. It is stored as the server can read it
//! like everything else in the MVP (D3): nothing here is private, encrypted or hidden from the
//! server, and nothing here says so. A draft is owned by the account in the session and is
//! never reachable by another member; there is no staff view of drafts.
//!
//! **No rules of a feature.** Nothing here knows about boards, mail or polls. The caller checks
//! its own permission (to post, to send) before it opens the editor, and deletes the draft
//! after its own write succeeded ([`discard_draft`]), so a failed post never loses text.

mod body;
mod context;
mod quote;
mod service;
mod store;

pub use body::{Body, BodyError, BodyLimits, MAX_BODY_CHARS, MAX_BODY_LINES};
pub use context::{ContextKey, ContextKeyError, MAX_CONTEXT_CHARS};
pub use quote::{MAX_QUOTE_DEPTH, QuoteLimits, is_quote_line, quote, quote_depth};
pub use service::{
    Draft, DraftError, SaveOutcome, discard_draft, list_drafts, load_draft, save_draft,
};
pub use store::{DraftPut, DraftStore, DraftSummary, MAX_DRAFTS_PER_MEMBER, StoredDraft};

#[cfg(test)]
mod tests;
