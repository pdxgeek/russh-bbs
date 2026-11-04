//! Branch tests for the loan, the ship and the shared world: what a debt, a worn drive, a
//! module, a hull or a first chart changes.
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
use russh_bbs_door_long_haul::content::{Faction, SlotKind};
use russh_bbs_door_long_haul::engine::{Action, BASE_CREDIT_LIMIT, LIMIT_LOSS_PER_REPO};
use russh_bbs_door_long_haul::events::{Event, Refusal};
use russh_bbs_door_long_haul::frontier::Hazard;
use russh_bbs_door_long_haul::player::{Cause, Player};
use russh_bbs_door_long_haul::ship::{Installed, Lot, Ship, WEAR_MAX};
use russh_bbs_door_long_haul::skills::Skill;
use russh_bbs_door_long_haul::world::{Delta, World};

fn lane_to(h: &Harness, hazard: Hazard) -> (u16, u16) {
    h.frontier
        .systems
        .iter()
        .find(|s| s.hazard == hazard && !s.uncharted)
        .map(|s| (h.frontier.neighbours(s.id)[0].0, s.id))
        .unwrap()
}

#[test]
fn interest_accrues_every_day_and_the_weekly_payment_is_taken() {
    let mut h = Harness::new(1, 1);
    let mut p = h.captain("dockhand");
    let start = p.debt.principal;
    for _ in 0..3 {
        h.next_day();
    }
    let events = h.settle(&mut p);
    assert!(events.iter().any(|e| matches!(e, Event::Interest { .. })));
    assert!(
        p.debt.principal > start && p.debt.principal < start * 102 / 100,
        "three days at 0.4%"
    );
    assert_eq!(p.days_left, 10);
    // A week in: the lender takes its payment from the purse.
    for _ in 0..4 {
        h.next_day();
    }
    let cash = p.cash;
    let events = h.settle(&mut p);
    let paid = events
        .iter()
        .find_map(|e| {
            if let Event::DuePaid { amount } = e {
                Some(*amount)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(p.cash, cash - paid);
    assert_eq!(p.debt.missed, 0);
    assert_eq!(p.debt.next_due, 20_014);
}

#[test]
fn three_missed_payments_repossess_the_ship_and_the_scar_stays() {
    let mut h = Harness::new(1, 1);
    let mut p = h.captain("dockhand");
    p.debit(Cause::Payment, p.cash);
    let before = p.debt.principal;
    p.ship.cargo.push(Lot {
        good: 0,
        tons: 5,
        paid: 100,
    });
    let mut repossessed = None;
    for week in 1..=3 {
        for _ in 0..7 {
            h.next_day();
        }
        let events = h.settle(&mut p);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::DueMissed { missed, .. } if *missed == week)),
            "week {week}: {events:?}"
        );
        if events
            .iter()
            .any(|e| matches!(e, Event::Repossessed { .. }))
        {
            repossessed = Some(week);
        }
    }
    assert_eq!(repossessed, Some(3));
    assert_eq!(p.repossessions, 1);
    assert_eq!(
        h.content.hulls[usize::from(p.ship.hull)].id,
        "courier",
        "back in a hand-me-down courier"
    );
    assert!(p.ship.cargo.is_empty());
    assert_eq!(p.docked, Some(h.frontier.home_station));
    assert!(
        p.debt.principal > before,
        "interest, penalties and the repossession fee"
    );
    assert_eq!(p.credit_limit(), BASE_CREDIT_LIMIT - LIMIT_LOSS_PER_REPO);
    assert_eq!(p.debt.missed, 0, "the count starts again");
    // The lender is warier now: the same loan that was fine before is refused.
    let room = p.credit_limit() - p.debt.principal;
    let r = h.go(&mut p, &Action::Borrow(room + 1));
    assert_eq!(
        r.unwrap_err(),
        Refusal::OverLimit {
            limit: p.credit_limit()
        }
    );
    // And it all survives a save.
    let again = Player::decode(&p.encode(), &h.content, &h.frontier).unwrap();
    assert_eq!(again.repossessions, 1);
}

