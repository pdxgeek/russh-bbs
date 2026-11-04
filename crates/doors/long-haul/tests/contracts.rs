//! Jobs and factions: the boards, standing as a gate, and each kind of contract end to end.
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

use common::{DAY0, Harness, orders};
use russh_bbs_door_long_haul::combat::{Intent, Range};
use russh_bbs_door_long_haul::content::Faction;
use russh_bbs_door_long_haul::contracts::{
    Kind, MAX_CONTRACTS, OFFICE_SHUT, Offer, PREMIUM_STANDING, board, offices,
};
use russh_bbs_door_long_haul::crew::{Goal, GoalKind, Role};
use russh_bbs_door_long_haul::engine::Action;
use russh_bbs_door_long_haul::events::{Event, Refusal};
use russh_bbs_door_long_haul::player::{Cause, Player};
use russh_bbs_door_long_haul::scene::Scene;
use russh_bbs_door_long_haul::skills::Skill;

/// The first (station, day, offer) on any board that satisfies `pick`.
fn find(h: &Harness, standing: &[i16; 5], pick: impl Fn(&Offer) -> bool) -> (u16, i64, Offer) {
    for day in (0..90).map(|d| DAY0 + d * 3) {
        for st in &h.frontier.stations {
            for o in board(&h.content, &h.frontier, st.id, day, standing) {
                if pick(&o) {
                    return (st.id, day, o);
                }
            }
        }
    }
    panic!("no such offer anywhere");
}

/// A captain with room for everything, docked at `station` on `day`.
fn ready(h: &mut Harness, station: u16, day: i64) -> Player {
    h.today = day;
    let mut p = h.captain("dockhand");
    p.last_day = day;
    p.debt.next_due = day + 7;
    p.debt.interest_day = day;
    p.wages_due = day + 7;
    p.standing = [0; 5];
    h.fit(&mut p, "cabin1");
    h.fit(&mut p, "cabin1");
    h.fit(&mut p, "pod1");
    h.dock_free(&mut p, station);
    p
}

fn accept(
    h: &mut Harness,
    p: &mut Player,
    station: u16,
    day: i64,
    o: &Offer,
) -> Result<Vec<Event>, Refusal> {
    let _ = (station, day);
    h.go(p, &Action::Accept(o.key))
}

#[test]
fn a_board_is_the_same_for_everyone_and_changes_every_three_days() {
    let h = Harness::new(1, 1);
    let st = h.frontier.home_station;
    let a = board(&h.content, &h.frontier, st, DAY0, &[0; 5]);
    let b = board(&h.content, &h.frontier, st, DAY0, &[0; 5]);
    assert_eq!(a, b);
    assert!(!a.is_empty());
    let later = board(&h.content, &h.frontier, st, DAY0 + 9, &[0; 5]);
    assert_ne!(
        a.iter().map(|o| o.key).collect::<Vec<_>>(),
        later.iter().map(|o| o.key).collect::<Vec<_>>()
    );
    // Every station posts something somewhere in a fortnight, and every office has a say.
    let mut seen = std::collections::BTreeSet::new();
    for st in h.frontier.stations.iter().take(60) {
        for d in 0..6 {
            for o in board(&h.content, &h.frontier, st.id, DAY0 + d * 3, &[0; 5]) {
                seen.insert((o.contract.faction.index(), o.contract.kind.index()));
            }
        }
    }
    for faction in Faction::ALL {
        assert!(
            seen.iter().any(|(f, _)| *f == faction.index()),
            "{} never posts a job",
            faction.name()
        );
    }
    for kind in Kind::ALL {
        assert!(
            seen.iter().any(|(_, k)| *k == kind.index()),
            "no {} job is ever posted",
            kind.name()
        );
    }
}

#[test]
fn the_guild_posts_only_at_research_stations_and_the_church_at_some() {
    let h = Harness::new(1, 2);
    for st in &h.frontier.stations {
        let o = offices(&h.content, &h.frontier, st.id);
        let eco = &h.content.economies[usize::from(st.economy)];
        assert!(o.contains(&eco.faction));
        assert_eq!(o.contains(&Faction::Guild), eco.id == "research");
    }
    assert!(
        h.frontier
            .stations
            .iter()
            .any(|s| offices(&h.content, &h.frontier, s.id).contains(&Faction::Church))
    );
}

