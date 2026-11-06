//! Economy and engine invariants, checked on random play.
//!
//! A fuzzer picks actions (many of them nonsense) for captains of every background across a
//! few seasons, and after every step checks what must always hold: no credits from nothing,
//! prices inside their band, stock inside its limits, nothing past capacity, and that a refused
//! action changed nothing at all (the captain, the world and the RNG).
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::assert_is_empty,
    clippy::similar_names
)]

mod common;

use common::Harness;
use proptest::prelude::*;
use russh_bbs_door::DoorRng;
use russh_bbs_door_long_haul::boarding::Drill;
use russh_bbs_door_long_haul::combat::{Maneuver, Orders};
use russh_bbs_door_long_haul::content::GoodClass;
use russh_bbs_door_long_haul::engine::{Action, RepairTarget};
use russh_bbs_door_long_haul::events::Event;
use russh_bbs_door_long_haul::market::{FULL_STOCK_DEPTHS, MULT_MAX, MULT_MIN, depth_at, step_mid};
use russh_bbs_door_long_haul::player::{Cause, Player};
use russh_bbs_door_long_haul::scene::Scene;
use russh_bbs_door_long_haul::ship::WEAR_MAX;

/// Random play inside an open scene: mostly scene actions, now and then a stray one.
fn pick_scene_action(p: &Player, pick: &mut DoorRng) -> Option<Action> {
    let scene = p.scene.as_ref()?;
    if pick.below(12) == 0 {
        return Some(Action::Undock);
    }
    Some(match scene {
        Scene::Event(_) => Action::Choose(u8::try_from(pick.below(6)).unwrap()),
        Scene::Combat(_) => match pick.below(10) {
            0 | 1 => Action::Run,
            2 => Action::Wreck {
                board: pick.below(2) == 0,
            },
            _ => Action::Fight(Orders {
                weapons: u8::try_from(pick.below(7)).unwrap(),
                shields: u8::try_from(pick.below(5)).unwrap(),
                engines: u8::try_from(pick.below(4)).unwrap(),
                maneuver: match pick.below(3) {
                    0 => Maneuver::Hold,
                    1 => Maneuver::Close,
                    _ => Maneuver::Open,
                },
            }),
        },
        Scene::Boarding(_) => Action::Skirmish(match pick.below(8) {
            0 => Drill::Move(u8::try_from(pick.below(5)).unwrap()),
            1 => Drill::Breach(u8::try_from(pick.below(5)).unwrap()),
            2 | 3 => Drill::Fight,
            4 => Drill::Search,
            5 => Drill::Patch,
            6 => Drill::Take,
            _ => {
                if pick.below(4) == 0 {
                    Drill::Withdraw
                } else {
                    Drill::Move(u8::try_from(pick.below(5)).unwrap())
                }
            }
        }),
    })
}

fn pick_action(h: &Harness, p: &Player, pick: &mut DoorRng) -> Action {
    if let Some(action) = pick_scene_action(p, pick) {
        return action;
    }
    let n_slots = h
        .frontier
        .station(p.docked.unwrap_or(h.frontier.home_station))
        .map_or(1, |s| u32::try_from(s.goods.len()).unwrap());
    let here = h.frontier.system(p.at).unwrap();
    let neighbours: Vec<u16> = h
        .frontier
        .neighbours(p.at)
        .iter()
        .map(|&(id, _)| id)
        .collect();
    let slot = u8::try_from(pick.below(n_slots)).unwrap();
    let tons = u16::try_from(pick.range(1, 45)).unwrap();
    match pick.below(48) {
        0 => Action::Dock(here.stations.first().copied().unwrap_or(0)),
        1 => Action::Dock(
            u16::try_from(pick.below(u32::try_from(h.frontier.stations.len()).unwrap())).unwrap(),
        ),
        2 => Action::Undock,
        3..=7 => Action::Jump(
            neighbours
                [usize::try_from(pick.below(u32::try_from(neighbours.len()).unwrap())).unwrap()],
        ),
        8..=14 => Action::Buy { slot, tons },
        15..=21 => Action::Sell { slot, tons },
        22 | 23 => Action::Refuel(u16::try_from(pick.range(1, 200)).unwrap()),
        24 => Action::Skim,
        25 => Action::DeepScan,
        26 => Action::SellSurvey,
        27 => Action::Repair(RepairTarget::All),
        28 => Action::Repair(RepairTarget::Hull),
        29 => Action::Repair(RepairTarget::Module(
            usize::try_from(pick.below(6)).unwrap(),
        )),
        30 => Action::BuyModule(
            u8::try_from(pick.below(u32::try_from(h.content.modules.len()).unwrap())).unwrap(),
        ),
        31 => Action::SellModule(usize::try_from(pick.below(6)).unwrap()),
        32 => Action::BuyHull(u8::try_from(pick.below(4)).unwrap()),
        33 => Action::Borrow(i64::from(pick.range(1, 8000))),
        34 | 35 => Action::Repay(i64::from(pick.range(1, 4000))),
        36 => Action::CallTow,
        37 | 38 => Action::Hire(u8::try_from(pick.below(3)).unwrap()),
        39 => Action::Dismiss(u8::try_from(pick.below(4)).unwrap()),
        40 => Action::Provision,
        41..=44 => {
            let offers = p.docked.map_or_else(Vec::new, |st| {
                russh_bbs_door_long_haul::contracts::board(
                    &h.content,
                    &h.frontier,
                    st,
                    p.last_day,
                    &p.standing,
                )
            });
            let key = offers
                .get(usize::try_from(pick.below(8)).unwrap())
                .map_or_else(|| pick.next_u64(), |o| o.key);
            Action::Accept(key)
        }
        45 => Action::Abandon(u8::try_from(pick.below(4)).unwrap()),
        _ => Action::Sell { slot, tons: 999 },
    }
}

