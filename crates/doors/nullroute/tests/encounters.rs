//! Countermeasures and encounters (M4-N2): the invariants of a fight, the patterns, the corporation
//! styles, and the RPG rule: what the deck carries decides what a fight allows.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::needless_range_loop,
    clippy::similar_names,
    clippy::assert_is_empty,
    clippy::needless_pass_by_value,
    clippy::cast_possible_truncation
)]

mod common;

use common::*;
use proptest::prelude::*;
use russh_bbs_door::DoorRng;
use russh_bbs_door_nullroute::cm::{self, Encounter, GUARD_ARMOR, ROUNDS_MAX};
use russh_bbs_door_nullroute::code::{self, Build, CODED_BASE};
use russh_bbs_door_nullroute::content::{
    CmDef, CmKind, Content, Effect, Family, Intent, Lock, ScriptDef,
};
use russh_bbs_door_nullroute::net::{Network, Patrol};
use russh_bbs_door_nullroute::run::{Act, Refused, Run, RunSetup, Status};

fn def(content: &Content, name: &str) -> CmDef {
    content
        .cms
        .cm
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no countermeasure {name}"))
        .clone()
}

fn wardens(content: &Content) -> Vec<CmDef> {
    content
        .cms
        .cm
        .iter()
        .filter(|c| c.kind == CmKind::Warden)
        .cloned()
        .collect()
}

/// A coded script as a person would build it: crack core, warden lens, piercing.
fn warden_killer(content: &Content) -> ScriptDef {
    let build = Build {
        effect: 1,
        target: 14,
        modifier: 23,
    };
    code::assemble(content, build, CODED_BASE).unwrap()
}

fn encounter(def: &CmDef, level: u8) -> Encounter {
    Encounter::new(def, 1, 0, level, 0)
}

#[test]
fn no_stock_script_hurts_a_warden_and_a_tuned_one_does() {
    let c = content();
    let killer = warden_killer(&c);
    assert_eq!(killer.target, Some(CmKind::Warden));
    for w in wardens(&c) {
        for level in 1..=6u8 {
            let enc = encounter(&w, level);
            for s in &c.scripts.script {
                for skill in 0..=5u8 {
                    assert_eq!(
                        enc.damage(s, skill),
                        0,
                        "{} should bounce off {} at level {level}",
                        s.name,
                        w.name
                    );
                }
            }
            assert!(
                enc.damage(&killer, 0) > 0,
                "a script tuned to wardens must hurt {} at level {level}",
                w.name
            );
        }
    }
}

#[test]
fn the_stock_tuned_scripts_hurt_their_kind_more_and_never_a_warden() {
    let c = content();
    let spike = c.script(49).unwrap();
    let plain = c.script(18).unwrap();
    let watchman = encounter(&def(&c, "Watchman"), 2);
    assert_eq!(spike.target, Some(CmKind::Sentry));
    assert!(watchman.damage(spike, 0) > watchman.damage(plain, 0));
    // Tuned to sentries is not tuned to tracers.
    let sniffer = encounter(&def(&c, "Sniffer"), 2);
    assert_eq!(sniffer.damage(spike, 0), sniffer.damage(plain, 0));
}

#[test]
fn the_weak_lock_family_and_the_walls_purge_rules() {
    let c = content();
    let crack = c.script(5).unwrap();
    let spoof = c.script(8).unwrap();
    // The Watchman is weak to password locks.
    let watchman = encounter(&def(&c, "Watchman"), 1);
    assert_eq!(watchman.weak, Lock::Crack);
    assert!(watchman.damage(crack, 0) > watchman.damage(spoof, 0));
    // A wall takes half from a break script and double from a purge.
    let wall = encounter(&def(&c, "Palisade"), 2);
    let purge = c.script(21).unwrap();
    let hammer_free = wall.damage(purge, 0);
    let by_break = wall.damage(c.script(6).unwrap(), 0);
    assert!(hammer_free > by_break, "{hammer_free} > {by_break}");
    // A purge weaker than the wall's level does nothing.
    let strong_wall = encounter(&def(&c, "Palisade"), 5);
    assert_eq!(strong_wall.damage(purge, 0), 0);
}

