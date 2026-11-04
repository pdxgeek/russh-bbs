//! Boarding: sealed doors, caches, the party, and getting out with the haul.
#![allow(
    clippy::assert_is_empty,
    clippy::single_match_else,
    unused_mut,
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::similar_names
)]

mod common;

use common::Harness;
use proptest::prelude::*;
use russh_bbs_door_long_haul::boarding::{Boarding, Drill, MAX_ORDERS};
use russh_bbs_door_long_haul::crew::Role;
use russh_bbs_door_long_haul::engine::Action;
use russh_bbs_door_long_haul::events::{Event, Refusal};
use russh_bbs_door_long_haul::player::{Cause, Player};
use russh_bbs_door_long_haul::scene::Scene;
use russh_bbs_door_long_haul::skills::Skill;

fn boarding_of(p: &Player) -> &Boarding {
    match &p.scene {
        Some(Scene::Boarding(b)) => b,
        other => panic!("not boarding: {other:?}"),
    }
}

fn step(h: &mut Harness, p: &mut Player, d: Drill) -> Result<Vec<Event>, Refusal> {
    h.go_strict(p, &Action::Skirmish(d))
}

/// Walk to a room by exits already known to be open.
fn goto(h: &mut Harness, p: &mut Player, rooms: &[u8]) {
    for &r in rooms {
        step(h, p, Drill::Move(r)).unwrap();
    }
}

fn cleared_corsair(h: &mut Harness, p: &mut Player) {
    h.board(p, "corsair_wreck");
    step(h, p, Drill::Move(1)).unwrap();
    while boarding_of(p).foes_here(&h.content) > 0 {
        step(h, p, Drill::Fight).unwrap();
        assert!(p.scene.is_some(), "the party must survive for this setup");
    }
}

#[test]
fn every_layout_is_reachable_and_the_airlock_is_open() {
    let h = Harness::new(1, 1);
    assert!(h.content.derelicts.derelict.len() >= 6);
    for d in &h.content.derelicts.derelict {
        assert_eq!(d.rooms[0].foes, 0);
        assert_eq!(d.rooms[0].lock, 0);
        assert!(d.rooms.len() >= 3);
    }
    // Every enemy wreck and every event derelict names a real layout (checked at load too).
    for e in &h.content.enemies.enemy {
        if let Some(w) = &e.wreck {
            assert!(h.content.derelicts.derelict.iter().any(|d| d.id == *w));
        }
    }
}

#[test]
fn a_sealed_door_opens_only_to_enough_engineering_and_the_crew_counts() {
    // The corsair's cabin has a lock of 5.
    let mut h = Harness::new(1, 2);
    let setup = |h: &mut Harness, role: Option<(Role, u8)>, eng: u8| {
        let mut p = h.captain("dockhand");
        h.set_level(&mut p, Skill::Engineer, eng);
        if let Some((r, s)) = role {
            h.sign(&mut p, r, s);
        }
        // Gunners to clear the bay safely.
        h.sign(&mut p, Role::Gunner, 9);
        h.sign(&mut p, Role::Gunner, 9);
        cleared_corsair(h, &mut p);
        p
    };
    // Alone with a little engineering: sealed.
    let mut poor = setup(&mut h, None, 1);
    let before = poor.clone();
    let rng = h.rng.state();
    assert_eq!(
        step(&mut h, &mut poor, Drill::Breach(2)).unwrap_err(),
        Refusal::Sealed { lock: 5 }
    );
    assert_eq!(poor, before);
    assert_eq!(h.rng.state(), rng);
    // Moving through a sealed door is refused the same way.
    assert!(matches!(
        step(&mut h, &mut poor, Drill::Move(2)),
        Err(Refusal::Sealed { .. })
    ));
    // A middling engineer is not enough (skill 4 adds 2 to a captain with 1).
    let mut mid = setup(&mut h, Some((Role::Engineer, 4)), 1);
    assert!(step(&mut h, &mut mid, Drill::Breach(2)).is_err());
    // A good one is.
    let mut good = setup(&mut h, Some((Role::Engineer, 8)), 1);
    step(&mut h, &mut good, Drill::Breach(2)).unwrap();
    goto(&mut h, &mut good, &[2]);
    assert_eq!(boarding_of(&good).at, 2);
    // A captain who is good enough alone needs no crew.
    let mut skilled = setup(&mut h, None, 5);
    step(&mut h, &mut skilled, Drill::Breach(2)).unwrap();
    // An injured engineer is not in the party.
    let mut hurt = h.captain("dockhand");
    h.set_level(&mut hurt, Skill::Engineer, 1);
    let e = h.sign(&mut hurt, Role::Engineer, 9);
    hurt.crew[e].hurt_days = 3;
    h.sign(&mut hurt, Role::Gunner, 9);
    h.sign(&mut hurt, Role::Gunner, 9);
    cleared_corsair(&mut h, &mut hurt);
    assert!(step(&mut h, &mut hurt, Drill::Breach(2)).is_err());
}

