//! The storage trait for the queue and the shapes it returns. Persistence only: permissions
//! and the person-facing answer are in [`super::service`]; the caps, the one-request-per-key
//! rule, the cooldown and expiry are enforced here, inside each backend's transaction.

use std::future::Future;

use super::request::{NewRequest, PendingRequest, RequestId};
use crate::access::NoMembers;
use crate::store::{Account, AccountId, StoreError, UnixSeconds};

/// The numbers a submission is checked against, read by the service and applied by the store in
/// the transaction that inserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubmitTerms {
    /// The most requests that may be waiting at once.
    pub max_pending: u32,
    /// Seconds the new request waits for a decision before it is deleted.
    pub ttl_secs: i64,
}

/// Why a request was not recorded. Nothing was written (expired rows may have been purged).
/// The person is told the same thing whatever this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The key already has a pending request.
    AlreadyPending,
    /// The queue holds `max_pending` requests.
    QueueFull,
    /// The key was denied less than [`DENIAL_COOLDOWN_SECS`](super::DENIAL_COOLDOWN_SECS) ago.
    Cooling,
    /// The key is bound to an account (a pending or suspended one reaches the waiting room).
    KeyHasAccount,
}

/// What a submission came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submitted {
    /// Recorded.
    Stored(RequestId),
    /// Not recorded, and why.
    Refused(Refusal),
}

/// A page of the queue and the store clock's reading when it was taken, so a screen can say
/// how old each request is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queue {
    /// The store's "now".
    pub now: UnixSeconds,
    /// Pending requests, oldest first.
    pub requests: Vec<PendingRequest>,
}

/// What a purge removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Purged {
    /// Pending requests past their expiry.
    pub requests: u64,
    /// Denial cooldowns past their expiry.
    pub cooldowns: u64,
}

/// Why an approval did not happen. Nothing was written.
#[derive(Debug, thiserror::Error)]
pub enum ApproveError {
    /// No such request: decided already, expired, or never there.
    #[error("no such request")]
    NotFound,
    /// The handle is taken by an account, or by one it looks like (D17).
    #[error("handle is taken")]
    HandleTaken,
    /// The request's key is bound to an account already.
    #[error("key already has an account")]
    KeyHasAccount,
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Why a denial or a vouch did not happen. Nothing was written.
#[derive(Debug, thiserror::Error)]
pub enum DecideError {
    /// No such request: decided already, expired, or never there.
    #[error("no such request")]
    NotFound,
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Why a vouch did not happen. Nothing was written.
#[derive(Debug, thiserror::Error)]
pub enum VouchError {
    /// No such request.
    #[error("no such request")]
    NotFound,
    /// This member has vouched for this request already.
    #[error("already vouched")]
    AlreadyVouched,
    /// The store failed (or the voucher's account does not exist).
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Queue persistence. Every method that writes also writes its audit row (docs/audit.md) in
/// the same transaction, except a submission and a purge, which are deliberately not audited
/// (D71: the log is append-only and an unauthenticated flood must not be able to grow it).
pub trait QueueStore: Send + Sync {
    /// Record `new` if the rules allow. In one transaction, in this order: delete requests and
    /// cooldowns past their expiry; refuse if the key has an account, is cooling down, already
    /// has a request, or the queue holds `terms.max_pending`; insert. Concurrent submissions
    /// cannot take the last slot twice, or the same key twice.
    fn submit_request(
        &self,
        new: &NewRequest,
        terms: SubmitTerms,
    ) -> impl Future<Output = Result<Submitted, StoreError>> + Send;

    /// The oldest `limit` pending requests with their vouches (active members only). Purges
    /// expired rows first.
    fn pending_requests(
        &self,
        limit: u32,
    ) -> impl Future<Output = Result<Queue, StoreError>> + Send;

    /// Approve request `id` on behalf of `actor`. In one transaction: take (delete) the
    /// request, so a second approver finds nothing; create an `active` account with the
    /// request's handle, invited by `actor`; bind the request's key to it; write
    /// `account.created` and `member.approved`. A taken handle or a key that has an account
    /// rolls everything back and the request stays.
    fn approve_request(
        &self,
        id: RequestId,
        actor: AccountId,
    ) -> impl Future<Output = Result<Account, ApproveError>> + Send;

    /// Deny request `id` on behalf of `actor`. In one transaction: delete the request (and its
    /// vouches), keep a [`cooldown_key`](super::cooldown_key) of its fingerprint for
    /// `cooldown_secs`, and write `member.denied` (target: the request id, no content).
    fn deny_request(
        &self,
        id: RequestId,
        actor: AccountId,
        cooldown_secs: i64,
    ) -> impl Future<Output = Result<(), DecideError>> + Send;

    /// Add `voucher`'s name to request `id` and write `member.vouched`. A member can vouch for
    /// a request once.
    fn vouch_request(
        &self,
        id: RequestId,
        voucher: AccountId,
    ) -> impl Future<Output = Result<(), VouchError>> + Send;

    /// The sysop's `queue_max_pending` setting as stored, if set. The text is read here and
    /// parsed in core ([`max_pending`](super::max_pending)) so both backends agree on what a
    /// bad value means. The queue reads it itself because it lives in the same database; a
    /// front end needs no second store handle for it.
    fn max_pending_setting(
        &self,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send;

    /// Delete requests and cooldowns past their expiry.
    fn purge_expired(&self) -> impl Future<Output = Result<Purged, StoreError>> + Send;
}

/// The empty directory has no accounts and no queue.
impl QueueStore for NoMembers {
    fn submit_request(
        &self,
        _new: &NewRequest,
        _terms: SubmitTerms,
    ) -> impl Future<Output = Result<Submitted, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn pending_requests(
        &self,
        _limit: u32,
    ) -> impl Future<Output = Result<Queue, StoreError>> + Send {
        std::future::ready(Ok(Queue {
            now: UnixSeconds::from_raw(0),
            requests: Vec::new(),
        }))
    }

    fn approve_request(
        &self,
        _id: RequestId,
        _actor: AccountId,
    ) -> impl Future<Output = Result<Account, ApproveError>> + Send {
        std::future::ready(Err(ApproveError::NotFound))
    }

    fn deny_request(
        &self,
        _id: RequestId,
        _actor: AccountId,
        _cooldown_secs: i64,
    ) -> impl Future<Output = Result<(), DecideError>> + Send {
        std::future::ready(Err(DecideError::NotFound))
    }

    fn vouch_request(
        &self,
        _id: RequestId,
        _voucher: AccountId,
    ) -> impl Future<Output = Result<(), VouchError>> + Send {
        std::future::ready(Err(VouchError::NotFound))
    }

    fn max_pending_setting(
        &self,
    ) -> impl Future<Output = Result<Option<String>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn purge_expired(&self) -> impl Future<Output = Result<Purged, StoreError>> + Send {
        std::future::ready(Ok(Purged::default()))
    }
}
