//! Ship combat: invariants on random play, and what each part of a build changes.
#![allow(
    clippy::assert_is_empty,
    clippy::single_match_else,
    unused_mut,
    clippy::match_wild_err_arm,
    clippy::single_match,
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::cast_possible_truncation
)]

mod common;

use common::{Harness, orders};
use proptest::prelude::*;
use russh_bbs_door::DoorRng;
use russh_bbs_door_long_haul::combat::{
    Combat, Intent, MAX_ROUNDS, Maneuver, Orders, Phase, Range, reach_pct,
};
use russh_bbs_door_long_haul::content::Faction;
use russh_bbs_door_long_haul::crew::Role;
use russh_bbs_door_long_haul::engine::Action;
use russh_bbs_door_long_haul::events::{Event, Refusal};
use russh_bbs_door_long_haul::player::{Cause, Player};
use russh_bbs_door_long_haul::scene::Scene;
use russh_bbs_door_long_haul::ship::Lot;

fn armed(h: &Harness, name: &str) -> Player {
    let mut p = h.captain(name);
    h.fit(&mut p, "lance1");
    p
}

fn fight_of(p: &Player) -> &Combat {
    match &p.scene {
        Some(Scene::Combat(c)) => c,
        other => panic!("not in a fight: {other:?}"),
    }
}

/// Damage the captain's guns do to the enemy over one round, summed over seeds.
fn damage_dealt(build: impl Fn(&Harness) -> Player, enemy: &str, rounds: u64) -> i64 {
    let mut total = 0;
    let mut h = Harness::new(1, 1);
    for seed in 0..rounds {
        h.reseed(seed);
        let mut p = build(&h);
        h.fight(&mut p, enemy);
        let before = {
            let c = fight_of(&p);
            i64::from(c.e_hull) + i64::from(c.e_shield)
        };
        let _ = h.go_strict(&mut p, &Action::Fight(orders(4, 0, 0)));
        let after = match &p.scene {
            Some(Scene::Combat(c)) => i64::from(c.e_hull.max(0)) + i64::from(c.e_shield),
            _ => 0,
        };
        total += before - after;
    }
    total
}

/// Hull lost by the captain over one round of an enemy's fire, summed over seeds.
fn damage_taken(build: impl Fn(&Harness) -> Player, ord: Orders, enemy: &str, rounds: u64) -> i64 {
    let mut total = 0;
    let mut h = Harness::new(1, 1);
    for seed in 0..rounds {
        h.reseed(seed);
        let mut p = build(&h);
        h.fight(&mut p, enemy);
        // Put the enemy where its guns bite, with a volley telegraphed.
        if let Some(Scene::Combat(c)) = &mut p.scene {
            c.range = c.def(&h.content).reach;
            c.intent = Intent::Volley;
            c.shown = Intent::Volley;
        }
        let hull = i64::from(p.ship.hull_hp);
        let _ = h.go_strict(&mut p, &Action::Fight(ord));
        total += hull - i64::from(p.ship.hull_hp);
    }
    total
}

#[test]
fn there_are_at_least_eight_enemy_types_with_telegraphed_intent() {
    let h = Harness::new(1, 1);
    assert!(h.content.enemies.enemy.len() >= 8);
    // Honest enemies (no feint) always show what they will do.
    for id in ["cutter", "enforcer", "sentinel"] {
        let ix = h.enemy_ix(id);
        assert_eq!(h.content.enemies.enemy[ix].feint_pm, 0);
        let mut rng = DoorRng::from_seed(5);
        let mut c = Combat::new(&h.content, u8::try_from(ix).unwrap(), 100, 0, &mut rng);
        for _ in 0..200 {
            c.plan(&h.content, 0, 0, &mut rng);
            assert_eq!(c.intent, c.shown, "{id} lied");
        }
    }
}

#[test]
fn a_scanner_and_a_pilot_see_through_feints() {
    let h = Harness::new(1, 2);
    let ix = u8::try_from(h.enemy_ix("corsair")).unwrap();
    let lies = |scan: u8, pilot: u8| {
        let mut rng = DoorRng::from_seed(11);
        let mut c = Combat::new(&h.content, ix, 100, 0, &mut rng);
        let mut n = 0;
        for _ in 0..2000 {
            c.plan(&h.content, scan, pilot, &mut rng);
            n += u32::from(c.intent != c.shown);
        }
        n
    };
    let blind = lies(0, 0);
    assert!(blind > 100, "a corsair feints sometimes ({blind})");
    assert!(lies(2, 0) < blind * 7 / 10);
    assert!(lies(3, 0) < blind / 2);
    assert!(lies(3, 10) == 0, "a good enough eye is never fooled");
}

