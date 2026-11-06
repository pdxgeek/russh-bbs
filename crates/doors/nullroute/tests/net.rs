//! Network generation: determinism, structure, the fairness guarantee, and the branch tests for
//! the RPG rule (the deck build changes which nodes are reachable).
#![allow(
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::identity_op
)]

mod common;

use common::*;
use proptest::prelude::*;
use russh_bbs_door_nullroute::content::{Content, Lock, Role};
use russh_bbs_door_nullroute::net::{Capability, MAX_NODES, Network, Overlay, Patrol};

fn make(content: &Content, season: u32, corp: u8, tier: u8) -> Network {
    Network::generate(content, season, content.corp(corp).unwrap(), tier).unwrap()
}

fn code<'a>(content: &'a Content, net: &Network, id: u8) -> &'a str {
    content
        .node(net.nodes[usize::from(id)].kind)
        .unwrap()
        .code
        .as_str()
}

fn objectives(content: &Content, net: &Network) -> Vec<u8> {
    ["VT", "AC", "SR"]
        .iter()
        .flat_map(|c| net.targets(content, c).map(|n| n.id).collect::<Vec<_>>())
        .collect()
}

fn connected(net: &Network) -> bool {
    let all = Capability {
        breaks: [9; 3],
        purge: 9,
        scan: u8::MAX,
    };
    let seen = net.reachable(&all, &Overlay::default());
    (0..net.nodes.len()).all(|i| seen & (1 << i) != 0)
}

#[test]
fn the_same_inputs_make_the_same_network_and_other_inputs_make_another() {
    let c = content();
    let a = make(&c, 1, 0, 2);
    let b = make(&c, 1, 0, 2);
    assert_eq!(format!("{:?}", a.nodes), format!("{:?}", b.nodes));
    assert_eq!(a.links, b.links);
    assert_ne!(
        format!("{:?}{:?}", a.nodes, a.links),
        format!("{:?}{:?}", make(&c, 2, 0, 2).nodes, make(&c, 2, 0, 2).links),
        "another season"
    );
    assert_ne!(
        format!("{:?}", a.links),
        format!("{:?}", make(&c, 1, 1, 2).links),
        "another corporation"
    );
}