#[test]
fn standing_decides_which_jobs_are_shown_and_what_they_pay() {
    let h = Harness::new(1, 3);
    let neutral = [0i16; 5];
    // A bounty needs standing 15 with whoever posts it.
    let (st, day, bounty) = find(&h, &neutral, |o| o.contract.kind == Kind::Bounty);
    assert!(bounty.locked.is_some(), "locked at standing 0");
    let mut friend = neutral;
    friend[bounty.contract.faction.index()] = 20;
    let there = board(&h.content, &h.frontier, st, day, &friend)
        .into_iter()
        .find(|o| o.key == bounty.key)
        .unwrap();
    assert!(there.locked.is_none(), "open at standing 20");
    // At 25 the pay goes up by about a third.
    let mut premium = neutral;
    premium[bounty.contract.faction.index()] = PREMIUM_STANDING;
    let rich = board(&h.content, &h.frontier, st, day, &premium)
        .into_iter()
        .find(|o| o.key == bounty.key)
        .unwrap();
    assert!(rich.contract.reward * 100 >= bounty.contract.reward * 130);
    // At -40 the office posts nothing for you.
    let mut outcast = neutral;
    outcast[bounty.contract.faction.index()] = OFFICE_SHUT;
    let shut = board(&h.content, &h.frontier, st, day, &outcast);
    assert!(
        shut.iter()
            .all(|o| o.contract.faction != bounty.contract.faction)
    );
}

#[test]
fn a_locked_job_cannot_be_taken_and_a_taken_job_leaves_the_board() {
    let mut h = Harness::new(1, 4);
    let (st, day, locked) = find(&h, &[0; 5], |o| o.locked.is_some());
    let mut p = ready(&mut h, st, day);
    let before = p.clone();
    assert!(matches!(
        accept(&mut h, &mut p, st, day, &locked),
        Err(Refusal::NeedStanding { .. })
    ));
    assert_eq!(p, before);
    // An open job is taken once.
    let (st, day, open) = find(&h, &[0; 5], |o| {
        o.locked.is_none() && o.contract.kind == Kind::Delivery && o.contract.tons <= 14
    });
    let mut p = ready(&mut h, st, day);
    let ev = accept(&mut h, &mut p, st, day, &open).unwrap();
    assert!(matches!(ev[0], Event::Accepted { .. }));
    assert_eq!(p.contracts.len(), 1);
    assert!(p.board_here_for_test(&h).iter().all(|o| o.key != open.key));
}

trait BoardExt {
    fn board_here_for_test(&self, h: &Harness) -> Vec<Offer>;
}

impl BoardExt for Player {
    fn board_here_for_test(&self, h: &Harness) -> Vec<Offer> {
        let st = self.docked.unwrap();
        board(&h.content, &h.frontier, st, self.last_day, &self.standing)
            .into_iter()
            .filter(|o| !self.taken.contains(&o.key))
            .collect()
    }
}

#[test]
fn delivery_takes_hold_space_and_pays_on_arrival_with_standing() {
    let mut h = Harness::new(1, 5);
    let (st, day, job) = find(&h, &[0; 5], |o| {
        o.locked.is_none()
            && o.contract.kind == Kind::Delivery
            && o.contract.faction == Faction::League
    });
    let mut p = ready(&mut h, st, day);
    let free_before = u32::from(p.stats(&h.content).cargo_cap) - p.hold_used();
    accept(&mut h, &mut p, st, day, &job).unwrap();
    assert_eq!(p.contract_tons(), u32::from(job.contract.tons));
    assert_eq!(
        u32::from(p.stats(&h.content).cargo_cap) - p.hold_used(),
        free_before - u32::from(job.contract.tons)
    );
    // Deliver: orbit the destination and dock.
    let to = job.contract.to;
    let cash = p.cash;
    h.orbit(&mut p, to);
    let ev = h.go(&mut p, &Action::Dock(to)).unwrap();
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::ContractDone {
            kind: Kind::Delivery,
            ..
        }
    )));
    assert!(p.contracts.is_empty());
    assert_eq!(p.contract_tons(), 0);
    assert_eq!(
        p.ledger.income[Cause::Contract.index()],
        i64::from(job.contract.reward)
    );
    assert!(p.cash > cash);
    assert_eq!(p.ledger.net(), p.cash);
    assert_eq!(p.stats.contracts_done, 1);
    assert!(p.standing[Faction::League.index()] >= 2);
    // The League's rival, the Free Captains, hear of it.
    assert!(p.standing[Faction::Free.index()] < 0);
}

