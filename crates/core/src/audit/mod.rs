//! The audit log (M2-C, D58): an append-only record of who changed what.
//!
//! * [`AuditEvent`] is a typed event: a closed [`Action`], an [`Actor`] (an account or the
//!   system), a [`Target`], an optional resource and a short sanitized [`Detail`]. Nothing in
//!   it can carry a token, key material or a password: every field is an id, a closed enum,
//!   a validated slug, a public [`Fingerprint`](crate::access::Fingerprint), or text that has
//!   been through the shared sanitizer and a length cap.
//! * The timestamp comes from a [`WallClock`] the store is given, not from the event, so a
//!   test controls it and an event cannot be back-dated.
//! * **One write helper.** Every core write that changes something worth auditing writes its
//!   event through the store's single helper, *inside the transaction of the change* (see
//!   `russh-bbs-store`, `audit_in`), so a change never lands without its row and a row never
//!   describes a change that rolled back. [`AuditStore::append`] is for events with no other
//!   write (a refusal, a ban attempt).
//! * **Append-only is enforced by the database**: triggers abort `UPDATE` and `DELETE` on both
//!   backends. [`AuditStore`] has no update or delete method either.
//! * Reading is [`view_log`]: gated by `audit.view`, bounded and paged, newest first. The sysop
//!   screen over it is M5.
//!
//! Retention (trimming old rows) is deliberately not here: it needs a sysop-controlled policy
//! and a way to prune an append-only table, and is M5.

mod event;
pub mod events;
mod service;
mod store;

pub use event::{Action, Actor, AuditEvent, DETAIL_MAX_CHARS, Detail, Target};
pub use service::{AuditViewError, view_log};
pub use store::{
    AuditEntry, AuditId, AuditQuery, AuditStore, FixedClock, MAX_PAGE, SystemWallClock, WallClock,
};

#[cfg(test)]
mod tests;