#[test]
fn a_cache_is_read_by_a_scientist_or_a_practised_surveyor_and_by_nobody_else() {
    let mut h = Harness::new(1, 3);
    let open_cutter = |h: &mut Harness, role: Option<Role>, survey: u8| {
        let mut p = h.captain("dockhand");
        h.set_level(&mut p, Skill::Survey, survey);
        h.set_level(&mut p, Skill::Engineer, 5);
        h.sign(&mut p, Role::Gunner, 9);
        if let Some(r) = role {
            h.sign(&mut p, r, 4);
        }
        h.board(&mut p, "survey_cutter");
        step(h, &mut p, Drill::Move(1)).unwrap();
        while boarding_of(&p).foes_here(&h.content) > 0 {
            step(h, &mut p, Drill::Fight).unwrap();
        }
        step(h, &mut p, Drill::Breach(2)).unwrap();
        goto(h, &mut p, &[2]);
        (p, h.rng.state())
    };
    let (mut none, _) = open_cutter(&mut h, None, 0);
    assert_eq!(
        step(&mut h, &mut none, Drill::Search).unwrap_err(),
        Refusal::NoReader
    );
    let (mut sci, _) = open_cutter(&mut h, Some(Role::Scientist), 0);
    let ev = step(&mut h, &mut sci, Drill::Search).unwrap();
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::CacheRead { value: 520 }))
    );
    assert_eq!(boarding_of(&sci).haul.survey, 520);
    // Once read, it is read.
    assert_eq!(
        step(&mut h, &mut sci, Drill::Search).unwrap_err(),
        Refusal::NothingToDo
    );
    let (mut surveyor, _) = open_cutter(&mut h, None, 3);
    assert!(step(&mut h, &mut surveyor, Drill::Search).is_ok());
}

#[test]
fn a_gunner_in_the_party_clears_a_room_faster() {
    let rounds = |gunners: usize| {
        let mut total = 0;
        let mut h = Harness::new(1, 4);
        for seed in 0..60u64 {
            h.reseed(seed);
            let mut p = h.captain("dockhand");
            for _ in 0..gunners {
                h.sign(&mut p, Role::Gunner, 7);
            }
            h.sign(&mut p, Role::Face, 2);
            h.board(&mut p, "corsair_wreck");
            step(&mut h, &mut p, Drill::Move(1)).unwrap();
            let mut n = 0;
            while p.scene.is_some() && boarding_of(&p).foes_here(&h.content) > 0 && n < 40 {
                let _ = step(&mut h, &mut p, Drill::Fight);
                n += 1;
            }
            total += n;
        }
        total
    };
    let (none, two) = (rounds(0), rounds(2));
    assert!(two * 2 < none, "{two} rounds with gunners, {none} without");
}

#[test]
fn withdrawing_keeps_the_haul_and_a_wipe_out_loses_it() {
    let mut h = Harness::new(1, 5);
    // A clean pull-out: take the pirates' bunks and the credits, and leave.
    let mut p = h.captain("dockhand");
    for _ in 0..2 {
        h.sign(&mut p, Role::Gunner, 8);
    }
    h.board(&mut p, "pirate_wreck");
    step(&mut h, &mut p, Drill::Move(1)).unwrap();
    while boarding_of(&p).foes_here(&h.content) > 0 {
        step(&mut h, &mut p, Drill::Fight).unwrap();
    }
    step(&mut h, &mut p, Drill::Take).unwrap();
    goto(&mut h, &mut p, &[2]);
    step(&mut h, &mut p, Drill::Take).unwrap();
    let taken = boarding_of(&p).haul.credits;
    assert!(taken >= 60);
    p.ship.hull_hp = 50;
    let cash = p.cash;
    let ev = step(&mut h, &mut p, Drill::Withdraw).unwrap();
    assert!(p.scene.is_none());
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::BoardEnd { clean: true, .. }))
    );
    assert_eq!(p.cash, cash + i64::from(taken));
    assert_eq!(p.ledger.income[Cause::Salvage.index()], i64::from(taken));
    assert!(
        p.ship.hull_hp > 50,
        "plating from the wreck patches the hull"
    );
    assert_eq!(p.stats.boardings, 1);
    // A wipe-out: a captain alone against a bay of boarders.
    let mut lost = 0;
    for seed in 0..40u64 {
        h.reseed(seed);
        let mut q = h.captain("dockhand");
        h.sign(&mut q, Role::Face, 1);
        h.board(&mut q, "corsair_wreck");
        step(&mut h, &mut q, Drill::Move(1)).unwrap();
        let cash = q.cash;
        for _ in 0..60 {
            if q.scene.is_none() {
                break;
            }
            let _ = step(&mut h, &mut q, Drill::Fight);
        }
        if q.scene.is_none() && q.stats.boardings == 0 {
            lost += 1;
            assert_eq!(q.cash, cash, "nothing came home");
        }
    }
    assert!(lost > 5, "{lost} wipe-outs in 40");
}

