//! The scheduler's persisted last runs against any backend (M3-C, D79), and a scheduler run on
//! top of a real store.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use russh_bbs_core::access::Slug;
use russh_bbs_core::limits::{Clock, ManualClock};
use russh_bbs_core::scheduler::{Job, JobFailure, LastRun, RunOutcome, Scheduler, SchedulerStore};
use russh_bbs_core::store::UnixSeconds;

use super::Harness;

fn name(text: &str) -> Slug {
    Slug::parse(text).unwrap()
}

fn at(seconds: i64) -> UnixSeconds {
    UnixSeconds::from_raw(seconds)
}

pub(crate) async fn a_last_run_round_trips_and_a_later_one_replaces_it<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    assert_eq!(s.last_run(&name("tidy")).await.unwrap(), None);

    for (when, outcome) in [
        (10, RunOutcome::Ok),
        (20, RunOutcome::Failed),
        (30, RunOutcome::TimedOut),
        (40, RunOutcome::Panicked),
    ] {
        let run = LastRun {
            at: at(when),
            outcome,
        };
        s.record_run(&name("tidy"), run).await.unwrap();
        assert_eq!(s.last_run(&name("tidy")).await.unwrap(), Some(run));
    }
    // Other jobs are unaffected.
    assert_eq!(s.last_run(&name("other")).await.unwrap(), None);
}

pub(crate) async fn the_database_refuses_an_unknown_outcome_and_a_stored_one_is_not_misread<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    assert!(
        h.raw("INSERT INTO scheduler_job (name, last_run_at, last_outcome) VALUES ('x', 1, 'wat')")
            .await
            .is_err()
    );
    assert!(
        h.raw("INSERT INTO scheduler_job (name, last_run_at, last_outcome) VALUES ('', 1, 'ok')")
            .await
            .is_err()
    );
    assert_eq!(s.last_run(&name("x")).await.unwrap(), None);
}

pub(crate) async fn a_scheduler_over_the_store_remembers_its_runs_across_a_restart<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = Arc::new(ManualClock::new(at(5_000)));
    let runs = Arc::new(AtomicU32::new(0));
    let job = |runs: &Arc<AtomicU32>| {
        let runs = Arc::clone(runs);
        Job::new(name("count"), Duration::from_secs(60), move || {
            let runs = Arc::clone(&runs);
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .unwrap()
    };
    let failing = Job::new(name("always-fails"), Duration::from_secs(60), || async {
        Err(JobFailure::new("on purpose"))
    })
    .unwrap();

    let mut first = Scheduler::new(h.store().clone(), Arc::clone(&clock) as Arc<dyn Clock>);
    first.register(job(&runs)).unwrap();
    first.register(failing.clone()).unwrap();
    assert_eq!(first.tick().await.len(), 2);
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    // The server restarts 30 s later: neither job is due.
    h.reopen().await;
    clock.advance(30);
    let mut second = Scheduler::new(h.store().clone(), Arc::clone(&clock) as Arc<dyn Clock>);
    second.register(job(&runs)).unwrap();
    second.register(failing).unwrap();
    assert_eq!(second.tick().await.len(), 0);

    clock.advance(30);
    assert_eq!(second.tick().await.len(), 2);
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    assert_eq!(
        h.store().last_run(&name("always-fails")).await.unwrap(),
        Some(LastRun {
            at: at(5_060),
            outcome: RunOutcome::Failed
        })
    );
}

pub(crate) async fn a_broken_store_errors_instead_of_reading_as_never_run<H: Harness>() {
    let h = H::fresh().await;
    h.close().await;
    let s = h.store();
    assert!(s.last_run(&name("tidy")).await.is_err());
    let run = LastRun {
        at: at(1),
        outcome: RunOutcome::Ok,
    };
    assert!(s.record_run(&name("tidy"), run).await.is_err());
}
