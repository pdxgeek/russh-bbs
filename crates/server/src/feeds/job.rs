//! The scheduler job that reads the feeds that are due (M3-C's scheduler, D79; M3-G, D89).

use std::sync::Arc;
use std::time::Duration;

use russh_bbs_core::access::Slug;
use russh_bbs_core::feeds::{FeedStore, Fetch, poll_due};
use russh_bbs_core::limits::{Clock, SystemClock};
use russh_bbs_core::scheduler::{InvalidJob, Job, JobFailure};

/// The job's persisted name.
pub const JOB_NAME: &str = "feeds-poll";
/// How often the scheduler asks which feeds are due. A feed's own interval (default an hour)
/// decides whether it is.
pub const CHECK_EVERY: Duration = Duration::from_secs(300);
/// Most feeds one run reads, so a run stays inside its timeout.
pub const MAX_FEEDS_PER_RUN: usize = 10;
/// A run's timeout: ten feeds at the fetcher's 20 seconds each, with room to spare.
const RUN_TIMEOUT: Duration = Duration::from_secs(280);

/// Build the job. Register it with `scheduler.register(job)?` before the scheduler starts.
///
/// # Errors
/// [`InvalidJob`] if the interval or timeout were out of range (they are constants, so never).
pub fn job<S, F>(store: S, fetcher: Arc<F>) -> Result<Job, InvalidJob>
where
    S: FeedStore + Clone + 'static,
    F: Fetch + 'static,
{
    // A constant slug; the fallback is unreachable but keeps this free of a panic path.
    let name = Slug::parse(JOB_NAME).map_err(|_| InvalidJob::Interval)?;
    Job::new(name, CHECK_EVERY, move || {
        let store = store.clone();
        let fetcher = Arc::clone(&fetcher);
        async move {
            poll_due(&store, &*fetcher, SystemClock.now(), MAX_FEEDS_PER_RUN)
                .await
                .map(|_| ())
                .map_err(JobFailure::new)
        }
    })?
    .with_timeout(RUN_TIMEOUT)
}