#[test]
fn weapons_like_a_band_and_the_band_decides_their_damage() {
    let h = Harness::new(1, 3);
    let mut rail = h.captain("dockhand");
    h.fit(&mut rail, "rail1");
    let mut scatter = h.captain("dockhand");
    h.fit(&mut scatter, "scatter1");
    let mut lance = h.captain("dockhand");
    h.fit(&mut lance, "lance1");
    assert!(rail.firepower(&h.content, Range::Far) > rail.firepower(&h.content, Range::Close));
    assert!(
        scatter.firepower(&h.content, Range::Close) > scatter.firepower(&h.content, Range::Far)
    );
    assert!(lance.firepower(&h.content, Range::Mid) > lance.firepower(&h.content, Range::Far));
    assert_eq!(reach_pct(Range::Far, Range::Far), 100);
    assert_eq!(reach_pct(Range::Far, Range::Close), 25);
    // Wear takes the edge off.
    let mut worn = lance.clone();
    worn.ship.modules.last_mut().unwrap().wear = 800;
    assert!(worn.firepower(&h.content, Range::Mid) < lance.firepower(&h.content, Range::Mid));
    // An unarmed ship does no harm at any range.
    assert_eq!(h.captain("dockhand").firepower(&h.content, Range::Mid), 0);
}

#[test]
fn power_is_a_budget_and_a_bad_order_changes_nothing() {
    let mut h = Harness::new(1, 4);
    let mut p = armed(&h, "dockhand");
    h.fight(&mut p, "raider");
    let budget = p.gear(&h.content).budget;
    let (before, rng) = (p.clone(), h.rng.state());
    let too_much = orders(budget, 1, 0);
    assert!(matches!(
        h.go_strict(&mut p, &Action::Fight(too_much)),
        Err(Refusal::OverPower { .. })
    ));
    assert_eq!(p, before);
    assert_eq!(h.rng.state(), rng);
    // A manoeuvre needs engine power.
    let no_engines = Orders {
        maneuver: Maneuver::Close,
        ..orders(2, 2, 0)
    };
    assert_eq!(
        h.go_strict(&mut p, &Action::Fight(no_engines)).unwrap_err(),
        Refusal::NoEnginePower
    );
    assert_eq!(p, before);
    // Within budget works.
    assert!(h.go_strict(&mut p, &Action::Fight(orders(2, 2, 2))).is_ok());
    // A reactor grows with the hull and with an engineer.
    let gunship = {
        let mut g = h.captain("dockhand");
        g.ship.hull = u8::try_from(h.content.hull_ix("gunship").unwrap()).unwrap();
        g
    };
    assert!(gunship.gear(&h.content).budget > h.captain("dockhand").gear(&h.content).budget);
    let mut with_engineer = h.captain("dockhand");
    h.sign(&mut with_engineer, Role::Engineer, 9);
    h.set_level(
        &mut with_engineer,
        russh_bbs_door_long_haul::skills::Skill::Engineer,
        4,
    );
    assert!(with_engineer.gear(&h.content).budget > h.captain("dockhand").gear(&h.content).budget);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn a_fight_always_ends_and_never_breaks_an_invariant(
        seed in any::<u64>(),
        enemy in 0usize..9,
        picks in proptest::collection::vec((0u8..7, 0u8..5, 0u8..4, 0u8..3, 0u8..12), 1..40),
        armed_ship in any::<bool>(),
        crewed in any::<bool>(),
    ) {
        let mut h = Harness::new(1, seed);
        let mut p = h.captain("dockhand");
        if armed_ship {
            h.fit(&mut p, "lance1");
            h.fit(&mut p, "shield1");
        }
        if crewed {
            h.sign(&mut p, Role::Gunner, 4);
            h.sign(&mut p, Role::Engineer, 4);
        }
        p.ship.cargo.push(Lot { good: h.good("ore"), tons: 10, paid: 200 });
        let max = h.content.hulls[usize::from(p.ship.hull)].hull_points;
        let enemy_id = h.content.enemies.enemy[enemy].id.clone();
        h.fight(&mut p, &enemy_id);
        let mut last_range = fight_of(&p).range;
        for (w, s, e, m, run) in picks {
            let Some(Scene::Combat(c)) = p.scene.clone() else { break };
            if c.phase == Phase::Won {
                let _ = h.go_strict(&mut p, &Action::Wreck { board: false });
                break;
            }
            let action = if run == 0 {
                Action::Run
            } else {
                Action::Fight(Orders {
                    weapons: w,
                    shields: s,
                    engines: e,
                    maneuver: [Maneuver::Hold, Maneuver::Close, Maneuver::Open][usize::from(m)],
                })
            };
            let before = p.clone();
            let rng_before = h.rng.state();
            match h.go_strict(&mut p, &action) {
                Err(_) => {
                    prop_assert_eq!(&p, &before);
                    prop_assert_eq!(h.rng.state(), rng_before);
                    // Running is never refused inside a live fight.
                    prop_assert!(!matches!(action, Action::Run));
                }
                Ok(_) => {}
            }
            prop_assert!(p.ship.hull_hp >= 1 && p.ship.hull_hp <= max);
            prop_assert_eq!(p.ledger.net(), p.cash);
            if let Some(Scene::Combat(c)) = &p.scene {
                let def = c.def(&h.content);
                prop_assert!(c.e_hull <= i16::try_from(def.hull).unwrap());
                prop_assert!(c.e_shield >= 0 && c.e_shield <= i16::try_from(def.shield).unwrap());
                prop_assert!(c.range.index().abs_diff(last_range.index()) <= 1);
                prop_assert!(c.round <= MAX_ROUNDS);
                last_range = c.range;
            }
        }
        // However it went, a captain can always get out: run until it ends.
        let mut guard = 0;
        while p.scene.is_some() && guard < 120 {
            guard += 1;
            let act = match &p.scene {
                Some(Scene::Combat(c)) if c.phase == Phase::Won => Action::Wreck { board: false },
                _ => Action::Run,
            };
            let _ = h.go_strict(&mut p, &act);
        }
        prop_assert!(p.scene.is_none(), "still fighting after {guard} runs");
        let bytes = p.encode();
        prop_assert_eq!(Player::decode(&bytes, &h.content, &h.frontier).unwrap(), p);
    }
}