#[test]
fn paying_on_time_keeps_the_ship_and_a_partial_payment_still_counts_as_a_miss() {
    let mut h = Harness::new(1, 1);
    let mut p = h.captain("dockhand");
    p.debit(Cause::Payment, p.cash - 300); // less than the 1,000 due
    for _ in 0..7 {
        h.next_day();
    }
    let events = h.settle(&mut p);
    let miss = events
        .iter()
        .find_map(|e| {
            if let Event::DueMissed { short, penalty, .. } = e {
                Some((*short, *penalty))
            } else {
                None
            }
        })
        .unwrap();
    assert!(miss.0 > 600 && miss.1 > 0, "short {miss:?}");
    assert_eq!(p.cash, 0, "the lender takes whatever there is");
    // Pay up before the next week and the count resets.
    p.credit(Cause::Loan, 5_000);
    for _ in 0..7 {
        h.next_day();
    }
    let events = h.settle(&mut p);
    assert!(events.iter().any(|e| matches!(e, Event::DuePaid { .. })));
    assert_eq!(p.debt.missed, 0);
}

#[test]
fn an_absence_costs_interest_for_at_most_four_weeks_and_two_payments() {
    let mut h = Harness::new(1, 1);
    let mut p = h.captain("dockhand");
    p.credit(Cause::Loan, 5_000);
    for _ in 0..60 {
        h.next_day();
    }
    let events = h.settle(&mut p);
    let dues = events
        .iter()
        .filter(|e| matches!(e, Event::DuePaid { .. } | Event::DueMissed { .. }))
        .count();
    assert_eq!(
        dues, 2,
        "only two weeks of payments are chased after an absence"
    );
    assert!(p.debt.next_due > h.today);
    assert_eq!(p.repossessions, 0);
}

#[test]
fn borrowing_and_repaying_move_cash_and_debt_together_and_a_long_jump_borrows_tomorrows_days() {
    let mut h = Harness::new(1, 1);
    let mut p = h.captain("dockhand");
    h.dock_free(&mut p, h.frontier.home_station);
    let (cash, debt) = (p.cash, p.debt.principal);
    h.go(&mut p, &Action::Borrow(2_000)).unwrap();
    assert_eq!((p.cash, p.debt.principal), (cash + 2_000, debt + 2_000));
    h.go(&mut p, &Action::Repay(500)).unwrap();
    assert_eq!((p.cash, p.debt.principal), (cash + 1_500, debt + 1_500));
    assert_eq!(
        h.go(&mut p, &Action::Repay(0)).unwrap_err(),
        Refusal::NothingToDo
    );
    // Overdrawing the allowance comes off tomorrow.
    p.days_left = 1;
    p.docked = None;
    let next = h.frontier.neighbours(p.at)[0].0;
    p.ship.fuel = 120;
    h.go(&mut p, &Action::Jump(next)).unwrap();
    assert!(p.days_left < 0, "{}", p.days_left);
    let debt_days = p.days_left;
    h.next_day();
    h.settle(&mut p);
    assert_eq!(p.days_left, 10 + debt_days);
    // With nothing left the drive refuses until tomorrow.
    p.days_left = 0;
    let next = h.frontier.neighbours(p.at)[0].0;
    assert_eq!(
        h.go(&mut p, &Action::Jump(next)).unwrap_err(),
        Refusal::NoDays
    );
}

#[test]
fn a_worn_drive_is_slower_and_can_fault_a_new_one_cannot() {
    let mut h = Harness::new(1, 1);
    let (from, to) = (0u16, h.frontier.neighbours(0)[0].0);
    let fresh = h.captain("dockhand");
    let mut worn = fresh.clone();
    worn.ship.modules[0].wear = 900;
    assert!(worn.stats(&h.content).speed_x10 < fresh.stats(&h.content).speed_x10);
    let faults = |h: &mut Harness, base: &Player| {
        let mut n = 0;
        for seed in 0..300 {
            h.reseed(seed);
            let mut p = base.clone();
            p.at = from;
            p.docked = None;
            let events = h.go(&mut p, &Action::Jump(to)).unwrap();
            if events.iter().any(|e| matches!(e, Event::DriveFault { .. })) {
                n += 1;
            }
        }
        n
    };
    assert_eq!(faults(&mut h, &fresh), 0);
    let n = faults(&mut h, &worn);
    assert!(
        n > 10,
        "a drive at 90% wear faults about 7% of jumps; saw {n}/300"
    );
    // A fault costs days and fuel.
    h.reseed(0);
    let seed = (0..300)
        .find(|&s| {
            h.reseed(s);
            let mut p = worn.clone();
            p.at = from;
            p.docked = None;
            h.go(&mut p, &Action::Jump(to))
                .unwrap()
                .iter()
                .any(|e| matches!(e, Event::DriveFault { .. }))
        })
        .unwrap();
    let (mut with, mut without) = (worn.clone(), worn.clone());
    for p in [&mut with, &mut without] {
        p.at = from;
        p.docked = None;
    }
    h.reseed(seed);
    h.go(&mut with, &Action::Jump(to)).unwrap();
    without.ship.modules[0].wear = 0; // no fault possible
    h.reseed(seed);
    h.go(&mut without, &Action::Jump(to)).unwrap();
    assert!(with.days_left < without.days_left || with.ship.fuel < without.ship.fuel);
}

