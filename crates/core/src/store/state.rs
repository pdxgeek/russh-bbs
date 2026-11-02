//! Account states and the moves between them (M2-D).
//!
//! The state is a closed enum and the allowed moves are one pure function,
//! [`AccountState::can_become`], so every caller (the sysop queue, a suspension, the invite
//! redemption) asks the same question and the store enforces the answer inside its
//! transaction. Only [`AccountState::Active`] is member access
//! ([`AccountState::is_member_access`], D35); every other state gets the `waiting` snapshot.
//!
//! ```text
//! pending --> approved --> active <--> suspended
//!    |                       ^   ^         ^
//!    +-----------------------+   |         |
//!    +---------------------------|---------+   (a pending or approved account can be suspended)
//! ```
//!
//! A denial is not a state: the queue (M2-E) deletes the request. An account is never moved
//! back to `pending`, and a move to the state it is already in is a refusal, not a no-op, so a
//! caller cannot write an audit row for a change that did not happen.

use std::future::Future;

use super::{AccountId, StoreError};
use crate::audit::Actor;

/// Where an account is in its life. Only [`AccountState::Active`] grants member access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountState {
    /// Applied or registered, awaiting a decision. Not a member.
    Pending,
    /// Accepted but not yet activated. Not a member.
    Approved,
    /// A working account.
    Active,
    /// Locked out by the sysop. Not a member.
    Suspended,
}

impl AccountState {
    /// Every state, in life order.
    pub const ALL: [Self; 4] = [Self::Pending, Self::Approved, Self::Active, Self::Suspended];

    /// The stable text stored in the database.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Active => "active",
            Self::Suspended => "suspended",
        }
    }

    /// Parse the stored text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "pending" => Some(Self::Pending),
            "approved" => Some(Self::Approved),
            "active" => Some(Self::Active),
            "suspended" => Some(Self::Suspended),
            _ => None,
        }
    }

    /// Whether this state may use the board as a member. The one rule the loader, the
    /// directory and the session start all use; no other code compares against `Active`.
    #[must_use]
    pub const fn is_member_access(self) -> bool {
        match self {
            Self::Active => true,
            Self::Pending | Self::Approved | Self::Suspended => false,
        }
    }

    /// Whether an account in this state may move to `to`. Pure; no match arm falls through to a
    /// default, so a new state is a compile error here.
    #[must_use]
    pub const fn can_become(self, to: Self) -> bool {
        match (self, to) {
            (Self::Pending, Self::Approved | Self::Active | Self::Suspended)
            | (Self::Approved, Self::Active | Self::Suspended)
            | (Self::Active, Self::Suspended)
            | (Self::Suspended, Self::Active) => true,
            (Self::Pending, Self::Pending)
            | (Self::Approved, Self::Pending | Self::Approved)
            | (Self::Active, Self::Pending | Self::Approved | Self::Active)
            | (Self::Suspended, Self::Pending | Self::Approved | Self::Suspended) => false,
        }
    }
}

/// A state change that was made, for the caller and the audit row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateChange {
    /// The account.
    pub account: AccountId,
    /// The state it was in.
    pub from: AccountState,
    /// The state it is in now.
    pub to: AccountState,
}

/// Why a state change was not made. Nothing was written.
#[derive(Debug, thiserror::Error)]
pub enum StateChangeError {
    /// No account has this id.
    #[error("no such account")]
    NotFound,
    /// [`AccountState::can_become`] says no.
    #[error("an account cannot move from {} to {}", .from.as_str(), .to.as_str())]
    NotAllowed {
        /// Its current state.
        from: AccountState,
        /// The refused state.
        to: AccountState,
    },
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Changing an account's state. Who may ask is the caller's rule (`members.approve`,
/// `members.suspend`); this is the one write.
pub trait AccountStateStore: Send + Sync {
    /// Move `account` to `to` if [`AccountState::can_become`] allows it from the state it is in
    /// **now**: the store reads it and writes in one transaction, so two racing callers cannot
    /// both act on a state that has already changed. Writes the `account.state_changed` audit
    /// row (by `actor`) in the same transaction.
    fn change_state(
        &self,
        account: AccountId,
        to: AccountState,
        actor: Actor,
    ) -> impl Future<Output = Result<StateChange, StateChangeError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::AccountState::{self, Active, Approved, Pending, Suspended};

    #[test]
    fn only_active_is_member_access() {
        for state in AccountState::ALL {
            assert_eq!(state.is_member_access(), state == Active, "{state:?}");
        }
    }

    #[test]
    fn the_allowed_moves_are_exactly_these() {
        let allowed = [
            (Pending, Approved),
            (Pending, Active),
            (Pending, Suspended),
            (Approved, Active),
            (Approved, Suspended),
            (Active, Suspended),
            (Suspended, Active),
        ];
        for from in AccountState::ALL {
            for to in AccountState::ALL {
                assert_eq!(
                    from.can_become(to),
                    allowed.contains(&(from, to)),
                    "{from:?} -> {to:?}"
                );
            }
        }
    }

    #[test]
    fn nothing_moves_back_to_pending_and_no_state_moves_to_itself() {
        for state in AccountState::ALL {
            assert!(!state.can_become(state));
            assert!(!state.can_become(Pending));
        }
    }

    #[test]
    fn text_round_trips_and_unknown_text_is_refused() {
        for state in AccountState::ALL {
            assert_eq!(AccountState::parse(state.as_str()), Some(state));
        }
        assert_eq!(AccountState::parse("banned"), None);
        assert_eq!(AccountState::parse("Active"), None);
        assert_eq!(AccountState::parse(""), None);
    }
}
