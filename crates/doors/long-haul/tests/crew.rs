//! Crew: hiring, upkeep, goals, unrest, injury, and what each role changes (the RPG rule).
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

use common::{DAY0, Harness, member};
use russh_bbs_door_long_haul::content::Faction;
use russh_bbs_door_long_haul::crew::{
    FOOD, Goal, GoalKind, PAY, RECRUITS, Role, epoch_of, recruit_key, recruits,
};
use russh_bbs_door_long_haul::engine::{Action, RepairTarget};
use russh_bbs_door_long_haul::events::{Event, Refusal};
use russh_bbs_door_long_haul::player::Player;
use russh_bbs_door_long_haul::ship::{Lot, Ship};
use russh_bbs_door_long_haul::skills::Skill;

fn docked(h: &Harness, p: &mut Player, economy: &str) -> u16 {
    let st = h.station(economy);
    h.dock_free(p, st);
    st
}

#[test]
fn recruits_are_a_function_of_the_station_and_the_epoch_and_follow_the_economy() {
    let h = Harness::new(1, 1);
    let st = h.station("industrial");
    let base = DAY0 - DAY0.rem_euclid(3);
    let a = recruits(&h.content, &h.frontier, st, base, 0);
    let b = recruits(&h.content, &h.frontier, st, base + 2, 0);
    assert_eq!(a.len(), RECRUITS);
    assert_eq!(a, b, "the same three-day epoch shows the same faces");
    assert_ne!(
        a,
        recruits(&h.content, &h.frontier, st, base + 6, 0),
        "later, new faces"
    );
    assert_eq!(epoch_of(base), epoch_of(base + 2));
    // Industrial yards send engineers and gunners, farms send quartermasters and medics.
    let share = |economy: &str, roles: &[Role]| {
        let (mut hit, mut all) = (0u32, 0u32);
        for st in h.stations(economy).into_iter().take(8) {
            for day in 0..30 {
                for r in recruits(&h.content, &h.frontier, st, DAY0 + day * 3, 0) {
                    all += 1;
                    hit += u32::from(roles.contains(&r.role));
                }
            }
        }
        f64::from(hit) / f64::from(all)
    };
    let like = [Role::Engineer, Role::Gunner];
    assert!(share("industrial", &like) > share("farming", &like) + 0.15);
    let like = [Role::Quartermaster, Role::Medic];
    assert!(share("farming", &like) > share("industrial", &like) + 0.15);
}

#[test]
fn a_good_name_brings_better_hands() {
    let h = Harness::new(1, 1);
    let st = h.station("military");
    let (mut low, mut high) = (0u32, 0u32);
    for d in 0..40 {
        low += recruits(&h.content, &h.frontier, st, DAY0 + d * 3, 0)
            .iter()
            .map(|c| u32::from(c.skill))
            .sum::<u32>();
        high += recruits(&h.content, &h.frontier, st, DAY0 + d * 3, 30)
            .iter()
            .map(|c| u32::from(c.skill))
            .sum::<u32>();
    }
    assert!(high > low, "{high} against {low}");
}

#[test]
fn hiring_costs_a_fee_takes_a_berth_and_cannot_be_repeated() {
    let mut h = Harness::new(1, 2);
    let mut p = h.captain("dockhand");
    let st = docked(&h, &mut p, "industrial");
    let wage = recruits(&h.content, &h.frontier, st, DAY0, 0)[0].wage;
    let cash = p.cash;
    let ev = h.go(&mut p, &Action::Hire(0)).unwrap();
    assert!(matches!(ev[0], Event::Hired { .. }));
    assert_eq!(p.crew.len(), 1);
    assert_eq!(p.cash, cash - 2 * i64::from(wage));
    assert_eq!(p.ledger.net(), p.cash, "the fee is on the ledger");
    // The same face is gone from the list.
    assert_eq!(
        h.go(&mut p, &Action::Hire(0)).unwrap_err(),
        Refusal::NoSuchThing
    );
    // The freighter has two hull berths; a third hand has nowhere to sleep.
    h.go(&mut p, &Action::Hire(1)).unwrap();
    let before = p.clone();
    assert_eq!(
        h.go(&mut p, &Action::Hire(2)).unwrap_err(),
        Refusal::NoBerth
    );
    assert_eq!(p, before, "a refusal changes nothing");
    // A cabin module adds two berths.
    h.fit(&mut p, "cabin1");
    h.go(&mut p, &Action::Hire(2)).unwrap();
    assert_eq!(p.crew.len(), 3);
    assert!(
        recruit_key(st, epoch_of(DAY0), 0) != recruit_key(st, epoch_of(DAY0), 1),
        "each face has its own key"
    );
}