/// Everything that must hold of a captain, whatever happened.
fn check_captain(h: &Harness, p: &Player) {
    let stats = p.stats(&h.content);
    assert!(p.cash >= 0, "the purse is never negative: {}", p.cash);
    assert_eq!(
        p.ledger.net(),
        p.cash,
        "every credit in the purse is on the ledger"
    );
    assert!(p.debt.principal >= 0);
    assert!(
        p.ship.fuel <= stats.fuel_cap,
        "fuel {} over cap {}",
        p.ship.fuel,
        stats.fuel_cap
    );
    assert!(
        p.hold_used() <= u32::from(stats.cargo_cap),
        "hold over capacity"
    );
    assert!(p.crew.len() <= 8 && p.contracts.len() <= 4);
    assert!(
        p.berths_used() <= p.berths(&h.content),
        "{} aboard in {} berths",
        p.berths_used(),
        p.berths(&h.content)
    );
    for c in &p.crew {
        assert!(c.loyalty <= 100 && (1..=10).contains(&c.skill));
        assert!(c.mood.iter().all(|m| (-10..=10).contains(m)));
        assert!(!c.sworn || c.loyalty >= 70, "a sworn hand stays loyal");
    }
    assert!(p.ship.modules.iter().all(|m| m.wear <= WEAR_MAX));
    let max_hp = h.content.hulls[usize::from(p.ship.hull)].hull_points;
    assert!(
        (1..=max_hp).contains(&p.ship.hull_hp),
        "hull {} of {max_hp}",
        p.ship.hull_hp
    );
    assert!(
        // A long jump may overdraw the day; settling clamps what is carried to nine days.
        (-60..=10).contains(&p.days_left),
        "days {}",
        p.days_left
    );
    for slot in russh_bbs_door_long_haul::content::SlotKind::ALL {
        let used = p.ship.used_slots(&h.content, slot);
        let have = usize::from(h.content.hulls[usize::from(p.ship.hull)].slots.of(slot));
        assert!(used <= have, "{} slots overfull", slot.name());
    }
    assert!(
        p.ship
            .of_kind(
                &h.content,
                russh_bbs_door_long_haul::content::SlotKind::Drive
            )
            .count()
            >= 1
    );
    // Money only comes from these places; the rest are costs.
    for cause in [
        Cause::Fuel,
        Cause::Repair,
        Cause::Fine,
        Cause::Tow,
        Cause::Dock,
        Cause::Payment,
        Cause::Crew,
    ] {
        assert_eq!(
            p.ledger.income[cause.index()],
            0,
            "{cause:?} can only cost credits"
        );
    }
    assert!(p.standing.iter().all(|s| (-100..=100).contains(s)));
    assert!(p.survey.len() <= 24 && p.ship.cargo.len() <= 32);
    // Nothing survives a round trip changed.
    let bytes = p.encode();
    assert!(bytes.len() < 64 * 1024, "save of {} bytes", bytes.len());
    assert_eq!(&Player::decode(&bytes, &h.content, &h.frontier).unwrap(), p);
}

