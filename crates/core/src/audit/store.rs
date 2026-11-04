//! The audit storage trait, the clock it is stamped by, and the bounded read query.

use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::event::{Action, Actor, AuditEvent, Detail, Target};
use crate::sanitize::{Sanitized, SingleLine};
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

/// Longest [`UnknownAction`] name kept, in characters. Longer stored names are cut.
pub const UNKNOWN_ACTION_MAX_CHARS: usize = 64;

/// The stored action name of a row this binary does not recognise (D77): a renamed action, or
/// one written by another version. It is only ever **read**: it is not an [`Action`], it
/// converts to nothing that [`AuditEvent`] accepts, and no store method takes it, so the code
/// cannot write one. The text went through the shared sanitizer and is cut to
/// [`UNKNOWN_ACTION_MAX_CHARS`], because it came from the database and is shown to a sysop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAction(Sanitized);

impl UnknownAction {
    /// Wrap a stored name, sanitized as one line and capped. Call this only where a row is read
    /// (the store).
    #[must_use]
    pub fn from_stored(name: &str) -> Self {
        Self(SingleLine::new(UNKNOWN_ACTION_MAX_CHARS).sanitize(name))
    }

    /// The sanitized name, for a label such as `unknown action "made.up"`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// A stored row whose action this binary does not know (D77). Its actor was validated like any
/// row's. Its target is kept only if this binary can read it: another version's action may
/// come with a target kind this one lacks, and refusing the row for that would bring back the
/// unreadable log. The resource is not carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownEntry {
    /// Its id.
    pub id: AuditId,
    /// When it was written, by the store's clock.
    pub at: UnixSeconds,
    /// The stored action name, sanitized and capped.
    pub action: UnknownAction,
    /// Who did it.
    pub actor: Actor,
    /// What it was done to, if this binary can read the stored target; else `None`.
    pub target: Option<Target>,
    /// The stored note, sanitized and capped like any detail.
    pub detail: Detail,
}

/// One row of a page of the log: an event this binary knows, or a labelled unknown one.
/// Reading never fails because of an action name (D77); a row that is structurally invalid
/// is still [`StoreError::Corrupt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEntry {
    /// An event the code could have written.
    Event(AuditEntry),
    /// A row with an action this binary does not know.
    Unknown(UnknownEntry),
}

impl LogEntry {
    /// Its id.
    #[must_use]
    pub const fn id(&self) -> AuditId {
        match self {
            Self::Event(entry) => entry.id,
            Self::Unknown(entry) => entry.id,
        }
    }

    /// When it was written.
    #[must_use]
    pub const fn at(&self) -> UnixSeconds {
        match self {
            Self::Event(entry) => entry.at,
            Self::Unknown(entry) => entry.at,
        }
    }

    /// The known event, if this is one.
    #[must_use]
    pub fn into_event(self) -> Option<AuditEntry> {
        match self {
            Self::Event(entry) => Some(entry),
            Self::Unknown(_) => None,
        }
    }
}

/// The events of `entries` this binary knows, in order. **Drops unknown rows**: for callers
/// that look for specific actions (an unknown row cannot be one); a screen of the log must show
/// every [`LogEntry`] instead.
#[must_use]
pub fn known_events(entries: Vec<LogEntry>) -> Vec<AuditEntry> {
    entries
        .into_iter()
        .filter_map(LogEntry::into_event)
        .collect()
}

/// A bounded, paged read, newest first. Every filter is optional and they combine with AND.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditQuery {
    /// Only this actor.
    pub actor: Option<Actor>,
    /// Only this action. Unknown rows (D77) match no action filter.
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

    /// One page of matching rows, newest first, at most [`MAX_PAGE`]. A row whose action this
    /// binary does not know is a [`LogEntry::Unknown`] (D77); a stored row that is structurally
    /// invalid (a bad actor, an unreadable target on a known action) is
    /// [`StoreError::Corrupt`].
    fn page(
        &self,
        query: &AuditQuery,
    ) -> impl Future<Output = Result<Vec<LogEntry>, StoreError>> + Send;
}

/// The empty directory has no log: nothing is ever appended, and nothing is ever read. A
/// test double for the front end, like its other store impls (`NoMembers` has no accounts to
/// act, so no event can be real).
impl AuditStore for crate::access::NoMembers {
    async fn append(&self, _event: &AuditEvent) -> Result<AuditId, StoreError> {
        Err(StoreError::AccountNotFound)
    }

    async fn page(&self, _query: &AuditQuery) -> Result<Vec<LogEntry>, StoreError> {
        Ok(Vec::new())
    }
}
