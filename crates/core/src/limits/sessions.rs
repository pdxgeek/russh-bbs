//! Concurrent sessions per account, counted in this process.
//!
//! **A two-process deployment is not covered**: each process has its own counter, so an account
//! could hold the limit in each. The board runs as one process on one database today (see
//! `docs/deploy.md`); shared counting needs a lease row in the database and is future work.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use crate::store::AccountId;

use super::kind::Limit;

/// How many signed-in sessions each account has open right now. Cloning shares the counts.
#[derive(Debug, Clone, Default)]
pub struct SessionCounter {
    open: Arc<Mutex<HashMap<AccountId, u64>>>,
}

impl SessionCounter {
    /// An empty counter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Take a slot for `account` if fewer than `max` are open. The slot is returned when the
    /// guard is dropped, however the session ended.
    #[must_use]
    pub fn try_acquire(&self, account: AccountId, max: Limit) -> Option<SessionSlot> {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        let count = open.entry(account).or_insert(0);
        if max.cap().is_some_and(|cap| *count >= cap) {
            if *count == 0 {
                open.remove(&account);
            }
            return None;
        }
        *count = count.saturating_add(1);
        Some(SessionSlot {
            account,
            counter: self.clone(),
        })
    }

    /// Sessions `account` has open now.
    #[must_use]
    pub fn open_for(&self, account: AccountId) -> u64 {
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&account)
            .copied()
            .unwrap_or(0)
    }

    fn release(&self, account: AccountId) {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = open.get_mut(&account) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                open.remove(&account);
            }
        }
    }
}

/// One open session's place in the count. Dropping it frees the place.
#[derive(Debug)]
pub struct SessionSlot {
    account: AccountId,
    counter: SessionCounter,
}

impl Drop for SessionSlot {
    fn drop(&mut self) {
        self.counter.release(self.account);
    }
}