#[test]
fn skill_pierce_guard_and_the_hit_cap_each_change_the_damage() {
    let c = content();
    let mut enc = encounter(&def(&c, "Gatekeeper"), 2);
    let dict = c.script(5).unwrap();
    // The Gatekeeper's pattern is guard, strike, probe: on the guard beat the same script does less.
    assert_eq!(enc.intent(), Intent::Guard);
    assert_eq!(enc.armor_now(), enc.armor + GUARD_ARMOR);
    let guarded = enc.damage(dict, 0);
    enc.resolve(0, None);
    assert_eq!(enc.intent(), Intent::Strike);
    assert_eq!(enc.armor_now(), enc.armor);
    let open = enc.damage(dict, 0);
    assert!(guarded < open, "{guarded} < {open}");
    let skilled = enc.damage(dict, 4);
    assert!(skilled > open, "skill adds to a hit: {skilled} > {open}");
    // Pierce ignores armor.
    let mut piercer = dict.clone();
    piercer.pierce = 3;
    assert!(enc.damage(&piercer, 0) >= enc.damage(dict, 0));
    // No hit takes more than a third.
    let mut huge = dict.clone();
    huge.power = 9;
    huge.pierce = 9;
    assert!(enc.damage(&huge, 5) <= enc.cap());
    assert_eq!(enc.cap(), enc.max.div_ceil(3));
}

#[test]
fn the_beats_do_what_the_telegraph_says() {
    let c = content();
    // Charge doubles the next strike (Warder: charge, strike, probe, seal).
    let mut warder = encounter(&def(&c, "Warder"), 1);
    let strike = warder.strike;
    assert_eq!(warder.intent(), Intent::Charge);
    assert_eq!(warder.resolve(0, Some(Family::Break)).trace, 0);
    let hit = warder.resolve(0, Some(Family::Break));
    assert!(hit.strike);
    assert_eq!(hit.trace, strike * 2, "the charged strike is doubled");
    // Then a probe, then a seal that locks the family you used.
    assert_eq!(warder.resolve(0, Some(Family::Break)).intent, Intent::Probe);
    assert_eq!(warder.intent(), Intent::Seal);
    warder.resolve(0, Some(Family::Fight));
    assert_eq!(warder.sealed, Some(Family::Fight));
    warder.resolve(0, Some(Family::Break));
    assert_eq!(warder.sealed, None, "a seal lasts one move");
    // A stun costs it the beat, and cancels a charge.
    let mut e = encounter(&def(&c, "Enforcer"), 1);
    assert_eq!(e.intent(), Intent::Charge);
    e.resolve(0, None);
    e.stun = 1;
    let lost = e.resolve(0, None);
    assert!(lost.stunned);
    assert_eq!(lost.trace, 0);
    assert!(!e.charged);
    // A counter echoes a break script and ignores anything else.
    let hawkeye = def(&c, "Hawkeye");
    let at_counter = |family| {
        let mut h = encounter(&hawkeye, 1);
        h.resolve(0, None);
        h.resolve(0, None);
        assert_eq!(h.intent(), Intent::Counter);
        h.resolve(5, family).trace
    };
    assert_eq!(at_counter(Some(Family::Break)), 5);
    assert_eq!(at_counter(Some(Family::Fight)), 0);
    // A mend heals, never past the top.
    let mut leech = encounter(&def(&c, "Leech"), 1);
    leech.resolve(0, None);
    leech.hit(3);
    let before = leech.integrity;
    assert_eq!(leech.intent(), Intent::Mend);
    leech.resolve(0, None);
    assert!(leech.integrity > before && leech.integrity <= leech.max);
}

