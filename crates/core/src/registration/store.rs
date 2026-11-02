//! The storage trait for `open` registration. Persistence only: permissions, the rate limit
//! and the mode check are in [`super::service`]; the cap and the account are made in one store
//! transaction here.

use std::future::Future;

use super::cap::DailyCap;
use crate::access::{Fingerprint, NoMembers};
use crate::sanitize::Username;
use crate::store::{Account, RegistrationMode, StoreError};

/// Why an `open` attempt was refused. For the log; the person reads one sentence for all of
/// them (D67's rule, applied here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenRefusal {
    /// The waiting snapshot does not hold `membership.open`.
    NotPermitted,
    /// The stored registration mode is not `open` (it changed after the room was built).
    NotOpen,
    /// This key tried too often.
    KeyRate,
    /// This network tried too often.
    NetworkRate,
    /// The handle is taken (or looks like one that is).
    Handle,
    /// The key is already bound to an account (a pending or suspended one reaches the room).
    Key,
}

/// Why no account was made. Nothing was written, except that the first cap refusal of a UTC
/// day leaves its audit row.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// Refused for a reason the person is not told.
    #[error("registration refused")]
    Rejected(OpenRefusal),
    /// The day's cap is used up. The room shows the same sentence as for any refusal; the
    /// next connection gets the `apply` form with a notice.
    #[error("the daily cap on new accounts is reached")]
    CapReached,
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// An account `open` registration made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    /// The new account: `active`, bound to the presenting key, no inviter.
    pub account: Account,
}

/// `open` registration persistence.
pub trait RegistrationStore: Send + Sync {
    /// The stored registration mode (D38): read per call, so a change is seen by the next
    /// connection without a restart. An unrecognised value is an error.
    fn registration_mode(
        &self,
    ) -> impl Future<Output = Result<RegistrationMode, StoreError>> + Send;

    /// The `open_daily_cap` setting as stored, if set (parsed in core by
    /// [`DailyCap::from_setting`] so both backends agree on what a bad value means).
    fn open_cap_setting(&self) -> impl Future<Output = Result<Option<String>, StoreError>> + Send;

    /// Whether today's (UTC, by the store's clock) count of `open` accounts has reached `cap`.
    /// Read only. Never true for [`DailyCap::Unlimited`].
    fn open_cap_reached(
        &self,
        cap: DailyCap,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// Make an `active` account named `username`, bound to `fingerprint`, with no inviter. In
    /// **one transaction**: count it against today's cap (a capped increment, so concurrent
    /// callers cannot take the last place twice; at the cap, flag the day, write its one
    /// `registration.capped` audit row if the last place did not already, commit that and
    /// return [`OpenError::CapReached`]), insert the account and its key, and write
    /// `account.created` (actor: the board). The account that takes the day's last place also
    /// writes the day's one `registration.capped` row, in the same transaction. A taken handle
    /// or a bound key rolls everything back, including the count and that row.
    fn open_account(
        &self,
        username: Username,
        fingerprint: &Fingerprint,
        cap: DailyCap,
    ) -> impl Future<Output = Result<Opened, OpenError>> + Send;
}

/// The empty directory has no accounts to make and no cap to hit.
impl RegistrationStore for NoMembers {
    fn registration_mode(
        &self,
    ) -> impl Future<Output = Result<RegistrationMode, StoreError>> + Send {
        std::future::ready(Ok(RegistrationMode::default()))
    }

    fn open_cap_setting(&self) -> impl Future<Output = Result<Option<String>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn open_cap_reached(
        &self,
        _cap: DailyCap,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(false))
    }

    fn open_account(
        &self,
        _username: Username,
        _fingerprint: &Fingerprint,
        _cap: DailyCap,
    ) -> impl Future<Output = Result<Opened, OpenError>> + Send {
        std::future::ready(Err(OpenError::Rejected(OpenRefusal::NotOpen)))
    }
}