#[test]
fn corporations_differ_in_style() {
    let c = content();
    let mut honey = [0u32; 5];
    let mut walls = [0u32; 5];
    let mut tracers = [0u32; 5];
    let mut sentries = [0u32; 5];
    let mut locks = [0u32; 5];
    let mut hidden = [0u32; 5];
    for season in 1..=60 {
        for corp in 0..5u8 {
            let net = make(&c, season, corp, 2);
            let i = usize::from(corp);
            honey[i] += u32::from(net.nodes.iter().any(|n| n.honeypot));
            walls[i] += net
                .nodes
                .iter()
                .filter(|n| n.patrol == Patrol::BlackWall)
                .count() as u32;
            tracers[i] += net
                .nodes
                .iter()
                .filter(|n| n.patrol == Patrol::Tracer)
                .count() as u32;
            sentries[i] += net
                .nodes
                .iter()
                .filter(|n| n.patrol == Patrol::Sentry)
                .count() as u32;
            locks[i] += net.nodes.iter().filter(|n| n.lock.is_some()).count() as u32;
            hidden[i] += net.links.iter().filter(|l| l.stealth > 0).count() as u32;
        }
    }
    // Mirrorglass (2) hides vaults behind decoys far more than Pennywhistle (3).
    assert!(honey[2] > honey[3] * 3, "{honey:?}");
    // Halcyon (1) runs more tracers than Kestrel (0); Kestrel locks more than Pennywhistle.
    assert!(tracers[1] > tracers[0], "{tracers:?}");
    assert!(locks[0] > locks[3], "{locks:?}");
    // Orrery (4) is fond of black walls; Pennywhistle has none.
    assert!(walls[4] > walls[1] && walls[3] == 0, "{walls:?}");
    assert!(hidden[2] > hidden[3], "{hidden:?}");
    let _ = sentries;
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn every_generated_network_is_well_formed(season in 0u32..200, corp in 0u8..12, tier in 1u8..=3) {
        let c = content();
        let net = make(&c, season, corp, tier);
        // Shape.
        prop_assert!(net.nodes.len() <= MAX_NODES);
        prop_assert!(net.nodes.len() >= 7);
        for (i, n) in net.nodes.iter().enumerate() {
            prop_assert_eq!(usize::from(n.id), i);
            prop_assert!((1..=9).contains(&n.security));
        }
        prop_assert_eq!(net.nodes[0].layer, 0);
        prop_assert!(net.nodes[0].lock.is_none() && net.nodes[0].patrol == Patrol::None && !net.nodes[0].honeypot);
        prop_assert_eq!(c.node(net.nodes[0].kind).unwrap().role, Role::Entry);
        // Links: in range, no loops, no duplicates, stealth in range.
        for (i, l) in net.links.iter().enumerate() {
            prop_assert!(usize::from(l.a) < net.nodes.len() && usize::from(l.b) < net.nodes.len());
            prop_assert!(l.a != l.b);
            prop_assert!(l.stealth == 0 || (2..2 + tier * 2).contains(&l.stealth));
            prop_assert!(!net.links[..i].iter().any(|o| (o.a == l.a && o.b == l.b) || (o.a == l.b && o.b == l.a)));
            // Links only join layers one or two apart.
            let gap = net.nodes[usize::from(l.a)].layer.abs_diff(net.nodes[usize::from(l.b)].layer);
            prop_assert!((1..=2).contains(&gap));
        }
        // Everything is connected to the entry (hidden links included).
        prop_assert!(connected(&net));
        // Visible links alone connect every layer to the next (no node is cut off if you never scan).
        // The objective floor has a vault, a console and a server room.
        for code_wanted in ["VT", "AC", "SR"] {
            prop_assert!(net.targets(&c, code_wanted).count() >= 1, "{}", code_wanted);
        }
        // A decoy is a vault on the objective floor, never anywhere else.
        for n in net.nodes.iter().filter(|n| n.honeypot) {
            prop_assert_eq!(code(&c, &net, n.id), "VT");
            prop_assert_eq!(n.value, 0);
        }
        // Real vaults hold something.
        for id in net.targets(&c, "VT").map(|n| n.id) {
            prop_assert!(net.nodes[usize::from(id)].value >= 40 * u32::from(tier));
        }
        // Security never exceeds the tier's cap, so the tier's scripts can always answer it.
        let cap = [2, 4, 7][usize::from(tier) - 1];
        prop_assert!(net.nodes.iter().all(|n| n.security <= cap));
        // Black walls only on tiers 2 and up.
        prop_assert!(tier >= 2 || net.nodes.iter().all(|n| n.patrol != Patrol::BlackWall));
        // Names are unique.
        for (i, n) in net.nodes.iter().enumerate() {
            prop_assert!(!net.nodes[..i].iter().any(|o| o.name == n.name));
        }
    }

    #[test]
    fn the_full_toolkit_reaches_every_objective(season in 0u32..200, corp in 0u8..12, tier in 1u8..=3) {
        let c = content();
        let net = make(&c, season, corp, tier);
        let reach = net.reachable(&Capability::full(), &Overlay::default());
        for id in objectives(&c, &net) {
            prop_assert!(reach & (1 << id) != 0);
        }
    }

    #[test]
    fn the_fairness_guarantee_holds_for_the_tier(season in 0u32..200, corp in 0u8..12, tier in 1u8..=3) {
        let c = content();
        let net = make(&c, season, corp, tier);
        let objs = objectives(&c, &net);
        for id in objs {
            let works: Vec<Lock> = Lock::ALL
                .into_iter()
                .filter(|l| net.reaches(id, &Capability::only(*l), &Overlay::default()))
                .collect();
            match tier {
                1 => prop_assert_eq!(works.len(), 3, "tier 1: every family alone reaches {}", id),
                2 => {
                    prop_assert!(works.len() >= 2);
                    prop_assert!(works.contains(&c.corp(corp).unwrap().favoured));
                }
                _ => prop_assert!(works.contains(&c.corp(corp).unwrap().favoured)),
            }
        }
    }

    #[test]
    fn tier_one_is_open_to_the_starter_scripts(season in 0u32..200, corp in 0u8..12) {
        // The three starter breaks are power 2 and the scan is 2: that must be enough at tier 1.
        let c = content();
        let net = make(&c, season, corp, 1);
        let starter = Capability { breaks: [2, 2, 2], purge: 0, scan: 2 };
        let reach = net.reachable(&starter, &Overlay::default());
        for id in objectives(&c, &net) {
            prop_assert!(reach & (1 << id) != 0);
        }
    }

    #[test]
    fn holes_only_ever_widen_what_a_deck_reaches_and_alert_only_narrows(season in 0u32..60, corp in 0u8..12, tier in 1u8..=3, mask in 0u32..(1 << 12)) {
        let c = content();
        let net = make(&c, season, corp, tier);
        let cap = Capability { breaks: [3, 3, 3], purge: 3, scan: 3 };
        let holes: Vec<u8> = (0..net.nodes.len() as u8).filter(|i| mask & (1 << (i % 12)) != 0).collect();
        let base = net.reachable(&cap, &Overlay::default());
        let with = net.reachable(&cap, &Overlay { holes: &holes, alert: 0 });
        prop_assert_eq!(base & with, base, "a hole never closes a way");
        let alerted = net.reachable(&cap, &Overlay { holes: &[], alert: 2 });
        prop_assert_eq!(base & alerted, alerted, "an alert never opens a way");
    }
}

// ---- Branch tests for the RPG rule: the deck decides which nodes are reachable. ----

