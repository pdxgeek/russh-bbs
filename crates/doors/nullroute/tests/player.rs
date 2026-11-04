//! The character: deck limits, skills, the rig, standing, heat, and the nights between visits.
//! Every test here is a branch of the RPG rule: a build choice that changes what happens.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::identity_op
)]

mod common;

use common::*;
use russh_bbs_door::DoorRng;
use russh_bbs_door_nullroute::content::Family;
use russh_bbs_door_nullroute::player::{JACK_IN_CYCLES, Player, part, skill_level};
use russh_bbs_door_nullroute::run::{RunResult, Status};
use russh_bbs_door_nullroute::world::{Contract, World};

const DAY: i64 = 20_000;

fn player() -> Player {
    Player::new(&content(), 7, "ann", 1, DAY)
}

fn contract(tmpl: u8, corp: u8, fixer: u8, tier: u8) -> Contract {
    Contract {
        id: u32::try_from(DAY).unwrap() * 16 + u32::from(tmpl),
        tmpl,
        corp,
        fixer,
        tier,
        reward: 100,
        expires: DAY + 2,
    }
}

fn result(status: Status, objective: bool, loot: u32, peak: u32) -> RunResult {
    RunResult {
        status,
        objective,
        loot,
        heat_cut: 0,
        peak,
        cycles_left: 20,
        broken: vec![],
        planted: false,
        xp: [1, 2, 0, 0, 1],
        used: Vec::new(),
        fragments: Vec::new(),
        defeated: 0,
        warden_down: false,
    }
}

#[test]
fn a_new_character_starts_with_a_real_choice_to_make() {
    let c = content();
    let p = player();
    assert_eq!(p.owned, [1, 5, 8, 11, 23]);
    assert_eq!(p.deck, [1, 5, 8, 23], "scan, crack, spoof, copy");
    assert_eq!((p.slots(), p.memory(), p.deck_memory(&c)), (4, 8, 7));
    assert_eq!((p.cycles, p.max_cycles()), (40, 40));
    // Overflow does not fit alongside the rest: slots, then memory.
    let mut p = p;
    let err = p.load(&c, 11).unwrap_err();
    assert!(err.contains("No free slot"), "{err}");
    p.unload(&c, 23).unwrap();
    assert!(p.load(&c, 11).is_ok());
    assert_eq!(p.deck_memory(&c), 7);
    assert!(p.load(&c, 11).unwrap_err().contains("Already"));
    assert!(p.load(&c, 6).unwrap_err().contains("own"));
}

#[test]
fn memory_decides_how_many_and_how_heavy_the_scripts_are() {
    let c = content();
    let mut p = player();
    p.owned.extend([6, 9]);
    p.deck.clear();
    assert!(p.load(&c, 6).is_ok()); // brute-forge, memory 4
    assert!(p.load(&c, 9).is_ok()); // cert-forger, memory 4: exactly 8
    // Two slots are free but eight memory is spent: a third script does not fit.
    assert!(p.load(&c, 23).unwrap_err().contains("memory"));
    p.marks = 1_000;
    p.upgrade(&c, part::MEMORY).unwrap();
    assert_eq!((p.slots(), p.memory()), (5, 12));
    assert!(p.load(&c, 23).is_ok());
}

#[test]
fn a_fried_part_counts_as_stock_until_repaired() {
    let c = content();
    let mut p = player();
    p.marks = 1_000;
    p.upgrade(&c, part::MEMORY).unwrap();
    p.upgrade(&c, part::CPU).unwrap();
    assert_eq!((p.max_cycles(), p.memory()), (44, 12));
    p.fried = (1 << part::MEMORY) | (1 << part::CPU);
    assert_eq!((p.max_cycles(), p.memory(), p.slots()), (40, 8, 4));
    assert!(p.upgrade(&c, part::CPU).unwrap_err().contains("Repair"));
    let cost = p.repair_cost(&c, part::CPU);
    assert!(cost > 0);
    p.repair(&c, part::CPU).unwrap();
    assert_eq!(p.max_cycles(), 44);
    assert!(p.repair(&c, part::CPU).unwrap_err().contains("not fried"));
}