#[test]
fn scanners_and_shields_decide_what_a_hazard_costs() {
    let mut h = Harness::new(1, 1);
    // Debris: a grade-2 scanner reads it; a shield soaks part of it.
    let (from, to) = lane_to(&h, Hazard::Debris);
    let hull_lost = |h: &mut Harness, p: &Player| {
        let mut total = 0u32;
        for seed in 0..40 {
            h.reseed(seed);
            let mut q = p.clone();
            q.at = from;
            q.docked = None;
            q.ship.fuel = 120;
            for e in h.go(&mut q, &Action::Jump(to)).unwrap() {
                if let Event::Hazard { hull_lost, .. } = e {
                    total += u32::from(hull_lost);
                }
            }
        }
        total
    };
    let bare = h.captain("dockhand");
    let mut shielded = bare.clone();
    shielded.ship.modules.push(Installed {
        module: h.module("shield2"),
        wear: 0,
    });
    let mut scanned = bare.clone();
    scanned.ship.modules.push(Installed {
        module: h.module("scan2"),
        wear: 0,
    });
    let (b, s, c) = (
        hull_lost(&mut h, &bare),
        hull_lost(&mut h, &shielded),
        hull_lost(&mut h, &scanned),
    );
    assert!(
        b > 0 && s < b && c == 0,
        "bare {b}, shielded {s}, scanned {c}"
    );
    // Radiation: shields cut the wear it puts on every module.
    let (from, to) = lane_to(&h, Hazard::Radiation);
    let wear_after = |h: &mut Harness, p: &Player| {
        let mut q = p.clone();
        q.at = from;
        q.docked = None;
        q.ship.fuel = 120;
        h.go(&mut q, &Action::Jump(to)).unwrap();
        q.ship
            .modules
            .iter()
            .map(|m| u32::from(m.wear))
            .sum::<u32>()
    };
    assert!(wear_after(&mut h, &shielded) < wear_after(&mut h, &bare));
}

#[test]
fn skimming_pays_in_fuel_and_a_refinery_pays_more_and_a_scanner_makes_it_safer() {
    let mut h = Harness::new(1, 1);
    let giant = h.frontier.systems.iter().find(|s| s.gas_giant).unwrap().id;
    let run = |h: &mut Harness, p: &Player| {
        let (mut fuel, mut hull) = (0u32, 0u32);
        for seed in 0..60 {
            h.reseed(seed);
            let mut q = p.clone();
            q.at = giant;
            q.docked = None;
            q.ship.fuel = 10;
            for e in h.go(&mut q, &Action::Skim).unwrap() {
                if let Event::Skimmed { units, hull_lost } = e {
                    fuel += u32::from(units);
                    hull += u32::from(hull_lost);
                }
            }
            assert_eq!(q.days_left, 9, "a skim costs a drive-day");
        }
        (fuel, hull)
    };
    let plain = h.captain("dockhand");
    let mut refined = plain.clone();
    refined.ship.modules.push(Installed {
        module: h.module("refine1"),
        wear: 0,
    });
    let mut ace = plain.clone();
    h.set_level(&mut ace, Skill::Pilot, 10);
    ace.ship.modules.push(Installed {
        module: h.module("scan3"),
        wear: 0,
    });
    let (p_fuel, p_hull) = run(&mut h, &plain);
    let (r_fuel, _) = run(&mut h, &refined);
    let (_, a_hull) = run(&mut h, &ace);
    assert!(p_fuel > 0 && r_fuel > p_fuel, "{r_fuel} vs {p_fuel}");
    assert!(
        a_hull < p_hull,
        "a good pilot with a good scanner is hurt less: {a_hull} vs {p_hull}"
    );
    // No giant, no skim; a full tank, no skim.
    let mut q = h.captain("dockhand");
    q.at = h.frontier.systems.iter().find(|s| !s.gas_giant).unwrap().id;
    q.docked = None;
    assert_eq!(
        h.go(&mut q, &Action::Skim).unwrap_err(),
        Refusal::NoGasGiant
    );
    q.at = giant;
    assert_eq!(h.go(&mut q, &Action::Skim).unwrap_err(), Refusal::TankFull);
}