fn cap(crack: u8, spoof: u8, overflow: u8, purge: u8) -> Capability {
    Capability {
        breaks: [crack, spoof, overflow],
        purge,
        scan: 0,
    }
}

#[test]
fn on_a_known_network_each_deck_reaches_a_different_set_of_nodes() {
    let net = hand_net();
    let none = Overlay::default();
    let reach = |c: Capability| net.reachable(&c, &none);
    // Bare: the router, its camera. (The hidden shortcut 1-5 needs a scan.)
    assert_eq!(reach(cap(0, 0, 0, 0)), 0b1_0000_0011);
    // Crack: also the terminal, and through it the honeypot.
    assert_eq!(reach(cap(2, 0, 0, 0)), 0b1_1001_0011);
    // Spoof: the badge reader, and through it the real vault and the console.
    assert_eq!(reach(cap(0, 2, 0, 0)), 0b1_0110_1011);
    // Overflow: the firewall and through it the real vault, but not the console.
    assert_eq!(reach(cap(0, 0, 2, 0)), 0b1_0010_0111);
    // Power 1 opens the level-1 terminal but not the level-2 badge reader or firewall.
    assert_eq!(reach(cap(1, 0, 0, 0)), reach(cap(2, 0, 0, 0)));
    assert_eq!(
        reach(cap(0, 1, 1, 0)),
        reach(cap(0, 0, 0, 0)),
        "too weak for level 2"
    );
    // Crack plus spoof reaches everything.
    assert_eq!(reach(cap(2, 2, 0, 0)).count_ones(), 8);
    // A scan with the power adds the hidden shortcut to the vault for any deck.
    let scanning = Capability {
        scan: 3,
        ..cap(0, 0, 0, 0)
    };
    assert!(net.reaches(5, &scanning, &none));
    assert!(!net.reaches(5, &cap(0, 0, 0, 0), &none));
}

#[test]
fn a_deck_that_cannot_reach_the_console_can_still_reach_the_vault_by_another_route() {
    let net = hand_net();
    let none = Overlay::default();
    let overflow_only = cap(0, 0, 2, 0);
    assert!(
        net.reaches(5, &overflow_only, &none),
        "vault via the firewall"
    );
    assert!(
        !net.reaches(6, &overflow_only, &none),
        "no console without a badge"
    );
    assert!(net.reaches(6, &cap(0, 2, 0, 0), &none));
}

#[test]
fn across_generated_networks_one_family_often_opens_what_another_does_not() {
    let c = content();
    let mut differing = 0;
    let mut total = 0;
    for season in 1..=40 {
        for corp in 0..5u8 {
            let net = make(&c, season, corp, 3);
            let sets: Vec<u32> = Lock::ALL
                .iter()
                .map(|l| net.reachable(&Capability::only(*l), &Overlay::default()))
                .collect();
            total += 1;
            if sets[0] != sets[1] || sets[1] != sets[2] {
                differing += 1;
            }
        }
    }
    assert!(
        differing * 10 >= total * 9,
        "{differing} of {total} tier-3 networks reward one family over another"
    );
}

#[test]
fn holes_and_alerts_move_the_reach_of_the_same_deck() {
    let net = hand_net();
    let c = cap(0, 0, 2, 0);
    let plain = Overlay::default();
    assert!(net.reaches(5, &c, &plain));
    // The corporation is on alert: the firewall is now level 3 and power 2 cannot open it.
    assert!(!net.reaches(
        5,
        &c,
        &Overlay {
            holes: &[],
            alert: 1
        }
    ));
    // But a hole on the firewall from an earlier run lets even this deck through, alert or not.
    assert!(net.reaches(
        5,
        &c,
        &Overlay {
            holes: &[2],
            alert: 2
        }
    ));
    // And a hole lets a deck with no break at all go through the badge reader.
    assert!(net.reaches(
        6,
        &cap(0, 0, 0, 0),
        &Overlay {
            holes: &[3, 6],
            alert: 0
        }
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// The best scripts anyone can buy (power 6 in every family, a power 6 purge, a power 6 scan)
    /// reach every objective on every network of every tier: no contract is one the shop cannot
    /// answer (M4-N4: tier 3 locks asked for power 7 and one network in five could not be opened).
    #[test]
    fn the_best_scripts_in_the_shop_reach_every_objective(season in 0u32..200, corp in 0u8..12, tier in 1u8..=3) {
        let c = content();
        let net = make(&c, season, corp, tier);
        let cap = Capability { breaks: [6; 3], purge: 6, scan: 6 };
        let mask = net.reachable(&cap, &Overlay::default());
        for code in ["VT", "AC", "SR"] {
            let reached = net
                .nodes
                .iter()
                .filter(|n| !n.honeypot && c.node(n.kind).is_some_and(|d| d.code == code))
                .any(|n| mask & (1 << n.id) != 0);
            prop_assert!(reached, "{code} on season {season} corp {corp} tier {tier}");
        }
    }
}
