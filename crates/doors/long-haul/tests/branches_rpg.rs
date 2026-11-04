//! The RPG rule for the engine slice: every element of a captain's build must change what
//! happens. Each test sets up the same situation for two builds and shows the outcomes differ,
//! so no skill, perk or standing is a number on a screen.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

mod common;

use common::Harness;
use russh_bbs_door_long_haul::content::{Faction, Perk};
use russh_bbs_door_long_haul::engine::{Action, RepairTarget};
use russh_bbs_door_long_haul::events::{Event, Refusal};
use russh_bbs_door_long_haul::market::spread_pm;
use russh_bbs_door_long_haul::player::{Player, SurveyLot};
use russh_bbs_door_long_haul::ship::{Installed, Lot, Ship};
use russh_bbs_door_long_haul::skills::{Skill, level_of, xp_for};

fn with_contraband(h: &Harness, p: &mut Player, tons: u16) {
    p.ship.cargo.push(Lot {
        good: h.good("ghostwire"),
        tons,
        paid: 4_000,
    });
}

/// Dock a contraband carrier at `station` once per seed; count how often customs find it.
fn caught_rate(h: &mut Harness, base: &Player, station: u16, trials: u64) -> (u32, i64) {
    let mut caught = 0;
    let mut fees = 0;
    for seed in 0..trials {
        h.reseed(seed);
        let mut p = base.clone();
        h.orbit(&mut p, station);
        let cash = p.cash;
        let events = h.go(&mut p, &Action::Dock(station)).unwrap();
        if events.iter().any(|e| matches!(e, Event::Seized { .. })) {
            caught += 1;
        }
        fees += events
            .iter()
            .map(|e| {
                if let Event::Docked { fee, .. } = e {
                    *fee
                } else {
                    0
                }
            })
            .sum::<i64>();
        assert!(p.cash <= cash);
    }
    (caught, fees)
}

#[test]
fn the_four_backgrounds_start_as_four_different_captains() {
    let h = Harness::new(1, 1);
    let mut seen = Vec::new();
    for bg in &h.content.backgrounds {
        let p = h.captain(&bg.id);
        seen.push((
            p.cash,
            p.debt.principal,
            p.xp,
            p.standing,
            p.stats(&h.content).scan,
        ));
        assert_eq!(p.ledger.net(), p.cash);
    }
    for (i, a) in seen.iter().enumerate() {
        for b in &seen[i + 1..] {
            assert_ne!(a, b);
        }
    }
    let dropout = h.captain("dropout");
    assert!(
        dropout.stats(&h.content).scan >= 1,
        "the Guild contact brings a scanner"
    );
    for other in ["dockhand", "washout", "runner"] {
        assert_eq!(h.captain(other).stats(&h.content).scan, 0);
    }
    assert_eq!(h.captain("runner").standing[Faction::Free.index()], 20);
    assert_eq!(h.captain("runner").standing[Faction::League.index()], -10);
}

#[test]
fn the_known_face_perk_makes_league_fuel_cheaper_and_only_there() {
    let mut h = Harness::new(1, 1);
    let league = h.station("industrial");
    let miners = h.station("mining");
    let cost = |h: &mut Harness, bg: &str, station: u16| {
        let mut p = h.captain(bg);
        h.dock_free(&mut p, station);
        p.ship.fuel = 0;
        let cash = p.cash;
        h.go(&mut p, &Action::Refuel(50)).unwrap();
        cash - p.cash
    };
    let brat = cost(&mut h, "dockhand", league);
    let other = cost(&mut h, "washout", league);
    assert!(brat < other, "{brat} vs {other}");
    assert_eq!(
        cost(&mut h, "dockhand", miners),
        cost(&mut h, "washout", miners)
    );
    assert_eq!(
        h.captain("dockhand").perk(&h.content),
        Some(Perk::KnownFace)
    );
}

