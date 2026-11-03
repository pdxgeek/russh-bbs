//! The scheduler's persistence: one row per job, holding its last run.

use std::future::Future;

use crate::access::Slug;
use crate::store::{StoreError, UnixSeconds};

/// How a run ended. Closed, so the store holds no free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// The job returned `Ok`.
    Ok,
    /// The job returned an error.
    Failed,
    /// The job ran past its timeout and was abandoned.
    TimedOut,
    /// The job panicked.
    Panicked,
}

impl RunOutcome {
    /// The stored name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::TimedOut => "timed-out",
            Self::Panicked => "panicked",
        }
    }

    /// The outcome with this stored name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        [Self::Ok, Self::Failed, Self::TimedOut, Self::Panicked]
            .into_iter()
            .find(|outcome| outcome.name() == name)
    }
}

/// A job's last recorded run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastRun {
    /// When the run started.
    pub at: UnixSeconds,
    /// How it ended.
    pub outcome: RunOutcome,
}

/// Persisted last runs.
pub trait SchedulerStore: Send + Sync {
    /// The job's last run, or `None` if it has never run.
    fn last_run(
        &self,
        job: &Slug,
    ) -> impl Future<Output = Result<Option<LastRun>, StoreError>> + Send;

    /// Record a run, replacing the job's earlier one.
    fn record_run(
        &self,
        job: &Slug,
        run: LastRun,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}
