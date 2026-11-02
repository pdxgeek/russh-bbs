//! Editing limits: a core service gated by `limits.edit`. There is no sysop screen yet (M5);
//! this is what that screen and the tests call. It takes the [`Granted`] proof from
//! [`crate::access::Snapshot::require`], so it cannot run without the check.
//!
//! Every edit is audited (`limit.changed`) by the store, in the same transaction as the write;
//! the actor is the account in the proof.

use crate::access::{Granted, Permission, Slug};
use crate::audit::Actor;
use crate::store::AccountId;

use super::kind::{Limit, LimitKind};
use super::store::{LimitStore, LimitWriteError};

/// Why a limit edit was refused.
#[derive(Debug, thiserror::Error)]
pub enum LimitEditError {
    /// The proof was not for `limits.edit`.
    #[error("not permitted to edit limits")]
    NotPermitted,
    /// The number is larger than the database can hold.
    #[error("that number is too large")]
    TooLarge,
    /// The write failed.
    #[error(transparent)]
    Write(#[from] LimitWriteError),
}

fn actor(granted: &Granted) -> Actor {
    granted.account().map_or(Actor::System, Actor::Account)
}

fn check(granted: &Granted, limit: Option<Limit>) -> Result<(), LimitEditError> {
    if granted.permission() != Permission::LimitsEdit {
        return Err(LimitEditError::NotPermitted);
    }
    if limit.is_some_and(|l| l.to_stored().is_none()) {
        return Err(LimitEditError::TooLarge);
    }
    Ok(())
}

/// Set a role's limit for a kind. Applies to every account holding the role from its next
/// session start (minutes and the like) or next action.
pub async fn set_role_limit<S: LimitStore>(
    store: &S,
    granted: &Granted,
    role: &Slug,
    kind: LimitKind,
    limit: Limit,
) -> Result<(), LimitEditError> {
    check(granted, Some(limit))?;
    Ok(store
        .set_role_limit(role, kind, limit, actor(granted))
        .await?)
}

/// Set an account's override for a kind. It replaces the roles' values for that account.
pub async fn set_member_limit<S: LimitStore>(
    store: &S,
    granted: &Granted,
    account: AccountId,
    kind: LimitKind,
    limit: Limit,
) -> Result<(), LimitEditError> {
    check(granted, Some(limit))?;
    Ok(store
        .set_member_limit(account, kind, Some(limit), actor(granted))
        .await?)
}

/// Remove an account's override, so its roles' values apply again.
pub async fn clear_member_limit<S: LimitStore>(
    store: &S,
    granted: &Granted,
    account: AccountId,
    kind: LimitKind,
) -> Result<(), LimitEditError> {
    check(granted, None)?;
    Ok(store
        .set_member_limit(account, kind, None, actor(granted))
        .await?)
}
