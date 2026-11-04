//! Coding (M4-N2): fragments, the module editor's budgets, and what a build does. Every test is a
//! branch of the RPG rule: a build choice that changes what happens.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

mod common;

use common::*;
use russh_bbs_door::DoorRng;
use russh_bbs_door_nullroute::code::{
    self, Build, CODE_CYCLES, CODED_BASE, CODED_MAX, MEMORY_BUDGET, NOISE_BUDGET,
};
use russh_bbs_door_nullroute::content::{CmKind, Effect};
use russh_bbs_door_nullroute::learn::{self, Stage};
use russh_bbs_door_nullroute::player::Player;
use russh_bbs_door_nullroute::run::{Act, Run};
use russh_bbs_door_nullroute::world::{Contract, World};

const DAY: i64 = 20_000;

const CRACK_CORE: u8 = 1;
const OVERFLOW_CORE: u8 = 3;
const JAM_CORE: u8 = 4;
const PURGE_CORE: u8 = 6;
const SENTRY_LENS: u8 = 11;
const WARDEN_LENS: u8 = 14;
const WARDEN_PRISM: u8 = 18;
const OVERCLOCK: u8 = 19;
const QUIET: u8 = 20;
const PIERCING: u8 = 23;
const POLYMORPHIC: u8 = 26;
const GREEDY: u8 = 29;

fn b(effect: u8, target: u8, modifier: u8) -> Build {
    Build {
        effect,
        target,
        modifier,
    }
}

/// A character with a workbench and the given fragments.
fn coder(fragments: &[u8]) -> Player {
    let c = content();
    let mut p = Player::new(&c, 5, "ann", 1, DAY);
    p.workbench = true;
    p.fragments = fragments.to_vec();
    p
}

#[test]
fn the_fragment_numbers_match_what_these_tests_say() {
    let c = content();
    for (n, name) in [
        (CRACK_CORE, "crack-core"),
        (OVERFLOW_CORE, "overflow-core"),
        (JAM_CORE, "jam-core"),
        (PURGE_CORE, "purge-core"),
        (SENTRY_LENS, "sentry-lens"),
        (WARDEN_LENS, "warden-lens"),
        (WARDEN_PRISM, "warden-prism"),
        (OVERCLOCK, "overclock"),
        (QUIET, "quiet"),
        (PIERCING, "piercing"),
        (POLYMORPHIC, "polymorphic"),
        (GREEDY, "greedy"),
    ] {
        assert_eq!(c.fragment(n).unwrap().name, name);
    }
}

#[test]
fn a_build_is_the_sum_of_its_fragments() {
    let c = content();
    let plain = code::stats(&c, b(CRACK_CORE, 0, 0)).unwrap();
    let tuned = code::stats(&c, b(CRACK_CORE, WARDEN_LENS, 0)).unwrap();
    assert!(tuned.memory > plain.memory && tuned.noise > plain.noise);
    assert_eq!(
        tuned.power, plain.power,
        "a lens costs, it does not add power"
    );
    let hot = code::stats(&c, b(CRACK_CORE, 0, OVERCLOCK)).unwrap();
    assert_eq!(hot.power, plain.power + 2);
    assert!(hot.noise > plain.noise);
    let script = code::assemble(&c, b(CRACK_CORE, WARDEN_LENS, PIERCING), CODED_BASE).unwrap();
    assert_eq!(script.effect, Effect::Crack);
    assert_eq!(script.target, Some(CmKind::Warden));
    assert_eq!(script.pierce, 2);
    assert!(script.name.contains("crack") && script.name.contains("warden"));
    assert!(russh_bbs_door::content::plain_text(&script.name));
}

#[test]
fn quiet_buys_silence_with_memory_and_greedy_does_not_fit_beside_a_prism() {
    let c = content();
    let plain = code::stats(&c, b(CRACK_CORE, 0, 0)).unwrap();
    let quiet = code::stats(&c, b(CRACK_CORE, 0, QUIET)).unwrap();
    assert!(quiet.noise < plain.noise);
    assert!(quiet.memory > plain.memory);
    // Power at any price: greedy plus a heavy target blows the memory budget.
    let err = code::assemble(&c, b(CRACK_CORE, WARDEN_PRISM, GREEDY), CODED_BASE).unwrap_err();
    assert!(err.contains("memory budget"), "{err}");
    // Greedy alone is loud but fits the memory: the noise is what stops the overclocked purge.
    let err = code::assemble(&c, b(PURGE_CORE, WARDEN_LENS, GREEDY), CODED_BASE).unwrap_err();
    assert!(err.contains("budget"), "{err}");
    // Greedy on a plain core fits, and is the one way past the strongest thing a shop sells.
    let top = c.scripts.script.iter().map(|s| s.power).max().unwrap();
    let greedy = code::assemble(&c, b(OVERFLOW_CORE, 0, GREEDY), CODED_BASE).unwrap();
    assert!(greedy.power > top, "{} > {top}", greedy.power);
    // The budgets are the constants the screen shows.
    for build in [b(CRACK_CORE, 0, 0), b(JAM_CORE, SENTRY_LENS, QUIET)] {
        let s = code::stats(&c, build).unwrap();
        assert!(s.memory <= MEMORY_BUDGET && s.noise <= NOISE_BUDGET);
        assert!(code::assemble(&c, build, CODED_BASE).is_ok());
    }
}