#[test]
fn old_codes_wave_a_washout_through_a_military_post_and_nobody_else() {
    let mut h = Harness::new(1, 1);
    let post = h.station("military");
    let mut washout = h.captain("washout");
    with_contraband(&h, &mut washout, 12);
    let mut brat = h.captain("dockhand");
    with_contraband(&h, &mut brat, 12);
    let (w_caught, w_fees) = caught_rate(&mut h, &washout, post, 200);
    let (b_caught, b_fees) = caught_rate(&mut h, &brat, post, 200);
    assert_eq!(w_caught, 0, "no inspection for the washout, ever");
    assert_eq!(w_fees, 0, "and no dock fee");
    assert!(
        b_caught > 100,
        "a military post finds contraband most of the time: {b_caught}/200"
    );
    assert!(b_fees > 0);
}

#[test]
fn the_fence_buys_where_others_are_refused_and_sells_dearer() {
    let mut h = Harness::new(1, 1);
    let haven = h.station("haven");
    let slot = [
        "ghostwire",
        "dreamsilt",
        "stripped_cores",
        "slug_guns",
        "gate_relics",
    ]
    .iter()
    .find_map(|g| h.slot_of(haven, g))
    .unwrap();
    // A washout (Free Captains standing -10) is turned away; the runner and a neutral brat are not.
    let mut washout = h.captain("washout");
    h.dock_free(&mut washout, haven);
    assert_eq!(
        h.go(&mut washout, &Action::Buy { slot, tons: 1 })
            .unwrap_err(),
        Refusal::NotTrusted
    );
    let mut brat = h.captain("dockhand");
    h.dock_free(&mut brat, haven);
    assert!(h.go(&mut brat, &Action::Buy { slot, tons: 1 }).is_ok());
    // A non-haven never sells it, whoever asks.
    let industrial = h.station("industrial");
    let mut runner = h.captain("runner");
    h.dock_free(&mut runner, industrial);
    let st = h.frontier.station(industrial).unwrap();
    if let Some(cslot) = st.goods.iter().position(|m| {
        h.content.goods[usize::from(m.good)].class
            == russh_bbs_door_long_haul::content::GoodClass::Contraband
    }) {
        let r = h.go(
            &mut runner,
            &Action::Buy {
                slot: u8::try_from(cslot).unwrap(),
                tons: 1,
            },
        );
        assert_eq!(r.unwrap_err(), Refusal::NotForSale);
    }
    // Selling: same cargo, same haven, the runner gets a tenth more, and Free standing rises.
    let sell = |h: &mut Harness, bg: &str| {
        let mut p = h.captain(bg);
        h.dock_free(&mut p, haven);
        let g = h.frontier.station(haven).unwrap().goods[usize::from(slot)].good;
        p.ship.cargo.push(Lot {
            good: g,
            tons: 4,
            paid: 100,
        });
        let cash = p.cash;
        h.go(&mut p, &Action::Sell { slot, tons: 4 }).unwrap();
        (
            p.cash - cash,
            p.standing[Faction::Free.index()] - h.captain(bg).standing[Faction::Free.index()],
        )
    };
    let (runner_gets, runner_standing) = sell(&mut h, "runner");
    let (brat_gets, brat_standing) = sell(&mut h, "dockhand");
    assert!(runner_gets > brat_gets, "{runner_gets} vs {brat_gets}");
    assert_eq!((runner_standing, brat_standing), (2, 2));
}

