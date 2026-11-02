//! The queue actions. A stranger's side ([`submit_request`]) takes the waiting snapshot and the
//! rate limiter; the staff side takes the [`Granted`] proof that
//! [`Snapshot::require`] returns, so a front end cannot call one without passing `core::access`
//! (docs/access.md, "Rules"). A proof for the wrong permission is refused as well.
//!
//! * `submit_request` needs `invite.request` for an invite request and `membership.apply` for a
//!   membership application (M2-G).
//! * `list_pending` needs `members.approve`, `members.deny` or `members.vouch`.
//! * `approve_request` needs `members.approve`, `deny_request` needs `members.deny` and
//!   `vouch_request` needs `members.vouch`.
//!
//! **No oracle.** [`submit_request`] never fails and never returns anything the person sees: the
//! waiting room shows the same words for every [`Submission`]. The reason for a drop is logged
//! (without the request's text); nothing about the queue's size, the key's history or the
//! board's members can be read from the answer or, beyond a few milliseconds of store time,
//! from how long it took.

use std::net::IpAddr;

use tracing::{info, warn};

use super::gate::{RequestGate, SubmitLimited};
use super::request::{
    DEFAULT_MAX_PENDING, DENIAL_COOLDOWN_SECS, InviteAsk, MAX_MAX_PENDING, Message, NewRequest,
    PENDING_TTL_SECS, RequestId, RequestKind,
};
use super::store::{
    ApproveError, DecideError, Purged, Queue, QueueStore, Refusal, SubmitTerms, Submitted,
    VouchError,
};
use crate::access::{Denied, Fingerprint, Granted, Permission, Snapshot};
use crate::sanitize::Username;
use crate::store::{Account, StoreError};
use crate::waiting_room::Clock;

/// Most requests one listing reads. The queue's cap is far below this unless the sysop raises
/// it; a bigger queue shows its oldest requests first.
pub const LIST_LIMIT: u32 = 500;

/// Why a request was not kept. For the log; the person is told nothing of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// The waiting snapshot does not hold `invite.request`.
    NotPermitted,
    /// A rate limit.
    RateLimited(SubmitLimited),
    /// The store's rules refused it.
    Refused(Refusal),
    /// The stored registration mode is not taking this kind of request (an application when the
    /// board is in `invite` mode; M2-G).
    WrongMode,
    /// The store failed.
    Store,
}

/// What a submission came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submission {
    /// Kept, as this request.
    Queued(RequestId),
    /// Not kept.
    Dropped(DropReason),
}

/// The cap on waiting requests: the sysop's `queue_max_pending` setting, else
/// [`DEFAULT_MAX_PENDING`]. A value that is not a number from 1 to [`MAX_MAX_PENDING`], or a
/// store that cannot be read, gives the default (logged), never "no cap".
pub async fn max_pending<S: QueueStore>(store: &S) -> u32 {
    match store.max_pending_setting().await {
        Ok(None) => DEFAULT_MAX_PENDING,
        Ok(Some(text)) => match text.trim().parse::<u32>() {
            Ok(n) if (1..=MAX_MAX_PENDING).contains(&n) => n,
            _ => {
                warn!(
                    "queue_max_pending is not a number from 1 to {MAX_MAX_PENDING}; using the default"
                );
                DEFAULT_MAX_PENDING
            }
        },
        Err(error) => {
            warn!(%error, "queue_max_pending could not be read; using the default");
            DEFAULT_MAX_PENDING
        }
    }
}