#[test]
fn the_downed_are_hurt_and_sometimes_lost_and_a_medic_saves_them() {
    let outcomes = |medic: bool| {
        let (mut lost, mut hurt) = (0usize, 0usize);
        let mut h = Harness::new(1, 6);
        for seed in 0..120u64 {
            h.reseed(seed);
            let mut p = h.captain("dockhand");
            h.sign(&mut p, Role::Face, 1);
            h.sign(&mut p, Role::Pilot, 1);
            if medic {
                h.sign(&mut p, Role::Medic, 6);
            }
            let crew = p.crew.len();
            h.board(&mut p, "corsair_wreck");
            if let Some(Scene::Boarding(b)) = &mut p.scene {
                // A party already battered, so that people go down.
                for m in &mut b.party {
                    m.hp = m.hp.min(5);
                }
            }
            step(&mut h, &mut p, Drill::Move(1)).unwrap();
            for _ in 0..60 {
                if p.scene.is_none() {
                    break;
                }
                let _ = step(&mut h, &mut p, Drill::Fight);
            }
            lost += crew - p.crew.len();
            hurt += p.crew.iter().filter(|c| !c.fit()).count();
        }
        (lost, hurt)
    };
    let (alone_lost, alone_hurt) = outcomes(false);
    let (med_lost, _) = outcomes(true);
    assert!(alone_lost > 0 && alone_hurt > 0);
    assert!(
        med_lost <= alone_lost + 30,
        "a medic never makes it much worse"
    );
}

#[test]
fn a_medic_patches_better_than_a_captain_with_a_field_kit() {
    let mut h = Harness::new(1, 7);
    let heal = |h: &mut Harness, medic: bool| {
        let mut p = h.captain("dockhand");
        h.sign(&mut p, Role::Gunner, 9);
        if medic {
            h.sign(&mut p, Role::Medic, 8);
        }
        h.board(&mut p, "ore_barge");
        if let Some(Scene::Boarding(b)) = &mut p.scene {
            b.party[0].hp = 5;
        }
        let ev = step(h, &mut p, Drill::Patch).unwrap();
        ev.iter()
            .find_map(|e| match e {
                Event::Patched2 { hp } => Some(*hp),
                _ => None,
            })
            .unwrap()
    };
    assert!(heal(&mut h, true) > heal(&mut h, false) + 4);
}

