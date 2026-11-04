#![allow(clippy::unwrap_used, clippy::panic)]

use std::net::IpAddr;
use std::time::Duration;

use super::gate::{AttemptGate, Backoff, DEFAULT_HANDLE_BACKOFF, DEFAULT_NETWORK_BACKOFF};
use crate::waiting_room::{ManualClock, Verdict};

const SECOND: Duration = Duration::from_secs(1);

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[allow(clippy::unnecessary_wraps, reason = "the gate takes an optional peer")]
fn ip(text: &str) -> Option<IpAddr> {
    Some(text.parse().unwrap())
}

fn gate(network: Backoff) -> (AttemptGate<ManualClock>, ManualClock) {
    let clock = ManualClock::new();
    (
        AttemptGate::new(DEFAULT_HANDLE_BACKOFF, network, 1000, clock.clone()),
        clock,
    )
}

/// A network that never limits, to look at the handle alone.
fn handle_only() -> (AttemptGate<ManualClock>, ManualClock) {
    gate(Backoff::UNLIMITED)
}

#[test]
fn the_handle_wait_doubles_after_the_free_failures_and_stops_at_the_ceiling() {
    let steps: Vec<u64> = (0..=12)
        .map(|n| DEFAULT_HANDLE_BACKOFF.wait_after(n).as_secs())
        .collect();
    // 4 free, then 1, 2, 4, 8, 16, and 30 from then on.
    assert_eq!(steps, [0, 0, 0, 0, 0, 1, 2, 4, 8, 16, 30, 30, 30]);
    assert_eq!(
        DEFAULT_HANDLE_BACKOFF.wait_after(u32::MAX),
        Duration::from_millis(u64::from(DEFAULT_HANDLE_BACKOFF.ceiling_ms))
    );
    let network: Vec<u64> = (0..=10)
        .map(|n| DEFAULT_NETWORK_BACKOFF.wait_after(n).as_secs())
        .collect();
    assert_eq!(network, [0, 0, 0, 0, 5, 10, 20, 40, 80, 160, 300]);
}

#[test]
fn free_failures_cost_no_wait_then_the_handle_backs_off_step_by_step() {
    let (gate, clock) = handle_only();
    for _ in 0..4 {
        assert!(gate.admit(None, "carol").is_allowed());
        gate.failed(None, "carol");
    }
    assert!(gate.admit(None, "carol").is_allowed(), "4 free failures");
    for wait in [1, 2, 4, 8, 16, 30, 30] {
        gate.failed(None, "carol");
        assert_eq!(
            gate.admit(None, "carol"),
            Verdict::Limited {
                retry_after: secs(wait)
            }
        );
        clock.advance(secs(wait).saturating_sub(Duration::from_millis(1)));
        assert!(!gate.admit(None, "carol").is_allowed(), "not quite yet");
        clock.advance(Duration::from_millis(1));
        assert!(gate.admit(None, "carol").is_allowed());
    }
}

#[test]
fn knocking_while_waiting_does_not_lengthen_the_wait() {
    let (gate, clock) = handle_only();
    for _ in 0..9 {
        gate.failed(None, "carol");
    }
    let Verdict::Limited { retry_after } = gate.admit(None, "carol") else {
        panic!("limited");
    };
    for _ in 0..1000 {
        assert!(!gate.admit(None, "carol").is_allowed());
    }
    clock.advance(SECOND);
    let Verdict::Limited { retry_after: later } = gate.admit(None, "carol") else {
        panic!("still limited");
    };
    assert_eq!(later, retry_after.saturating_sub(SECOND));
}

#[test]
fn a_correct_password_clears_the_handle() {
    let (gate, _clock) = handle_only();
    for _ in 0..9 {
        gate.failed(None, "carol");
    }
    assert!(!gate.admit(None, "carol").is_allowed());
    gate.succeeded("carol");
    assert!(gate.admit(None, "carol").is_allowed());
    // And the count starts over: four more free failures.
    for _ in 0..4 {
        gate.failed(None, "carol");
    }
    assert!(gate.admit(None, "carol").is_allowed());
}

#[test]
fn a_correct_password_does_not_clear_the_network() {
    let (gate, _clock) = gate(DEFAULT_NETWORK_BACKOFF);
    for _ in 0..3 {
        gate.failed(ip("203.0.113.9"), "bob");
    }
    gate.failed(ip("203.0.113.9"), "bob");
    // The same address then signs in to an account of its own.
    gate.succeeded("mallory");
    assert!(!gate.admit(ip("203.0.113.9"), "mallory").is_allowed());
}

