//! Scheduler rules against an in-memory store and a manual clock. No test sleeps.
#![allow(clippy::unwrap_used, clippy::panic, clippy::unused_async_trait_impl)] // unused_async_trait_impl: in-memory fakes of async traits

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;

use super::*;
use crate::access::Slug;
use crate::limits::{Clock, ManualClock};
use crate::store::{StoreError, UnixSeconds};

#[derive(Default, Clone)]
struct MemoryStore {
    runs: Arc<Mutex<HashMap<String, LastRun>>>,
    fail_reads: Arc<std::sync::atomic::AtomicBool>,
}

impl SchedulerStore for MemoryStore {
    async fn last_run(&self, job: &Slug) -> Result<Option<LastRun>, StoreError> {
        if self.fail_reads.load(Ordering::SeqCst) {
            return Err(StoreError::Corrupt("test".into()));
        }
        Ok(self.runs.lock().unwrap().get(job.as_str()).copied())
    }

    async fn record_run(&self, job: &Slug, run: LastRun) -> Result<(), StoreError> {
        self.runs
            .lock()
            .unwrap()
            .insert(job.as_str().to_owned(), run);
        Ok(())
    }
}

fn name(text: &str) -> Slug {
    Slug::parse(text).unwrap()
}

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(UnixSeconds::from_raw(1_000)))
}

fn counting(name_: &str, every: u64, counter: &Arc<AtomicU32>) -> Job {
    let counter = Arc::clone(counter);
    Job::new(name(name_), Duration::from_secs(every), move || {
        let counter = Arc::clone(&counter);
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    })
    .unwrap()
}

fn scheduler(store: &MemoryStore, clock: &Arc<ManualClock>) -> Scheduler<MemoryStore> {
    Scheduler::new(store.clone(), Arc::clone(clock) as Arc<dyn Clock>)
}

#[tokio::test]
async fn a_job_that_never_ran_is_due_then_waits_its_interval() {
    let (store, clock, runs) = (MemoryStore::default(), clock(), Arc::new(AtomicU32::new(0)));
    let mut s = scheduler(&store, &clock);
    s.register(counting("tidy", 60, &runs)).unwrap();

    assert_eq!(s.tick().await.len(), 1);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    clock.advance(59);
    assert!(s.tick().await.is_empty(), "one second early");
    clock.advance(1);
    assert_eq!(s.tick().await.len(), 1, "exactly at the interval");
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn the_last_run_is_persisted_so_a_restart_does_not_rerun_it() {
    let (store, clock, runs) = (MemoryStore::default(), clock(), Arc::new(AtomicU32::new(0)));
    let mut first = scheduler(&store, &clock);
    first.register(counting("tidy", 60, &runs)).unwrap();
    first.tick().await;
    assert_eq!(
        store.runs.lock().unwrap().get("tidy").copied(),
        Some(LastRun {
            at: UnixSeconds::from_raw(1_000),
            outcome: RunOutcome::Ok
        })
    );

    // A new scheduler over the same store: the process restarted 30 s later.
    clock.advance(30);
    let mut second = scheduler(&store, &clock);
    second.register(counting("tidy", 60, &runs)).unwrap();
    assert_eq!(second.tick().await, Vec::new());
    clock.advance(30);
    assert_eq!(second.tick().await.len(), 1);
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn jobs_are_independent_and_names_are_unique() {
    let (store, clock) = (MemoryStore::default(), clock());
    let (a, b) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)));
    let mut s = scheduler(&store, &clock);
    s.register(counting("fast", 10, &a)).unwrap();
    s.register(counting("slow", 100, &b)).unwrap();
    assert_eq!(
        s.register(counting("fast", 5, &a)),
        Err(RegisterError::Duplicate("fast".into()))
    );
    assert_eq!(s.jobs(), vec![name("fast"), name("slow")]);
    s.tick().await;
    clock.advance(10);
    s.tick().await;
    assert_eq!((a.load(Ordering::SeqCst), b.load(Ordering::SeqCst)), (2, 1));
}

