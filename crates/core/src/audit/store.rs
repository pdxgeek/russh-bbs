//! The audit storage trait, the clock it is stamped by, and the bounded read query.

use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::event::{Action, Actor, AuditEvent};
use crate::store::{StoreError, UnixSeconds};

/// The largest page a read returns, whatever the query asks for.
pub const MAX_PAGE: u32 = 100;

/// Where timestamps come from. A store is given one at construction, so a test sets the time
/// and nothing can back-date a row.
pub trait WallClock: Send + Sync + fmt::Debug {
    /// The current time.
    fn now(&self) -> UnixSeconds;
}

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemWallClock;

impl WallClock for SystemWallClock {
    fn now(&self) -> UnixSeconds {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        UnixSeconds::from_raw(secs)
    }
}

/// A clock a test sets by hand.
#[derive(Debug, Default)]
pub struct FixedClock(AtomicI64);

impl FixedClock {
    /// A clock stopped at `at`.
    #[must_use]
    pub const fn new(at: i64) -> Self {
        Self(AtomicI64::new(at))
    }

    /// Move the clock.
    pub fn set(&self, at: i64) {
        self.0.store(at, Ordering::SeqCst);
    }
}

impl WallClock for FixedClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds::from_raw(self.0.load(Ordering::SeqCst))
    }
}

/// Store-assigned id of an audit row. Increases with every append.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AuditId(i64);

impl AuditId {
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

/// A stored audit row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// Its id.
    pub id: AuditId,
    /// When it was written, by the store's clock.
    pub at: UnixSeconds,
    /// What happened.
    pub event: AuditEvent,
}

/// A bounded, paged read, newest first. Every filter is optional and they combine with AND.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditQuery {
    /// Only this actor.
    pub actor: Option<Actor>,
    /// Only this action.
    pub action: Option<Action>,
    /// Only rows at or after this time.
    pub since: Option<UnixSeconds>,
    /// Only rows before this time.
    pub until: Option<UnixSeconds>,
    /// Page cursor: only rows older than this id (the last id of the previous page).
    pub before: Option<AuditId>,
    /// Page size; clamped to `1..=`[`MAX_PAGE`].
    pub limit: u32,
}

impl Default for AuditQuery {
    fn default() -> Self {
        Self {
            actor: None,
            action: None,
            since: None,
            until: None,
            before: None,
            limit: MAX_PAGE,
        }
    }
}

impl AuditQuery {
    /// The page size actually used.
    #[must_use]
    pub fn page_size(&self) -> u32 {
        self.limit.clamp(1, MAX_PAGE)
    }
}

/// Append and read. **No update, no delete**: the table is append-only (the database also
/// enforces it).
pub trait AuditStore: Send + Sync {
    /// Append one event, stamped by the store's clock, in its own transaction. Writes that
    /// change something write their event inside their own transaction instead (the store's
    /// `audit_in` helper); this is for events with no other write.
    fn append(
        &self,
        event: &AuditEvent,
    ) -> impl Future<Output = Result<AuditId, StoreError>> + Send;

    /// One page of matching rows, newest first, at most [`MAX_PAGE`]. A stored row the code
    /// could not have written is [`StoreError::Corrupt`].
    fn page(
        &self,
        query: &AuditQuery,
    ) -> impl Future<Output = Result<Vec<AuditEntry>, StoreError>> + Send;
}