#[test]
fn bad_recipes_are_refused() {
    let c = content();
    // A modifier in the effect slot, a target in the modifier slot, a made-up fragment.
    assert!(code::stats(&c, b(QUIET, 0, 0)).is_err());
    assert!(code::stats(&c, b(CRACK_CORE, OVERCLOCK, 0)).is_err());
    assert!(code::stats(&c, b(CRACK_CORE, 0, WARDEN_LENS)).is_err());
    assert!(code::stats(&c, b(99, 0, 0)).is_err());
    assert!(code::stats(&c, b(0, 0, 0)).is_err());
}

#[test]
fn the_same_recipe_is_the_same_signature_and_never_a_stock_scripts() {
    let c = content();
    let a = code::signature(b(CRACK_CORE, WARDEN_LENS, PIERCING));
    assert_eq!(a, code::signature(b(CRACK_CORE, WARDEN_LENS, PIERCING)));
    assert_ne!(a, code::signature(b(CRACK_CORE, WARDEN_LENS, 0)));
    assert_ne!(a, code::signature(b(CRACK_CORE, 0, PIERCING)));
    for s in &c.scripts.script {
        assert_ne!(a, s.sig);
        assert!(code::signature(b(CRACK_CORE, 0, 0)) > 1000);
    }
    let one = code::assemble(&c, b(CRACK_CORE, 0, 0), CODED_BASE).unwrap();
    let two = code::assemble(&c, b(CRACK_CORE, 0, 0), CODED_BASE + 3).unwrap();
    assert_eq!(
        one.sig, two.sig,
        "where it sits in the case does not matter"
    );
}

#[test]
fn coding_burns_fragments_and_cycles_and_needs_a_workbench() {
    let c = content();
    let mut p = coder(&[CRACK_CORE, WARDEN_LENS, PIERCING, PIERCING]);
    p.workbench = false;
    assert_eq!(
        p.code(&c, b(CRACK_CORE, WARDEN_LENS, PIERCING)),
        Err("You need a workbench to code.".into())
    );
    p.workbench = true;
    let cycles = p.cycles;
    let msg = p.code(&c, b(CRACK_CORE, WARDEN_LENS, PIERCING)).unwrap();
    assert!(msg.starts_with("Coded crack-warden-piercing"), "{msg}");
    assert_eq!(p.cycles, cycles - CODE_CYCLES);
    assert_eq!(p.fragments, vec![PIERCING], "only the spare is left");
    assert!(p.owned.contains(&CODED_BASE));
    let s = p.script(&c, CODED_BASE).unwrap();
    assert_eq!(s.target, Some(CmKind::Warden));
    // The same recipe twice, or one you cannot afford, changes nothing.
    assert!(p.code(&c, b(CRACK_CORE, WARDEN_LENS, PIERCING)).is_err());
    let before = (p.fragments.clone(), p.cycles, p.owned.clone());
    assert!(p.code(&c, b(JAM_CORE, 0, 0)).is_err());
    assert_eq!((p.fragments.clone(), p.cycles, p.owned.clone()), before);
    // Too few cycles.
    p.fragments = vec![JAM_CORE];
    p.cycles = CODE_CYCLES - 1;
    assert!(p.code(&c, b(JAM_CORE, 0, 0)).is_err());
    assert_eq!(p.fragments, vec![JAM_CORE]);
    // Over budget: nothing is burned.
    p.cycles = 40;
    p.fragments = vec![CRACK_CORE, WARDEN_PRISM, GREEDY];
    assert!(p.code(&c, b(CRACK_CORE, WARDEN_PRISM, GREEDY)).is_err());
    assert_eq!(p.fragments.len(), 3);
}

#[test]
fn the_case_holds_sixteen_and_a_scrap_makes_room() {
    let c = content();
    let mut p = coder(&[]);
    let effects = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let mods = [0u8, 19, 20, 21, 22, 23, 24, 25, 27, 28, 30];
    let mut made = Vec::new();
    'outer: for e in effects {
        for m in mods {
            let build = b(e, 0, m);
            if code::assemble(&c, build, CODED_BASE).is_err() {
                continue;
            }
            p.fragments = [e, m].into_iter().filter(|n| *n != 0).collect();
            p.cycles = 40;
            if p.code(&c, build).is_ok() {
                made.push(build);
            }
            if made.len() == CODED_MAX {
                break 'outer;
            }
        }
    }
    assert_eq!(made.len(), CODED_MAX);
    p.fragments = vec![JAM_CORE, SENTRY_LENS];
    p.cycles = 40;
    let extra = b(JAM_CORE, SENTRY_LENS, 0);
    assert!(
        p.code(&c, extra).unwrap_err().contains("No room"),
        "the case is full"
    );
    p.deck.push(CODED_BASE + 2);
    assert!(p.scrap(CODED_BASE + 2).is_ok());
    assert!(!p.owned.contains(&(CODED_BASE + 2)) && !p.deck.contains(&(CODED_BASE + 2)));
    assert!(p.script(&c, CODED_BASE + 2).is_none());
    assert!(p.code(&c, extra).is_ok(), "the slot is free again");
    assert!(p.scrap(10).is_err(), "a stock script cannot be scrapped");
}