#[test]
fn recruits_will_not_sign_with_a_captain_their_faction_despises() {
    let mut h = Harness::new(1, 3);
    let mut p = h.captain("dockhand");
    let st = docked(&h, &mut p, "military");
    let faction = h.content.economies[usize::from(h.frontier.station(st).unwrap().economy)].faction;
    assert_eq!(faction, Faction::League);
    p.standing[Faction::League.index()] = -30;
    assert_eq!(
        h.go(&mut p, &Action::Hire(0)).unwrap_err(),
        Refusal::Unwelcome(Faction::League)
    );
    // The same recruits sign at a haven, whose faction has no quarrel with this captain.
    let haven = docked(&h, &mut p, "haven");
    let _ = haven;
    h.go(&mut p, &Action::Hire(0)).unwrap();
    // And a captain the League likes can hire at the post.
    let mut friend = h.captain("washout");
    docked(&h, &mut friend, "military");
    h.go(&mut friend, &Action::Hire(0)).unwrap();
}

#[test]
fn a_role_adds_to_the_captains_own_skill_and_an_injured_hand_adds_nothing() {
    let mut h = Harness::new(1, 4);
    let mut p = h.captain("dockhand");
    assert_eq!(p.eff(Skill::Pilot), p.level(Skill::Pilot));
    let i = h.sign(&mut p, Role::Pilot, 6);
    assert_eq!(p.eff(Skill::Pilot), p.level(Skill::Pilot) + 3);
    // Half their skill rounded up, and one more once sworn.
    p.crew[i].sworn = true;
    assert_eq!(p.eff(Skill::Pilot), p.level(Skill::Pilot) + 4);
    // Hurt: nothing. Scarred: a point less of skill.
    p.crew[i].hurt_days = 3;
    assert_eq!(p.eff(Skill::Pilot), p.level(Skill::Pilot));
    p.crew[i].hurt_days = 0;
    p.crew[i].scars = 2;
    assert_eq!(p.crew[i].effective(), 4);
    assert_eq!(p.eff(Skill::Pilot), p.level(Skill::Pilot) + 3);
    // The captain's own level is the cap.
    h.set_level(&mut p, Skill::Pilot, 9);
    assert_eq!(p.eff(Skill::Pilot), 10);
    // Experience is the captain's own, not the crew's.
    let mut events = Vec::new();
    let before = p.xp;
    p.gain(Skill::Pilot, 5, &mut events);
    assert_eq!(p.xp[Skill::Pilot.index()], before[Skill::Pilot.index()] + 5);
}

#[test]
fn a_pilot_saves_fuel_and_days_an_engineer_saves_credits_and_a_face_narrows_the_spread() {
    let mut h = Harness::new(1, 5);
    let solo = h.captain("dockhand");
    let mut crewed = solo.clone();
    h.sign(&mut crewed, Role::Pilot, 8);
    h.sign(&mut crewed, Role::Engineer, 8);
    h.sign(&mut crewed, Role::Face, 8);
    // Piloting.
    let (a, b) = (solo.stats(&h.content), crewed.stats(&h.content));
    assert!(Ship::fuel_for(&b, 40) < Ship::fuel_for(&a, 40));
    assert!(b.speed_x10 > a.speed_x10, "a pilot makes the ship faster");
    // Engineering: the same worn ship costs less to repair.
    let (mut worn_a, mut worn_b) = (solo.clone(), crewed.clone());
    for p in [&mut worn_a, &mut worn_b] {
        p.ship.modules[0].wear = 600;
        p.ship.hull_hp = 50;
    }
    let st = h.station("industrial");
    h.dock_free(&mut worn_a, st);
    h.dock_free(&mut worn_b, st);
    let (qa, qb) = h.with_env(|env| {
        (
            worn_a.repair_quote(env, RepairTarget::All),
            worn_b.repair_quote(env, RepairTarget::All),
        )
    });
    assert!(qb.0 < qa.0, "repairs: {} against {}", qb.0, qa.0);
    // Trading: the face's skill narrows the spread.
    let station = h.frontier.station(st).unwrap();
    assert!(
        crewed.spread_at(&h.content, station) < solo.spread_at(&h.content, station),
        "spread"
    );
}