#[test]
fn a_hold_too_full_a_ship_too_small_and_a_full_book_each_refuse_a_job() {
    let mut h = Harness::new(1, 6);
    let (st, day, job) = find(&h, &[0; 5], |o| {
        o.locked.is_none() && o.contract.kind == Kind::Delivery && o.contract.tons >= 12
    });
    let mut p = ready(&mut h, st, day);
    let cap = p.stats(&h.content).cargo_cap;
    p.ship.cargo.push(russh_bbs_door_long_haul::ship::Lot {
        good: h.good("water"),
        tons: cap - 3,
        paid: 10,
    });
    let before = p.clone();
    assert_eq!(
        accept(&mut h, &mut p, st, day, &job).unwrap_err(),
        Refusal::HoldWouldOverflow
    );
    assert_eq!(p, before);
    // Passengers need berths.
    let (st, day, pax) = find(&h, &[0; 5], |o| {
        o.locked.is_none() && o.contract.kind == Kind::Passenger && o.contract.count >= 2
    });
    let mut q = ready(&mut h, st, day);
    q.ship.modules.retain(|m| {
        h.content.modules[usize::from(m.module)].slot
            != russh_bbs_door_long_haul::content::SlotKind::Cabin
    });
    h.sign(&mut q, Role::Pilot, 1);
    h.sign(&mut q, Role::Face, 1);
    assert_eq!(
        accept(&mut h, &mut q, st, day, &pax).unwrap_err(),
        Refusal::NoBerth
    );
    // Four jobs at a time.
    let home = h.frontier.home_station;
    let mut r = ready(&mut h, home, DAY0);
    let offers: Vec<Offer> = (0..40)
        .flat_map(|d| {
            board(
                &h.content,
                &h.frontier,
                h.frontier.home_station,
                DAY0 + d * 3,
                &[0; 5],
            )
        })
        .filter(|o| o.locked.is_none() && o.contract.kind == Kind::Survey)
        .collect();
    let _ = offers;
    for n in 0..MAX_CONTRACTS {
        r.contracts
            .push(russh_bbs_door_long_haul::contracts::Contract {
                kind: Kind::Survey,
                faction: Faction::Guild,
                from: h.frontier.home_station,
                to: h.frontier.home_station,
                good: 0,
                tons: 0,
                pax: 0,
                count: 0,
                target: u16::try_from(n).unwrap(),
                reward: 100,
                due: DAY0 + 50,
                progress: 0,
                mood: 7,
            });
    }
    let (st, day, any) = find(&h, &[0; 5], |o| {
        o.locked.is_none() && o.contract.kind == Kind::Delivery && o.contract.tons <= 8
    });
    h.dock_free(&mut r, st);
    h.today = day;
    r.last_day = day;
    r.debt.next_due = day + 7;
    r.debt.interest_day = day;
    assert_eq!(
        accept(&mut h, &mut r, st, day, &any).unwrap_err(),
        Refusal::TooManyJobs
    );
}