fn check_world(h: &Harness) {
    for st in &h.frontier.stations {
        for (slot, ms) in st.goods.iter().enumerate() {
            let depth = depth_at(&h.content.goods, &h.frontier, st.id, slot);
            let stock = h.world.stock_of(st.id, slot);
            assert!(
                (0..=depth * FULL_STOCK_DEPTHS).contains(&stock),
                "stock {stock} of depth {depth}"
            );
            let base = h.content.goods[usize::from(ms.good)].base;
            let mid = step_mid(base, depth, stock);
            let (lo, hi) = (
                i64::from(base) * MULT_MIN / 1000,
                i64::from(base) * MULT_MAX / 1000,
            );
            assert!(
                mid >= lo.max(1) && mid <= hi.max(1),
                "price {mid} outside [{lo}, {hi}]"
            );
        }
    }
    assert!(h.world.events.len() <= 24);
}

/// What an action is allowed to add to net worth: profit on cargo sold, new survey data, and
/// credits that come from contracts, salvage and events (the ledger says which).
fn allowed_gain(events: &[Event], survey_bonus: i64, new_data: i64, earned: i64) -> i64 {
    events
        .iter()
        .map(|e| match e {
            Event::Sold { profit, .. } => (*profit).max(0),
            _ => 0,
        })
        .sum::<i64>()
        + survey_bonus
        + new_data
        + earned
}

fn earned_total(p: &Player) -> i64 {
    [Cause::Contract, Cause::Salvage, Cause::Event]
        .iter()
        .map(|c| p.ledger.income[c.index()])
        .sum()
}

fn play(season: u32, seed: u64, steps: usize) -> std::collections::BTreeSet<String> {
    let mut kinds = std::collections::BTreeSet::new();
    let mut h = Harness::new(season, seed);
    let mut pick = DoorRng::from_seed(seed ^ 0xABCD);
    let backgrounds: Vec<String> = h.content.backgrounds.iter().map(|b| b.id.clone()).collect();
    let mut p = h.captain(&backgrounds[usize::try_from(pick.below(4)).unwrap()]);
    for step in 0..steps {
        if step % 14 == 13 {
            h.next_day();
            check_world(&h);
        }
        let action = pick_action(&h, &p, &mut pick);
        let (before, world_before, rng_before) = (p.clone(), h.world.clone(), h.rng.state());
        let nw_before = p.net_worth(&h.content);
        let survey_value: i64 = p.survey.iter().map(|s| i64::from(s.value)).sum();
        // Settle on its own first, so the refusal check below compares like with like.
        let settle_events = h.settle(&mut p);
        let (settled, world_settled, rng_settled) = (p.clone(), h.world.clone(), h.rng.state());
        let nw_settled = p.net_worth(&h.content);
        // A repossession swaps the ship for a hand-me-down and takes part of its value off the
        // loan, which can leave a captain with a worthless hull a little better off.
        let repossessed = settle_events
            .iter()
            .any(|e| matches!(e, Event::Repossessed { .. }));
        assert!(
            nw_settled <= nw_before || repossessed,
            "interest and payments cannot raise net worth"
        );
        let earned_before = earned_total(&p);
        let result = h.go_strict(&mut p, &action);
        match &result {
            Err(refusal) => {
                assert_eq!(
                    p, settled,
                    "{action:?} refused ({refusal:?}) but changed the captain"
                );
                assert_eq!(
                    h.world, world_settled,
                    "{action:?} refused but changed the world"
                );
                assert_eq!(
                    h.rng.state(),
                    rng_settled,
                    "{action:?} refused but used the RNG"
                );
            }
            Ok(events) => {
                for e in events {
                    kinds.insert(
                        format!("{e:?}")
                            .split(|c: char| !c.is_alphanumeric())
                            .next()
                            .unwrap_or("")
                            .to_owned(),
                    );
                }
                let sold_survey: i64 = events
                    .iter()
                    .map(|e| match e {
                        Event::SurveySold { total, .. } => total - survey_value,
                        _ => 0,
                    })
                    .sum();
                let lots_after: i64 = p.survey.iter().map(|s| i64::from(s.value)).sum();
                let kept = if events.iter().any(|e| matches!(e, Event::SurveySold { .. })) {
                    0
                } else {
                    survey_value
                };
                let gain = allowed_gain(
                    events,
                    sold_survey.max(0),
                    (lots_after - kept).max(0),
                    earned_total(&p) - earned_before,
                );
                let nw_after = p.net_worth(&h.content);
                assert!(
                    nw_after <= nw_settled + gain,
                    "{action:?} raised net worth by {} with only {gain} allowed",
                    nw_after - nw_settled
                );
            }
        }
        check_captain(&h, &p);
        let _ = (&before, &world_before, &rng_before);
    }
    check_world(&h);
    kinds
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn random_play_never_breaks_an_invariant(seed in any::<u64>(), season in 1u32..6) {
        play(season, seed, 500);
    }
}