#[test]
fn a_deck_that_no_longer_fits_the_rig_cannot_jack_in() {
    let c = content();
    let mut p = player();
    p.marks = 1_000;
    p.upgrade(&c, part::MEMORY).unwrap();
    p.owned.push(6);
    p.deck = vec![1, 5, 6, 23]; // memory 1 + 2 + 4 + 2 = 9, fits 12
    p.fried |= 1 << part::MEMORY; // back to 8
    let err = p
        .begin_run(&c, 1, &World::default(), &contract(0, 0, 1, 1))
        .unwrap_err();
    assert!(err.contains("no longer fits"), "{err}");
    assert_eq!(p.cycles, 40, "nothing was spent");
}

#[test]
fn skills_grow_with_use_and_gate_the_best_scripts() {
    let c = content();
    assert_eq!(
        [0, 3, 4, 11, 12, 29, 30, 59, 60, 99, 100, 5_000].map(skill_level),
        [0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5]
    );
    let mut p = player();
    p.marks = 5_000;
    p.owned.push(7); // skeleton-key: break skill 3, memory 5
    p.rep = 30;
    p.deck.clear();
    p.rig[part::MEMORY] = 1;
    assert!(p.load(&c, 7).unwrap_err().contains("skill 3"));
    p.xp[Family::Break.index()] = 30;
    assert_eq!(p.skill(Family::Break), 3);
    assert!(p.load(&c, 7).is_ok());
    // The skill also feeds the run: it rises with the xp a finished run reports.
    let mut p = player();
    let k = contract(0, 0, 1, 1);
    p.begin_run(&c, 1, &World::default(), &k).unwrap();
    p.finish_run(
        &c,
        &k,
        &result(Status::Out, false, 0, 10),
        &mut DoorRng::from_seed(1),
    );
    assert_eq!(p.xp, [1, 2, 0, 0, 1]);
}

#[test]
fn a_job_needs_a_script_that_can_do_it() {
    let c = content();
    let mut p = player();
    // Template 2 is "plant a backdoor" and the starter deck has no implant.
    let k = contract(2, 0, 1, 1);
    let err = p.begin_run(&c, 1, &World::default(), &k).unwrap_err();
    assert!(err.contains("Plant"), "{err}");
    assert_eq!((p.cycles, p.pending, p.runs), (40, None, 0));
    p.owned.push(24);
    p.unload(&c, 23).unwrap();
    p.load(&c, 24).unwrap();
    let run = p.begin_run(&c, 1, &World::default(), &k).unwrap();
    assert_eq!(p.cycles, 40 - JACK_IN_CYCLES);
    assert_eq!(p.pending, Some(k.id));
    assert_eq!(run.cycles, p.cycles);
}

#[test]
fn proxies_cost_more_when_hot_and_are_spent_by_the_next_run() {
    let c = content();
    let mut p = player();
    p.marks = 500;
    assert_eq!(p.proxy_price(&c), 15);
    p.heat = 30;
    assert_eq!(p.proxy_price(&c), 22);
    p.buy_proxy(&c).unwrap();
    p.buy_proxy(&c).unwrap();
    p.buy_proxy(&c).unwrap();
    assert!(p.buy_proxy(&c).unwrap_err().contains("cannot chain"));
    let run = p
        .begin_run(&c, 1, &World::default(), &contract(0, 0, 1, 1))
        .unwrap();
    assert_eq!(run.buffer(), 3 * 8);
    assert_eq!(p.proxies, 0);
}