#[tokio::test]
async fn a_failing_job_is_recorded_waits_its_interval_and_does_not_stop_the_others() {
    let (store, clock, good) = (MemoryStore::default(), clock(), Arc::new(AtomicU32::new(0)));
    let mut s = scheduler(&store, &clock);
    s.register(
        Job::new(name("bad"), Duration::from_secs(60), || async {
            Err(JobFailure::new("it broke"))
        })
        .unwrap(),
    )
    .unwrap();
    s.register(counting("good", 60, &good)).unwrap();

    let reports = s.tick().await;
    let outcome = |job: &str| {
        reports
            .iter()
            .find(|r| r.job.as_str() == job)
            .unwrap()
            .outcome
    };
    assert_eq!(outcome("bad"), RunOutcome::Failed);
    assert_eq!(outcome("good"), RunOutcome::Ok);
    assert_eq!(good.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.runs.lock().unwrap()["bad"].outcome,
        RunOutcome::Failed
    );
    // The failed attempt counts as a run: no retry until the interval has passed.
    assert_eq!(s.tick().await, Vec::new());
}

#[tokio::test]
async fn a_panicking_job_is_contained_and_the_scheduler_keeps_going() {
    let (store, clock, good) = (MemoryStore::default(), clock(), Arc::new(AtomicU32::new(0)));
    let mut s = scheduler(&store, &clock);
    s.register(
        Job::new(name("boom"), Duration::from_secs(10), || async {
            panic!("a job panicked on purpose");
        })
        .unwrap(),
    )
    .unwrap();
    s.register(counting("good", 10, &good)).unwrap();

    let reports = s.tick().await;
    assert!(
        reports
            .iter()
            .any(|r| r.job.as_str() == "boom" && r.outcome == RunOutcome::Panicked)
    );
    assert_eq!(good.load(Ordering::SeqCst), 1);
    clock.advance(10);
    let again = s.tick().await;
    assert_eq!(
        again.len(),
        2,
        "the panicking job is due again and runs again"
    );
    assert_eq!(good.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn a_job_that_hangs_is_abandoned_at_its_timeout() {
    let (store, clock) = (MemoryStore::default(), clock());
    let mut s = scheduler(&store, &clock);
    s.register(
        Job::new(name("stuck"), Duration::from_secs(10), || async {
            std::future::pending::<()>().await;
            Ok(())
        })
        .unwrap()
        .with_timeout(Duration::from_secs(30))
        .unwrap(),
    )
    .unwrap();
    let reports = s.tick().await; // the paused runtime jumps to the timeout
    assert_eq!(reports[0].outcome, RunOutcome::TimedOut);
    assert_eq!(
        store.runs.lock().unwrap()["stuck"].outcome,
        RunOutcome::TimedOut
    );
}

#[tokio::test]
async fn overlapping_ticks_never_run_one_job_twice_at_once() {
    let (store, clock) = (MemoryStore::default(), clock());
    let (entered, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let (running, peak, total) = (
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU32::new(0)),
    );
    let job = {
        let (entered, release, running, peak, total) = (
            Arc::clone(&entered),
            Arc::clone(&release),
            Arc::clone(&running),
            Arc::clone(&peak),
            Arc::clone(&total),
        );
        Job::new(name("once"), Duration::from_secs(1), move || {
            let (entered, release, running, peak, total) = (
                Arc::clone(&entered),
                Arc::clone(&release),
                Arc::clone(&running),
                Arc::clone(&peak),
                Arc::clone(&total),
            );
            async move {
                total.fetch_add(1, Ordering::SeqCst);
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                entered.notify_one();
                release.notified().await;
                running.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .unwrap()
    };
    let mut s = scheduler(&store, &clock);
    s.register(job).unwrap();
    let s = Arc::new(s);

    let first = tokio::spawn({
        let s = Arc::clone(&s);
        async move { s.tick().await }
    });
    entered.notified().await; // the first run is in progress
    clock.advance(100); // long overdue, but it has not finished
    assert!(s.tick().await.is_empty(), "the overlapping tick skips it");
    release.notify_one();
    assert_eq!(first.await.unwrap().len(), 1);
    assert_eq!(
        (total.load(Ordering::SeqCst), peak.load(Ordering::SeqCst)),
        (1, 1)
    );
}

#[tokio::test]
async fn a_store_that_cannot_be_read_skips_the_job_instead_of_guessing() {
    let (store, clock, runs) = (MemoryStore::default(), clock(), Arc::new(AtomicU32::new(0)));
    let mut s = scheduler(&store, &clock);
    s.register(counting("tidy", 60, &runs)).unwrap();
    store.fail_reads.store(true, Ordering::SeqCst);
    assert_eq!(s.tick().await, Vec::new());
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    store.fail_reads.store(false, Ordering::SeqCst);
    assert_eq!(
        s.tick().await.len(),
        1,
        "the flag was released; the next tick runs it"
    );
}

#[tokio::test]
async fn run_ticks_until_shutdown_and_then_returns() {
    let (store, clock) = (MemoryStore::default(), clock());
    let stop = Arc::new(Notify::new());
    let ran = Arc::new(AtomicU32::new(0));
    let mut s = scheduler(&store, &clock).with_poll(Duration::from_secs(3600));
    s.register({
        let (stop, ran) = (Arc::clone(&stop), Arc::clone(&ran));
        Job::new(name("then-stop"), Duration::from_secs(60), move || {
            let (stop, ran) = (Arc::clone(&stop), Arc::clone(&ran));
            async move {
                ran.fetch_add(1, Ordering::SeqCst);
                stop.notify_one(); // ask the scheduler to shut down while it is running
                Ok(())
            }
        })
        .unwrap()
    })
    .unwrap();
    s.run(async { stop.notified().await }).await;
    assert_eq!(ran.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn shutdown_aborts_a_running_job_and_frees_it() {
    let (store, clock) = (MemoryStore::default(), clock());
    let entered = Arc::new(Notify::new());
    let mut s = scheduler(&store, &clock);
    s.register({
        let entered = Arc::clone(&entered);
        Job::new(name("long"), Duration::from_secs(60), move || {
            let entered = Arc::clone(&entered);
            async move {
                entered.notify_one();
                std::future::pending::<()>().await;
                Ok(())
            }
        })
        .unwrap()
    })
    .unwrap();
    s.run(async { entered.notified().await }).await; // returns although the job never ends
    assert!(
        store.runs.lock().unwrap().is_empty(),
        "an aborted run records nothing"
    );
    assert!(
        !s.entries[0].running.load(Ordering::SeqCst),
        "the running flag was released"
    );
}

/// Sets a flag when dropped: proof that a job's future is gone, not merely told to stop.
struct DropFlag(Arc<std::sync::atomic::AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Regression for FIX-M4-1: a job mid-poll on another worker used to outlive `run` (dropping a
/// `JoinSet` only requests the abort), so it wrote to the store after the final WAL checkpoint.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_returns_only_after_a_busy_job_is_gone() {
    let (store, clock) = (MemoryStore::default(), clock());
    let entered = Arc::new(Notify::new());
    let gone = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut s = scheduler(&store, &clock);
    s.register({
        let (entered, gone) = (Arc::clone(&entered), Arc::clone(&gone));
        Job::new(name("busy"), Duration::from_secs(60), move || {
            let (entered, gone) = (Arc::clone(&entered), Arc::clone(&gone));
            async move {
                let _flag = DropFlag(gone);
                entered.notify_one();
                // Holds this worker inside one poll, as a job under CPU load can be.
                std::thread::sleep(Duration::from_millis(300));
                std::future::pending::<()>().await;
                Ok(())
            }
        })
        .unwrap()
    })
    .unwrap();
    s.run(async { entered.notified().await }).await;
    assert!(
        gone.load(Ordering::SeqCst),
        "run returned while the job was still running"
    );
}

#[test]
fn bad_jobs_are_refused_at_construction() {
    let work = || async { Ok(()) };
    assert_eq!(
        Job::new(name("x"), Duration::from_millis(999), work).unwrap_err(),
        InvalidJob::Interval
    );
    assert_eq!(
        Job::new(name("x"), MAX_INTERVAL + Duration::from_secs(1), work).unwrap_err(),
        InvalidJob::Interval
    );
    assert_eq!(
        Job::new(name("x"), MIN_INTERVAL, work)
            .unwrap()
            .with_timeout(Duration::ZERO)
            .unwrap_err(),
        InvalidJob::Timeout
    );
}

#[test]
fn outcome_names_round_trip() {
    for outcome in [
        RunOutcome::Ok,
        RunOutcome::Failed,
        RunOutcome::TimedOut,
        RunOutcome::Panicked,
    ] {
        assert_eq!(RunOutcome::parse(outcome.name()), Some(outcome));
    }
    assert_eq!(RunOutcome::parse("nope"), None);
}