#[test]
fn the_guild_contact_sells_survey_data_for_a_quarter_more_and_a_scanner_decides_who_can_scan() {
    let mut h = Harness::new(1, 1);
    let lab = h.station("research");
    let sell = |h: &mut Harness, bg: &str| {
        let mut p = h.captain(bg);
        h.dock_free(&mut p, lab);
        p.survey.push(SurveyLot {
            system: 0,
            value: 800,
        });
        let cash = p.cash;
        h.go(&mut p, &Action::SellSurvey).unwrap();
        p.cash - cash
    };
    assert_eq!(sell(&mut h, "dropout"), 1_000);
    assert_eq!(sell(&mut h, "dockhand"), 800);
    // Scanning in depth needs a scanner at all.
    let mut brat = h.captain("dockhand");
    h.orbit(&mut brat, h.frontier.home_station);
    assert_eq!(
        h.go(&mut brat, &Action::DeepScan).unwrap_err(),
        Refusal::NoScanner
    );
    let mut dropout = h.captain("dropout");
    h.orbit(&mut dropout, h.frontier.home_station);
    let events = h.go(&mut dropout, &Action::DeepScan).unwrap();
    assert!(events.iter().any(|e| matches!(e, Event::Scanned { .. })));
    assert_eq!(
        h.go(&mut dropout, &Action::DeepScan).unwrap_err(),
        Refusal::AlreadyScanned
    );
    // Only a research station has a Guild office.
    let mut dropout2 = h.captain("dropout");
    h.dock_free(&mut dropout2, h.station("mining"));
    dropout2.survey.push(SurveyLot {
        system: 1,
        value: 100,
    });
    assert_eq!(
        h.go(&mut dropout2, &Action::SellSurvey).unwrap_err(),
        Refusal::NoGuild
    );
}

#[test]
fn trading_skill_narrows_the_spread_and_changes_what_the_same_trade_pays() {
    let mut h = Harness::new(1, 1);
    let st = h.frontier.home_station;
    let slot = h
        .slot_of(st, "alloy")
        .or_else(|| h.slot_of(st, "water"))
        .unwrap_or(0);
    let round = |h: &mut Harness, level: u8| {
        let mut p = h.captain("dockhand");
        h.set_level(&mut p, Skill::Trade, level);
        h.dock_free(&mut p, st);
        let start = p.cash;
        h.go(&mut p, &Action::Buy { slot, tons: 10 }).unwrap();
        h.go(&mut p, &Action::Sell { slot, tons: 10 }).unwrap();
        start - p.cash
    };
    let (novice, master) = (round(&mut h, 0), round(&mut h, 10));
    assert!(
        master < novice,
        "a master loses {master} on the round trip, a novice {novice}"
    );
    assert!(spread_pm(10, 0) < spread_pm(0, 0));
}

#[test]
fn a_bad_name_with_the_stations_faction_widens_the_spread() {
    let mut h = Harness::new(1, 1);
    let st = h.station("industrial");
    let round = |h: &mut Harness, standing: i16| {
        let mut p = h.captain("dockhand");
        p.standing[Faction::League.index()] = standing;
        h.dock_free(&mut p, st);
        let slot = h.slot_of(st, "alloy").unwrap_or(0);
        let start = p.cash;
        h.go(&mut p, &Action::Buy { slot, tons: 10 }).unwrap();
        h.go(&mut p, &Action::Sell { slot, tons: 10 }).unwrap();
        start - p.cash
    };
    assert!(round(&mut h, 30) < round(&mut h, 0));
    assert!(round(&mut h, 0) < round(&mut h, -30));
}