#[test]
fn a_gunner_hits_harder() {
    let alone = damage_dealt(|h| armed(h, "dockhand"), "raider", 300);
    let with_gunner = damage_dealt(
        |h| {
            let mut p = armed(h, "dockhand");
            h.sign(&mut p, Role::Gunner, 8);
            p
        },
        "raider",
        300,
    );
    assert!(
        with_gunner > alone * 12 / 10,
        "{with_gunner} against {alone}"
    );
}

#[test]
fn shields_and_engines_each_keep_the_hull_whole() {
    let ship = |h: &Harness| {
        let mut p = armed(h, "dockhand");
        h.fit(&mut p, "shield1");
        p
    };
    let bare = damage_taken(ship, orders(6, 0, 0), "raider", 300);
    let shielded = damage_taken(ship, orders(1, 5, 0), "raider", 300);
    let evasive = damage_taken(ship, orders(1, 0, 5), "raider", 300);
    assert!(shielded < bare / 2, "shields: {shielded} against {bare}");
    assert!(evasive < bare, "engines: {evasive} against {bare}");
    // A good pilot dodges more than a poor one.
    let piloted = damage_taken(
        |h| {
            let mut p = ship(h);
            h.sign(&mut p, Role::Pilot, 10);
            p
        },
        orders(6, 0, 0),
        "raider",
        300,
    );
    assert!(piloted < bare);
}

#[test]
fn an_engineer_patches_the_hull_between_exchanges() {
    let mut h = Harness::new(1, 5);
    let mut p = armed(&h, "dockhand");
    h.sign(&mut p, Role::Engineer, 6);
    p.ship.hull_hp = 60;
    h.fight(&mut p, "skiff");
    let events = h
        .go_strict(&mut p, &Action::Fight(orders(3, 2, 1)))
        .unwrap();
    assert!(events.iter().any(|e| matches!(e, Event::Patched { .. })));
    let mut alone = armed(&h, "dockhand");
    alone.ship.hull_hp = 60;
    h.fight(&mut alone, "skiff");
    let events = h
        .go_strict(&mut alone, &Action::Fight(orders(3, 2, 1)))
        .unwrap();
    assert!(!events.iter().any(|e| matches!(e, Event::Patched { .. })));
}