#[test]
fn a_late_job_fails_and_costs_standing_and_a_given_up_job_costs_less() {
    let mut h = Harness::new(1, 7);
    let (st, day, job) = find(&h, &[0; 5], |o| {
        o.locked.is_none() && o.contract.kind == Kind::Delivery
    });
    let mut p = ready(&mut h, st, day);
    accept(&mut h, &mut p, st, day, &job).unwrap();
    let faction = job.contract.faction;
    // Days pass beyond the deadline.
    let mut late = p.clone();
    h.today = job.contract.due + 1;
    let ev = h.settle(&mut late);
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::ContractFailed { late: true, .. }))
    );
    assert!(late.contracts.is_empty());
    assert_eq!(late.contract_tons(), 0, "the client's cargo is taken back");
    assert!(late.standing[faction.index()] < p.standing[faction.index()]);
    // Giving it up costs three; being late costs four.
    h.today = day;
    let mut quit = p.clone();
    h.go(&mut quit, &Action::Abandon(0)).unwrap();
    assert_eq!(quit.standing[faction.index()], -3);
    assert!(late.standing[faction.index()] <= -4);
    assert_eq!(
        h.go(&mut quit, &Action::Abandon(0)).unwrap_err(),
        Refusal::NoSuchThing
    );
}

#[test]
fn passengers_pay_by_how_content_they_were_and_get_hungry() {
    let mut h = Harness::new(1, 8);
    let (st, day, job) = find(&h, &[0; 5], |o| {
        o.locked.is_none() && o.contract.kind == Kind::Passenger
    });
    let pay_at = |h: &mut Harness, mood: u8| {
        let mut p = ready(h, st, day);
        accept(h, &mut p, st, day, &job).unwrap();
        p.contracts[0].mood = mood;
        let to = job.contract.to;
        h.orbit(&mut p, to);
        let before = p.cash;
        h.go(&mut p, &Action::Dock(to)).unwrap();
        p.cash - before
    };
    let (happy, sour) = (pay_at(&mut h, 10), pay_at(&mut h, 0));
    assert!(happy > sour + sour / 2, "{happy} against {sour}");
    // Unfed passengers lose heart; fed ones do not.
    let mut p = ready(&mut h, st, day);
    accept(&mut h, &mut p, st, day, &job).unwrap();
    let mut fed = p.clone();
    fed.rations_until = day + 20;
    h.today = day + 3;
    h.settle(&mut p);
    h.settle(&mut fed);
    assert!(p.contracts[0].mood < fed.contracts[0].mood);
}

#[test]
fn a_bounty_is_paid_for_the_right_ship_and_only_the_right_ship() {
    let mut h = Harness::new(1, 9);
    let (st, day, job) = find(&h, &[40; 5], |o| {
        o.contract.kind == Kind::Bounty && o.contract.faction == Faction::League
    });
    let mut p = ready(&mut h, st, day);
    p.standing = [40; 5];
    h.fit(&mut p, "lance2");
    accept(&mut h, &mut p, st, day, &job).unwrap();
    let target = job.contract.pax;
    let target_id = h.content.enemies.enemy[usize::from(target)].id.clone();
    let wreck = |h: &mut Harness, p: &mut Player, id: &str| {
        h.fight(p, id);
        if let Some(Scene::Combat(c)) = &mut p.scene {
            c.e_hull = 1;
            c.e_shield = 0;
            c.range = Range::Mid;
            c.intent = Intent::Brace;
            c.shown = Intent::Brace;
        }
        for _ in 0..12 {
            if h.go_strict(p, &Action::Fight(orders(6, 0, 0))).is_ok() && p.scene.is_none() {
                break;
            }
            if matches!(&p.scene, Some(Scene::Combat(c)) if c.phase == russh_bbs_door_long_haul::combat::Phase::Won)
            {
                let _ = h.go_strict(p, &Action::Wreck { board: false });
            }
        }
    };
    // A different pirate does not count.
    let other = h
        .content
        .enemies
        .enemy
        .iter()
        .find(|e| e.faction == Faction::Free && e.id != target_id)
        .unwrap()
        .id
        .clone();
    p.docked = None;
    wreck(&mut h, &mut p, &other);
    assert_eq!(p.contracts[0].progress, 0, "wrong quarry");
    // The right one does.
    wreck(&mut h, &mut p, &target_id);
    assert_eq!(p.contracts[0].progress, 1);
    // Collect at the posting station.
    p.scene = None;
    h.orbit(&mut p, st);
    let cash = p.cash;
    h.today = day;
    let ev = h.go(&mut p, &Action::Dock(st)).unwrap();
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::ContractDone {
            kind: Kind::Bounty,
            ..
        }
    )));
    assert!(p.cash > cash + i64::from(job.contract.reward) / 2);
}

