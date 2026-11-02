//! The in-memory state of an open or closed founder claim.

use std::sync::{Mutex, PoisonError};

use subtle::Choice;

use super::token::{SetupToken, TokenHash};

/// Whether the claim is open and, if so, the hash that opens it. Shared by every session.
///
/// The gate lives in memory only: a restart issues a new token, and the database holds nothing
/// but the "claimed" flag (D41). The mutex is held for a copy of 32 bytes and never across an
/// `.await`.
#[derive(Debug)]
pub struct FounderGate {
    hash: Mutex<Option<TokenHash>>,
}

impl FounderGate {
    /// A gate that opens with `token`.
    #[must_use]
    pub fn open_with(token: &SetupToken) -> Self {
        Self {
            hash: Mutex::new(Some(token.hash())),
        }
    }

    /// A gate that no token opens: the board is already claimed.
    #[must_use]
    pub fn closed() -> Self {
        Self {
            hash: Mutex::new(None),
        }
    }

    /// Whether `candidate` is the live token. Takes the same steps whether the gate is open
    /// or closed, and compares in constant time, so neither the answer's timing nor its
    /// content separates "wrong token" from "already claimed".
    #[must_use]
    pub fn accepts(&self, candidate: &str) -> bool {
        let stored = *self.hash.lock().unwrap_or_else(PoisonError::into_inner);
        let is_open = Choice::from(u8::from(stored.is_some()));
        let reference = stored.unwrap_or_else(TokenHash::unmatchable);
        let matches = TokenHash::of(candidate).ct_eq(&reference);
        bool::from(matches & is_open)
    }

    /// Close the gate for good. The token no longer matches anything.
    pub fn close(&self) {
        *self.hash.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// Whether a token is still live. For the start-up log only; sessions never see this.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.hash
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }
}