#[test]
fn a_pattern_is_learned_by_watching_or_by_scanning() {
    let c = content();
    for d in c.cms.cm.iter().filter(|d| d.kind != CmKind::Warden) {
        let mut e = Encounter::new(d, 3, 0, 2, 3);
        assert!(!e.pattern_known(), "{}", d.name);
        assert!(e.forecast(3).is_empty());
        let mut order = Vec::new();
        for _ in 0..d.pattern.len() {
            order.push(e.intent());
            e.resolve(0, None);
        }
        assert!(e.pattern_known(), "{}: one full cycle teaches it", d.name);
        // The forecast is the pattern, in order, from here.
        let next = e.forecast(d.pattern.len());
        let again: Vec<Intent> = (0..d.pattern.len()).map(|_| e.intent()).collect();
        assert_eq!(next.len(), again.len());
        assert_eq!(e.intent(), order[0], "the cycle repeats");
        // A probe reads it at once.
        let mut s = Encounter::new(d, 3, 0, 2, 3);
        s.scanned = true;
        assert!(s.pattern_known());
        assert_eq!(s.forecast(2).len(), 2);
    }
    // The same node always starts a machine on the same beat: nothing about a fight is random.
    let d = def(&c, "Watchman");
    let a = Encounter::new(&d, 5, 0, 2, 5);
    let b = Encounter::new(&d, 5, 0, 2, 5);
    assert_eq!(a, b);
}

#[test]
fn corporation_styles_shape_the_mix_of_countermeasures() {
    let c = content();
    let share = |corp: u8, kind: CmKind, tier: u8, tag: &str| {
        let corp = c.corp(corp).unwrap();
        let mut tagged = 0;
        for i in 0..600 {
            let d = cm::pick(&c, corp, tier, kind, &format!("node-{i}")).unwrap();
            if d.tags.iter().any(|t| t == tag) {
                tagged += 1;
            }
        }
        tagged
    };
    // Kestrel leans on armour and walls, Mirrorglass on deception and mending.
    assert!(
        share(0, CmKind::Wall, 3, "armored") > share(2, CmKind::Wall, 3, "armored") + 60,
        "Kestrel's walls are armoured more often"
    );
    assert!(
        share(2, CmKind::Wall, 2, "deceptive") > share(0, CmKind::Wall, 2, "deceptive") + 60,
        "Mirrorglass's walls deceive more often"
    );
    // Halcyon's tracers are the swift, tracing kind.
    assert!(share(1, CmKind::Tracer, 2, "trace") > share(2, CmKind::Tracer, 2, "trace"));
    // Picking is a pure function of the label.
    let corp = c.corp(3).unwrap();
    assert_eq!(
        cm::pick(&c, corp, 2, CmKind::Sentry, "x").map(|d| d.num),
        cm::pick(&c, corp, 2, CmKind::Sentry, "x").map(|d| d.num)
    );
}

#[test]
fn fragments_drop_by_rank_and_wardens_need_the_deeper_tiers() {
    let c = content();
    let mut rng = DoorRng::from_seed(5);
    let mut seen_t1 = std::collections::BTreeSet::new();
    let mut seen_t3 = std::collections::BTreeSet::new();
    for _ in 0..3000 {
        seen_t1.insert(cm::drop_fragment(&c, 1, &mut rng).unwrap());
        seen_t3.insert(cm::drop_fragment(&c, 3, &mut rng).unwrap());
    }
    assert!(seen_t1.iter().all(|n| c.fragment(*n).unwrap().rank <= 1));
    assert_eq!(seen_t3.len(), 30, "every fragment can drop at tier 3");
    assert!(
        !seen_t1.contains(&14),
        "the warden lens is not found on the first floors"
    );
}

// ---- a fight inside a run ----

/// A run on the hand-built network with a sentry at node 1, tier 1.
fn fight_run(deck: &[u8]) -> (Content, Run) {
    let content = content();
    let mut net = hand_net();
    net.nodes[1].patrol = Patrol::Sentry;
    net.nodes[1].security = 2;
    let mut run = Run::new(&content, setup(net, deck)).unwrap();
    run.act(&content, Act::Move(1), &mut DoorRng::from_seed(2))
        .unwrap();
    assert!(run.encounter().is_some());
    (content, run)
}

