//! The storage trait for limits: persistence only. The rules are in [`super::effective`] and
//! [`super::meter`]; the two backends only read and write rows.

use std::future::Future;

use crate::access::Slug;
use crate::audit::Actor;
use crate::store::{AccountId, StoreError};

use super::clock::Day;
use super::effective::{MemberLimit, RoleLimit};
use super::kind::{Limit, LimitKind};

/// What an atomic, capped usage increment did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageOutcome {
    /// The amount was added; this is the new total for the day.
    Added(u64),
    /// Adding it would have passed the cap, so nothing was added; this is the current total.
    Full(u64),
}

/// Why a limit write failed.
#[derive(Debug, thiserror::Error)]
pub enum LimitWriteError {
    /// The storage backend failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// No role has this name.
    #[error("no such role")]
    UnknownRole,
    /// No account has this id.
    #[error("no such account")]
    UnknownAccount,
}

/// Role limits, member overrides and per-day usage.
pub trait LimitStore: Send + Sync {
    /// Every stored limit of the named roles (all kinds).
    fn role_limits(
        &self,
        roles: &[Slug],
    ) -> impl Future<Output = Result<Vec<RoleLimit>, StoreError>> + Send;

    /// Every override stored for the account.
    fn member_limits(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<MemberLimit>, StoreError>> + Send;

    /// Set a role's value for a kind, replacing any earlier one. [`Limit::Unlimited`] is
    /// stored too (it is an explicit value, not a missing one).
    ///
    /// Audited (`limit.changed`) in the same transaction when the value changed; setting the
    /// value a role already has writes no row. `actor` is who made the edit.
    fn set_role_limit(
        &self,
        role: &Slug,
        kind: LimitKind,
        limit: Limit,
        actor: Actor,
    ) -> impl Future<Output = Result<(), LimitWriteError>> + Send;

    /// Set (`Some`) or clear (`None`) the account's override for a kind. Audited like
    /// [`LimitStore::set_role_limit`]; clearing an override that is not set writes no row.
    fn set_member_limit(
        &self,
        account: AccountId,
        kind: LimitKind,
        limit: Option<Limit>,
        actor: Actor,
    ) -> impl Future<Output = Result<(), LimitWriteError>> + Send;

    /// What the account has used of `kind` on `day`.
    fn usage(
        &self,
        account: AccountId,
        kind: LimitKind,
        day: Day,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send;

    /// Add `amount` to the account's usage of `kind` on `day`, atomically. With a `cap`, the
    /// amount is added only if the new total stays within it; otherwise nothing changes.
    fn add_usage(
        &self,
        account: AccountId,
        kind: LimitKind,
        day: Day,
        amount: u64,
        cap: Option<u64>,
    ) -> impl Future<Output = Result<UsageOutcome, StoreError>> + Send;

    /// Delete the account's usage rows for days before `before`.
    fn prune_usage(
        &self,
        account: AccountId,
        before: Day,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

/// The empty directory has no accounts, so there is nothing to limit or count.
impl LimitStore for crate::access::NoMembers {
    fn role_limits(
        &self,
        _roles: &[Slug],
    ) -> impl Future<Output = Result<Vec<RoleLimit>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn member_limits(
        &self,
        _account: AccountId,
    ) -> impl Future<Output = Result<Vec<MemberLimit>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn set_role_limit(
        &self,
        _role: &Slug,
        _kind: LimitKind,
        _limit: Limit,
        _actor: Actor,
    ) -> impl Future<Output = Result<(), LimitWriteError>> + Send {
        std::future::ready(Err(LimitWriteError::UnknownRole))
    }

    fn set_member_limit(
        &self,
        _account: AccountId,
        _kind: LimitKind,
        _limit: Option<Limit>,
        _actor: Actor,
    ) -> impl Future<Output = Result<(), LimitWriteError>> + Send {
        std::future::ready(Err(LimitWriteError::UnknownAccount))
    }

    fn usage(
        &self,
        _account: AccountId,
        _kind: LimitKind,
        _day: Day,
    ) -> impl Future<Output = Result<u64, StoreError>> + Send {
        std::future::ready(Ok(0))
    }

    fn add_usage(
        &self,
        _account: AccountId,
        _kind: LimitKind,
        _day: Day,
        amount: u64,
        _cap: Option<u64>,
    ) -> impl Future<Output = Result<UsageOutcome, StoreError>> + Send {
        std::future::ready(Ok(UsageOutcome::Added(amount)))
    }

    fn prune_usage(
        &self,
        _account: AccountId,
        _before: Day,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Ok(()))
    }
}
