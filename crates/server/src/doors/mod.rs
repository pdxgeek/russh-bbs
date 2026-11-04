//! The server side of the door framework (M4-D0, D98): the door list, the session hand-off and
//! the executor that runs a door's saves against the store.
//!
//! This module is thin on purpose. The rules are in `russh_bbs_core::doors` (saves, caps,
//! seasons, bars, the staff tools) and in the `russh-bbs-door` crate (the `Door` trait, the
//! sandbox, the clock, the RNG, the tick); this module only
//!
//! * lists the doors the board has built in ([`Doors`]),
//! * turns the member's keys into door keys and a door's canvas into the member's box
//!   ([`DoorsScreen`], [`render`]),
//! * and runs the two things a door needs the store for, opening and flushing, with the member's
//!   permission snapshot and a time limit ([`run`]).
//!
//! **The hand-off.** Choosing Doors in the main menu moves the session here (the member's flow
//! waits underneath, as for forums and mail). Choosing a door opens it; the door gets keys and a
//! canvas inside the same box and status row as every other screen, so the security markers, the
//! transport's footer row and the logoff keys (Ctrl-C and Ctrl-D, which a door never sees) stay
//! the board's. A door that answers `Exit`, or Esc at the door list, hands the session back.
//!
//! **Over ssh and over telnet** this module is the same code: the transport only decides whether
//! the screen reserves a footer row, which is already outside the area a door is given.

mod draw;
mod exec;
mod screen;
mod strings;

use std::sync::Arc;

use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::limits::{Clock, SystemClock};
use russh_bbs_core::scheduler::{Scheduler, SchedulerStore};
use russh_bbs_door::Door;
use russh_bbs_door::tick::{self, TickPolicy};
use russh_bbs_door_emberdeep::Emberdeep;
use russh_bbs_door_example::CoinToss;
use russh_bbs_door_hallpass::Hallpass;
use russh_bbs_door_nullroute::Nullroute;
use russh_bbs_door_saltmarch::Saltmarch;

pub(crate) use draw::{DrawCtx, render};
pub(crate) use exec::{Request, Response, run};
pub(crate) use screen::{DoorEffect, DoorsScreen};

/// The doors this board has built in. Order is the order of the door list.
#[derive(Clone)]
pub struct Doors {
    entries: Vec<Arc<dyn Door>>,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for Doors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.entries.iter().map(|door| door.info().id.as_str()))
            .finish()
    }
}

impl Doors {
    /// A registry of exactly `entries`.
    #[must_use]
    pub fn new(entries: Vec<Arc<dyn Door>>) -> Self {
        Self {
            entries,
            clock: Arc::new(SystemClock),
        }
    }

    /// The same doors on another clock (tests set the day).
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// The clock doors and their ticks read.
    #[must_use]
    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// The doors that ship with the board: the example coin toss, until a game replaces it as
    /// the first door, and the games (Saltmarch, Emberdeep; the rest register here when they exist).
    ///
    /// # Errors
    /// Only if a built-in door has an invalid id or broken content, which tests rule out.
    pub fn builtin() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self::new(vec![
            CoinToss::shared()?,
            Saltmarch::shared()?,
            Emberdeep::shared()?,
            Nullroute::shared()?,
            Hallpass::shared()?,
        ]))
    }

    /// No doors: the Doors menu item then opens an empty list.
    #[must_use]
    pub fn none() -> Self {
        Self::new(Vec::new())
    }

    /// The doors, in list order.
    #[must_use]
    pub fn all(&self) -> &[Arc<dyn Door>] {
        &self.entries
    }

    /// The doors that have a daily tick (the scheduler gets one job each).
    pub fn with_daily_tick(&self) -> impl Iterator<Item = &Arc<dyn Door>> {
        self.entries.iter().filter(|door| door.has_daily_tick())
    }
}

/// Why the doors' ticks could not be scheduled. A start-up error.
#[derive(Debug, thiserror::Error)]
pub enum TickSetupError {
    /// A tick job could not be built.
    #[error("a door tick job is invalid: {0}")]
    Job(#[from] russh_bbs_core::scheduler::InvalidJob),
    /// Two jobs share a name (two doors with the same id).
    #[error("a door tick job could not be registered: {0}")]
    Register(#[from] russh_bbs_core::scheduler::RegisterError),
}

/// Register one daily-tick job per door that has one, on the board's scheduler (M3-C, D79).
/// Returns how many were registered. Call it before the scheduler starts.
///
/// # Errors
/// See [`TickSetupError`].
pub fn register_ticks<S>(
    scheduler: &mut Scheduler<S>,
    store: &S,
    doors: &Doors,
) -> Result<usize, TickSetupError>
where
    S: DoorStore + SchedulerStore + Clone + 'static,
{
    let shared = Arc::new(store.clone());
    let mut count = 0;
    for door in doors.with_daily_tick() {
        let job = tick::job(
            Arc::clone(&shared),
            Arc::clone(door),
            Arc::clone(doors.clock()),
            TickPolicy::default(),
        )?;
        scheduler.register(job)?;
        count += 1;
    }
    Ok(count)
}

impl PartialEq for Doors {
    fn eq(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && Arc::ptr_eq(&self.clock, &other.clock)
            && self
                .entries
                .iter()
                .zip(&other.entries)
                .all(|(a, b)| Arc::ptr_eq(a, b))
    }
}

impl Eq for Doors {}

#[cfg(test)]
mod tests;