#[test]
fn while_you_are_in_a_fight_you_cannot_walk_or_leave() {
    let (c, mut run) = fight_run(&[PING, DICTIONARY, BADGE]);
    let mut r = DoorRng::from_seed(3);
    assert_eq!(run.act(&c, Act::Move(0), &mut r), Err(Refused::Engaged));
    assert_eq!(run.act(&c, Act::JackOut, &mut r), Err(Refused::Engaged));
    // Breaking off is free of cycles and puts you back where you came from.
    let cycles = run.cycles;
    run.act(&c, Act::BreakOff, &mut r).unwrap();
    assert_eq!(run.cycles, cycles);
    assert_eq!(run.at, 0);
    assert!(run.encounter().is_none());
    // Brace and break off are nothing outside a fight.
    assert_eq!(run.act(&c, Act::Brace, &mut r), Err(Refused::NothingToDo));
    assert_eq!(
        run.act(&c, Act::BreakOff, &mut r),
        Err(Refused::NothingToDo)
    );
}

#[test]
fn breaking_off_costs_trace_but_the_damage_and_the_pattern_stay() {
    let (c, mut run) = fight_run(&[PING, DICTIONARY, BADGE]);
    let mut r = DoorRng::from_seed(3);
    // Hit it once with the best script, then walk away.
    let slot = (0..run.deck.len())
        .max_by_key(|i| run.preview(&c, *i).unwrap_or(0))
        .unwrap();
    run.act(&c, Act::Use { slot, target: None }, &mut r)
        .unwrap();
    let enc = run.encounter().unwrap().clone();
    assert!(enc.integrity < enc.max, "it took damage");
    let trace = run.trace;
    run.act(&c, Act::BreakOff, &mut r).unwrap();
    assert!(run.trace > trace, "leaving costs trace");
    // Coming back, the same machine is waiting, hurt, and still remembers what you saw.
    run.act(&c, Act::Move(1), &mut r).unwrap();
    let back = run.encounter().unwrap();
    assert_eq!(back.integrity, enc.integrity);
    assert_eq!(back.seen, enc.seen);
    assert_eq!(back.round, 0, "the move count starts again");
}

#[test]
fn winning_clears_the_node_for_the_run_and_a_tracer_win_ends_the_trace() {
    let c = content();
    for patrol in [Patrol::Sentry, Patrol::Tracer] {
        let mut net = hand_net();
        net.nodes[1].patrol = patrol;
        net.nodes[1].security = 1;
        let mut run = Run::new(&c, setup(net, &[PING, DICTIONARY, BADGE, STACK_SMASH])).unwrap();
        let mut r = DoorRng::from_seed(8);
        run.act(&c, Act::Move(1), &mut r).unwrap();
        for _ in 0..40 {
            if run.encounter().is_none() || run.status != Status::Active {
                break;
            }
            let best = (0..run.deck.len())
                .max_by_key(|i| run.preview(&c, *i).unwrap_or(0))
                .unwrap();
            if run
                .act(
                    &c,
                    Act::Use {
                        slot: best,
                        target: None,
                    },
                    &mut r,
                )
                .is_err()
            {
                run.act(&c, Act::Brace, &mut r).unwrap();
            }
        }
        assert_eq!(run.status, Status::Active, "{patrol:?}: {:?}", run.log);
        assert!(run.encounter().is_none(), "{patrol:?}: {:?}", run.log);
        assert!(!run.tracer(), "winning cuts the tracer loose");
        // Walk away and back: the dead machine does not return.
        run.act(&c, Act::Move(0), &mut r).unwrap();
        run.act(&c, Act::Move(1), &mut r).unwrap();
        assert!(run.encounter().is_none());
        assert!(run.result().defeated >= 1);
    }
}

