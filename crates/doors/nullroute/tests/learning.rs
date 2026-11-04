//! Script degradation (M4-N2): a corporation learns the scripts used against it too often, a
//! learned script stops working there, and scripts recover over the nights and the seasons.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::assert_is_empty
)]

mod common;

use common::*;
use russh_bbs_door::DoorRng;
use russh_bbs_door_nullroute::content::{Content, Lock};
use russh_bbs_door_nullroute::learn::{self, DECAY_PER_NIGHT, LEARNED_AT, Stage, WARY_AT};
use russh_bbs_door_nullroute::net::{Network, Node};
use russh_bbs_door_nullroute::run::{Act, Refused, Run, RunResult, Status};
use russh_bbs_door_nullroute::world::World;

const HEAP_SPRAY: u8 = 12;
const ROOT_SHELL: u8 = 13;

fn used(sig: u16) -> RunResult {
    RunResult {
        status: Status::Out,
        objective: false,
        loot: 0,
        heat_cut: 0,
        peak: 0,
        cycles_left: 0,
        broken: Vec::new(),
        planted: false,
        xp: [0; 5],
        used: vec![sig],
        fragments: Vec::new(),
        defeated: 0,
        warden_down: false,
        fights: 0,
        broke_off: 0,
        warden_woke: false,
    }
}

/// The hand network with a hard overflow lock (level 4) on the south door.
fn hard_net() -> Network {
    let mut net = hand_net();
    let node: &mut Node = &mut net.nodes[2];
    node.lock = Some(Lock::Overflow);
    node.security = 4;
    net
}

fn run_with(deck: &[u8], learned: Vec<(u16, u32)>, c: &Content) -> Run {
    let mut s = setup(hard_net(), deck);
    s.learned = learned;
    s.cycles = 200;
    // Soak the trace so that a long run of misses is not a burn: these tests are about the script.
    s.buffer = 2000;
    Run::new(c, s).unwrap()
}

fn open_south(run: &mut Run, c: &Content, slot: usize) -> Result<bool, Refused> {
    let mut r = DoorRng::from_seed(3);
    for _ in 0..40 {
        if run.is_open(2) {
            return Ok(true);
        }
        run.act(
            c,
            Act::Use {
                slot,
                target: Some(2),
            },
            &mut r,
        )?;
    }
    Ok(run.is_open(2))
}

#[test]
fn the_stages_follow_the_uses_and_the_starters_are_exempt() {
    let c = content();
    let spray = c.script(HEAP_SPRAY).unwrap();
    assert_eq!(learn::stage(spray, 0), Stage::Fresh);
    assert_eq!(learn::stage(spray, WARY_AT - 1), Stage::Fresh);
    assert_eq!(learn::stage(spray, WARY_AT), Stage::Wary);
    assert_eq!(learn::stage(spray, LEARNED_AT - 1), Stage::Wary);
    assert_eq!(learn::stage(spray, LEARNED_AT), Stage::Patched);
    // Everyone has the starters; nobody patches for them.
    for s in c.scripts.script.iter().filter(|s| s.starter) {
        assert_eq!(learn::stage(s, 250), Stage::Fresh, "{}", s.name);
    }
}

#[test]
fn a_patched_script_does_nothing_and_a_different_deck_gets_through() {
    let c = content();
    // Fresh: heap-spray (power 4) opens the level-4 lock.
    let mut fresh = run_with(&[PING, HEAP_SPRAY], Vec::new(), &c);
    assert_eq!(open_south(&mut fresh, &c, 1), Ok(true));
    // Patched: the same deck is refused, free of cost, and the lock stays shut.
    let mut patched = run_with(&[PING, HEAP_SPRAY], vec![(u16::from(HEAP_SPRAY), 25)], &c);
    let (cycles, trace) = (patched.cycles, patched.trace);
    assert_eq!(patched.stage(&c, 1), Stage::Patched);
    assert_eq!(open_south(&mut patched, &c, 1), Err(Refused::Patched));
    assert_eq!((patched.cycles, patched.trace), (cycles, trace));
    // A deck with another script for the same lock still works: degradation forces the choice.
    let mut other = run_with(
        &[PING, HEAP_SPRAY, ROOT_SHELL],
        vec![(u16::from(HEAP_SPRAY), 25)],
        &c,
    );
    assert_eq!(open_south(&mut other, &c, 2), Ok(true));
    // And a stock deck of starters never could: the lock is level 4.
    let mut starters = run_with(&[PING, STACK_SMASH], Vec::new(), &c);
    assert_eq!(open_south(&mut starters, &c, 1), Ok(false));
}

