#![allow(clippy::unwrap_used)]

use std::net::IpAddr;
use std::time::Duration;

use super::{KeyedLimiter, ManualClock, RatePolicy, Verdict, client_network};

const SECOND: Duration = Duration::from_secs(1);

fn limiter(burst: u32, capacity: usize) -> (KeyedLimiter<u32, ManualClock>, ManualClock) {
    let clock = ManualClock::new();
    (
        KeyedLimiter::new(RatePolicy::new(burst, SECOND), capacity, clock.clone()),
        clock,
    )
}

#[test]
fn a_burst_is_allowed_then_the_source_is_limited() {
    let (limiter, _clock) = limiter(3, 100);
    for _ in 0..3 {
        assert!(limiter.check(&1).is_allowed());
    }
    assert_eq!(
        limiter.check(&1),
        Verdict::Limited {
            retry_after: SECOND
        }
    );
}

#[test]
fn sources_do_not_share_an_allowance() {
    let (limiter, _clock) = limiter(1, 100);
    assert!(limiter.check(&1).is_allowed());
    assert!(!limiter.check(&1).is_allowed());
    assert!(limiter.check(&2).is_allowed());
}

#[test]
fn the_allowance_refills_one_action_per_interval() {
    let (limiter, clock) = limiter(2, 100);
    assert!(limiter.check(&1).is_allowed());
    assert!(limiter.check(&1).is_allowed());
    assert!(!limiter.check(&1).is_allowed());

    clock.advance(Duration::from_millis(999));
    assert!(!limiter.check(&1).is_allowed(), "not yet one full interval");
    clock.advance(Duration::from_millis(1));
    assert!(limiter.check(&1).is_allowed(), "one action earned back");
    assert!(!limiter.check(&1).is_allowed(), "and only one");
}

#[test]
fn a_limited_attempt_costs_nothing() {
    let (limiter, clock) = limiter(1, 100);
    assert!(limiter.check(&1).is_allowed());
    for _ in 0..50 {
        assert!(!limiter.check(&1).is_allowed());
    }
    clock.advance(SECOND);
    assert!(
        limiter.check(&1).is_allowed(),
        "hammering while limited must not extend the wait"
    );
}

#[test]
fn retry_after_shrinks_as_time_passes() {
    let (limiter, clock) = limiter(1, 100);
    assert!(limiter.check(&1).is_allowed());
    clock.advance(Duration::from_millis(400));
    assert_eq!(
        limiter.check(&1),
        Verdict::Limited {
            retry_after: Duration::from_millis(600)
        }
    );
}

#[test]
fn a_long_idle_period_does_not_bank_more_than_the_burst() {
    let (limiter, clock) = limiter(2, 100);
    assert!(limiter.check(&1).is_allowed());
    clock.advance(Duration::from_secs(3600));
    assert!(limiter.check(&1).is_allowed());
    assert!(limiter.check(&1).is_allowed());
    assert!(!limiter.check(&1).is_allowed());
}

#[test]
fn degenerate_policies_are_clamped_not_rejected() {
    let clock = ManualClock::new();
    let limiter = KeyedLimiter::new(RatePolicy::new(0, Duration::ZERO), 0, clock);
    assert!(limiter.check(&1_u8).is_allowed());
    assert!(!limiter.check(&1_u8).is_allowed());
}

#[test]
fn memory_stays_bounded_under_a_flood_of_distinct_sources() {
    let capacity = 64;
    let (limiter, _clock) = limiter(1, capacity);
    for source in 0..10_000_u32 {
        assert!(
            limiter.check(&source).is_allowed(),
            "a new source is allowed"
        );
        assert!(limiter.tracked() <= capacity, "table grew past its cap");
    }
    assert!(limiter.tracked() > 0);
}

#[test]
fn idle_sources_are_purged_before_busy_ones_are_evicted() {
    let (limiter, clock) = limiter(1, 4);
    // Four sources fill the table.
    for source in 0..4 {
        assert!(limiter.check(&source).is_allowed());
    }
    // A second later all four are idle again, so a newcomer displaces none of the busy ones.
    clock.advance(SECOND);
    assert!(limiter.check(&10).is_allowed());
    assert_eq!(limiter.tracked(), 1, "idle entries were purged");
    assert!(!limiter.check(&10).is_allowed(), "the newcomer is tracked");
}

#[test]
fn a_busy_limited_source_survives_a_modest_flood() {
    let (limiter, _clock) = limiter(1, 64);
    assert!(limiter.check(&0).is_allowed());
    // Fewer new sources than the table has room for: nobody is evicted.
    for source in 1..32 {
        assert!(limiter.check(&source).is_allowed());
    }
    assert!(!limiter.check(&0).is_allowed(), "source 0 is still limited");
}

#[test]
fn ipv4_is_limited_per_address() {
    let a: IpAddr = "192.0.2.1".parse().unwrap();
    let b: IpAddr = "192.0.2.2".parse().unwrap();
    assert_ne!(client_network(a), client_network(b));
}

#[test]
fn ipv6_is_limited_per_slash_64() {
    let a: IpAddr = "2001:db8:1:2:aaaa:bbbb:cccc:dddd".parse().unwrap();
    let b: IpAddr = "2001:db8:1:2:1:2:3:4".parse().unwrap();
    let other: IpAddr = "2001:db8:1:3::1".parse().unwrap();
    assert_eq!(client_network(a), client_network(b));
    assert_ne!(client_network(a), client_network(other));
}

#[test]
fn ipv4_mapped_ipv6_counts_as_ipv4() {
    let mapped: IpAddr = "::ffff:192.0.2.1".parse().unwrap();
    let plain: IpAddr = "192.0.2.1".parse().unwrap();
    assert_eq!(client_network(mapped), client_network(plain));
}