#[test]
fn running_is_always_allowed_and_a_good_pilot_far_away_gets_clear() {
    let mut h = Harness::new(1, 6);
    let skiff = h.content.enemies.enemy[h.enemy_ix("skiff")].clone();
    let slow = h.content.enemies.enemy[h.enemy_ix("sentinel")].clone();
    let mut p = h.captain("dockhand");
    // An unarmed ship has no answer but this one.
    h.fight(&mut p, "skiff");
    assert!(h.go_strict(&mut p, &Action::Run).is_ok());
    // The odds respond to the band, the pilot and the enemy's speed.
    let alone = h.captain("dockhand");
    let mut piloted = alone.clone();
    h.sign(&mut piloted, Role::Pilot, 10);
    h.set_level(
        &mut piloted,
        russh_bbs_door_long_haul::skills::Skill::Pilot,
        6,
    );
    assert!(
        alone.run_chance(&h.content, &skiff, Range::Far)
            > alone.run_chance(&h.content, &skiff, Range::Close)
    );
    assert!(
        piloted.run_chance(&h.content, &skiff, Range::Mid)
            > alone.run_chance(&h.content, &skiff, Range::Mid)
    );
    assert!(
        alone.run_chance(&h.content, &slow, Range::Mid)
            > alone.run_chance(&h.content, &skiff, Range::Mid)
    );
    // And it shows in what happens.
    let escapes = |p: &Player| {
        let mut n = 0;
        let mut h = Harness::new(1, 1);
        for seed in 0..300u64 {
            h.reseed(seed);
            let mut q = p.clone();
            h.fight(&mut q, "skiff");
            let _ = h.go_strict(&mut q, &Action::Run);
            n += u32::from(q.scene.is_none() && q.stats.fled == 1);
        }
        n
    };
    assert!(escapes(&piloted) > escapes(&alone) + 30);
    assert_eq!(piloted.stats.fled, 0);
}

#[test]
fn a_win_pays_salvage_costs_standing_and_may_offer_the_wreck() {
    let mut h = Harness::new(1, 7);
    let mut p = armed(&h, "dockhand");
    h.fit(&mut p, "lance2");
    p.standing = [0; 5];
    h.fight(&mut p, "cutter");
    // Soften it so one volley finishes it.
    if let Some(Scene::Combat(c)) = &mut p.scene {
        c.e_hull = 1;
        c.e_shield = 0;
        c.range = Range::Mid;
        c.intent = Intent::Brace;
        c.shown = Intent::Brace;
    }
    let cash = p.cash;
    let mut events = Vec::new();
    for _ in 0..20 {
        if let Ok(mut ev) = h.go_strict(&mut p, &Action::Fight(orders(6, 0, 0))) {
            events.append(&mut ev);
        }
        if p.scene.is_none() {
            break;
        }
    }
    assert!(
        events.iter().any(|e| matches!(e, Event::Wrecked { .. })),
        "{events:?}"
    );
    assert!(p.cash > cash);
    assert_eq!(p.ledger.income[Cause::Salvage.index()], p.cash - cash);
    assert_eq!(p.ledger.net(), p.cash);
    assert_eq!(p.stats.fights_won, 1);
    // Wrecking a League cutter is a deed the League remembers, and the Free Captains like.
    assert_eq!(p.standing[Faction::League.index()], -25);
    assert!(p.standing[Faction::Free.index()] > 0);
    // A pirate wreck can be boarded.
    let mut q = armed(&h, "dockhand");
    q.docked = None;
    h.fight(&mut q, "skiff");
    if let Some(Scene::Combat(c)) = &mut q.scene {
        c.e_hull = 1;
        c.e_shield = 0;
        c.intent = Intent::Brace;
        c.shown = Intent::Brace;
        c.range = Range::Mid;
    }
    for _ in 0..20 {
        let _ = h.go_strict(&mut q, &Action::Fight(orders(6, 0, 0)));
        if matches!(&q.scene, Some(Scene::Combat(c)) if c.phase == Phase::Won) || q.scene.is_none()
        {
            break;
        }
    }
    if matches!(&q.scene, Some(Scene::Combat(c)) if c.phase == Phase::Won) {
        h.go_strict(&mut q, &Action::Wreck { board: true }).unwrap();
        assert!(matches!(q.scene, Some(Scene::Boarding(_))));
    }
}