#[test]
fn modules_change_the_ship_and_slots_are_a_hard_limit() {
    let mut h = Harness::new(1, 1);
    let yard = h.frontier.home_station;
    let mut p = h.captain("dockhand");
    p.credit(Cause::Loan, 60_000);
    h.dock_free(&mut p, yard);
    let before = p.stats(&h.content);
    h.go(&mut p, &Action::BuyModule(h.module("pod2"))).unwrap();
    h.go(&mut p, &Action::BuyModule(h.module("tank2"))).unwrap();
    let after = p.stats(&h.content);
    assert_eq!(after.cargo_cap, before.cargo_cap + 18);
    assert_eq!(after.fuel_cap, before.fuel_cap + 60);
    // One drive slot, and it is full.
    let r = h.go(&mut p, &Action::BuyModule(h.module("ion3")));
    assert_eq!(r.unwrap_err(), Refusal::SlotsFull(SlotKind::Drive));
    // A faster drive means a swap: sell the old one first.
    let drive_ix = p
        .ship
        .modules
        .iter()
        .position(|m| m.module == h.module("ion1"))
        .unwrap();
    h.go(&mut p, &Action::SellModule(drive_ix)).unwrap_err(); // the only drive cannot go
    // Only at a shipyard.
    h.dock_free(&mut p, h.station("mining"));
    let mut q = p.clone();
    q.docked = Some(h.station("mining"));
    q.at = h.frontier.station(h.station("mining")).unwrap().system;
    if !h.frontier.station(h.station("mining")).unwrap().shipyard {
        assert_eq!(
            h.go(&mut q, &Action::BuyModule(h.module("pod1")))
                .unwrap_err(),
            Refusal::NoShipyard
        );
    }
    // A worn module fetches less than a new one.
    let new = Ship::module_resale(
        &Installed {
            module: h.module("scan2"),
            wear: 0,
        },
        &h.content,
    );
    let worn = Ship::module_resale(
        &Installed {
            module: h.module("scan2"),
            wear: 800,
        },
        &h.content,
    );
    assert!(worn < new);
    // A full hold stops you selling the pod that holds it.
    let mut full = h.captain("dockhand");
    h.dock_free(&mut full, yard);
    let pod = full
        .ship
        .modules
        .iter()
        .position(|m| m.module == h.module("pod1"))
        .unwrap();
    full.ship.cargo.push(Lot {
        good: 0,
        tons: 40,
        paid: 100,
    });
    assert_eq!(
        h.go(&mut full, &Action::SellModule(pod)).unwrap_err(),
        Refusal::HoldWouldOverflow
    );
}