#[test]
fn a_quartermaster_hides_the_hold_and_a_scientist_sells_the_survey() {
    let mut h = Harness::new(1, 6);
    let st = h.station("industrial");
    let load = |h: &Harness, p: &mut Player| {
        p.ship.cargo.push(Lot {
            good: h.good("ghostwire"),
            tons: 4,
            paid: 1000,
        });
    };
    let rate = |crewed: bool| {
        let mut h = Harness::new(1, 6);
        let mut caught = 0;
        for seed in 0..300 {
            h.reseed(seed);
            let mut p = h.captain("dockhand");
            load(&h, &mut p);
            if crewed {
                h.sign(&mut p, Role::Quartermaster, 8);
            }
            h.orbit(&mut p, st);
            let ev = h.go(&mut p, &Action::Dock(st)).unwrap();
            caught += u32::from(ev.iter().any(|e| matches!(e, Event::Seized { .. })));
        }
        caught
    };
    let (alone, with_qm) = (rate(false), rate(true));
    assert!(
        with_qm + 40 < alone,
        "caught {with_qm} with, {alone} without"
    );
    // Surveying.
    let system = h.frontier.systems.iter().find(|s| s.richness > 5).unwrap();
    let solo = h.captain("dropout");
    let mut crewed = solo.clone();
    h.sign(&mut crewed, Role::Scientist, 8);
    assert!(crewed.survey_value(&h.content, system) > solo.survey_value(&h.content, system));
}

#[test]
fn wages_come_due_each_week_and_unpaid_wages_sour_the_crew() {
    let mut h = Harness::new(1, 7);
    let mut p = h.captain("dockhand");
    let i = h.sign(&mut p, Role::Gunner, 3);
    p.wages_due = DAY0 + 7;
    // Paid: the purse drops and the pay mood rises.
    let cash = p.cash;
    h.today = DAY0 + 7;
    let ev = h.settle(&mut p);
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::WagesPaid { amount: 60 }))
    );
    let _ = cash;
    assert_eq!(
        p.ledger.spend[russh_bbs_door_long_haul::player::Cause::Crew.index()],
        60
    );
    assert_eq!(p.crew[i].mood[PAY], 2);
    assert_eq!(p.wages_due, DAY0 + 14);
    // Unpaid: a missed week, a bitter hand.
    p.cash = 0;
    p.ledger = russh_bbs_door_long_haul::player::Ledger::default();
    h.today = DAY0 + 14;
    let ev = h.settle(&mut p);
    assert!(ev.iter().any(|e| matches!(e, Event::WagesMissed { .. })));
    assert!(p.crew[i].mood[PAY] < 0);
}

#[test]
fn rations_decide_the_food_mood_and_a_quartermaster_buys_them_cheaper() {
    let mut h = Harness::new(1, 8);
    let mut p = h.captain("dockhand");
    docked(&h, &mut p, "farming");
    assert_eq!(
        h.go(&mut p, &Action::Provision).unwrap_err(),
        Refusal::NothingToDo,
        "no mouths, no rations"
    );
    let i = h.sign(&mut p, Role::Medic, 2);
    h.sign(&mut p, Role::Face, 2);
    let plain = p.provision_quote();
    let mut with_qm = p.clone();
    with_qm.crew.clear();
    h.sign(&mut with_qm, Role::Quartermaster, 8);
    h.sign(&mut with_qm, Role::Face, 2);
    assert!(with_qm.provision_quote() < plain);
    let cash = p.cash;
    h.go(&mut p, &Action::Provision).unwrap();
    assert_eq!(p.cash, cash - plain);
    // Fed for a week: food mood climbs. Unfed: it falls.
    let mut fed = p.clone();
    let mut hungry = p.clone();
    hungry.rations_until = 0;
    h.today += 5;
    h.settle(&mut fed);
    h.settle(&mut hungry);
    assert!(fed.crew[i].mood[FOOD] > 0);
    assert!(hungry.crew[i].mood[FOOD] < 0);
    assert!(
        hungry.crew[i].loyalty < fed.crew[i].loyalty,
        "and so does loyalty"
    );
}

