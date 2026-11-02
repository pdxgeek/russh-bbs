use std::time::{Duration, Instant};

use super::SessionBudget;
use crate::ssh::limits::Limits;

fn limits(requests: u32, bytes: u32, strikes: u32) -> Limits {
    Limits {
        requests_per_second: requests,
        input_bytes_per_second: bytes,
        flood_strikes: strikes,
        ..Limits::default()
    }
}

#[test]
fn a_burst_up_to_two_seconds_of_rate_is_allowed_then_requests_are_refused() {
    let t0 = Instant::now();
    let mut budget = SessionBudget::new(&limits(10, 1000, 1000), t0);
    let allowed = (0..100).filter(|_| budget.request(t0)).count();
    assert_eq!(allowed, 20, "10/s with a 2 s burst");
}

#[test]
fn requests_refill_at_the_rate() {
    let t0 = Instant::now();
    let mut budget = SessionBudget::new(&limits(10, 1000, 1000), t0);
    while budget.request(t0) {}
    let later = t0 + Duration::from_secs(1);
    let allowed = (0..100).filter(|_| budget.request(later)).count();
    assert_eq!(allowed, 10);
}

#[test]
fn input_beyond_the_budget_is_cut_to_what_is_left() {
    let t0 = Instant::now();
    let mut budget = SessionBudget::new(&limits(100, 1000, 1000), t0);
    assert_eq!(budget.input(t0, 1500), 1500, "inside the 2000 byte burst");
    assert_eq!(budget.input(t0, 1500), 500, "only 500 left");
    assert_eq!(budget.input(t0, 10), 0);
    assert_eq!(budget.dropped_bytes(), 1010);
}

#[test]
fn a_flood_collects_strikes_and_is_exceeded() {
    let t0 = Instant::now();
    let mut budget = SessionBudget::new(&limits(10, 1000, 50), t0);
    for _ in 0..1000 {
        budget.request(t0);
    }
    assert!(budget.exceeded());
}

#[test]
fn ordinary_use_never_collects_a_strike() {
    let t0 = Instant::now();
    let mut budget = SessionBudget::new(&limits(200, 8192, 1000), t0);
    // A fast typist: 30 key presses a second for a minute, one byte each.
    for tick in 0..1800_u64 {
        let now = t0 + Duration::from_millis(tick * 33);
        assert!(budget.request(now));
        assert_eq!(budget.input(now, 1), 1);
    }
    assert!(!budget.exceeded());
    assert_eq!(budget.dropped_bytes(), 0);
}

#[test]
fn strikes_are_forgotten_after_a_quiet_spell() {
    let t0 = Instant::now();
    let mut budget = SessionBudget::new(&limits(1, 1000, 30), t0);
    for _ in 0..25 {
        budget.request(t0);
    }
    assert!(!budget.exceeded());
    let later = t0 + Duration::from_secs(60);
    for _ in 0..25 {
        budget.request(later);
    }
    assert!(!budget.exceeded(), "the old strikes were forgotten");
}
