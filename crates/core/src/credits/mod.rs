//! The credits ledger (M3-C, D79): append-only, non-transferable, balance derived.
//!
//! - **One writer.** [`post`] is the only way credits move. It takes a [`Movement`] (earn or
//!   spend an [`Amount`]) and a reason, and the store applies it in one transaction that
//!   refuses an overdraft: a spend larger than the balance changes nothing and returns
//!   [`CreditError::Overdraft`]. Two spends racing for the last credits cannot both succeed.
//! - **Append-only.** Rows are never updated or deleted. The database refuses it (triggers on
//!   both backends, like the audit log, D59), so a later bug cannot rewrite history. Dropping the
//!   table or replacing the database file is the owner's power and is not claimed to be
//!   prevented.
//! - **Balance is derived.** There is no balance column: [`balance`] is the sum of the account's
//!   rows, so it cannot disagree with the ledger.
//! - **Non-transferable.** A row belongs to one account and there is no function that moves
//!   credits from one account to another, nor a field naming a second account. A member cannot
//!   give, sell or trade credits.
//! - **No earning rules here.** Nothing in this module decides *when* credits are earned or
//!   what they cost; callers pass a [`Slug`] reason such as `daily-login`. The credits earn rate
//!   of D62 waits for a slice that defines those rules.
//! - **Bounds.** One movement is at most [`MAX_AMOUNT`] and a balance at most [`MAX_BALANCE`],
//!   so sums cannot overflow; an earn that would pass the cap is refused with
//!   [`CreditError::BalanceFull`].
//! - **Time** is the store's injected wall clock (as for the audit log), so a caller cannot
//!   back-date a row.

use std::future::Future;

use crate::access::Slug;
use crate::store::{AccountId, StoreError, UnixSeconds};

/// The most one movement may carry.
pub const MAX_AMOUNT: u64 = 1_000_000_000;

/// The most a balance may hold.
pub const MAX_BALANCE: u64 = 1_000_000_000_000_000;

/// The most rows [`history`] returns in a page.
pub const MAX_PAGE: u32 = 100;

/// Store-assigned id of a ledger row. Increases with every row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LedgerId(i64);

impl LedgerId {
    /// Wrap a raw id. Call this only where a row is mapped (the store).
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// The raw id, for a query.
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }
}

/// A positive number of credits, at most [`MAX_AMOUNT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Amount(u64);

/// Why a number is not an [`Amount`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an amount must be between 1 and {MAX_AMOUNT} credits")]
pub struct InvalidAmount;

impl Amount {
    /// Validate a number of credits.
    pub const fn new(credits: u64) -> Result<Self, InvalidAmount> {
        if credits == 0 || credits > MAX_AMOUNT {
            Err(InvalidAmount)
        } else {
            Ok(Self(credits))
        }
    }

    /// The number of credits.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Credits coming in or going out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Movement {
    /// The account gains credits.
    Earn(Amount),
    /// The account loses credits, if it has them.
    Spend(Amount),
}

impl Movement {
    /// The signed change to the balance.
    #[must_use]
    pub fn delta(self) -> i64 {
        // An Amount is at most MAX_AMOUNT, so it always fits.
        match self {
            Self::Earn(amount) => i64::try_from(amount.0).unwrap_or(i64::MAX),
            Self::Spend(amount) => -i64::try_from(amount.0).unwrap_or(i64::MAX),
        }
    }
}

/// Why a movement was not recorded.
#[derive(Debug, thiserror::Error)]
pub enum CreditError {
    /// The storage backend failed (or the account does not exist: [`StoreError::AccountNotFound`]).
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A spend larger than the balance. Nothing changed.
    #[error("not enough credits: balance {balance}, needed {needed}")]
    Overdraft {
        /// The balance when the spend was refused.
        balance: u64,
        /// What the spend asked for.
        needed: u64,
    },
    /// An earn that would pass [`MAX_BALANCE`]. Nothing changed.
    #[error("the balance is full at {balance}")]
    BalanceFull {
        /// The balance when the earn was refused.
        balance: u64,
    },
}

/// A recorded movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Posted {
    /// The new ledger row.
    pub id: LedgerId,
    /// The account's balance after it.
    pub balance: u64,
}

/// A stored ledger row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    /// Its id.
    pub id: LedgerId,
    /// The one account it belongs to.
    pub account: AccountId,
    /// When it was written, by the store's clock.
    pub at: UnixSeconds,
    /// The signed change: positive earns, negative spends.
    pub delta: i64,
    /// Why, as a short slug.
    pub reason: Slug,
}

/// Ledger persistence. Call [`post`], not [`CreditStore::append`], to move credits.
pub trait CreditStore: Send + Sync {
    /// Append one row for `account` in a single transaction: refuse (changing nothing) when
    /// `delta` would take the balance below zero or above [`MAX_BALANCE`], and
    /// [`StoreError::AccountNotFound`] for an account that does not exist. Concurrent calls for
    /// one account are serialized, so the refusal is never raced. The row is stamped by the
    /// store's clock.
    fn append(
        &self,
        account: AccountId,
        delta: i64,
        reason: &Slug,
    ) -> impl Future<Output = Result<Posted, CreditError>> + Send;

    /// The account's balance: the sum of its rows (zero for none).
    fn balance(&self, account: AccountId) -> impl Future<Output = Result<u64, StoreError>> + Send;

    /// Up to `limit` of the account's rows, newest first, starting below `before` when given.
    fn entries(
        &self,
        account: AccountId,
        before: Option<LedgerId>,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<LedgerEntry>, StoreError>> + Send;
}

/// Earn or spend credits for `account`. The one way credits move; see the [module docs](self).
pub async fn post<S: CreditStore>(
    store: &S,
    account: AccountId,
    movement: Movement,
    reason: &Slug,
) -> Result<Posted, CreditError> {
    store.append(account, movement.delta(), reason).await
}

/// The account's balance, derived from the ledger.
pub async fn balance<S: CreditStore>(store: &S, account: AccountId) -> Result<u64, StoreError> {
    store.balance(account).await
}

/// A page of the account's ledger, newest first, at most [`MAX_PAGE`] rows.
pub async fn history<S: CreditStore>(
    store: &S,
    account: AccountId,
    before: Option<LedgerId>,
    limit: u32,
) -> Result<Vec<LedgerEntry>, StoreError> {
    store
        .entries(account, before, limit.clamp(1, MAX_PAGE))
        .await
}

#[cfg(test)]
mod tests;