#[test]
fn meeting_a_goal_makes_a_hand_sworn_and_neglect_does_not() {
    let mut h = Harness::new(1, 9);
    let mut p = h.captain("dockhand");
    // A visit goal for a neighbouring system.
    let next = h.frontier.neighbours(p.at)[0].0;
    let v = h.sign(&mut p, Role::Pilot, 3);
    p.crew[v].goal = Goal {
        kind: GoalKind::Visit,
        target: u32::from(next),
        progress: 0,
        text: 0,
    };
    // A docks goal: one new station.
    let d = h.sign(&mut p, Role::Engineer, 3);
    p.crew[d].goal = Goal {
        kind: GoalKind::Docks,
        target: 1,
        progress: 0,
        text: 9,
    };
    // A wealth goal the purse already meets.
    let w = h.sign(&mut p, Role::Face, 3);
    p.crew[w].goal = Goal {
        kind: GoalKind::Wealth,
        target: 1,
        progress: 0,
        text: 9,
    };
    p.crew[w].loyalty = 40;
    let ev = h.go(&mut p, &Action::Jump(next)).unwrap();
    assert!(p.crew[v].sworn, "arriving at the system they wanted");
    assert!(p.crew[w].sworn, "the purse is fat enough");
    assert!(!p.crew[d].sworn);
    assert!(
        ev.iter()
            .filter(|e| matches!(e, Event::GoalMet { .. }))
            .count()
            >= 2,
        "{ev:?}"
    );
    assert!(p.crew[v].loyalty >= 70, "sworn means loyal");
    let station = h.frontier.system(next).unwrap().stations.first().copied();
    if let Some(station) = station {
        p.scene = None;
        h.go(&mut p, &Action::Dock(station)).unwrap();
        assert!(p.crew[d].sworn, "a new dock was the goal");
    }
}

#[test]
fn a_won_fight_counts_toward_a_fighting_goal() {
    let mut h = Harness::new(1, 10);
    let mut p = h.captain("dockhand");
    h.fit(&mut p, "lance1");
    let i = h.sign(&mut p, Role::Gunner, 4);
    p.crew[i].goal = Goal {
        kind: GoalKind::Fights,
        target: 1,
        progress: 0,
        text: 3,
    };
    h.fight(&mut p, "skiff");
    let mut safety = 0;
    while p.scene.is_some() && safety < 60 {
        safety += 1;
        let _ = h.go(&mut p, &Action::Fight(common::orders(4, 2, 0)));
    }
    assert!(p.stats.fights_won + p.stats.fights_lost + p.stats.fled <= 1);
    if p.stats.fights_won == 1 {
        assert!(
            p.crew
                .first()
                .is_none_or(|c| c.sworn || c.goal.progress >= 1)
        );
    }
}

#[test]
fn a_sworn_hand_cannot_fall_below_the_floor_and_never_walks() {
    let mut h = Harness::new(1, 11);
    let mut p = h.captain("dockhand");
    let i = h.sign(&mut p, Role::Medic, 3);
    p.crew[i].sworn = true;
    p.crew[i].loyalty = 90;
    p.crew[i].mood.fill(-10);
    p.rations_until = 0;
    h.today += 25;
    h.settle(&mut p);
    assert!(p.crew[i].loyalty >= 70);
    let st = h.station("industrial");
    h.orbit(&mut p, st);
    for seed in 0..50 {
        h.reseed(seed);
        let mut q = p.clone();
        h.go(&mut q, &Action::Dock(st)).unwrap();
        assert_eq!(q.crew.len(), 1, "a sworn hand stays");
    }
}

#[test]
fn neglect_ends_in_desertion_or_mutiny_at_a_dock() {
    let mut h = Harness::new(1, 12);
    let st = h.station("industrial");
    // One unhappy hand deserts at a dock (some of the time).
    let mut deserted = 0;
    for seed in 0..60 {
        h.reseed(seed);
        let mut p = h.captain("dockhand");
        let i = h.sign(&mut p, Role::Gunner, 3);
        p.crew[i].loyalty = 5;
        p.crew[i].mood = [-5, -5, -3, -3];
        h.orbit(&mut p, st);
        let ev = h.go(&mut p, &Action::Dock(st)).unwrap();
        if ev.iter().any(|e| matches!(e, Event::Deserted { .. })) {
            deserted += 1;
            assert!(p.crew.is_empty());
            assert_eq!(p.stats.deserted, 1);
        }
    }
    assert!((15..=45).contains(&deserted), "{deserted} of 60 deserted");
    // Two together may mutiny, and the strongbox pays for it.
    let mut mutinies = 0;
    for seed in 0..60 {
        h.reseed(seed);
        let mut p = h.captain("dockhand");
        for role in [Role::Gunner, Role::Pilot, Role::Face] {
            let i = h.sign(&mut p, role, 3);
            p.crew[i].loyalty = 5;
            p.crew[i].mood = [-5, -5, -3, -3];
        }
        let happy = h.sign(&mut p, Role::Medic, 3);
        p.crew[happy].sworn = true;
        p.crew[happy].loyalty = 90;
        p.cash = 1000;
        p.ledger = russh_bbs_door_long_haul::player::Ledger::default();
        p.ledger.income[0] = 1000;
        h.orbit(&mut p, st);
        let ev = h.go(&mut p, &Action::Dock(st)).unwrap();
        if let Some(Event::Mutiny { left, stolen }) =
            ev.iter().find(|e| matches!(e, Event::Mutiny { .. }))
        {
            mutinies += 1;
            assert_eq!(*left, 3);
            assert!(*stolen > 0 && *stolen <= 200 + 20);
            assert_eq!(p.crew.len(), 1, "the sworn hand did not go");
            assert_eq!(p.stats.mutinies, 1);
        }
    }
    assert!(mutinies > 10, "{mutinies} mutinies in 60");
}

