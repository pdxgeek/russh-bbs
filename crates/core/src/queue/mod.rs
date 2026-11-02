//! The sysop queue: requests for an invite, and the decisions on them (M2-E, D71, D72).
//!
//! A stranger on the waiting room's *request an invite* form (mode `invite`, D2) leaves a
//! handle they would like, a short message and nothing else; the key they connected with is
//! the key the account will be bound to if a sysop says yes. The requests wait here for
//! `members.approve` / `members.deny` holders to decide and for `members.vouch` holders to
//! speak for the person.
//!
//! * [`submit_request`]: the stranger's side. Everything that can go wrong (the rate limit, a
//!   full queue, a key that already asked, a key that was just refused, a key that already has
//!   an account, a store failure) is a [`Submission::Dropped`] and **the person sees the same
//!   words as for a request that was kept** (the waiting room reveals nothing, D23). Drops are
//!   logged with the reason and never with the request's text.
//! * [`QueueStore`]: persistence. One transaction per operation; the caps, the one-per-key
//!   rule and the cooldown are enforced where the insert happens, so a race cannot break them.
//! * [`approve_request`], [`deny_request`], [`vouch_request`], [`list_pending`]: the staff
//!   actions. Each calls [`Snapshot::require`](crate::access::Snapshot::require) itself, so no
//!   front end can skip the check, and each writes its audit row in the store's transaction.
//! * [`RequestGate`]: the per-key and per-network rate limits, on M1-C's limiter (D37).
//! * [`QueueScreen`]: the sysop and vouch screens as a pure state machine (nothing drawn).
//!
//! **Minimal data.** A pending request holds the sanitized handle and message, the key's
//! public fingerprint and two timestamps. No address is stored (the network limit lives in
//! memory only). A denial deletes the request and keeps only a hash of the fingerprint and an
//! expiry for [`DENIAL_COOLDOWN_SECS`]; both are purged when they expire.

mod gate;
pub mod keys;
mod request;
mod screen;
mod service;
mod store;

pub use gate::{
    DEFAULT_KEY_RATE, DEFAULT_NETWORK_RATE, DEFAULT_TRACKED, RequestGate, SubmitLimited,
};
pub use request::{
    DEFAULT_MAX_PENDING, DENIAL_COOLDOWN_SECS, InviteAsk, MAX_MAX_PENDING, MESSAGE_MAX_CHARS,
    Message, NewRequest, PENDING_TTL_SECS, PendingRequest, RequestId, Voucher, cooldown_key,
};
pub use screen::{
    PAGE_SIZE, QueueKey, QueueMode, QueueScreen, QueueStep, QueueView, Row, ViewLine, age_text,
};
pub use service::{
    Approval, DropReason, LIST_LIMIT, QueueError, Request, Response, Submission, approve_request,
    deny_request, execute, list_pending, max_pending, purge_expired, submit_request, vouch_request,
};
pub use store::{
    ApproveError, DecideError, Purged, Queue, QueueStore, Refusal, SubmitTerms, Submitted,
    VouchError,
};

#[cfg(test)]
mod tests;