#[test]
fn a_wary_corporation_blunts_a_script_by_a_point_and_makes_it_louder() {
    let c = content();
    // Heap-spray is power 4 against a level 4 lock: wary makes it 3, and a weaker script never works.
    let mut wary = run_with(
        &[PING, HEAP_SPRAY],
        vec![(u16::from(HEAP_SPRAY), WARY_AT)],
        &c,
    );
    assert_eq!(wary.stage(&c, 1), Stage::Wary);
    assert_eq!(open_south(&mut wary, &c, 1), Ok(false), "{:?}", wary.log);
    // Against a lock it could never open anyway, the miss is louder when they are wary: 15 a try
    // against 10, both doubled for the miss.
    let miss = |learned: Vec<(u16, u32)>| {
        let mut net = hard_net();
        net.nodes[2].security = 6;
        let mut s = setup(net, &[PING, HEAP_SPRAY]);
        s.learned = learned;
        let mut run = Run::new(&c, s).unwrap();
        run.act(
            &c,
            Act::Use {
                slot: 1,
                target: Some(2),
            },
            &mut DoorRng::from_seed(4),
        )
        .unwrap();
        run.trace
    };
    let fresh = miss(Vec::new());
    let known = miss(vec![(u16::from(HEAP_SPRAY), WARY_AT)]);
    assert!(known > fresh, "{known} > {fresh}");
}

#[test]
fn only_the_corporation_that_was_hit_learns() {
    let mut w = World::default();
    for _ in 0..LEARNED_AT {
        w.apply_run(0, 1, &used(u16::from(HEAP_SPRAY)));
    }
    assert_eq!(w.learned_by(0), vec![(u16::from(HEAP_SPRAY), LEARNED_AT)]);
    assert!(
        w.learned_by(1).is_empty(),
        "other corporations know nothing"
    );
    // A run counts a script once, however often it fired inside it.
    let mut one = World::default();
    one.apply_run(2, 1, &used(7));
    assert_eq!(one.learned_by(2), vec![(7, 1)]);
}

#[test]
fn scripts_recover_over_the_nights_and_a_new_season_starts_clean() {
    let c = content();
    let mut w = World::default();
    w.sync_season(1);
    for _ in 0..LEARNED_AT + 4 {
        w.apply_run(0, 1, &used(u16::from(HEAP_SPRAY)));
    }
    let spray = c.script(HEAP_SPRAY).unwrap();
    let stage_of = |w: &World| {
        let uses = w
            .learned_by(0)
            .iter()
            .find(|(s, _)| *s == u16::from(HEAP_SPRAY))
            .map_or(0, |(_, n)| *n);
        learn::stage(spray, uses)
    };
    assert_eq!(stage_of(&w), Stage::Patched);
    let mut nights: u32 = 0;
    let mut day = 100;
    while stage_of(&w) != Stage::Fresh {
        day += 1;
        w.nightly(&c, &mut DoorRng::from_label("t", day), day);
        nights += 1;
        assert!(nights < 40, "it must recover");
    }
    assert!(
        nights >= LEARNED_AT / DECAY_PER_NIGHT / 2,
        "recovery takes time: {nights} nights"
    );
    assert!(w.learned.is_empty() || stage_of(&w) == Stage::Fresh);
    // A new season forgets everything, including what was open.
    let mut w2 = World::default();
    w2.sync_season(1);
    for _ in 0..LEARNED_AT {
        w2.apply_run(0, 1, &used(u16::from(HEAP_SPRAY)));
    }
    w2.sync_season(2);
    assert!(w2.learned.is_empty());
    assert_eq!(w2.season, 2);
    // The same season changes nothing.
    let before = w2.clone();
    w2.sync_season(2);
    assert_eq!(w2, before);
}

#[test]
fn the_world_keeps_a_bounded_number_of_rows() {
    let mut w = World::default();
    for corp in 0..5u8 {
        for sig in 0..40u16 {
            w.apply_run(corp, 1, &used(100 + sig));
        }
    }
    assert!(w.learned.len() <= learn::ROWS_MAX);
}