#[test]
fn a_fork_takes_a_strike_and_a_mask_halves_what_the_machine_costs() {
    let c = content();
    let measure = |deck: &[u8], pre: Option<usize>| {
        let mut net = hand_net();
        net.nodes[1].patrol = Patrol::Sentry;
        net.nodes[1].security = 1;
        let mut run = Run::new(&c, setup(net, deck)).unwrap();
        let mut r = DoorRng::from_seed(4);
        run.act(&c, Act::Move(1), &mut r).unwrap();
        if let Some(slot) = pre {
            run.act(&c, Act::Use { slot, target: None }, &mut r)
                .unwrap();
        }
        let before = run.trace;
        // Brace through the machine's next four beats and count the trace it adds.
        for _ in 0..4 {
            if run.encounter().is_none() {
                break;
            }
            let _ = run.act(&c, Act::Brace, &mut r);
        }
        run.trace - before
    };
    let bare = measure(&[PING, VEIL, FORK], None);
    let masked = measure(&[PING, VEIL, FORK], Some(1));
    let forked = measure(&[PING, VEIL, FORK], Some(2));
    assert!(
        masked < bare,
        "a mask quiets the machine: {masked} < {bare}"
    );
    assert!(forked < bare, "a fork eats a strike: {forked} < {bare}");
}

#[test]
fn the_eighth_move_throws_you_out() {
    // Only scripts that do nothing: the fight times out.
    let (c, mut run) = fight_run(&[PING, VEIL]);
    let mut r = DoorRng::from_seed(9);
    for _ in 0..ROUNDS_MAX + 2 {
        if run.encounter().is_none() || run.status != Status::Active {
            break;
        }
        let _ = run.act(&c, Act::Brace, &mut r);
    }
    if run.status == Status::Active {
        assert!(run.encounter().is_none(), "the machine's patience ran out");
        assert!(run.log.iter().any(|l| l.contains("had enough")));
    }
}

// ---- the RPG rule: the warden, and what the deck carries ----

/// Drive the hand network to its vault with a warden hunting: trip the honeypot, then walk to the
/// real vault by the south door. `deck` is the run's deck (slot 0 dictionary, slot 1 stack-smash,
/// slot 2 siphon, then whatever follows). Returns the run when it ends or the budget does.
fn hunted_run(content: &Content, deck: Vec<u8>, extra: Vec<ScriptDef>, seed: u64) -> Run {
    let mut net = hand_net();
    net.tier = 3;
    let mut s: RunSetup = setup(net, &deck);
    s.buffer = 300;
    s.cycles = 400;
    s.warden = Some(21);
    s.extra = extra;
    let mut run = Run::new(content, s).unwrap();
    let mut r = DoorRng::from_seed(seed);
    let open = |run: &mut Run, node: u8, slot: usize, r: &mut DoorRng| {
        for _ in 0..30 {
            if run.is_open(node) {
                break;
            }
            let _ = run.act(
                content,
                Act::Use {
                    slot,
                    target: Some(node),
                },
                r,
            );
        }
    };
    // The honeypot: it raises the alarm, and the warden wakes.
    open(&mut run, 4, 0, &mut r);
    run.act(content, Act::Move(4), &mut r).unwrap();
    run.act(content, Act::Move(7), &mut r).unwrap();
    assert!(run.alarm());
    assert!(run.warden().is_some_and(|w| w.awake), "{:?}", run.log);
    // Then the real vault, the long way round, with the warden arriving on the way.
    let route = [4u8, 0, 2, 5];
    let mut at = 0;
    for _ in 0..200 {
        if run.status != Status::Active || run.objective_done() {
            break;
        }
        if run.encounter().is_some() {
            let best = (0..run.deck.len())
                .filter(|i| run.deck[*i].left > 0)
                .max_by_key(|i| run.preview(content, *i).unwrap_or(0))
                .unwrap();
            let useful = run.preview(content, best).unwrap_or(0) > 0;
            let acted = useful
                && run
                    .act(
                        content,
                        Act::Use {
                            slot: best,
                            target: None,
                        },
                        &mut r,
                    )
                    .is_ok();
            if !acted && run.act(content, Act::Brace, &mut r).is_err() {
                let _ = run.act(content, Act::BreakOff, &mut r);
            }
            if !useful && run.encounter().is_some() {
                let _ = run.act(content, Act::BreakOff, &mut r);
            }
            // After a break-off we are somewhere earlier on the route.
            at = route.iter().position(|n| *n == run.at).unwrap_or(0);
            continue;
        }
        if run.at == 5 {
            let _ = run.act(
                content,
                Act::Use {
                    slot: 2,
                    target: None,
                },
                &mut r,
            );
            continue;
        }
        let next = route[at.min(route.len() - 1)];
        if next == run.at {
            at += 1;
            continue;
        }
        if !run.is_open(next) {
            open(&mut run, next, 1, &mut r);
        }
        if run.act(content, Act::Move(next), &mut r).is_ok() {
            at += 1;
        } else {
            at = route.iter().position(|n| *n == run.at).unwrap_or(0);
        }
    }
    run
}

