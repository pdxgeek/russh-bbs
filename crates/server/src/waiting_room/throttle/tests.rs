#![allow(clippy::unwrap_used)]

use std::net::IpAddr;
use std::time::Duration;

use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::waiting_room::{ManualClock, RatePolicy};

use super::Throttle;

fn fingerprint(n: u8) -> Fingerprint {
    // 43 base64 characters after the prefix.
    let body: String = format!("{n:03}").repeat(15).chars().take(43).collect();
    Fingerprint::parse(&format!("SHA256:{body}")).unwrap()
}

fn throttle(clock: &ManualClock, max_tracked: usize) -> Throttle<ManualClock> {
    Throttle::new(
        RatePolicy::new(3, Duration::from_secs(10)),
        RatePolicy::new(2, Duration::from_secs(30)),
        max_tracked,
        clock,
    )
}

#[test]
fn one_address_is_limited_after_its_burst_and_others_are_not() {
    let clock = ManualClock::new();
    let throttle = throttle(&clock, 100);
    let noisy: IpAddr = "192.0.2.1".parse().unwrap();
    let quiet: IpAddr = "192.0.2.2".parse().unwrap();
    for _ in 0..3 {
        assert!(throttle.admit_connection(noisy).is_allowed());
    }
    assert!(!throttle.admit_connection(noisy).is_allowed());
    assert!(throttle.admit_connection(quiet).is_allowed());

    clock.advance(Duration::from_secs(10));
    assert!(throttle.admit_connection(noisy).is_allowed(), "refilled");
}

#[test]
fn addresses_in_one_ipv6_slash_64_share_an_allowance() {
    let clock = ManualClock::new();
    let throttle = throttle(&clock, 100);
    for i in 0..3_u16 {
        let addr: IpAddr = format!("2001:db8:0:1::{i:x}").parse().unwrap();
        assert!(throttle.admit_connection(addr).is_allowed());
    }
    let sibling: IpAddr = "2001:db8:0:1:ffff::1".parse().unwrap();
    assert!(!throttle.admit_connection(sibling).is_allowed());
}

#[test]
fn a_key_is_limited_after_its_burst_and_other_keys_are_not() {
    let clock = ManualClock::new();
    let throttle = throttle(&clock, 100);
    let (a, b) = (fingerprint(1), fingerprint(2));
    assert!(throttle.admit_key(&a).is_allowed());
    assert!(throttle.admit_key(&a).is_allowed());
    assert!(!throttle.admit_key(&a).is_allowed());
    assert!(throttle.admit_key(&b).is_allowed());

    clock.advance(Duration::from_secs(30));
    assert!(throttle.admit_key(&a).is_allowed(), "refilled");
}

#[test]
fn the_two_limiters_are_independent() {
    let clock = ManualClock::new();
    let throttle = throttle(&clock, 100);
    let addr: IpAddr = "192.0.2.9".parse().unwrap();
    for _ in 0..3 {
        throttle.admit_connection(addr);
    }
    assert!(!throttle.admit_connection(addr).is_allowed());
    assert!(throttle.admit_key(&fingerprint(3)).is_allowed());
}

#[test]
fn a_flood_of_distinct_addresses_and_keys_cannot_grow_the_tables() {
    let clock = ManualClock::new();
    let throttle = throttle(&clock, 32);
    for i in 0..5_000_u32 {
        let [a, b, c, d] = i.to_be_bytes();
        assert!(
            throttle
                .admit_connection(IpAddr::from([a, b, c, d]))
                .is_allowed()
        );
    }
    for n in 0..100_u8 {
        throttle.admit_key(&fingerprint(n));
    }
    assert!(throttle.networks.tracked() <= 32);
    assert!(throttle.keys.tracked() <= 32);
}