#[test]
fn a_clean_job_pays_the_fixer_the_fence_and_leaves_heat_rep_and_standing() {
    let c = content();
    let mut p = player();
    let k = contract(0, 0, 1, 1); // Kestrel (patron 0) hired by Sister Quill (1)
    p.begin_run(&c, 1, &World::default(), &k).unwrap();
    let marks = p.marks;
    let notes = p.finish_run(
        &c,
        &k,
        &result(Status::Out, true, 100, 40),
        &mut DoorRng::from_seed(1),
    );
    assert!(notes[0].contains("Job done"), "{notes:?}");
    assert_eq!(
        p.marks,
        marks + 100 + 50,
        "reward + 50 percent of the data at the back-alley fence"
    );
    assert_eq!((p.jobs, p.rep, p.cycles), (1, 3, 20));
    assert_eq!(p.heat, 1 + 3, "tier^2 + the template's 3");
    assert_eq!(
        (p.standing[1], p.standing[0]),
        (1, -1),
        "the hirer likes it, the corporation's patron does not"
    );
    assert!(p.done.contains(&k.id));
    assert!(p.closed(&c, &k).unwrap().contains("Already done"));
    assert!(p.pending.is_none());
}

#[test]
fn a_better_fence_pays_more_for_the_same_data() {
    let c = content();
    let take = |fence: u8| {
        let mut p = player();
        p.fence = fence;
        let k = contract(0, 0, 1, 1);
        p.begin_run(&c, 1, &World::default(), &k).unwrap();
        let before = p.marks;
        p.finish_run(
            &c,
            &k,
            &result(Status::Out, false, 100, 10),
            &mut DoorRng::from_seed(1),
        );
        p.marks - before
    };
    assert_eq!([take(0), take(1), take(2)], [50, 65, 80]);
}

#[test]
fn getting_out_without_the_job_pays_only_for_data_and_costs_no_heat() {
    let c = content();
    let mut p = player();
    let k = contract(0, 0, 1, 1);
    p.begin_run(&c, 1, &World::default(), &k).unwrap();
    p.finish_run(
        &c,
        &k,
        &result(Status::Out, false, 0, 90),
        &mut DoorRng::from_seed(1),
    );
    assert_eq!((p.heat, p.jobs, p.standing[1]), (0, 0, 0));
    assert!(!p.done.contains(&k.id));
}

#[test]
fn being_burned_costs_the_loot_the_day_heat_and_a_part() {
    let c = content();
    let mut p = player();
    let k = contract(0, 0, 1, 2);
    p.rep = 10;
    p.begin_run(&c, 1, &World::default(), &k).unwrap();
    let marks = p.marks;
    // Cooling level 0 fries with 80 percent: some seed fries a part.
    let mut fried = false;
    for seed in 0..20 {
        let mut q = p.clone();
        q.finish_run(
            &c,
            &k,
            &result(Status::Burned, true, 500, 100),
            &mut DoorRng::from_seed(seed),
        );
        assert_eq!(
            (q.marks, q.cycles, q.heat, q.burns, q.jobs),
            (marks, 0, 10 + 6, 1, 0),
            "loot and reward are gone"
        );
        fried |= q.fried != 0;
    }
    assert!(fried);
}

#[test]
fn cooling_protects_the_rig() {
    let c = content();
    let fries = |cooling: u8| {
        let mut n = 0;
        for seed in 0..400 {
            let mut p = player();
            p.rig[part::COOLING] = cooling;
            let k = contract(0, 0, 1, 1);
            p.pending = Some(k.id);
            p.finish_run(
                &c,
                &k,
                &result(Status::Burned, false, 0, 100),
                &mut DoorRng::from_seed(seed),
            );
            n += u32::from(p.fried != 0);
        }
        n
    };
    let (c0, c3) = (fries(0), fries(3));
    assert!(c0 > 250 && c0 < 350, "about 80 percent: {c0}");
    assert!(c3 > 40 && c3 < 120, "about 20 percent: {c3}");
}