#[test]
fn an_escort_pays_by_how_much_ship_is_left() {
    let mut h = Harness::new(1, 10);
    let (st, day, job) = find(&h, &[40; 5], |o| o.contract.kind == Kind::Escort);
    let pay = |h: &mut Harness, ward: u8| {
        let mut p = ready(h, st, day);
        p.standing = [40; 5];
        accept(h, &mut p, st, day, &job).unwrap();
        p.contracts[0].progress = ward;
        let to = job.contract.to;
        h.orbit(&mut p, to);
        let before = p.cash;
        h.go(&mut p, &Action::Dock(to)).unwrap();
        p.cash - before
    };
    let (whole, half) = (pay(&mut h, 100), pay(&mut h, 50));
    assert!(whole > half + half / 2, "{whole} against {half}");
}

#[test]
fn a_survey_job_is_done_by_scanning_the_system_and_bringing_the_data_home() {
    let mut h = Harness::new(1, 11);
    let (st, day, job) = find(&h, &[10; 5], |o| o.contract.kind == Kind::Survey);
    let mut p = ready(&mut h, st, day);
    p.standing = [10; 5];
    h.fit(&mut p, "scan2");
    accept(&mut h, &mut p, st, day, &job).unwrap();
    let target = job.contract.target;
    // Docking at the posting station first pays nothing.
    let ev = h.go(&mut p, &Action::Undock).unwrap();
    let _ = ev;
    h.orbit(&mut p, st);
    // Scan the target system.
    p.at = target;
    p.docked = None;
    p.scanned.retain(|s| *s != target);
    h.set_level(&mut p, Skill::Survey, 2);
    h.go(&mut p, &Action::DeepScan).unwrap();
    assert_eq!(p.contracts[0].progress, 1);
    // Home again.
    h.orbit(&mut p, st);
    h.today = day;
    let ev = h.go(&mut p, &Action::Dock(st)).unwrap();
    assert!(
        ev.iter().any(|e| matches!(
            e,
            Event::ContractDone {
                kind: Kind::Survey,
                ..
            }
        )),
        "{ev:?}"
    );
}

#[test]
fn contraband_for_the_havens_is_found_by_customs_and_the_job_dies_with_it() {
    let mut h = Harness::new(1, 12);
    let (st, day, job) = find(&h, &[0; 5], |o| {
        o.locked.is_none()
            && o.contract.faction == Faction::Free
            && o.contract.kind == Kind::Delivery
    });
    let mut p = ready(&mut h, st, day);
    accept(&mut h, &mut p, st, day, &job).unwrap();
    assert!(p.contract_contraband(&h.content) > 0);
    let customs = h.station("industrial");
    let mut failed = 0;
    for seed in 0..80u64 {
        h.reseed(seed);
        let mut q = p.clone();
        h.orbit(&mut q, customs);
        let ev = h.go(&mut q, &Action::Dock(customs)).unwrap();
        if ev.iter().any(|e| {
            matches!(
                e,
                Event::ContractFailed {
                    kind: Kind::Delivery,
                    ..
                }
            )
        }) {
            failed += 1;
            assert!(q.contracts.is_empty());
        }
    }
    assert!(failed > 5 && failed < 70, "{failed} of 80");
    // A quartermaster hides client cargo as well as the captain's own.
    let mut with_qm = p.clone();
    h.sign(&mut with_qm, Role::Quartermaster, 9);
    let mut failed_qm = 0;
    for seed in 0..80u64 {
        h.reseed(seed);
        let mut q = with_qm.clone();
        h.orbit(&mut q, customs);
        let ev = h.go(&mut q, &Action::Dock(customs)).unwrap();
        failed_qm += i32::from(ev.iter().any(|e| matches!(e, Event::ContractFailed { .. })));
    }
    assert!(
        failed_qm < failed,
        "{failed_qm} with a quartermaster, {failed} without"
    );
}