/// Offer a request from the waiting room. Needs `invite.request` (or `membership.apply` for an
/// application) in `access` (the waiting snapshot), then the per-key and per-network limits, then the store's rules. Every outcome is
/// returned for the log and the tests; the front end must show the same words for all of them.
///
/// `peer` is used for the network limit only and is not kept.
pub async fn submit_request<S, C>(
    store: &S,
    gate: &RequestGate<C>,
    access: &Snapshot,
    ask: InviteAsk,
    fingerprint: &Fingerprint,
    peer: Option<IpAddr>,
) -> Submission
where
    S: QueueStore,
    C: Clock,
{
    let needed = match ask.kind {
        RequestKind::Invite => Permission::InviteRequest,
        RequestKind::Application => Permission::MembershipApply,
    };
    if access.require(needed, None).is_err() {
        info!(
            kind = ask.kind.as_str(),
            "request dropped: permission not held"
        );
        return Submission::Dropped(DropReason::NotPermitted);
    }
    if let Err(limited) = gate.admit(fingerprint, peer) {
        info!(?limited, "invite request dropped: rate limited");
        return Submission::Dropped(DropReason::RateLimited(limited));
    }
    let terms = SubmitTerms {
        max_pending: max_pending(store).await,
        ttl_secs: PENDING_TTL_SECS,
    };
    let new = NewRequest {
        handle: ask.handle,
        message: Message::new(&ask.message),
        fingerprint: fingerprint.clone(),
        kind: ask.kind,
    };
    match store.submit_request(&new, terms).await {
        Ok(Submitted::Stored(id)) => {
            info!(request = %id, "invite request queued");
            Submission::Queued(id)
        }
        Ok(Submitted::Refused(refusal)) => {
            match refusal {
                Refusal::QueueFull => warn!(
                    cap = terms.max_pending,
                    "invite request dropped: the queue is full"
                ),
                Refusal::AlreadyPending | Refusal::Cooling | Refusal::KeyHasAccount => {
                    info!(?refusal, "invite request dropped");
                }
            }
            Submission::Dropped(DropReason::Refused(refusal))
        }
        Err(error) => {
            warn!(%error, "invite request dropped: the store failed");
            Submission::Dropped(DropReason::Store)
        }
    }
}

/// Why a staff action did not happen.
#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    /// The session does not hold the permission, or the proof is for another one.
    #[error(transparent)]
    Denied(#[from] Denied),
    /// The proof is for a permission this action does not use.
    #[error("not permitted")]
    NotPermitted,
    /// No such request: decided already, expired, or never there.
    #[error("no such request")]
    NotFound,
    /// Approval: the handle is taken.
    #[error("handle is taken")]
    HandleTaken,
    /// Approval: the request's key already has an account.
    #[error("key already has an account")]
    KeyHasAccount,
    /// Vouch: the member has vouched for this request already.
    #[error("already vouched")]
    AlreadyVouched,
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

fn proof(granted: &Granted, permission: Permission) -> Result<(), QueueError> {
    if granted.permission() == permission {
        Ok(())
    } else {
        Err(QueueError::NotPermitted)
    }
}

/// The pending requests, oldest first (needs `members.approve`, `members.deny` or
/// `members.vouch`).
pub async fn list_pending<S: QueueStore>(
    store: &S,
    access: &Snapshot,
) -> Result<Queue, QueueError> {
    let allowed = [
        Permission::MembersApprove,
        Permission::MembersDeny,
        Permission::MembersVouch,
    ]
    .into_iter()
    .any(|permission| access.require(permission, None).is_ok());
    if !allowed {
        return Err(QueueError::NotPermitted);
    }
    Ok(store.pending_requests(LIST_LIMIT).await?)
}

/// What an approval made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    /// The new account: `active`, bound to the key that asked.
    pub account: Account,
}

/// Approve request `id` (needs the proof of `members.approve`). Creates the account.
pub async fn approve_request<S: QueueStore>(
    store: &S,
    granted: &Granted,
    id: RequestId,
) -> Result<Approval, QueueError> {
    proof(granted, Permission::MembersApprove)?;
    let actor = granted
        .account()
        .ok_or(QueueError::Store(StoreError::AccountNotFound))?;
    match store.approve_request(id, actor).await {
        Ok(account) => {
            info!(request = %id, account = %account.id, actor = %actor, "request approved");
            Ok(Approval { account })
        }
        Err(ApproveError::NotFound) => Err(QueueError::NotFound),
        Err(ApproveError::HandleTaken) => Err(QueueError::HandleTaken),
        Err(ApproveError::KeyHasAccount) => Err(QueueError::KeyHasAccount),
        Err(ApproveError::Store(error)) => Err(error.into()),
    }
}

/// Deny request `id` (needs the proof of `members.deny`). The request is deleted and its key is
/// turned away for [`DENIAL_COOLDOWN_SECS`]; nothing else is kept.
pub async fn deny_request<S: QueueStore>(
    store: &S,
    granted: &Granted,
    id: RequestId,
) -> Result<(), QueueError> {
    proof(granted, Permission::MembersDeny)?;
    let actor = granted
        .account()
        .ok_or(QueueError::Store(StoreError::AccountNotFound))?;
    match store.deny_request(id, actor, DENIAL_COOLDOWN_SECS).await {
        Ok(()) => {
            info!(request = %id, actor = %actor, "request denied");
            Ok(())
        }
        Err(DecideError::NotFound) => Err(QueueError::NotFound),
        Err(DecideError::Store(error)) => Err(error.into()),
    }
}

