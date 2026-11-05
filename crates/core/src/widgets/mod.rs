//! Widgets: small live panels the pane layouts place (UI-10).
//!
//! Business rules live here; the server only draws. The pieces:
//!
//! * [`WidgetId`]: the closed set of built-in widgets, and who may be shown each
//!   ([`WidgetId::reachable_by`], the same permission as the screen it summarises).
//! * [`Rules`] and [`Choice`]: the sysop's hide/lock/default settings and a member's per-layout
//!   list, joined by [`Rules::resolve`] (a lock the sysop adds later wins over a stored list).
//! * [`WidgetStore`]: persistence of the member's lists, on both backends.
//! * [`fetch`] and [`WidgetData`]: what the widgets show, read through the services the screens
//!   use (so no widget can show a board, mail or poll the viewer cannot open) and sanitized.
//!
//! Nothing here is "private": a widget shows what the server can read, to the viewer who may
//! see it, and never carries a security marker (markers are chrome, drawn by the frame).

mod data;
mod id;
mod place;
mod store;

pub use data::{
    Caller, Credits, DoorLine, Headline, LIST_ROWS, Line, MailCount, NewPosts, PollView,
    RecentMail, RoomCount, SERIES_DAYS, Status, WidgetData, balance_series, fetch,
};
pub use id::WidgetId;
pub use place::{Choice, EditError, Lock, MAX_PER_LAYOUT, Rules, RulesError, builtin_default};
pub use store::{WidgetStore, save};

#[cfg(test)]
mod tests;