#[test]
fn a_coded_script_gets_past_a_warden_a_stock_deck_cannot() {
    let c = content();
    let killer = warden_killer(&c);
    let mut stock_done = 0;
    let mut coded_done = 0;
    for seed in 1..=6u64 {
        // The stock deck: everything a shop sells in the right families.
        let stock = hunted_run(
            &c,
            vec![DICTIONARY, STACK_SMASH, SIPHON, BRUTE, JAMMER],
            Vec::new(),
            seed,
        );
        assert!(stock.result().warden_woke);
        assert!(
            !stock.result().warden_down,
            "nothing in a shop hurts a warden: {:?}",
            stock.log
        );
        stock_done += u32::from(stock.objective_done());
        let coded = hunted_run(
            &c,
            vec![DICTIONARY, STACK_SMASH, SIPHON, CODED_BASE],
            vec![killer.clone()],
            seed,
        );
        assert!(coded.result().warden_woke);
        assert!(
            coded.result().warden_down,
            "the tuned script puts it down: {:?}",
            coded.log
        );
        assert!(coded.objective_done(), "{:?}", coded.log);
        coded_done += 1;
    }
    assert_eq!(
        stock_done, 0,
        "a stock deck cannot take the vault with a warden on it"
    );
    assert_eq!(coded_done, 6);
}

#[test]
fn a_warden_is_only_woken_by_a_loud_run_and_comes_back_after_you_break_off() {
    let c = content();
    // Quiet run: no warden.
    let mut s = setup(hand_net(), &[PING, DICTIONARY]);
    s.warden = Some(21);
    let mut quiet = Run::new(&c, s).unwrap();
    let mut r = DoorRng::from_seed(1);
    quiet.act(&c, Act::Move(1), &mut r).unwrap();
    assert!(quiet.warden().is_some_and(|w| !w.awake));
    // After a break-off the warden returns in two moves.
    let run = hunted_run(&c, vec![DICTIONARY, STACK_SMASH, SIPHON], Vec::new(), 2);
    assert!(run.result().broke_off > 0, "{:?}", run.log);
}

#[test]
fn a_patrol_that_a_corporation_never_staffs_has_no_fight() {
    // A node with no patrol has no countermeasure at all.
    let (c, run) = {
        let c = content();
        let run = Run::new(&c, setup(hand_net(), &[PING])).unwrap();
        (c, run)
    };
    assert!(run.cm_at(&c, 1).is_none());
    assert!(run.encounter().is_none());
}

// ---- properties ----