#[test]
fn a_coded_script_loads_like_any_other_and_weighs_what_it_says() {
    let c = content();
    let mut p = coder(&[CRACK_CORE, WARDEN_LENS, PIERCING]);
    p.code(&c, b(CRACK_CORE, WARDEN_LENS, PIERCING)).unwrap();
    let weight = p.script(&c, CODED_BASE).unwrap().memory;
    p.deck.clear();
    p.load(&c, CODED_BASE).unwrap();
    assert_eq!(p.deck_memory(&c), weight);
    // The memory budget of the rig is still the limit: a stock rig holds 8.
    let heavy = p.load(&c, 7);
    assert!(heavy.is_err() || p.deck_memory(&c) <= p.memory());
}

#[test]
fn a_coded_script_goes_into_a_run_and_counts_for_reach() {
    let c = content();
    let mut p = coder(&[OVERFLOW_CORE, OVERCLOCK]);
    p.code(&c, b(OVERFLOW_CORE, 0, OVERCLOCK)).unwrap();
    let def = p.script(&c, CODED_BASE).unwrap();
    assert_eq!(def.power, 5);
    p.deck.clear();
    p.load(&c, CODED_BASE).unwrap();
    p.load(&c, 1).unwrap();
    let contract = Contract {
        id: 1,
        tmpl: 1,
        corp: 0,
        fixer: 1,
        tier: 1,
        reward: 10,
        expires: DAY + 2,
    };
    let steal = c
        .contracts
        .contract
        .iter()
        .find(|t| t.effect == Effect::Copy)
        .unwrap();
    let contract = Contract {
        tmpl: steal.num,
        ..contract
    };
    // The deck has no copier: the job is refused, as ever.
    assert!(p.begin_run(&c, 1, &World::default(), &contract).is_err());
    p.load(&c, 23).unwrap();
    let run = p.begin_run(&c, 1, &World::default(), &contract).unwrap();
    assert_eq!(run.deck.len(), 3);
    assert_eq!(run.script(&c, CODED_BASE).unwrap().power, 5);
    assert_eq!(run.capability(&c).breaks[2], 5, "the coded overflow counts");
}

#[test]
fn the_best_build_prefers_a_warden_tuning_then_power() {
    let c = content();
    let have = [
        CRACK_CORE,
        JAM_CORE,
        SENTRY_LENS,
        WARDEN_LENS,
        OVERCLOCK,
        PIERCING,
    ];
    let best = code::best_build(&c, &have).unwrap();
    let def = code::assemble(&c, best, CODED_BASE).unwrap();
    assert_eq!(def.target, Some(CmKind::Warden));
    assert!(code::best_build(&c, &[QUIET, PIERCING]).is_none());
}

#[test]
fn a_coded_script_is_used_in_a_run_and_grows_the_skill() {
    let c = content();
    let def = code::assemble(&c, b(OVERFLOW_CORE, 0, OVERCLOCK), CODED_BASE).unwrap();
    let (_, _) = (def.clone(), 0);
    let mut s = setup(hand_net(), &[PING, STACK_SMASH, CODED_BASE]);
    s.extra = vec![def];
    let mut run = Run::new(&c, s).unwrap();
    let mut r = DoorRng::from_seed(1);
    let mut opened = false;
    for _ in 0..30 {
        let _ = run.act(
            &c,
            Act::Use {
                slot: 2,
                target: Some(2),
            },
            &mut r,
        );
        if run.is_open(2) {
            opened = true;
            break;
        }
    }
    assert!(opened, "{:?}", run.log);
    let result = run.result();
    assert!(result.xp[1] > 0, "a coded break grows the Break skill");
    assert_eq!(result.used.len(), 1);
    assert_eq!(
        result.used[0],
        code::signature(b(OVERFLOW_CORE, 0, OVERCLOCK))
    );
}

#[test]
fn polymorphic_builds_are_learned_half_as_fast() {
    let c = content();
    let plain = code::assemble(&c, b(CRACK_CORE, 0, 0), CODED_BASE).unwrap();
    let slow = code::assemble(&c, b(CRACK_CORE, 0, POLYMORPHIC), CODED_BASE).unwrap();
    assert!(slow.slow && !plain.slow);
    assert_eq!(learn::stage(&plain, learn::LEARNED_AT), Stage::Patched);
    assert_eq!(learn::stage(&slow, learn::LEARNED_AT), Stage::Wary);
    assert_eq!(learn::stage(&slow, learn::LEARNED_AT * 2), Stage::Patched);
}