#[test]
fn piloting_cuts_fuel_and_days_and_reads_a_nebula() {
    let mut h = Harness::new(1, 1);
    let mut novice = h.captain("dockhand");
    let mut ace = h.captain("dockhand");
    h.set_level(&mut ace, Skill::Pilot, 10);
    let (a, b) = (novice.stats(&h.content), ace.stats(&h.content));
    assert!(Ship::fuel_for(&b, 80) < Ship::fuel_for(&a, 80));
    assert!(Ship::days_for(&b, 80) <= Ship::days_for(&a, 80));
    assert!(b.speed_x10 > a.speed_x10);
    // A nebula costs the novice a day; a level-5 pilot (no scanner) reads it.
    let (from, to) = h
        .frontier
        .systems
        .iter()
        .find(|s| s.hazard == russh_bbs_door_long_haul::frontier::Hazard::Nebula && !s.uncharted)
        .map(|s| (h.frontier.neighbours(s.id)[0].0, s.id))
        .unwrap();
    for p in [&mut novice, &mut ace] {
        p.at = from;
        p.docked = None;
    }
    h.set_level(&mut ace, Skill::Pilot, 5);
    let hazard = |h: &mut Harness, p: &mut Player| {
        let e = h.go(p, &Action::Jump(to)).unwrap();
        e.into_iter()
            .find_map(|e| {
                if let Event::Hazard {
                    avoided, days_lost, ..
                } = e
                {
                    Some((avoided, days_lost))
                } else {
                    None
                }
            })
            .unwrap()
    };
    assert_eq!(hazard(&mut h, &mut novice), (false, 1));
    assert_eq!(hazard(&mut h, &mut ace), (true, 0));
    assert!(
        novice.days_left < ace.days_left,
        "the lost day came off the novice's allowance"
    );
}

#[test]
fn engineering_makes_repairs_cheaper_and_wear_slower() {
    let mut h = Harness::new(1, 1);
    let yard = h.frontier.home_station;
    let quote = |h: &mut Harness, level: u8| {
        let mut p = h.captain("dockhand");
        h.set_level(&mut p, Skill::Engineer, level);
        h.dock_free(&mut p, yard);
        for m in &mut p.ship.modules {
            m.wear = 600;
        }
        p.ship.hull_hp = 60;
        let env = russh_bbs_door_long_haul::engine::Env::new(
            &h.content,
            &h.frontier,
            &mut h.world,
            &mut h.rng,
            h.today,
        );
        let (cost, days) = p.repair_quote(&env, RepairTarget::All);
        (cost, days, p.wear_for(100))
    };
    let (novice, ace) = (quote(&mut h, 0), quote(&mut h, 10));
    assert!(ace.0 < novice.0, "repair cost {} vs {}", ace.0, novice.0);
    assert!(ace.2 < novice.2, "wear per use {} vs {}", ace.2, novice.2);
    // Paying for it clears the wear, spends the days and takes the credits.
    let mut p = h.captain("dockhand");
    p.credit(russh_bbs_door_long_haul::player::Cause::Loan, 5_000);
    h.dock_free(&mut p, yard);
    for m in &mut p.ship.modules {
        m.wear = 600;
    }
    let cash = p.cash;
    let events = h.go(&mut p, &Action::Repair(RepairTarget::All)).unwrap();
    assert!(p.ship.modules.iter().all(|m| m.wear == 0));
    assert!(p.cash < cash && p.days_left < 10);
    assert!(events.iter().any(|e| matches!(e, Event::Repaired { .. })));
    assert_eq!(
        h.go(&mut p, &Action::Repair(RepairTarget::All))
            .unwrap_err(),
        Refusal::NothingToRepair
    );
}

#[test]
fn stealth_decides_how_often_customs_find_the_hold() {
    let mut h = Harness::new(1, 1);
    let station = h.station("industrial");
    let mut sneak = h.captain("dockhand");
    h.set_level(&mut sneak, Skill::Stealth, 10);
    with_contraband(&h, &mut sneak, 8);
    let mut clumsy = h.captain("dockhand");
    with_contraband(&h, &mut clumsy, 8);
    let (found, _) = caught_rate(&mut h, &clumsy, station, 300);
    let (missed, _) = caught_rate(&mut h, &sneak, station, 300);
    assert!(found > 40, "a clumsy captain is found {found}/300 times");
    assert_eq!(
        missed, 0,
        "a level-10 sneak is never found at a 300-per-mille dock"
    );
}