#[test]
fn a_finished_job_counts_toward_a_crew_members_goal() {
    let mut h = Harness::new(1, 13);
    let (st, day, job) = find(&h, &[0; 5], |o| {
        o.locked.is_none() && o.contract.kind == Kind::Delivery
    });
    let mut p = ready(&mut h, st, day);
    let i = h.sign(&mut p, Role::Face, 3);
    p.crew[i].goal = Goal {
        kind: GoalKind::Contracts,
        target: 1,
        progress: 0,
        text: 7,
    };
    accept(&mut h, &mut p, st, day, &job).unwrap();
    let to = job.contract.to;
    h.orbit(&mut p, to);
    let ev = h.go(&mut p, &Action::Dock(to)).unwrap();
    assert!(p.crew[i].sworn);
    assert!(ev.iter().any(|e| matches!(e, Event::GoalMet { .. })));
}

#[test]
fn the_guild_pays_its_friends_more_and_shuts_its_office_on_an_enemy() {
    let mut h = Harness::new(1, 14);
    let st = h.station("research");
    let sell = |h: &mut Harness, guild: i16| {
        let mut p = h.captain("dockhand");
        p.standing = [0; 5];
        p.standing[Faction::Guild.index()] = guild;
        h.dock_free(&mut p, st);
        p.survey.push(russh_bbs_door_long_haul::player::SurveyLot {
            system: 0,
            value: 1000,
        });
        let r = h.go(&mut p, &Action::SellSurvey);
        r.map(|_| p.cash)
    };
    let (friend, plain, foe) = (
        sell(&mut h, 30).unwrap(),
        sell(&mut h, 0).unwrap(),
        sell(&mut h, -30).unwrap(),
    );
    assert!(friend > plain && plain > foe, "{friend} {plain} {foe}");
    assert_eq!(
        sell(&mut h, -50).unwrap_err(),
        Refusal::Barred(Faction::Guild)
    );
}

#[test]
fn a_repossession_takes_the_jobs_and_the_hands_that_have_no_berth() {
    let mut h = Harness::new(1, 15);
    let (st, day, job) = find(&h, &[0; 5], |o| {
        o.locked.is_none() && o.contract.kind == Kind::Delivery
    });
    let mut p = ready(&mut h, st, day);
    accept(&mut h, &mut p, st, day, &job).unwrap();
    h.sign(&mut p, Role::Pilot, 2);
    h.sign(&mut p, Role::Face, 2);
    h.sign(&mut p, Role::Medic, 2);
    p.cash = 0;
    p.ledger = russh_bbs_door_long_haul::player::Ledger::default();
    p.debt.missed = 2;
    p.debt.next_due = day + 1;
    p.debt.principal = 20_000;
    h.today = day + 1;
    let ev = h.settle(&mut p);
    assert!(
        ev.iter().any(|e| matches!(e, Event::Repossessed { .. })),
        "{ev:?}"
    );
    assert!(p.contracts.is_empty());
    assert!(p.crew.len() <= p.berths(&h.content));
    assert!(ev.iter().any(|e| matches!(e, Event::Dismissed { .. })));
}

#[test]
fn contracts_round_trip_and_a_save_with_a_bad_station_is_refused() {
    let mut h = Harness::new(1, 16);
    let (st, day, job) = find(&h, &[0; 5], |o| {
        o.locked.is_none() && o.contract.kind == Kind::Delivery
    });
    let mut p = ready(&mut h, st, day);
    accept(&mut h, &mut p, st, day, &job).unwrap();
    let bytes = p.encode();
    assert_eq!(Player::decode(&bytes, &h.content, &h.frontier).unwrap(), p);
    let mut bad = p.clone();
    bad.contracts[0].to = u16::MAX;
    assert!(Player::decode(&bad.encode(), &h.content, &h.frontier).is_none());
}