#[test]
fn being_beaten_costs_cargo_cash_and_possibly_a_hand() {
    let mut beaten = 0;
    let mut lost_hands = 0;
    let mut h = Harness::new(1, 1);
    for seed in 0..80u64 {
        h.reseed(seed);
        let mut p = h.captain("dockhand");
        h.sign(&mut p, Role::Pilot, 3);
        h.sign(&mut p, Role::Face, 3);
        p.ship.cargo.push(Lot {
            good: h.good("ore"),
            tons: 20,
            paid: 400,
        });
        p.ship.hull_hp = 14;
        h.fight(&mut p, "corsair");
        let (cash, crew) = (p.cash, p.crew.len());
        for _ in 0..30 {
            if p.scene.is_none() {
                break;
            }
            let _ = h.go_strict(&mut p, &Action::Fight(orders(0, 0, 0)));
        }
        if p.stats.fights_lost == 1 {
            beaten += 1;
            assert!(p.cash < cash);
            assert!(p.ship.cargo_tons() < 20);
            assert_eq!(p.ledger.net(), p.cash);
            assert!(p.ship.hull_hp >= 1);
            lost_hands += crew - p.crew.len();
        }
    }
    assert!(beaten > 50, "{beaten} of 80 beaten");
    assert!(lost_hands > 0);
}

#[test]
fn customs_take_the_contraband_and_fine_a_beaten_captain() {
    let mut h = Harness::new(1, 8);
    let mut p = h.captain("dockhand");
    p.ship.cargo.push(Lot {
        good: h.good("ghostwire"),
        tons: 5,
        paid: 1000,
    });
    p.ship.hull_hp = 10;
    let debt = p.debt.principal;
    h.fight(&mut p, "gunboat");
    for _ in 0..20 {
        if p.scene.is_none() {
            break;
        }
        let _ = h.go_strict(&mut p, &Action::Fight(orders(0, 0, 0)));
    }
    assert_eq!(p.ship.contraband_tons(&h.content), 0);
    assert!(p.debt.principal > debt || p.cash < 1500, "fined");
}

#[test]
fn boarders_are_turned_back_by_a_crew_and_strip_a_lone_captains_hold() {
    let strip = |crew: bool| {
        let mut stripped = 0u32;
        let mut h = Harness::new(1, 1);
        for seed in 0..120u64 {
            h.reseed(seed);
            let mut p = h.captain("dockhand");
            if crew {
                for _ in 0..2 {
                    h.sign(&mut p, Role::Gunner, 6);
                }
            }
            p.ship.cargo.push(Lot {
                good: h.good("ore"),
                tons: 20,
                paid: 400,
            });
            h.fight(&mut p, "corsair");
            if let Some(Scene::Combat(c)) = &mut p.scene {
                c.range = Range::Close;
                c.intent = Intent::Grapple;
                c.shown = Intent::Grapple;
            }
            let _ = h.go_strict(&mut p, &Action::Fight(orders(0, 0, 0)));
            stripped += 20 - p.ship.cargo_tons().min(20);
        }
        stripped
    };
    let (alone, crewed) = (strip(false), strip(true));
    assert!(
        crewed * 2 < alone,
        "stripped {crewed} with a crew, {alone} alone"
    );
}

#[test]
fn an_escorted_ship_takes_some_of_the_fire() {
    let mut h = Harness::new(1, 9);
    let mut p = armed(&h, "dockhand");
    p.contracts
        .push(russh_bbs_door_long_haul::contracts::Contract {
            kind: russh_bbs_door_long_haul::contracts::Kind::Escort,
            faction: Faction::League,
            from: h.frontier.home_station,
            to: h.frontier.home_station,
            good: 0,
            tons: 0,
            pax: 0,
            count: 0,
            target: 0,
            reward: 500,
            due: 99_999,
            progress: 100,
            mood: 7,
        });
    let mut hit = false;
    for seed in 0..60u64 {
        h.reseed(seed);
        let mut q = p.clone();
        h.fight(&mut q, "raider");
        for _ in 0..6 {
            if q.scene.is_none() {
                break;
            }
            let _ = h.go_strict(&mut q, &Action::Fight(orders(3, 1, 2)));
        }
        if q.contracts[0].progress < 100 {
            hit = true;
            assert!(q.contracts[0].progress >= 1, "never wiped out in one fight");
        }
    }
    assert!(hit, "the ward is sometimes the target");
}
