//! The shared world: the nightly tick, what a season remembers, and the byte formats.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::identity_op
)]

mod common;

use common::*;
use russh_bbs_door::DoorRng;
use russh_bbs_door_nullroute::player::Player;
use russh_bbs_door_nullroute::run::{RunResult, Status};
use russh_bbs_door_nullroute::save::{decode, decode_world, encode, encode_world};
use russh_bbs_door_nullroute::world::{
    ALERT_MAX, BOARD_MAX, CONTRACT_DAYS, Contract, Learned, NIGHTLY_CONTRACTS, NetState, World,
    gen_contracts,
};

fn broke(corp: u8, objective: bool, planted: bool, holes: &[u8]) -> RunResult {
    let _ = corp;
    RunResult {
        status: Status::Out,
        objective,
        loot: 0,
        heat_cut: 0,
        peak: 10,
        cycles_left: 0,
        broken: holes.to_vec(),
        planted,
        xp: [0; 5],
        used: Vec::new(),
        fragments: Vec::new(),
        defeated: 0,
        warden_down: false,
        fights: 0,
        broke_off: 0,
        warden_woke: false,
    }
}

fn night(world: &mut World, day: i64) {
    world.nightly(&content(), &mut DoorRng::from_label("t", day), day);
}

#[test]
fn each_night_posts_contracts_and_old_ones_lapse() {
    let mut w = World::default();
    night(&mut w, 100);
    assert_eq!(w.board.len(), NIGHTLY_CONTRACTS);
    assert_eq!(w.day, 100);
    let c = content();
    for k in &w.board {
        assert_eq!(k.expires, 100 + CONTRACT_DAYS - 1);
        assert!(k.id / 16 == 100 && (1..=3).contains(&k.tier));
        let tmpl = c.contract(k.tmpl).unwrap();
        assert_eq!(tmpl.tier, k.tier);
        assert!((tmpl.reward_min..=tmpl.reward_max).contains(&k.reward));
        assert_ne!(
            c.corp(k.corp).unwrap().patron,
            k.fixer,
            "a fixer never hires against their own client"
        );
    }
    let mut ids: Vec<u32> = w.board.iter().map(|k| k.id).collect();
    ids.dedup();
    assert_eq!(ids.len(), w.board.len());
    night(&mut w, 101);
    night(&mut w, 102);
    assert_eq!(w.board.len(), 3 * NIGHTLY_CONTRACTS);
    night(&mut w, 103);
    assert!(
        w.board.iter().all(|k| k.expires >= 103),
        "day 100's jobs lapsed"
    );
    assert_eq!(w.board.len(), 3 * NIGHTLY_CONTRACTS);
    // The board never grows past its cap, whatever the tick does.
    for d in 104..140 {
        night(&mut w, d);
        assert!(w.board.len() <= BOARD_MAX);
    }
}

#[test]
fn the_tier_mix_leans_easy_and_every_tier_appears() {
    let c = content();
    let mut rng = DoorRng::from_seed(1);
    let all: Vec<Contract> = (0..400)
        .flat_map(|d| gen_contracts(&c, &mut rng, d, 8))
        .collect();
    let n = |t: u8| all.iter().filter(|k| k.tier == t).count();
    assert!(
        n(1) > n(2) && n(2) > n(3) && n(3) > 0,
        "{} {} {}",
        n(1),
        n(2),
        n(3)
    );
    for tmpl in &c.contracts.contract {
        assert!(
            all.iter().any(|k| k.tmpl == tmpl.num),
            "template {} never posted",
            tmpl.title
        );
    }
}

#[test]
fn a_breach_leaves_holes_an_alert_and_maybe_a_backdoor_for_the_season() {
    let mut w = World::default();
    w.apply_run(0, 2, &broke(0, true, true, &[3, 5, 3]));
    let net = w.net(0, 2).unwrap();
    assert_eq!((net.alert, net.backdoor, net.breaches), (1, true, 1));
    assert_eq!(net.holes, [3, 5], "a hole is remembered once");
    assert_eq!(w.breaches, 1);
    // Failing still leaves the holes you opened, but no alert.
    w.apply_run(1, 1, &broke(1, false, false, &[2]));
    let net = w.net(1, 1).unwrap();
    assert_eq!(
        (net.alert, net.backdoor, net.holes.clone()),
        (0, false, vec![2])
    );
    // The alert saturates.
    for _ in 0..5 {
        w.apply_run(0, 2, &broke(0, true, false, &[]));
    }
    assert_eq!(w.net(0, 2).unwrap().alert, ALERT_MAX);
    assert!(w.net(4, 3).is_none());
}