#[test]
fn hull_classes_differ_and_a_trade_in_moves_what_fits() {
    let mut h = Harness::new(1, 1);
    let yard = h.frontier.home_station;
    let courier = &h.content.hulls[h.content.hull_ix("courier").unwrap()];
    let gunship = &h.content.hulls[h.content.hull_ix("gunship").unwrap()];
    assert!(courier.speed_pm > h.content.hulls[h.content.hull_ix("freighter").unwrap()].speed_pm);
    assert!(gunship.slots.weapon > courier.slots.weapon && gunship.cargo < courier.cargo + 10);
    let mut p = h.captain("dockhand");
    p.credit(Cause::Loan, 40_000);
    h.dock_free(&mut p, yard);
    // The 40-ton hold will not fit a gunship.
    p.ship.cargo.push(Lot {
        good: 0,
        tons: 38,
        paid: 100,
    });
    let gun = u8::try_from(h.content.hull_ix("gunship").unwrap()).unwrap();
    assert_eq!(
        h.go(&mut p, &Action::BuyHull(gun)).unwrap_err(),
        Refusal::HoldWouldOverflow
    );
    p.ship.cargo.clear();
    let cash = p.cash;
    let events = h.go(&mut p, &Action::BuyHull(gun)).unwrap();
    assert_eq!(p.ship.hull, gun);
    assert!(p.cash < cash);
    // The gunship has one cargo slot, so the freighter's one pod fits and the spare tank does not.
    let tanks = p.ship.used_slots(&h.content, SlotKind::Tank);
    assert_eq!(tanks, 1);
    assert!(events.iter().any(|e| matches!(e, Event::HullBought { .. })));
    assert_eq!(
        h.go(&mut p, &Action::BuyHull(gun)).unwrap_err(),
        Refusal::NothingToDo
    );
    assert!(p.stats(&h.content).power == 0, "no weapons fitted yet");
    h.go(&mut p, &Action::BuyModule(h.module("lance1")))
        .unwrap();
    assert_eq!(p.stats(&h.content).power, 4);
}

#[test]
fn a_broken_hull_will_not_jump_until_repaired() {
    let mut h = Harness::new(1, 1);
    let mut p = h.captain("dockhand");
    p.ship.hull_hp = 10;
    p.docked = None;
    let next = h.frontier.neighbours(p.at)[0].0;
    assert_eq!(
        h.go(&mut p, &Action::Jump(next)).unwrap_err(),
        Refusal::HullTooWeak
    );
    p.ship.hull_hp = 30;
    assert!(h.go(&mut p, &Action::Jump(next)).is_ok());
}

#[test]
fn a_stranded_ship_can_be_towed_and_the_fee_goes_on_the_loan_if_the_purse_is_empty() {
    let mut h = Harness::new(1, 1);
    let mut p = h.captain("dockhand");
    // Not stranded while it can still jump.
    p.docked = None;
    assert_eq!(
        h.go(&mut p, &Action::CallTow).unwrap_err(),
        Refusal::NotStranded
    );
    p.ship.fuel = 0;
    p.debit(Cause::Payment, p.cash);
    let far = h
        .frontier
        .systems
        .iter()
        .rev()
        .find(|s| s.stations.is_empty() && !s.uncharted)
        .unwrap()
        .id;
    p.at = far;
    let debt = p.debt.principal;
    let events = h.go(&mut p, &Action::CallTow).unwrap();
    assert!(events.iter().any(|e| matches!(e, Event::Towed { .. })));
    assert!(p.docked.is_some(), "towed in and docked");
    assert!(p.debt.principal >= debt + 300, "the fee went on the loan");
    assert_eq!(p.stats.tows, 1);
    assert_eq!(p.cash, 0);
}

#[test]
fn the_first_captain_to_arrive_charts_a_system_for_everyone_and_a_second_gets_nothing() {
    let mut h = Harness::new(1, 1);
    let target = h.frontier.systems.iter().find(|s| s.uncharted).unwrap().id;
    let from = h.frontier.neighbours(target)[0].0;
    let mut first = h.captain("dropout");
    first.captain = "Ardent".to_owned();
    first.at = from;
    first.docked = None;
    first.ship.fuel = 120;
    assert!(!h.world.is_charted(&h.frontier, target));
    let events = h.go(&mut first, &Action::Jump(target)).unwrap();
    let value = events
        .iter()
        .find_map(|e| {
            if let Event::Charted { value, .. } = e {
                Some(*value)
            } else {
                None
            }
        })
        .unwrap();
    assert!(value > 100);
    assert_eq!(first.survey.len(), 1);
    assert!(h.world.is_charted(&h.frontier, target));
    assert_eq!(
        h.world.charted[usize::from(target)].as_ref().unwrap().name,
        "Ardent"
    );
    // The change travels as a delta; applied to another board copy it charts there too.
    let mut other = World::fresh(&h.content, &h.frontier, h.today);
    assert!(!other.is_charted(&h.frontier, target));
    for d in &h.deltas {
        other.apply(&h.content, &h.frontier, d);
    }
    assert!(other.is_charted(&h.frontier, target));
    assert!(
        h.deltas
            .iter()
            .any(|d| matches!(d, Delta::Chart { system, .. } if *system == target))
    );
    // A second captain arrives after: no chart, no survey data, and the first name stays.
    let mut second = h.captain("dropout");
    second.captain = "Belated".to_owned();
    second.at = from;
    second.docked = None;
    second.ship.fuel = 120;
    let events = h.go(&mut second, &Action::Jump(target)).unwrap();
    assert!(!events.iter().any(|e| matches!(e, Event::Charted { .. })));
    assert!(second.survey.is_empty());
    assert_eq!(
        h.world.charted[usize::from(target)].as_ref().unwrap().name,
        "Ardent"
    );
    // The scanner decides how much the first chart is worth.
    let blind = {
        let p = h.captain("dockhand");
        let sys = &h.frontier.systems[usize::from(target)];
        p.survey_value(&h.content, sys)
    };
    assert!(blind < value);
}

