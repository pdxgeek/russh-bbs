//! The scheduler itself: decides what is due, runs it in isolation, records the outcome.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};

use super::job::{Job, JobFailure, RunFuture};
use super::store::{LastRun, RunOutcome, SchedulerStore};
use crate::access::Slug;
use crate::limits::Clock;
use crate::store::UnixSeconds;

/// How often [`Scheduler::run`] looks for due jobs.
pub const DEFAULT_POLL: Duration = Duration::from_secs(5);

/// Why a job could not be registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegisterError {
    /// A job with this name is already registered.
    #[error("a job named {0:?} is already registered")]
    Duplicate(String),
}

/// One run [`Scheduler::tick`] made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    /// The job.
    pub job: Slug,
    /// How its run ended.
    pub outcome: RunOutcome,
}

pub(super) struct Entry {
    job: Job,
    pub(super) running: AtomicBool,
}

/// Holds a job's running flag; releases it when dropped, including when the tick is cancelled.
struct RunningGuard(Arc<Entry>);

impl RunningGuard {
    fn take(entry: &Arc<Entry>) -> Option<Self> {
        entry
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(Arc::clone(entry)))
    }
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::Release);
    }
}

/// Runs registered jobs when they are due. See the [module docs](super).
pub struct Scheduler<S> {
    store: S,
    clock: Arc<dyn Clock>,
    poll: Duration,
    pub(super) entries: Vec<Arc<Entry>>,
}

impl<S: SchedulerStore> Scheduler<S> {
    /// A scheduler with no jobs.
    pub fn new(store: S, clock: Arc<dyn Clock>) -> Self {
        Self {
            store,
            clock,
            poll: DEFAULT_POLL,
            entries: Vec::new(),
        }
    }

    /// Look for due jobs every `poll` in [`Scheduler::run`] (at least 10 ms).
    #[must_use]
    pub fn with_poll(mut self, poll: Duration) -> Self {
        self.poll = poll.max(Duration::from_millis(10));
        self
    }

    /// Add a job. Names are unique.
    pub fn register(&mut self, job: Job) -> Result<(), RegisterError> {
        if self.entries.iter().any(|e| e.job.name == job.name) {
            return Err(RegisterError::Duplicate(job.name.as_str().to_owned()));
        }
        info!(
            job = job.name.as_str(),
            interval_seconds = job.interval_seconds,
            "job registered"
        );
        self.entries.push(Arc::new(Entry {
            job,
            running: AtomicBool::new(false),
        }));
        Ok(())
    }

    /// The names of the registered jobs, in registration order.
    #[must_use]
    pub fn jobs(&self) -> Vec<Slug> {
        self.entries.iter().map(|e| e.job.name.clone()).collect()
    }

    /// Run every job that is due now, all at once, and return when they have finished and their
    /// outcomes are recorded. A job that is still running from an overlapping call is skipped.
    pub async fn tick(&self) -> Vec<RunReport> {
        let now = self.clock.now();
        // A task yields `None` when its run passed the timeout.
        let mut tasks: JoinSet<Option<Result<(), JobFailure>>> = JoinSet::new();
        // task id -> (job, guard); the guard lives until the outcome is recorded.
        let mut started: HashMap<tokio::task::Id, (Arc<Entry>, RunningGuard)> = HashMap::new();
        for entry in &self.entries {
            // Take the flag first: two overlapping ticks cannot both pass this.
            let Some(guard) = RunningGuard::take(entry) else {
                debug!(job = entry.job.name.as_str(), "still running; skipped");
                continue;
            };
            match self.store.last_run(&entry.job.name).await {
                Ok(last) if !is_due(last, entry.job.interval_seconds, now) => continue,
                Ok(_) => {}
                Err(error) => {
                    warn!(job = entry.job.name.as_str(), %error, "cannot read the last run; job skipped");
                    continue;
                }
            }
            let work: RunFuture = (entry.job.run)();
            let timeout = entry.job.timeout;
            let handle = tasks.spawn(async move { tokio::time::timeout(timeout, work).await.ok() });
            started.insert(handle.id(), (Arc::clone(entry), guard));
        }

        let mut reports = Vec::new();
        while let Some(joined) = tasks.join_next_with_id().await {
            let (id, outcome) = match joined {
                Ok((id, finished)) => (id, outcome_of(finished)),
                Err(join_error) => (join_error.id(), failed_to_join(&join_error)),
            };
            let Some((entry, _guard)) = started.remove(&id) else {
                continue;
            };
            let name = &entry.job.name;
            match outcome {
                RunOutcome::Ok => debug!(job = name.as_str(), "job finished"),
                RunOutcome::Failed => {} // the message was logged in `outcome_of`
                RunOutcome::TimedOut => {
                    error!(job = name.as_str(), "job timed out and was abandoned");
                }
                RunOutcome::Panicked => error!(job = name.as_str(), "job panicked"),
            }
            if let Err(error) = self
                .store
                .record_run(name, LastRun { at: now, outcome })
                .await
            {
                // The job ran but is not marked as run, so it will be due again next tick.
                error!(job = name.as_str(), %error, "cannot record the run");
            }
            reports.push(RunReport {
                job: name.clone(),
                outcome,
            });
        }
        reports
    }

    /// Tick every poll interval until `shutdown` resolves, then return. Never fails: nothing a
    /// job or the store does ends this loop.
    pub async fn run(&self, shutdown: impl Future<Output = ()>) {
        tokio::pin!(shutdown);
        info!(jobs = self.entries.len(), "scheduler started");
        loop {
            tokio::select! {
                () = &mut shutdown => break,
                _ = self.tick() => {}
            }
            tokio::select! {
                () = &mut shutdown => break,
                () = tokio::time::sleep(self.poll) => {}
            }
        }
        info!("scheduler stopped");
    }
}

fn is_due(last: Option<LastRun>, interval_seconds: i64, now: UnixSeconds) -> bool {
    last.is_none_or(|last| now.as_raw() >= last.at.as_raw().saturating_add(interval_seconds))
}

fn outcome_of(finished: Option<Result<(), JobFailure>>) -> RunOutcome {
    match finished {
        Some(Ok(())) => RunOutcome::Ok,
        Some(Err(failure)) => {
            warn!(%failure, "job failed");
            RunOutcome::Failed
        }
        None => RunOutcome::TimedOut,
    }
}

fn failed_to_join(error: &tokio::task::JoinError) -> RunOutcome {
    if error.is_panic() {
        RunOutcome::Panicked
    } else {
        // Cancelled: only when the runtime is shutting down.
        RunOutcome::Failed
    }
}