#[test]
fn the_nightly_patch_closes_holes_by_the_corporations_diligence() {
    let mut c = content();
    let nights = |c: &russh_bbs_door_nullroute::content::Content, patch: u32| {
        let mut c = c.clone();
        for corp in &mut c.corps.corp {
            corp.patch_pct = patch;
        }
        let mut w = World::default();
        w.nets.push(NetState {
            corp: 0,
            tier: 1,
            alert: 2,
            holes: vec![1, 2, 3, 4, 5],
            backdoor: true,
            breaches: 0,
        });
        w.nightly(&c, &mut DoorRng::from_seed(3), 50);
        w
    };
    let sloppy = nights(&c, 0);
    let net = sloppy.net(0, 1).unwrap();
    assert_eq!(
        (net.holes.len(), net.backdoor, net.alert),
        (5, true, 1),
        "nobody patched; the alert still cools"
    );
    let diligent = nights(&c, 100);
    // Alert 2 cooled to 1, so the entry stays, but with no holes.
    let net = diligent.net(0, 1).unwrap();
    assert!(
        net.holes.is_empty(),
        "every hole is patched (a backdoor is patched at half the rate)"
    );
    c.corps.corp.clear();
    // With no corporations in the content, the default patch rate applies and nothing panics.
    let mut w = World::default();
    w.nets.push(NetState {
        corp: 7,
        tier: 1,
        alert: 0,
        holes: vec![1],
        backdoor: false,
        breaches: 0,
    });
    w.nightly(&c, &mut DoorRng::from_seed(1), 5);
}

#[test]
fn over_many_nights_holes_close_at_about_the_patch_rate() {
    let c = content();
    let rate = c.corp(0).unwrap().patch_pct;
    let (mut closed, mut total) = (0u32, 0u32);
    for seed in 0..200 {
        let mut w = World::default();
        w.nets.push(NetState {
            corp: 0,
            tier: 1,
            alert: 0,
            holes: (0..10).collect(),
            backdoor: false,
            breaches: 1,
        });
        w.nightly(&c, &mut DoorRng::from_seed(seed), 9);
        closed += 10 - u32::try_from(w.net(0, 1).unwrap().holes.len()).unwrap();
        total += 10;
    }
    let pct = closed * 100 / total;
    assert!(pct.abs_diff(rate) <= 4, "{pct} vs {rate}");
}

#[test]
fn the_market_moves_but_stays_in_range() {
    let mut w = World::default();
    let mut seen = std::collections::BTreeSet::new();
    for d in 0..300 {
        night(&mut w, d);
        assert!((80..=130).contains(&w.market_pct));
        seen.insert(w.market_pct);
    }
    assert!(seen.len() > 10);
}

#[test]
fn a_tick_depends_only_on_the_world_the_content_and_the_day() {
    let run = || {
        let mut w = World::default();
        for d in 0..10 {
            w.nightly(&content(), &mut DoorRng::from_label("nullroute", d), d);
        }
        encode_world(&w)
    };
    assert_eq!(run(), run());
}