#[test]
fn failures_are_forgotten_after_a_quiet_spell() {
    let (gate, clock) = handle_only();
    for _ in 0..8 {
        gate.failed(None, "carol");
    }
    clock.advance(DEFAULT_HANDLE_BACKOFF.forget());
    assert!(gate.admit(None, "carol").is_allowed());
    // One more failure counts as the first, not the ninth.
    gate.failed(None, "carol");
    assert!(gate.admit(None, "carol").is_allowed());
}

#[test]
fn handles_do_not_share_a_count_and_unknown_names_are_limited_alike() {
    let (gate, _clock) = handle_only();
    for _ in 0..6 {
        gate.failed(None, "carol");
        gate.failed(None, "ghost");
    }
    assert!(!gate.admit(None, "carol").is_allowed());
    assert!(!gate.admit(None, "ghost").is_allowed());
    assert!(gate.admit(None, "bob").is_allowed());
    assert_eq!(
        gate.admit(None, "carol"),
        gate.admit(None, "ghost"),
        "an unknown name waits exactly as a member does"
    );
}

#[test]
fn the_handle_key_is_lowercased_and_junk_shares_one_key() {
    assert_eq!(AttemptGate::<ManualClock>::handle_key(" Carol "), "carol");
    assert_eq!(AttemptGate::<ManualClock>::handle_key("a\u{7}b"), "");
    assert_eq!(AttemptGate::<ManualClock>::handle_key(""), "");
}

#[test]
fn one_network_is_stopped_before_the_handle_waits_and_never_holds_it_shut() {
    let (gate, clock) = gate(DEFAULT_NETWORK_BACKOFF);
    let attacker = ip("198.51.100.7");
    let elsewhere = ip("203.0.113.5");
    for attempt in 1..=25_u32 {
        // The attacker waits out the network's wait each time.
        while let Verdict::Limited { retry_after } = gate.admit(attacker, "carol") {
            // While the network waits, the handle's own wait is shorter, so the owner (from
            // another address) is never blocked longer than the attacker.
            if let Verdict::Limited { retry_after: owner } = gate.admit(elsewhere, "carol") {
                assert!(
                    owner < retry_after,
                    "attempt {attempt}: {owner:?} {retry_after:?}"
                );
            }
            clock.advance(retry_after);
        }
        gate.failed(attacker, "carol");
        if attempt <= 4 {
            // The handle has its 4 free failures, whatever the network does.
            assert!(gate.admit(elsewhere, "carol").is_allowed());
        }
        if attempt == 4 {
            assert!(
                !gate.admit(attacker, "carol").is_allowed(),
                "the network waits first"
            );
            assert!(
                gate.admit(elsewhere, "carol").is_allowed(),
                "the handle has not backed off"
            );
        }
    }
}

#[test]
fn a_network_sweeping_many_handles_is_stopped_by_the_network() {
    let (gate, _clock) = gate(DEFAULT_NETWORK_BACKOFF);
    let peer = ip("198.51.100.7");
    for name in ["a", "b", "c", "d"] {
        assert!(gate.admit(peer, name).is_allowed());
        gate.failed(peer, name);
    }
    assert!(
        !gate.admit(peer, "e").is_allowed(),
        "a fresh handle, same network"
    );
    assert!(gate.admit(ip("203.0.113.5"), "e").is_allowed());
}

#[test]
fn an_ipv6_network_counts_as_its_64() {
    let (gate, _clock) = gate(DEFAULT_NETWORK_BACKOFF);
    for last in 1..=4_u16 {
        gate.failed(ip(&format!("2001:db8:1:2::{last}")), "x");
    }
    assert!(!gate.admit(ip("2001:db8:1:2:aaaa::9"), "y").is_allowed());
    assert!(gate.admit(ip("2001:db8:1:3::1"), "y").is_allowed());
}

#[test]
fn the_tables_stay_bounded() {
    let clock = ManualClock::new();
    let gate = AttemptGate::new(DEFAULT_HANDLE_BACKOFF, DEFAULT_NETWORK_BACKOFF, 16, clock);
    for n in 0..500_u32 {
        gate.failed(
            ip(&format!("10.{}.{}.1", n / 250, n % 250)),
            &format!("h{n}"),
        );
    }
    let (handles, networks) = gate.tracked();
    assert!(handles <= 16 && networks <= 16, "{handles} {networks}");
}
