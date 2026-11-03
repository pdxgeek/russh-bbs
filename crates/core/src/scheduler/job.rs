//! What a scheduled job is: a name, an interval and a way to build the work.

use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::access::Slug;

/// The shortest interval a job may have.
pub const MIN_INTERVAL: Duration = Duration::from_secs(1);

/// The longest interval a job may have (one year), so due-time arithmetic cannot overflow.
pub const MAX_INTERVAL: Duration = Duration::from_hours(365 * 24);

/// How long a run may take before it is abandoned, unless the job says otherwise.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_mins(5);

/// Why a job could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidJob {
    /// The interval is outside [`MIN_INTERVAL`]..=[`MAX_INTERVAL`].
    #[error("a job's interval must be between 1 second and 1 year")]
    Interval,
    /// The timeout is zero.
    #[error("a job's timeout must be more than zero")]
    Timeout,
}

/// Why a run failed, for the log. It is never stored or shown to a member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobFailure(Cow<'static, str>);

impl JobFailure {
    /// A failure described by `message`.
    pub fn new(message: impl fmt::Display) -> Self {
        Self(Cow::Owned(message.to_string()))
    }
}

impl fmt::Display for JobFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The future one run produces.
pub(super) type RunFuture = Pin<Box<dyn Future<Output = Result<(), JobFailure>> + Send + 'static>>;

/// A named periodic job.
#[derive(Clone)]
pub struct Job {
    pub(super) name: Slug,
    pub(super) interval_seconds: i64,
    pub(super) timeout: Duration,
    pub(super) run: Arc<dyn Fn() -> RunFuture + Send + Sync>,
}

impl fmt::Debug for Job {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Job")
            .field("name", &self.name)
            .field("interval_seconds", &self.interval_seconds)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl Job {
    /// A job that runs `work` every `every` (at least once a second, at most once a year).
    /// `work` is called once per run and returns that run's future.
    pub fn new<F, Fut>(name: Slug, every: Duration, work: F) -> Result<Self, InvalidJob>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), JobFailure>> + Send + 'static,
    {
        if every < MIN_INTERVAL || every > MAX_INTERVAL {
            return Err(InvalidJob::Interval);
        }
        Ok(Self {
            name,
            // Bounded by MAX_INTERVAL, so this always fits.
            interval_seconds: i64::try_from(every.as_secs()).map_err(|_| InvalidJob::Interval)?,
            timeout: DEFAULT_TIMEOUT,
            run: Arc::new(move || Box::pin(work())),
        })
    }

    /// Abandon a run that takes longer than `timeout`.
    pub fn with_timeout(mut self, timeout: Duration) -> Result<Self, InvalidJob> {
        if timeout.is_zero() {
            return Err(InvalidJob::Timeout);
        }
        self.timeout = timeout;
        Ok(self)
    }

    /// The job's name.
    #[must_use]
    pub fn name(&self) -> &Slug {
        &self.name
    }
}