#[test]
fn survey_skill_and_scanner_grade_decide_what_a_survey_is_worth() {
    let h = Harness::new(1, 1);
    let sys = &h.frontier.systems[5];
    let mut novice = h.captain("dropout");
    let mut expert = h.captain("dropout");
    h.set_level(&mut expert, Skill::Survey, 10);
    assert!(expert.survey_value(&h.content, sys) > novice.survey_value(&h.content, sys));
    let blind = h.captain("dockhand");
    assert!(blind.survey_value(&h.content, sys) < novice.survey_value(&h.content, sys));
    novice.ship.modules.push(Installed {
        module: h.module("scan3"),
        wear: 0,
    });
    assert!(
        novice.survey_value(&h.content, sys) > h.captain("dropout").survey_value(&h.content, sys)
    );
}

#[test]
fn skills_grow_by_doing_and_only_by_doing() {
    let mut h = Harness::new(1, 1);
    let st = h.frontier.home_station;
    let mut p = h.captain("dockhand");
    h.dock_free(&mut p, st);
    let xp0 = p.xp;
    // Trading trains Trading, and a level shows up as an event.
    p.xp[Skill::Trade.index()] = xp_for(3) - 1;
    let slot = h
        .slot_of(st, "water")
        .or_else(|| h.slot_of(st, "grain"))
        .unwrap_or(0);
    let events = h.go(&mut p, &Action::Buy { slot, tons: 1 }).unwrap();
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::SkillUp {
                skill: Skill::Trade,
                level: 3
            }
        )),
        "{events:?}"
    );
    assert_eq!(level_of(p.xp[Skill::Trade.index()]), 3);
    for other in [Skill::Pilot, Skill::Engineer, Skill::Stealth, Skill::Survey] {
        assert_eq!(
            p.xp[other.index()],
            xp0[other.index()],
            "{other:?} must not move from trading"
        );
    }
    // Flying trains Piloting.
    let (_, next) = (0, h.frontier.neighbours(p.at)[0].0);
    p.docked = None;
    let before = p.xp[Skill::Pilot.index()];
    h.go(&mut p, &Action::Jump(next)).unwrap();
    assert!(p.xp[Skill::Pilot.index()] > before);
    // Repairing trains Engineering; slipping customs trains Stealth.
    let yard = h.frontier.home_station;
    h.dock_free(&mut p, yard);
    p.ship.modules[0].wear = 500;
    let eng = p.xp[Skill::Engineer.index()];
    h.go(&mut p, &Action::Repair(RepairTarget::All)).unwrap();
    assert!(p.xp[Skill::Engineer.index()] > eng);
    let mut sneak = h.captain("dockhand");
    with_contraband(&h, &mut sneak, 1);
    h.set_level(&mut sneak, Skill::Stealth, 10);
    let before = sneak.xp[Skill::Stealth.index()];
    let station = h.station("industrial");
    h.orbit(&mut sneak, station);
    let events = h.go(&mut sneak, &Action::Dock(station)).unwrap();
    assert!(events.iter().any(|e| matches!(e, Event::Slipped)));
    assert!(sneak.xp[Skill::Stealth.index()] > before);
}

#[test]
fn a_bad_name_closes_a_factions_docks_until_it_thaws() {
    let mut h = Harness::new(1, 1);
    let industrial = h.station("industrial");
    let mining = h.station("mining");
    let mut p = h.captain("dockhand");
    p.standing[Faction::League.index()] = -45;
    h.orbit(&mut p, industrial);
    assert_eq!(
        h.go(&mut p, &Action::Dock(industrial)).unwrap_err(),
        Refusal::Barred(Faction::League)
    );
    // The Miners do not share the League's opinion.
    h.orbit(&mut p, mining);
    assert!(h.go(&mut p, &Action::Dock(mining)).is_ok());
    // A week of quiet thaws it a point a day.
    for _ in 0..7 {
        h.next_day();
    }
    h.settle(&mut p);
    assert_eq!(p.standing[Faction::League.index()], -38);
    h.orbit(&mut p, industrial);
    assert!(h.go(&mut p, &Action::Dock(industrial)).is_ok());
}