#[test]
fn reputation_standing_and_heat_each_close_doors() {
    let c = content();
    let mut p = player();
    let t2 = contract(4, 0, 1, 2);
    let t3 = contract(8, 0, 1, 3);
    assert!(p.closed(&c, &t2).unwrap().contains("reputation 10"));
    assert!(p.closed(&c, &t3).unwrap().contains("reputation 40"));
    p.rep = 10;
    assert!(p.closed(&c, &t2).is_none());
    p.rep = 40;
    assert!(
        p.closed(&c, &t3).unwrap().contains("standing 2"),
        "tier 3 wants trust first"
    );
    p.standing[1] = 2;
    assert!(p.closed(&c, &t3).is_none());
    p.heat = 60;
    assert!(p.closed(&c, &t3).unwrap().contains("Too hot"));
    assert!(p.closed(&c, &t2).is_none());
    p.heat = 85;
    assert!(p.closed(&c, &t2).unwrap().contains("Too hot"));
    // A fixer you have crossed three times will not deal with you at all.
    p.heat = 0;
    p.standing[1] = -3;
    assert!(
        p.closed(&c, &contract(0, 0, 1, 1))
            .unwrap()
            .contains("will not deal")
    );
    assert!(
        p.closed(&c, &contract(0, 0, 2, 1)).is_none(),
        "another fixer still will"
    );
}

#[test]
fn hitting_a_fixers_client_three_times_closes_that_fixers_contracts() {
    let c = content();
    let mut p = player();
    // Kestrel's patron is Marrow (0). Three jobs against Kestrel for other fixers.
    for (i, fixer) in [1u8, 2, 1].into_iter().enumerate() {
        let mut k = contract(0, 0, fixer, 1);
        k.id += 100 + u32::try_from(i).unwrap();
        p.begin_run(&c, 1, &World::default(), &k).unwrap();
        p.cycles = 40;
        p.finish_run(
            &c,
            &k,
            &result(Status::Out, true, 0, 10),
            &mut DoorRng::from_seed(1),
        );
    }
    assert_eq!(p.standing[0], -3);
    let from_marrow = contract(3, 3, 0, 1);
    assert!(
        p.closed(&c, &from_marrow)
            .unwrap()
            .contains("Marrow will not deal")
    );
    assert_eq!((p.standing[1], p.standing[2]), (2, 1));
}

#[test]
fn nights_pass_while_you_are_away_bots_earn_heat_cools_and_the_catch_up_is_capped() {
    let c = content();
    let mut p = player();
    p.bots = 2;
    p.heat = 30;
    p.marks = 0;
    p.cycles = 3;
    let notes = p.rollover(&c, DAY + 3);
    assert_eq!(p.marks, 3 * 2 * 7);
    assert_eq!(p.heat, 30 - 3 * 4);
    assert_eq!((p.day, p.cycles), (DAY + 3, 40));
    assert_eq!(
        notes.iter().filter(|n| n.contains("bots earned")).count(),
        3
    );
    // A month away is seven nights, not thirty.
    let mut q = player();
    q.bots = 1;
    q.marks = 0;
    q.rollover(&c, DAY + 30);
    assert_eq!(q.marks, 7 * 7);
    // The same day again does nothing.
    let marks = q.marks;
    assert!(q.rollover(&c, DAY + 30).is_empty());
    assert_eq!(q.marks, marks);
}

#[test]
fn a_hot_player_loses_marks_and_a_bot_in_a_raid_but_never_what_is_in_the_safe() {
    let c = content();
    let raided = |seed: u64| {
        let mut p = Player::new(&c, seed, "ann", 1, DAY);
        p.heat = 90;
        p.bots = 3;
        p.marks = 400;
        p.stash = 300;
        p.rollover(&c, DAY + 1);
        p
    };
    let all: Vec<Player> = (0..40).map(raided).collect();
    assert!(
        all.iter().any(|p| p.bots == 2 && p.marks < 400),
        "some raid happens"
    );
    assert!(all.iter().any(|p| p.bots == 3), "some do not");
    assert!(
        all.iter().all(|p| p.stash == 300),
        "the safe is never touched"
    );
    // Reconnecting cannot reroll the night: the same character and day always give the same result.
    assert_eq!(raided(5), raided(5));
}