/// Add the proof holder's name to request `id` (needs the proof of `members.vouch`), once.
pub async fn vouch_request<S: QueueStore>(
    store: &S,
    granted: &Granted,
    id: RequestId,
) -> Result<(), QueueError> {
    proof(granted, Permission::MembersVouch)?;
    let voucher = granted
        .account()
        .ok_or(QueueError::Store(StoreError::AccountNotFound))?;
    match store.vouch_request(id, voucher).await {
        Ok(()) => {
            info!(request = %id, voucher = %voucher, "request vouched for");
            Ok(())
        }
        Err(VouchError::NotFound) => Err(QueueError::NotFound),
        Err(VouchError::AlreadyVouched) => Err(QueueError::AlreadyVouched),
        Err(VouchError::Store(error)) => Err(error.into()),
    }
}

/// Delete requests and cooldowns past their expiry. Housekeeping: the store also does this on
/// every submission and listing, and the server runs it on a timer so a quiet board still
/// forgets.
pub async fn purge_expired<S: QueueStore>(store: &S) -> Result<Purged, StoreError> {
    let purged = store.purge_expired().await?;
    if purged.requests > 0 || purged.cooldowns > 0 {
        info!(
            requests = purged.requests,
            cooldowns = purged.cooldowns,
            "expired queue rows purged"
        );
    }
    Ok(purged)
}

/// What a staff screen asks for. The front end runs it with [`execute`] and gives the answer
/// back to the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// The pending requests.
    List,
    /// Approve one.
    Approve(RequestId),
    /// Deny one.
    Deny(RequestId),
    /// Vouch for one.
    Vouch(RequestId),
}

/// The answer to a [`Request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// The queue.
    Queue(Queue),
    /// Approved: this handle now has an account.
    Approved(Username),
    /// Denied.
    Denied,
    /// Vouched.
    Vouched,
    /// The request is gone (decided or expired).
    Gone,
    /// Approval refused: the handle is taken.
    HandleTaken,
    /// Approval refused: the key has an account.
    KeyHasAccount,
    /// Vouch refused: already vouched.
    AlreadyVouched,
    /// The permission is not held.
    NotPermitted,
    /// The store failed or timed out; nothing is claimed about what was written.
    Failed,
}

/// Run a request for a session holding `access`. Every request starts with
/// [`Snapshot::require`]; a store failure is [`Response::Failed`], never a guess.
pub async fn execute<S: QueueStore>(store: &S, access: &Snapshot, request: Request) -> Response {
    let result = match request {
        Request::List => list_pending(store, access).await.map(Response::Queue),
        Request::Approve(id) => match access.require(Permission::MembersApprove, None) {
            Ok(granted) => approve_request(store, &granted, id)
                .await
                .map(|done| Response::Approved(done.account.username)),
            Err(_) => Err(QueueError::NotPermitted),
        },
        Request::Deny(id) => match access.require(Permission::MembersDeny, None) {
            Ok(granted) => deny_request(store, &granted, id)
                .await
                .map(|()| Response::Denied),
            Err(_) => Err(QueueError::NotPermitted),
        },
        Request::Vouch(id) => match access.require(Permission::MembersVouch, None) {
            Ok(granted) => vouch_request(store, &granted, id)
                .await
                .map(|()| Response::Vouched),
            Err(_) => Err(QueueError::NotPermitted),
        },
    };
    match result {
        Ok(response) => response,
        Err(QueueError::NotFound) => Response::Gone,
        Err(QueueError::HandleTaken) => Response::HandleTaken,
        Err(QueueError::KeyHasAccount) => Response::KeyHasAccount,
        Err(QueueError::AlreadyVouched) => Response::AlreadyVouched,
        Err(QueueError::Denied(_) | QueueError::NotPermitted) => Response::NotPermitted,
        Err(QueueError::Store(error)) => {
            warn!(%error, "queue request failed");
            Response::Failed
        }
    }
}
