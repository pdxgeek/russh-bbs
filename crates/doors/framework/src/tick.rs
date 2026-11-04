//! The daily tick: one scheduler job per door that has days (M3-C, D79).
//!
//! A tick is a pure function of the shared world, the day and a per-day seeded RNG
//! ([`Door::daily_tick`](crate::Door::daily_tick)). The framework makes it safe to run often:
//!
//! - **Idempotent per day.** The world blob and the last day applied are written together in one
//!   compare-and-swap, so a day is applied once however often [`run_due`] runs and however many
//!   processes race (the loser sees a conflict, reloads, and finds nothing to do).
//! - **Catches up after downtime, with a cap.** After a gap, the missed days are applied oldest
//!   first, but at most [`TickPolicy::max_catch_up`] of them; older days are skipped and
//!   reported. The first ever run applies only today.
//! - **All or nothing.** If the door fails on any day, nothing is written and the run reports
//!   the error; the next run starts again from the same stored day.
//! - **Never backwards.** If the clock reads an earlier day than the last one applied, nothing
//!   runs (a day is never applied twice).
//! - **Same numbers anywhere.** The RNG for a day is seeded from the door id and the day number
//!   only, so a replay of the world, or a second process, computes the same result.
//!
//! [`job`] wraps it as a scheduler [`Job`] running hourly; `main` registers one per door that
//! reports [`Door::has_daily_tick`](crate::Door::has_daily_tick).

use std::sync::Arc;
use std::time::Duration;

use russh_bbs_core::doors::{DoorStore, DoorStoreError, Revision, WorldWrite};
use russh_bbs_core::limits::{Clock, Day};
use russh_bbs_core::scheduler::{InvalidJob, Job, JobFailure};
use russh_bbs_core::store::StoreError;
use tracing::{info, warn};

use crate::door::Door;
use crate::host::{OpenError, load_world};
use crate::rng::DoorRng;

/// How often the scheduler job checks whether a new day has started.
pub const CHECK_EVERY: Duration = Duration::from_hours(1);

/// The most catch-up days a policy may allow.
pub const MAX_CATCH_UP_LIMIT: u32 = 31;

/// How a tick run behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickPolicy {
    /// At most this many missed days are applied after downtime (1 to [`MAX_CATCH_UP_LIMIT`]).
    pub max_catch_up: u32,
}

impl Default for TickPolicy {
    fn default() -> Self {
        Self { max_catch_up: 7 }
    }
}

/// Why a tick failed.
#[derive(Debug, thiserror::Error)]
pub enum TickError {
    /// The door's tick refused (a bug or bad data in the door).
    #[error("the door's daily tick failed: {0}")]
    Door(String),
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The world could not be read or migrated.
    #[error(transparent)]
    World(#[from] OpenError),
    /// The world grew past its cap.
    #[error("the door's world is over its size cap")]
    TooLarge,
}

/// What one run did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TickReport {
    /// The days applied, oldest first.
    pub applied: Vec<Day>,
    /// Missed days beyond the cap that were not applied.
    pub skipped: u32,
    /// Another process applied the day first; this run wrote nothing.
    pub lost_race: bool,
}

/// What a door's tick sees and changes: the day, its RNG and the world.
pub struct TickCtx<'a> {
    day: Day,
    rng: DoorRng,
    world: &'a mut Option<Vec<u8>>,
}

impl TickCtx<'_> {
    /// The day being applied.
    #[must_use]
    pub const fn day(&self) -> Day {
        self.day
    }

    /// This day's random numbers (seeded by door and day).
    pub fn rng(&mut self) -> &mut DoorRng {
        &mut self.rng
    }

    /// The world so far (`None` before the first write).
    #[must_use]
    pub fn world(&self) -> Option<&[u8]> {
        self.world.as_deref()
    }

    /// Replace the world.
    pub fn set_world(&mut self, bytes: Vec<u8>) {
        *self.world = Some(bytes);
    }
}

/// Apply every due day for `door`, if any. See the [module docs](self).
///
/// # Errors
/// See [`TickError`]. Nothing is written on an error.
pub async fn run_due<S: DoorStore>(
    store: &S,
    door: &dyn Door,
    clock: &dyn Clock,
    policy: TickPolicy,
) -> Result<TickReport, TickError> {
    let info = door.info();
    let today = Day::of(clock.now());
    let cap = i64::from(policy.max_catch_up.clamp(1, MAX_CATCH_UP_LIMIT));
    let loaded = load_world(store, door).await?;
    let (mut world, revision, last) = match loaded {
        Some((bytes, revision, last)) => (Some(bytes), revision, last),
        None => (None, Revision::NONE, None),
    };
    let first_due = match last {
        None => today,
        Some(last) if last >= today => {
            if last > today {
                warn!(door = %info.id, last = last.as_raw(), today = today.as_raw(),
                    "the clock reads an earlier day than the last tick; nothing applied");
            }
            return Ok(TickReport::default());
        }
        Some(last) => Day::from_raw(last.as_raw() + 1),
    };
    let missed = today.as_raw() - first_due.as_raw() + 1;
    let skipped = u32::try_from((missed - cap).max(0)).unwrap_or(u32::MAX);
    let start = Day::from_raw(first_due.as_raw() + (missed - cap).max(0));
    let mut applied = Vec::new();
    for raw in start.as_raw()..=today.as_raw() {
        let day = Day::from_raw(raw);
        let mut ctx = TickCtx {
            day,
            rng: DoorRng::from_label(info.id.as_str(), raw),
            world: &mut world,
        };
        door.daily_tick(&mut ctx)?;
        applied.push(day);
    }
    let bytes = world.unwrap_or_default();
    let write = WorldWrite {
        expected: revision,
        version: info.world_version,
        bytes: &bytes,
        tick: Some(today),
    };
    match store.write_world(&info.id, &write).await {
        Ok(_) => {
            info!(door = %info.id, days = applied.len(), skipped, "door tick applied");
            Ok(TickReport {
                applied,
                skipped,
                lost_race: false,
            })
        }
        Err(DoorStoreError::Conflict) => Ok(TickReport {
            applied: Vec::new(),
            skipped: 0,
            lost_race: true,
        }),
        Err(DoorStoreError::TooLarge { .. }) => Err(TickError::TooLarge),
        Err(DoorStoreError::Store(error)) => Err(error.into()),
    }
}

/// The scheduler job for `door`'s daily tick: hourly, so a restart or a missed hour costs at
/// most an hour of lateness, and idempotent so running it often is free.
///
/// # Errors
/// Never in practice; the interval is a valid constant.
pub fn job<S>(
    store: Arc<S>,
    door: Arc<dyn Door>,
    clock: Arc<dyn Clock>,
    policy: TickPolicy,
) -> Result<Job, InvalidJob>
where
    S: DoorStore + 'static,
{
    let name = door.info().id.tick_job();
    Job::new(name, CHECK_EVERY, move || {
        let store = Arc::clone(&store);
        let door = Arc::clone(&door);
        let clock = Arc::clone(&clock);
        async move {
            run_due(store.as_ref(), door.as_ref(), clock.as_ref(), policy)
                .await
                .map(|_| ())
                .map_err(JobFailure::new)
        }
    })
}
