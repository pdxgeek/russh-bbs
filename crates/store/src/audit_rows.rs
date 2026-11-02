//! Audit rows and their one-time mapping to and from core's types, shared by both backends.
//! The SQL (placeholders, `RETURNING`) is in each backend's own `audit.rs`.

use russh_bbs_core::audit::{
    Action, Actor, AuditEntry, AuditEvent, AuditId, AuditQuery, Detail, LogEntry, Target,
    UnknownAction, UnknownEntry,
};
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

/// The stored columns of an event (everything but id and time).
pub(crate) struct Columns {
    pub(crate) actor_kind: &'static str,
    pub(crate) actor_id: Option<i64>,
    pub(crate) action: &'static str,
    pub(crate) target_kind: &'static str,
    pub(crate) target_ref: String,
    pub(crate) resource_kind: &'static str,
    pub(crate) resource_id: i64,
    pub(crate) detail: String,
}

impl Columns {
    pub(crate) fn of(event: &AuditEvent) -> Self {
        let (actor_kind, actor_id) = match event.actor {
            Actor::System => ("system", None),
            Actor::Account(id) => ("account", Some(id.as_raw())),
        };
        let (target_kind, target_ref) = event.target.parts();
        let (resource_kind, resource_id) = event.resource_parts();
        Self {
            actor_kind,
            actor_id,
            action: event.action.name(),
            target_kind,
            target_ref,
            resource_kind,
            resource_id,
            detail: event.detail.as_str().to_owned(),
        }
    }
}

/// An `audit_log` row as read by the checked queries.
pub(crate) struct AuditRow {
    pub(crate) id: i64,
    pub(crate) at: i64,
    pub(crate) actor_kind: String,
    pub(crate) actor_id: Option<i64>,
    pub(crate) action: String,
    pub(crate) target_kind: String,
    pub(crate) target_ref: String,
    pub(crate) resource_kind: String,
    pub(crate) resource_id: i64,
    pub(crate) detail: String,
}

impl AuditRow {
    /// Validate the stored text. A row the code could not have written is `Corrupt`, with one
    /// exception (D77): an action name this binary does not know reads as an
    /// [`LogEntry::Unknown`] and is never an error, so one renamed or foreign action cannot
    /// make the append-only log unreadable. Its actor is still validated; its target is kept
    /// only if this binary can read it.
    pub(crate) fn into_entry(self) -> Result<LogEntry, StoreError> {
        let corrupt = |what: &str| StoreError::Corrupt(format!("stored audit row {what}"));
        let actor = match (self.actor_kind.as_str(), self.actor_id) {
            ("system", None) => Actor::System,
            ("account", Some(id)) => Actor::Account(AccountId::from_raw(id)),
            _ => return Err(corrupt("has an invalid actor")),
        };
        let id = AuditId::from_raw(self.id);
        let at = UnixSeconds::from_raw(self.at);
        let target = Target::from_parts(&self.target_kind, &self.target_ref);
        let Some(action) = Action::parse(&self.action) else {
            return Ok(LogEntry::Unknown(UnknownEntry {
                id,
                at,
                action: UnknownAction::from_stored(&self.action),
                actor,
                target,
                detail: Detail::new(&self.detail),
            }));
        };
        let target = target.ok_or_else(|| corrupt("has an invalid target"))?;
        let resource = AuditEvent::resource_from_parts(&self.resource_kind, self.resource_id)
            .ok_or_else(|| corrupt("has an invalid resource"))?;
        let mut event = AuditEvent::new(actor, action, target).detail(&self.detail);
        event.resource = resource;
        Ok(LogEntry::Event(AuditEntry { id, at, event }))
    }
}

/// A query as bind values; `None` means "no filter on this".
pub(crate) struct Filter {
    pub(crate) actor_kind: Option<&'static str>,
    pub(crate) actor_id: Option<i64>,
    pub(crate) action: Option<&'static str>,
    pub(crate) since: Option<i64>,
    pub(crate) until: Option<i64>,
    pub(crate) before: Option<i64>,
    pub(crate) limit: i64,
}

impl Filter {
    pub(crate) fn of(query: &AuditQuery) -> Self {
        let (actor_kind, actor_id) = match query.actor {
            None => (None, None),
            Some(Actor::System) => (Some("system"), None),
            Some(Actor::Account(id)) => (Some("account"), Some(id.as_raw())),
        };
        Self {
            actor_kind,
            actor_id,
            action: query.action.map(Action::name),
            since: query.since.map(UnixSeconds::as_raw),
            until: query.until.map(UnixSeconds::as_raw),
            before: query.before.map(AuditId::as_raw),
            limit: i64::from(query.page_size()),
        }
    }
}