#[test]
fn news_reaches_a_system_one_hop_a_night() {
    use russh_bbs_door_long_haul::world::NewsEvent;
    let mut h = Harness::new(1, 1);
    let kind = u8::try_from(
        h.content
            .news
            .iter()
            .position(|n| n.id == "plague")
            .unwrap(),
    )
    .unwrap();
    let origin = h.station("research");
    let origin_sys = h.frontier.station(origin).unwrap().system;
    h.world.events.push(NewsEvent {
        kind,
        origin,
        born: h.today,
        duration: 6,
    });
    let hops = h.frontier.hops_from(origin_sys);
    let far = h
        .frontier
        .systems
        .iter()
        .find(|s| hops[usize::from(s.id)] == 3)
        .unwrap()
        .id;
    let day = |h: &Harness, offset: i64, at: u16| {
        h.world
            .news_at(&h.content, &h.frontier, at, h.today + offset)
            .len()
    };
    assert_eq!(day(&h, 0, origin_sys), 1, "the origin hears at once");
    assert_eq!(day(&h, 0, far), 0, "three hops away it has not arrived");
    assert_eq!(day(&h, 2, far), 0);
    assert_eq!(day(&h, 3, far), 1, "after three nights it has");
    assert_eq!(day(&h, 6, far), 0, "and it is over after six");
}

#[test]
fn a_plague_makes_medicine_dear_where_it_strikes_and_then_the_market_recovers() {
    use russh_bbs_door_long_haul::world::NewsEvent;
    let mut h = Harness::new(1, 1);
    let kind = u8::try_from(
        h.content
            .news
            .iter()
            .position(|n| n.id == "plague")
            .unwrap(),
    )
    .unwrap();
    let med = h.good("medicine");
    let origin = h
        .frontier
        .stations
        .iter()
        .find(|s| s.goods.iter().any(|g| g.good == med))
        .unwrap()
        .id;
    let slot = usize::from(h.slot_of(origin, "medicine").unwrap());
    let before = h.world.stock_of(origin, slot);
    h.world.events.push(NewsEvent {
        kind,
        origin,
        born: h.today,
        duration: 6,
    });
    for _ in 0..5 {
        h.next_day();
    }
    let during = h.world.stock_of(origin, slot);
    assert!(
        during < before * 8 / 10,
        "stock of medicine falls under plague: {before} -> {during}"
    );
    for _ in 0..25 {
        h.next_day();
    }
    let after = h.world.stock_of(origin, slot);
    assert!(
        after > during,
        "and recovers once it is over: {during} -> {after}"
    );
}

#[test]
fn a_ships_wear_never_passes_the_limit_and_a_save_keeps_every_scar() {
    let mut h = Harness::new(1, 1);
    let mut p = h.captain("dockhand");
    p.ship.modules[0].wear = WEAR_MAX - 1;
    p.ship.hull_hp = 40;
    p.standing[Faction::Miners.index()] = -12;
    p.docked = None;
    for _ in 0..4 {
        let n = h.frontier.neighbours(p.at)[0].0;
        p.ship.fuel = 120;
        p.days_left = 10;
        let _ = h.go(&mut p, &Action::Jump(n));
        assert!(p.ship.modules.iter().all(|m| m.wear <= WEAR_MAX));
    }
    let again = Player::decode(&p.encode(), &h.content, &h.frontier).unwrap();
    assert_eq!(again, p);
}
