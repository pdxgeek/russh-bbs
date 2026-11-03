//! The event scheduler (M3-C, D79): named periodic jobs with a persisted last run.
//!
//! # Registering a job (for M3-G and any later slice)
//!
//! ```ignore
//! use std::time::Duration;
//! use russh_bbs_core::access::Slug;
//! use russh_bbs_core::scheduler::{Job, JobFailure};
//!
//! let name = Slug::parse("feeds-poll")?;              // persisted key; a slug, 1-32 chars
//! let job = Job::new(name, Duration::from_secs(900), move || {
//!     let store = store.clone();                      // the closure builds a fresh future per run
//!     async move {
//!         poll_feeds(&store).await.map_err(JobFailure::new) // Err is logged, never fatal
//!     }
//! })?
//! .with_timeout(Duration::from_secs(120))?;           // optional; default 5 minutes
//! scheduler.register(job)?;                           // before `run`; a duplicate name is refused
//! ```
//!
//! `crates/server/src/main.rs` builds one [`Scheduler`] (`Scheduler::new(store, Arc::new(SystemClock))`)
//! and starts it with the server; a slice adds its job with one `register` call there, before the
//! scheduler starts, and makes the binding `let mut scheduler`.
//!
//! # Rules
//!
//! - **Due time.** A job is due when it has never run, or when `last run + interval` has been
//!   reached on the injected [`Clock`](crate::limits::Clock). The last run is the time the job
//!   *started*, stored in `scheduler_job` through [`SchedulerStore`], so a restart does not
//!   re-run a job that ran a minute ago and does not skip one that is overdue.
//! - **A failed run counts as a run.** The attempt time is recorded with the outcome
//!   ([`RunOutcome`]), so a job that keeps failing waits its interval; it does not spin.
//! - **Never twice at once on a process.** Each job has a running flag taken before its last run is
//!   read and held until its outcome is recorded; a second [`Scheduler::tick`] that overlaps the
//!   first skips that job. Nothing coordinates two *processes* on one database; the board runs
//!   one server per database.
//! - **A bad job cannot stop the others or the server.** Each run is its own task with a timeout:
//!   an `Err`, a panic and a timeout are logged, recorded and reported, and the scheduler goes on.
//!   A store that cannot be read makes the scheduler skip the job for this tick (it does not
//!   guess), and is logged.
//! - **Shutdown.** [`Scheduler::run`] stops when its `shutdown` future resolves. A tick in
//!   progress is dropped, which aborts the running job tasks at their next `await`; a job must
//!   therefore be safe to cancel (do its writes in transactions). The aborted run's outcome is not
//!   recorded, so it is due again at the next start.
//! - **Time.** Tests drive [`Scheduler::tick`] with a [`ManualClock`](crate::limits::ManualClock);
//!   nothing here sleeps except [`Scheduler::run`]'s wait between ticks.
//!
//! Job failure text goes to the log only; the store keeps the closed outcome, never a message.

mod job;
mod runner;
mod store;

pub use job::{DEFAULT_TIMEOUT, InvalidJob, Job, JobFailure, MAX_INTERVAL, MIN_INTERVAL};
pub use runner::{DEFAULT_POLL, RegisterError, RunReport, Scheduler};
pub use store::{LastRun, RunOutcome, SchedulerStore};

#[cfg(test)]
mod tests;
