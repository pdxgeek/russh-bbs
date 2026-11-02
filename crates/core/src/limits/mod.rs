//! Resource limits per role, with a per-member override (M2-H, D33, D62; `docs/access.md`,
//! "Resource limits").
//!
//! - [`LimitKind`] is the closed set of things that can be limited; [`Limit`] is a number or
//!   unlimited.
//! - [`effective_limits`] is the pure rule: `sysop` unlimited; else the member's override; else
//!   the most generous value among the roles held; else unlimited.
//! - [`LimitStore`] is persistence. [`begin_member_session`] and [`MemberMeter`] enforce what
//!   exists today (concurrent sessions, calls per day, minutes per day); the other kinds are
//!   stored and resolved here and enforced by the slice that owns the action.
//! - [`set_role_limit`], [`set_member_limit`] and [`clear_member_limit`] edit limits, gated by
//!   the `limits.edit` permission.
//! - Edits are audited by the store in the same transaction (`limit.changed`, M2-C).
//! - The order is **permission first, limit second**, both in core. A refusal is a
//!   [`LimitRefusal`] that says which limit and when it resets; nothing is dropped silently.

mod clock;
mod edit;
mod effective;
mod kind;
mod meter;
mod sessions;
mod store;

pub use clock::{Clock, Day, ManualClock, SECONDS_PER_DAY, SystemClock};
pub use edit::{LimitEditError, clear_member_limit, set_member_limit, set_role_limit};
pub use effective::{EffectiveLimits, MemberLimit, RoleLimit, effective_limits, roles_held};
pub use kind::{Limit, LimitKind};
pub use meter::{
    CHECKPOINT_SECONDS, LimitError, LimitRefusal, MemberMeter, USAGE_RETENTION_DAYS,
    begin_member_session, load_effective,
};
pub use sessions::{SessionCounter, SessionSlot};
pub use store::{LimitStore, LimitWriteError, UsageOutcome};

#[cfg(test)]
mod tests;