#[test]
fn injury_heals_with_time_faster_with_a_medic_and_a_scar_costs_a_point() {
    let mut h = Harness::new(1, 13);
    let mut alone = h.captain("dockhand");
    let i = h.sign(&mut alone, Role::Gunner, 4);
    alone.crew[i].hurt_days = 6;
    let mut tended = alone.clone();
    h.sign(&mut tended, Role::Medic, 3);
    h.today += 3;
    h.settle(&mut alone);
    h.settle(&mut tended);
    assert_eq!(alone.crew[i].hurt_days, 3);
    assert_eq!(tended.crew[i].hurt_days, 0, "twice as fast with a medic");
    // A bad hurt without a medic can scar.
    let mut scarred = 0;
    for seed in 0..200 {
        h.reseed(seed);
        let mut p = h.captain("dockhand");
        h.sign(&mut p, Role::Pilot, 5);
        let mut ev = Vec::new();
        h.with_env(|env| p.injure(0, env.rng, 3, 4, &mut ev));
        scarred += u32::from(p.crew[0].scars);
        assert!(p.crew[0].hurt_days >= 3);
        assert_eq!(p.crew[0].effective(), 5 - p.crew[0].scars);
    }
    assert!((10..=60).contains(&scarred), "scarred {scarred} of 200");
    // With a medic aboard, never.
    for seed in 0..100 {
        h.reseed(seed);
        let mut p = h.captain("dockhand");
        h.sign(&mut p, Role::Pilot, 5);
        h.sign(&mut p, Role::Medic, 2);
        let mut ev = Vec::new();
        h.with_env(|env| p.injure(0, env.rng, 3, 4, &mut ev));
        assert_eq!(p.crew[0].scars, 0);
    }
}

#[test]
fn a_hand_trains_by_use_and_dismissing_a_sworn_hand_sours_the_rest() {
    let mut h = Harness::new(1, 14);
    let mut p = h.captain("dockhand");
    let a = h.sign(&mut p, Role::Pilot, 1);
    let b = h.sign(&mut p, Role::Engineer, 2);
    let mut ups = 0;
    for _ in 0..40 {
        ups += u32::from(p.crew[a].train());
    }
    assert!(
        p.crew[a].skill >= 3 && ups >= 2,
        "skill {}",
        p.crew[a].skill
    );
    p.crew[b].sworn = true;
    p.crew[b].loyalty = 80;
    let st = docked(&h, &mut p, "mining");
    let _ = st;
    h.go(&mut p, &Action::Dismiss(0)).unwrap();
    assert_eq!(p.crew.len(), 1);
    // Casting off the unsworn pilot cost the sworn engineer nothing; casting off the sworn
    // engineer would. Put another aboard to see it.
    let c = h.sign(&mut p, Role::Medic, 2);
    h.go(&mut p, &Action::Dismiss(0)).unwrap();
    assert!(p.crew[0].mood[3] < 0, "treatment fell");
    let _ = c;
}

#[test]
fn a_lost_hand_stays_lost_and_the_ship_remembers() {
    let mut h = Harness::new(1, 15);
    let mut p = h.captain("dockhand");
    h.sign(&mut p, Role::Medic, 3);
    h.sign(&mut p, Role::Face, 3);
    let mut ev = Vec::new();
    p.lose_crew(0, &mut ev);
    assert!(matches!(
        ev[0],
        Event::CrewLost {
            role: Role::Medic,
            ..
        }
    ));
    assert_eq!(p.stats.crew_lost, 1);
    assert!(!p.has_role(Role::Medic, 1));
    assert!(p.crew[0].mood[2] < 0, "danger mood fell for the rest");
    // Saves round-trip crew, mood and goals.
    let bytes = p.encode();
    let back = Player::decode(&bytes, &h.content, &h.frontier).unwrap();
    assert_eq!(back, p);
    let _ = member(Role::Pilot, 1);
}