#[test]
fn a_notorious_smuggler_is_turned_away_by_the_military_and_a_clean_captain_is_not() {
    let mut h = Harness::new(1, 1);
    let post = h.station("military");
    let mut smuggler = h.captain("dockhand");
    smuggler.standing[Faction::Free.index()] = 55;
    h.orbit(&mut smuggler, post);
    assert_eq!(
        h.go(&mut smuggler, &Action::Dock(post)).unwrap_err(),
        Refusal::KnownSmuggler
    );
    let mut clean = h.captain("dockhand");
    h.orbit(&mut clean, post);
    assert!(h.go(&mut clean, &Action::Dock(post)).is_ok());
}

#[test]
fn getting_caught_costs_the_cargo_a_fine_and_the_stations_faction_its_regard() {
    let mut h = Harness::new(1, 1);
    for (economy, faction) in [("industrial", Faction::League), ("mining", Faction::Miners)] {
        let station = h.station(economy);
        let mut found = None;
        for seed in 0..400 {
            h.reseed(seed);
            let mut p = h.captain("dockhand");
            with_contraband(&h, &mut p, 30);
            h.orbit(&mut p, station);
            let (cash, standing) = (p.cash, p.standing[faction.index()]);
            let events = h.go(&mut p, &Action::Dock(station)).unwrap();
            if events.iter().any(|e| matches!(e, Event::Seized { .. })) {
                found = Some((p, cash, standing));
                break;
            }
        }
        let (p, cash, standing) = found.unwrap_or_else(|| panic!("never caught at {economy}"));
        assert_eq!(
            p.ship.contraband_tons(&h.content),
            0,
            "the contraband is gone"
        );
        assert!(p.cash < cash, "and a fine was paid");
        assert_eq!(p.standing[faction.index()], standing - 8);
        assert_eq!(p.stats.caught, 1);
    }
}

#[test]
fn trade_news_raises_the_inspection_chance_where_it_reaches() {
    use russh_bbs_door_long_haul::world::NewsEvent;
    let mut h = Harness::new(1, 1);
    let crackdown = u8::try_from(
        h.content
            .news
            .iter()
            .position(|n| n.id == "crackdown")
            .unwrap(),
    )
    .unwrap();
    let origin = h.station("military");
    let near = h
        .frontier
        .neighbours(h.frontier.station(origin).unwrap().system)[0]
        .0;
    let near_station = h.frontier.systems[usize::from(near)]
        .stations
        .first()
        .copied();
    h.world.events.push(NewsEvent {
        kind: crackdown,
        origin,
        born: h.today,
        duration: 5,
    });
    assert_eq!(
        h.world
            .inspection_bonus(&h.content, &h.frontier, origin, h.today),
        350
    );
    if let Some(s) = near_station {
        assert_eq!(
            h.world
                .inspection_bonus(&h.content, &h.frontier, s, h.today),
            350
        );
    }
    assert_eq!(
        h.world
            .inspection_bonus(&h.content, &h.frontier, origin, h.today + 6),
        0,
        "it ends"
    );
}

#[test]
fn legal_trade_earns_standing_with_the_station_that_buys_it() {
    let mut h = Harness::new(1, 1);
    let st = h.station("industrial");
    let mut p = h.captain("dockhand");
    h.dock_free(&mut p, st);
    let start = p.standing[Faction::League.index()];
    let slot = h
        .slot_of(st, "ore")
        .or_else(|| h.slot_of(st, "rare_earths"))
        .or_else(|| h.slot_of(st, "water"))
        .unwrap();
    let good = h.frontier.station(st).unwrap().goods[usize::from(slot)].good;
    p.ship.cargo.push(Lot {
        good,
        tons: 40,
        paid: 100,
    });
    h.go(&mut p, &Action::Sell { slot, tons: 40 }).unwrap();
    let gained = p.standing[Faction::League.index()] - start;
    assert!(
        gained >= 1,
        "forty tons of trade should earn a point; got {gained}"
    );
}