#[test]
fn the_world_format_is_pinned_and_round_trips() {
    let w = World {
        day: 20_001,
        market_pct: 110,
        nets: vec![NetState {
            corp: 2,
            tier: 3,
            alert: 1,
            holes: vec![4, 9],
            backdoor: true,
            breaches: 258,
        }],
        board: vec![Contract {
            id: 320_016,
            tmpl: 5,
            corp: 1,
            fixer: 2,
            tier: 2,
            reward: 150,
            expires: 20_003,
        }],
        breaches: 7,
        season: 3,
        learned: vec![Learned {
            corp: 2,
            sig: 1031,
            uses: 9,
        }],
        ..World::default()
    };
    let bytes = encode_world(&w);
    assert_eq!(
        bytes,
        [
            0x21, 0x4e, 0, 0, 0, 0, 0, 0,   // day
            110, // market
            7, 0, 0, 0, // breaches
            1, // nets
            2, 3, 1, 1, 2, 1, // corp, tier, alert, backdoor, breaches u16
            2, 4, 9, // holes
            1, // board
            0x10, 0xe2, 0x04, 0, 5, 1, 2, 2, 150, 0, 0, 0, 0x23, 0x4e, 0, 0, 0, 0, 0, 0, 3, 0, 0,
            0, // season (version 2)
            1, 2, 0x07, 0x04, 9, // learned: count, corp, sig u16, uses (version 2)
            0, // price history: none (version 3)
            0, 0, 0, 0, // social: next id
            0, 0, 0, 0, 0, // posted, orders, deliveries, bounties, deeds
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, // tallies: nine counters of 4 bytes
            0, 0, // the finale: an empty name, not cracked
            0, 0, 0, 0, // hall, pairs, crews, ops
        ]
    );
    assert_eq!(decode_world(&bytes), Some(w));
}

#[test]
fn garbage_is_never_a_world() {
    let mut w = World::default();
    night(&mut w, 100);
    w.apply_run(0, 1, &broke(0, true, false, &[1, 2]));
    let bytes = encode_world(&w);
    assert_eq!(decode_world(&bytes), Some(w));
    for cut in 0..bytes.len() {
        assert_eq!(decode_world(&bytes[..cut]), None, "prefix {cut}");
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert_eq!(decode_world(&trailing), None);
    assert_eq!(decode_world(&[]), None);
    assert_eq!(decode_world(&[0xff; 64]), None);
}

#[test]
fn a_save_round_trips_and_the_format_is_pinned() {
    let c = content();
    let mut p = Player::new(&c, 0x0102_0304_0506_0708, "Brindle", 3, 20_000);
    p.marks = 1234;
    p.stash = 50;
    p.heat = 17;
    p.rep = 9;
    p.rig = [1, 0, 2, 0, 3];
    p.fried = 0b100;
    p.xp = [1, 2, 3, 4, 5];
    p.bots = 2;
    p.workbench = true;
    p.fence = 1;
    p.standing = [1, -2, 3, 0, 0, 0, 0, -5];
    p.done = vec![320_016, 5];
    p.pending = Some(77);
    p.proxies = 1;
    p.runs = 4;
    p.jobs = 3;
    p.burns = 1;
    p.earned = 99_999;
    let bytes = encode(&p);
    assert_eq!(decode(&bytes), Some(p.clone()));
    // Pinned head: seed, season, name, day.
    assert_eq!(
        bytes[..30],
        [
            8, 7, 6, 5, 4, 3, 2, 1, 3, 0, 0, 0, 7, b'B', b'r', b'i', b'n', b'd', b'l', b'e', 0x20,
            0x4e, 0, 0, 0, 0, 0, 0, 40, 0
        ]
    );
    for cut in 0..bytes.len() {
        assert_eq!(decode(&bytes[..cut]), None, "prefix {cut}");
    }
    let mut trailing = bytes.clone();
    trailing.push(1);
    assert_eq!(decode(&trailing), None);
    // Out-of-range fields are refused, not clamped.
    let mut bad = p.clone();
    bad.rig[0] = 4;
    assert_eq!(decode(&encode(&bad)), None);
    // A name that is not UTF-8.
    let mut bytes = encode(&p);
    bytes[13] = 0xff;
    assert_eq!(decode(&bytes), None);
}

#[test]
fn a_save_stays_far_under_the_framework_cap() {
    let c = content();
    let mut p = Player::new(&c, 1, &"x".repeat(40), 1, 20_000);
    p.owned = (1..=27).collect();
    p.done = (0..48).collect();
    assert!(encode(&p).len() < 700, "{}", encode(&p).len());
    let mut w = World::default();
    for d in 0..30 {
        night(&mut w, d);
        for corp in 0..5 {
            for tier in 1..=3 {
                w.apply_run(
                    corp,
                    tier,
                    &broke(corp, true, true, &(0..20).collect::<Vec<_>>()),
                );
            }
        }
    }
    assert!(encode_world(&w).len() < 4_000, "{}", encode_world(&w).len());
}