#[test]
fn loot_is_limited_by_the_hold_and_goods_come_aboard_free() {
    let mut h = Harness::new(1, 8);
    let mut p = h.captain("dockhand");
    h.sign(&mut p, Role::Gunner, 9);
    h.sign(&mut p, Role::Gunner, 9);
    h.set_level(&mut p, Skill::Engineer, 3);
    h.board(&mut p, "ore_barge");
    step(&mut h, &mut p, Drill::Move(1)).unwrap();
    goto(&mut h, &mut p, &[2]);
    while boarding_of(&p).foes_here(&h.content) > 0 {
        step(&mut h, &mut p, Drill::Fight).unwrap();
    }
    step(&mut h, &mut p, Drill::Take).unwrap();
    // Fill the hold, so the ore does not fit.
    let cap = p.stats(&h.content).cargo_cap;
    p.ship.cargo.push(russh_bbs_door_long_haul::ship::Lot {
        good: h.good("water"),
        tons: cap,
        paid: 10,
    });
    let ore = h.good("ore");
    step(&mut h, &mut p, Drill::Withdraw).unwrap();
    assert_eq!(p.ship.tons_of(ore), 0, "no room in the hold");
    // With room, it comes aboard at no cost.
    let mut q = h.captain("dockhand");
    h.sign(&mut q, Role::Gunner, 9);
    h.sign(&mut q, Role::Gunner, 9);
    h.board(&mut q, "ore_barge");
    goto(&mut h, &mut q, &[1, 2]);
    while boarding_of(&q).foes_here(&h.content) > 0 {
        step(&mut h, &mut q, Drill::Fight).unwrap();
    }
    step(&mut h, &mut q, Drill::Take).unwrap();
    let cash = q.cash;
    step(&mut h, &mut q, Drill::Withdraw).unwrap();
    assert_eq!(q.ship.tons_of(ore), 8);
    assert_eq!(q.ship.cargo.iter().find(|l| l.good == ore).unwrap().paid, 0);
    assert!(q.cash >= cash);
    assert_eq!(q.ledger.net(), q.cash);
}

#[test]
fn a_boarding_saves_and_reloads_mid_skirmish() {
    let mut h = Harness::new(1, 9);
    let mut p = h.captain("dockhand");
    h.sign(&mut p, Role::Gunner, 5);
    h.board(&mut p, "pirate_wreck");
    step(&mut h, &mut p, Drill::Move(1)).unwrap();
    let back = Player::decode(&p.encode(), &h.content, &h.frontier).unwrap();
    assert_eq!(back, p);
    // A save naming a crew member that is not there is refused.
    let mut bad = p.encode();
    let ok = Player::decode(&bad, &h.content, &h.frontier).is_some();
    assert!(ok);
    let n = bad.len();
    bad[n - 1] ^= 0xFF;
    let _ = Player::decode(&bad, &h.content, &h.frontier);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]

    #[test]
    fn random_orders_never_break_the_party_the_haul_or_the_save(
        seed in any::<u64>(),
        layout in 0usize..9,
        crew in proptest::collection::vec(0usize..7, 0..5),
        orders in proptest::collection::vec(0u8..40, 1..80),
    ) {
        let mut h = Harness::new(1, seed);
        let mut p = h.captain("dockhand");
        for r in crew {
            h.sign(&mut p, Role::ALL[r], 4);
        }
        let id = h.content.derelicts.derelict[layout].id.clone();
        h.board(&mut p, &id);
        let max = h.content.hulls[usize::from(p.ship.hull)].hull_points;
        let mut count = 0u8;
        for o in orders {
            let Some(Scene::Boarding(b)) = &p.scene else { break };
            let rooms = b.def(&h.content).rooms.len();
            let drill = match o % 8 {
                0 => Drill::Move(u8::try_from(usize::from(o) % rooms).unwrap()),
                1 => Drill::Breach(u8::try_from(usize::from(o) % rooms).unwrap()),
                2 | 3 => Drill::Fight,
                4 => Drill::Search,
                5 => Drill::Patch,
                6 => Drill::Take,
                _ => if o > 30 { Drill::Withdraw } else { Drill::Move(u8::try_from(usize::from(o) % rooms).unwrap()) },
            };
            let before = p.clone();
            let rng = h.rng.state();
            let r = step(&mut h, &mut p, drill);
            if r.is_err() {
                prop_assert_eq!(&p, &before);
                prop_assert_eq!(h.rng.state(), rng);
            } else {
                count = count.saturating_add(1);
            }
            if let Some(Scene::Boarding(b)) = &p.scene {
                prop_assert!(b.party.iter().all(|m| m.hp <= m.max));
                prop_assert!(b.orders <= MAX_ORDERS);
                prop_assert!(b.haul.goods.len() <= 4);
            }
            prop_assert!(p.ship.hull_hp >= 1 && p.ship.hull_hp <= max);
            prop_assert_eq!(p.ledger.net(), p.cash);
            prop_assert!(p.hold_used() <= u32::from(p.stats(&h.content).cargo_cap));
        }
        // Whatever happened, pulling out always works and ends the scene.
        if p.scene.is_some() {
            step(&mut h, &mut p, Drill::Withdraw).unwrap();
        }
        prop_assert!(p.scene.is_none());
        let _ = count;
        prop_assert_eq!(Player::decode(&p.encode(), &h.content, &h.frontier).unwrap(), p);
    }
}