#[test]
fn dropping_the_line_mid_run_costs_heat_on_the_next_visit() {
    let c = content();
    let mut p = player();
    let k = contract(0, 0, 1, 1);
    p.begin_run(&c, 1, &World::default(), &k).unwrap();
    assert!(p.pending.is_some());
    let notes = p.rollover(&c, DAY);
    assert_eq!(p.heat, 8);
    assert!(p.pending.is_none());
    assert!(notes[0].contains("dropped the line"));
    // And it is spent only once.
    assert!(p.rollover(&c, DAY).is_empty());
}

#[test]
fn laying_low_trades_cycles_for_heat() {
    let c = content();
    let mut p = player();
    assert!(p.lay_low(&c).unwrap_err().contains("not hot"));
    p.heat = 20;
    p.lay_low(&c).unwrap();
    assert_eq!((p.heat, p.cycles), (8, 32));
    p.cycles = 2;
    assert!(p.lay_low(&c).is_err());
}

#[test]
fn shop_prices_follow_the_market_and_reputation_gates_the_good_stuff() {
    let c = content();
    let mut p = player();
    p.marks = 1_000;
    p.rep = 5;
    p.buy_script(&c, 6, 100).unwrap(); // brute-forge 140
    assert_eq!(p.marks, 860);
    p.buy_script(&c, 9, 130).unwrap(); // cert-forger 140 x 1.3 = 182
    assert_eq!(p.marks, 860 - 182);
    assert!(p.buy_script(&c, 9, 100).unwrap_err().contains("already"));
    assert!(
        p.buy_script(&c, 7, 100)
            .unwrap_err()
            .contains("reputation 25"),
        "skeleton-key is gated"
    );
    assert!(p.buy_script(&c, 99, 100).is_err());
    p.marks = 5;
    assert!(
        p.buy_script(&c, 12, 100)
            .unwrap_err()
            .contains("marks needed")
    );
}

#[test]
fn the_hideout_buys_bots_a_workbench_a_fence_and_a_safe() {
    let c = content();
    let mut p = player();
    p.marks = 2_000;
    for _ in 0..5 {
        p.buy_bot(&c).unwrap();
    }
    assert!(p.buy_bot(&c).unwrap_err().contains("cannot power"));
    p.buy_workbench(&c).unwrap();
    assert!(p.buy_workbench(&c).is_err());
    p.upgrade_fence(&c).unwrap();
    p.upgrade_fence(&c).unwrap();
    assert!(p.upgrade_fence(&c).is_err());
    let before = p.marks;
    p.move_stash(&c, true).unwrap();
    assert_eq!((p.marks, p.stash), (before - 50, 50));
    p.move_stash(&c, false).unwrap();
    assert_eq!(p.stash, 0);
    assert!(p.move_stash(&c, false).is_err());
}

#[test]
fn a_new_season_keeps_the_character_and_resets_the_rest() {
    let c = content();
    let mut p = player();
    p.marks = 900;
    p.heat = 40;
    p.rep = 31;
    p.bots = 3;
    p.xp = [10, 30, 0, 0, 5];
    p.owned.push(6);
    p.rig[part::SHIELD] = 2;
    p.standing[1] = 3;
    p.done.push(5);
    p.new_season(&c, 2, DAY + 90);
    assert_eq!(
        (p.season, p.heat, p.bots, p.marks, p.rep),
        (2, 0, 0, 90, 15)
    );
    assert_eq!(
        (p.xp, p.rig[part::SHIELD], p.owned.contains(&6)),
        ([10, 30, 0, 0, 5], 2, true)
    );
    assert_eq!((p.standing[1], p.done.len()), (0, 0));
}