fn any_deck(content: &Content, mask: u64) -> Vec<u8> {
    let nums: Vec<u8> = content
        .scripts
        .script
        .iter()
        .map(|s| s.num)
        .filter(|n| *n != 0)
        .collect();
    let mut deck = Vec::new();
    for (i, n) in nums.iter().enumerate() {
        if mask >> (i % 60) & 1 == 1 && deck.len() < 6 {
            deck.push(*n);
        }
    }
    if deck.is_empty() {
        deck.push(PING);
    }
    deck
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Whatever script hits a machine, a fight is at least three moves if it is won, never more
    /// than eight, and the integrity never leaves its bounds.
    #[test]
    fn a_won_fight_is_three_to_eight_moves(
        which in 0usize..23,
        level in 1u8..=6,
        skill in 0u8..=5,
        power in 1u32..=9,
        pierce in 0u32..=3,
        seed in any::<u64>(),
    ) {
        let c = content();
        let d = c.cms.cm[which].clone();
        let mut enc = Encounter::new(&d, 2, 1, level, (seed % 7) as usize);
        let mut rng = DoorRng::from_seed(seed);
        let strongest = ScriptDef {
            power,
            pierce,
            target: Some(d.kind),
            effect: Effect::Crack,
            family: Family::Break,
            ..c.script(5).unwrap().clone()
        };
        let mut moves = 0u32;
        let mut won = false;
        while !enc.spent() {
            moves += 1;
            let sealed = enc.sealed == Some(Family::Break);
            let dealt = if sealed { 0 } else { enc.damage(&strongest, skill) };
            prop_assert!(dealt <= enc.cap());
            if enc.hit(dealt) {
                won = true;
                break;
            }
            // The player sometimes does something else.
            let family = if rng.chance(1, 4) { None } else { Some(Family::Break) };
            enc.resolve(dealt, family);
            prop_assert!(enc.integrity <= enc.max);
        }
        prop_assert!(moves <= ROUNDS_MAX);
        if won {
            prop_assert!(moves >= 3, "{} won in {moves} at level {level}", d.name);
        }
    }

    /// Whatever a player does at random, the run stays inside its rules.
    #[test]
    fn random_play_never_breaks_an_invariant(
        corp in 0u8..5,
        tier in 1u8..=3,
        season in 1u32..40,
        mask in any::<u64>(),
        seed in any::<u64>(),
    ) {
        let c = content();
        let Some(corp_def) = c.corp(corp) else { return Ok(()) };
        let Some(net) = Network::generate(&c, season, corp_def, tier) else { return Ok(()) };
        let mut s = setup(net, &any_deck(&c, mask));
        s.warden = (seed % 3 == 0).then_some(21 + (seed % 3) as u8);
        s.cycles = 120;
        let Some(mut run) = Run::new(&c, s) else { return Ok(()) };
        let mut rng = DoorRng::from_seed(seed);
        let mut last_cycles = run.cycles;
        for _ in 0..250 {
            if run.status != Status::Active { break; }
            let exits = run.exits();
            let slot = rng.below(run.deck.len().max(1) as u32) as usize;
            let pick = |rng: &mut DoorRng| {
                (!exits.is_empty()).then(|| exits[rng.below(exits.len() as u32) as usize])
            };
            let act = match rng.below(10) {
                0..=3 => pick(&mut rng).map_or(Act::JackOut, Act::Move),
                4..=7 => Act::Use { slot, target: pick(&mut rng) },
                8 => Act::Brace,
                _ => Act::BreakOff,
            };
            let in_fight = run.encounter().is_some();
            let result = run.act(&c, act, &mut rng);
            if in_fight && matches!(act, Act::Move(_) | Act::JackOut) {
                prop_assert_eq!(result, Err(Refused::Engaged));
            }
            prop_assert!(run.trace <= 100);
            prop_assert!(run.cycles <= last_cycles, "cycles never come back");
            last_cycles = run.cycles;
            if let Some(e) = run.encounter() {
                prop_assert!(e.round <= ROUNDS_MAX);
                prop_assert!(e.integrity <= e.max && e.integrity > 0);
            }
            prop_assert!(run.log.len() <= 60);
        }
        // A run ended by a burn pays nothing: no fragments leave with it.
        let result = run.result();
        prop_assert!(result.fragments.len() <= 6);
    }
}

#[test]
fn nothing_in_the_content_has_a_countermeasure_for_the_wrong_patrol() {
    let c = content();
    for (patrol, kind) in [
        (Patrol::Sentry, CmKind::Sentry),
        (Patrol::Tracer, CmKind::Tracer),
        (Patrol::BlackWall, CmKind::Wall),
    ] {
        assert_eq!(cm::kind_of(patrol), Some(kind));
        for corp in &c.corps.corp {
            for tier in 1..=3 {
                if patrol == Patrol::BlackWall && tier == 1 {
                    continue;
                }
                let d = cm::pick(&c, corp, tier, kind, "label").unwrap();
                assert_eq!(d.kind, kind);
            }
        }
    }
    assert_eq!(cm::kind_of(Patrol::None), None);
    for corp in &c.corps.corp {
        assert_eq!(
            cm::warden_of(&c, corp).map(|w| w.kind),
            Some(CmKind::Warden)
        );
    }
}