#[test]
fn the_fuzzer_reaches_most_kinds_of_event() {
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..40 {
        seen.extend(play(1 + u32::try_from(seed % 4).unwrap(), seed, 600));
    }
    for want in [
        "Docked",
        "Jumped",
        "Bought",
        "Sold",
        "Refueled",
        "Repaired",
        "Charted",
        "ModuleBought",
        "ModuleSold",
        "HullBought",
        "Borrowed",
        "Repaid",
        "Towed",
        "Skimmed",
        "Scanned",
        "SkillUp",
        "Transit",
        "Outcome",
        "Hired",
        "Accepted",
    ] {
        assert!(
            seen.contains(want),
            "the fuzzer never produced {want}; saw {seen:?}"
        );
    }
}

#[test]
fn a_long_fixed_play_is_reproducible() {
    // The same seed plays the same game: the final state is byte-for-byte equal.
    let run = |seed| {
        let mut h = Harness::new(2, seed);
        let mut pick = DoorRng::from_seed(9);
        let mut p = h.captain("dockhand");
        for step in 0..400 {
            if step % 14 == 13 {
                h.next_day();
            }
            let a = pick_action(&h, &p, &mut pick);
            let _ = h.go(&mut p, &a);
        }
        (p.encode(), h.world.encode())
    };
    assert_eq!(run(77), run(77));
    assert_ne!(run(77).0, run(78).0);
}

#[test]
fn a_round_trip_at_one_station_loses_the_spread_at_every_trade_level() {
    use russh_bbs_door_long_haul::skills::Skill;
    let mut h = Harness::new(1, 5);
    let st = h.frontier.home_station;
    for level in [0u8, 5, 10] {
        let mut p = h.captain("dockhand");
        h.set_level(&mut p, Skill::Trade, level);
        h.dock_free(&mut p, st);
        let start = p.cash;
        for slot in 0..h.frontier.station(st).unwrap().goods.len() {
            let slot = u8::try_from(slot).unwrap();
            let good = h.frontier.station(st).unwrap().goods[usize::from(slot)].good;
            if h.content.goods[usize::from(good)].class == GoodClass::Contraband {
                continue;
            }
            if h.go(&mut p, &Action::Buy { slot, tons: 25 }).is_ok() {
                h.go(&mut p, &Action::Sell { slot, tons: 25 }).unwrap();
            }
        }
        assert!(p.cash < start, "level {level}: {} vs {start}", p.cash);
        assert!(p.ship.cargo.is_empty());
    }
}

#[test]
fn fifty_nights_of_ticks_keep_every_price_inside_its_band() {
    let mut h = Harness::new(3, 1);
    for _ in 0..50 {
        h.next_day();
        check_world(&h);
    }
    assert!(h.world.events.iter().all(|e| e.active(h.today)));
}

#[test]
fn a_player_trade_moves_the_shared_price_and_the_next_night_pulls_it_back() {
    let mut h = Harness::new(1, 2);
    let st = h.frontier.home_station;
    let mut p = h.captain("dockhand");
    h.dock_free(&mut p, st);
    let slot = (0..h.frontier.station(st).unwrap().goods.len())
        .find(|&s| {
            h.content.goods[usize::from(h.frontier.station(st).unwrap().goods[s].good)].class
                == GoodClass::Legal
        })
        .map(|s| u8::try_from(s).unwrap())
        .unwrap();
    let slot_ix = usize::from(slot);
    let before = h.world.stock_of(st, slot_ix);
    h.go(&mut p, &Action::Buy { slot, tons: 40 }).unwrap();
    let after = h.world.stock_of(st, slot_ix);
    assert!(after < before, "buying draws the stock down");
    let depth = depth_at(&h.content.goods, &h.frontier, st, slot_ix);
    let good = h.frontier.station(st).unwrap().goods[slot_ix].good;
    let base = h.content.goods[usize::from(good)].base;
    assert!(
        step_mid(base, depth, after) >= step_mid(base, depth, before),
        "and the price rises"
    );
    h.next_day();
    let healed = h.world.stock_of(st, slot_ix);
    assert!(
        healed > after,
        "the market recovers toward its target overnight: {after} -> {healed}"
    );
}
