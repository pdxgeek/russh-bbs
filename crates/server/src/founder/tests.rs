#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::founder::{ClaimOutcome, claim_founder};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::waiting_room::{ManualClock, RatePolicy, Verdict};
use russh_bbs_store::SqliteStore;

use super::*;

fn fingerprint(n: u32) -> Fingerprint {
    Fingerprint::parse(&format!("SHA256:{n:A>43}")).unwrap()
}

async fn store() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
    (dir, store)
}

/// Pull the token out of the announcement, the way a sysop (or a test) reads the console.
fn token_in(output: &str) -> String {
    output
        .lines()
        .find(|l| l.starts_with("    "))
        .unwrap()
        .trim()
        .to_owned()
}

#[tokio::test]
async fn an_unclaimed_board_prints_a_token_once_and_a_claimed_board_prints_nothing() {
    let (_dir, store) = store().await;
    let mut out = Vec::new();
    let founder = start(&store, &mut out).await.unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("SETUP TOKEN"));
    assert_eq!(token_in(&text).len(), 39);
    assert!(founder.gate().is_open());

    let outcome = claim_founder(founder.gate(), &store, &token_in(&text), &fingerprint(1)).await;
    assert!(matches!(outcome, ClaimOutcome::Claimed(_)));

    let mut again = Vec::new();
    let reopened = start(&store, &mut again).await.unwrap();
    assert!(again.is_empty(), "nothing is printed once claimed");
    assert!(!reopened.gate().is_open());
}

#[tokio::test]
async fn a_restart_while_unclaimed_issues_a_new_token_and_voids_the_old_one() {
    let (_dir, store) = store().await;
    let mut first = Vec::new();
    let _ = start(&store, &mut first).await.unwrap();
    let old = token_in(&String::from_utf8(first).unwrap());

    let mut second = Vec::new();
    let restarted = start(&store, &mut second).await.unwrap();
    let new = token_in(&String::from_utf8(second).unwrap());
    assert_ne!(old, new);

    let stale = claim_founder(restarted.gate(), &store, &old, &fingerprint(1)).await;
    assert_eq!(stale, ClaimOutcome::Rejected);
    let fresh = claim_founder(restarted.gate(), &store, &new, &fingerprint(1)).await;
    assert!(matches!(fresh, ClaimOutcome::Claimed(_)));
}

#[tokio::test]
async fn the_database_holds_no_trace_of_the_token() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bbs.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let mut out = Vec::new();
    let founder = start(&store, &mut out).await.unwrap();
    let token = token_in(&String::from_utf8(out).unwrap());
    let outcome = claim_founder(founder.gate(), &store, &token, &fingerprint(1)).await;
    assert!(matches!(outcome, ClaimOutcome::Claimed(_)));
    store.close().await;
    // The WAL is checkpointed on close; scan every file the database owns.
    let plain = token.replace('-', "");
    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        let hay = String::from_utf8_lossy(&bytes);
        assert!(!hay.contains(&token) && !hay.contains(&plain));
    }
}

#[test]
fn attempts_are_rate_limited_per_key_and_per_network() {
    let clock = ManualClock::new();
    let founder = Founder::with_rates(
        FounderGate::closed(),
        RatePolicy::new(2, Duration::from_secs(30)),
        RatePolicy::new(4, Duration::from_secs(10)),
        &clock,
    );
    let ip: IpAddr = "203.0.113.9".parse().unwrap();
    let key = fingerprint(1);
    assert!(founder.admit_attempt(&key, Some(ip)).is_allowed());
    assert!(founder.admit_attempt(&key, Some(ip)).is_allowed());
    assert!(matches!(
        founder.admit_attempt(&key, Some(ip)),
        Verdict::Limited { .. }
    ));
    // Another key from the same network is still allowed until the network limit.
    assert!(
        founder
            .admit_attempt(&fingerprint(2), Some(ip))
            .is_allowed()
    );
    assert!(matches!(
        founder.admit_attempt(&fingerprint(3), Some(ip)),
        Verdict::Limited { .. }
    ));
    clock.advance(Duration::from_secs(60));
    assert!(founder.admit_attempt(&key, Some(ip)).is_allowed());
}

#[test]
fn the_founder_name_is_a_valid_username() {
    assert!(sanitize_username(russh_bbs_core::founder::FOUNDER_USERNAME).is_ok());
}
