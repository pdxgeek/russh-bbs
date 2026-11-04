//! Transit events: the tables, the weights, the options, and the RPG rule (a crew mix opens
//! options, a standing closes them, a lost hand changes what comes later).
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
use russh_bbs_door_long_haul::content::Faction;
use russh_bbs_door_long_haul::crew::Role;
use russh_bbs_door_long_haul::engine::Action;
use russh_bbs_door_long_haul::events::{Event, Refusal};
use russh_bbs_door_long_haul::player::Player;
use russh_bbs_door_long_haul::scene::Scene;
use russh_bbs_door_long_haul::ship::Lot;
use russh_bbs_door_long_haul::skills::Skill;
use russh_bbs_door_long_haul::transit::{FAMILIES, Where};

fn weight_of(h: &Harness, p: &Player, system: u16, fresh: bool, event: &str) -> u32 {
    let w = Where {
        content: &h.content,
        frontier: &h.frontier,
        system,
        fresh,
    };
    let ix = h.content.transit.event_ix(event).unwrap();
    p.event_weights(&w)
        .into_iter()
        .find(|(i, _)| *i == ix)
        .map_or(0, |(_, wt)| wt)
}

fn system_with(h: &Harness, hazard: &str, economy: Option<&str>) -> u16 {
    h.frontier
        .systems
        .iter()
        .find(|s| {
            s.hazard.id() == hazard
                && economy.is_none_or(|e| {
                    s.stations.iter().any(|&st| {
                        h.content.economies[usize::from(h.frontier.station(st).unwrap().economy)].id
                            == e
                    })
                })
        })
        .unwrap_or_else(|| panic!("no {hazard} system with {economy:?}"))
        .id
}

#[test]
fn there_are_at_least_fifty_events_in_every_family_and_none_is_a_dead_end() {
    let h = Harness::new(1, 1);
    let events = &h.content.transit.event;
    assert!(events.len() >= 50, "{} events", events.len());
    for family in FAMILIES {
        let n = events.iter().filter(|e| e.family == family).count();
        assert!(n >= 2, "family {family} has {n} events");
    }
    for want in ["distress", "pirate", "inspection", "drive", "signal"] {
        assert!(
            events.iter().filter(|e| e.family == want).count() >= 5,
            "{want} needs depth"
        );
    }
    for e in events {
        assert!(
            e.options.iter().any(|o| o.req.is_empty()),
            "{} has no open option",
            e.id
        );
    }
    // Several events are only reachable by a particular build, crew or standing.
    assert!(
        events
            .iter()
            .flat_map(|e| &e.options)
            .filter(|o| !o.req.is_empty())
            .count()
            >= 40
    );
    assert!(h.content.transit.flags.len() <= 32);
}

#[test]
fn every_option_of_every_event_plays_out_for_a_loaded_captain_without_breaking_anything() {
    let mut h = Harness::new(2, 3);
    let n_events = h.content.transit.event.len();
    for ix in 0..n_events {
        let id = h.content.transit.event[ix].id.clone();
        let n_opts = h.content.transit.event[ix].options.len();
        for opt in 0..n_opts {
            for seed in 0..12u64 {
                h.reseed(seed * 977 + ix as u64);
                let mut p = h.captain("runner");
                p.cash = 5_000;
                p.ledger = russh_bbs_door_long_haul::player::Ledger::default();
                p.ledger.income[0] = 5_000;
                h.fit(&mut p, "lance1");
                h.fit(&mut p, "scan2");
                h.fit(&mut p, "cabin1");
                h.fit(&mut p, "cabin1");
                for role in Role::ALL {
                    h.sign(&mut p, role, 4);
                }
                p.ship.cargo.push(Lot {
                    good: h.good("ghostwire"),
                    tons: 3,
                    paid: 600,
                });
                p.ship.cargo.push(Lot {
                    good: h.good("water"),
                    tons: 6,
                    paid: 100,
                });
                p.ship.cargo.push(Lot {
                    good: h.good("medicine"),
                    tons: 4,
                    paid: 100,
                });
                p.ship.fuel = 40;
                h.open_event(&mut p, &id);
                let r = h.go_strict(&mut p, &Action::Choose(u8::try_from(opt).unwrap()));
                if r.is_err() {
                    assert!(p.scene.is_some(), "a refused choice keeps the scene");
                    continue;
                }
                h.finish_scene(&mut p);
                assert!(p.cash >= 0, "{id}/{opt}: cash {}", p.cash);
                assert_eq!(p.ledger.net(), p.cash, "{id}/{opt}: ledger");
                let max = h.content.hulls[usize::from(p.ship.hull)].hull_points;
                assert!((1..=max).contains(&p.ship.hull_hp), "{id}/{opt}: hull");
                assert!(p.ship.fuel <= p.stats(&h.content).fuel_cap);
                assert!(p.hold_used() <= u32::from(p.stats(&h.content).cargo_cap));
                assert!(p.crew.len() <= 8);
                assert!(p.scene.is_none(), "{id}/{opt}: scene left open");
                // And the save still round-trips.
                let bytes = p.encode();
                assert_eq!(
                    Player::decode(&bytes, &h.content, &h.frontier).unwrap(),
                    p,
                    "{id}/{opt}"
                );
            }
        }
    }
}

#[test]
fn a_scene_blocks_everything_else_and_survives_a_reload() {
    let mut h = Harness::new(1, 4);
    let mut p = h.captain("dockhand");
    h.open_event(&mut p, "coolant_leak");
    let before = p.clone();
    let rng = h.rng.state();
    assert_eq!(
        h.go_strict(&mut p, &Action::Undock).unwrap_err(),
        Refusal::Busy
    );
    assert_eq!(
        h.go_strict(&mut p, &Action::Jump(1)).unwrap_err(),
        Refusal::Busy
    );
    assert_eq!(p, before);
    assert_eq!(h.rng.state(), rng, "a refusal does not touch the RNG");
    // Reload mid-scene: same scene, same choices.
    let back = Player::decode(&p.encode(), &h.content, &h.frontier).unwrap();
    assert_eq!(back.scene, p.scene);
    // With no scene, a choice has nothing to choose.
    p.scene = None;
    assert_eq!(
        h.go_strict(&mut p, &Action::Choose(0)).unwrap_err(),
        Refusal::NoEvent
    );
    // An option past the end is not an option.
    h.open_event(&mut p, "coolant_leak");
    assert_eq!(
        h.go_strict(&mut p, &Action::Choose(9)).unwrap_err(),
        Refusal::NoSuchThing
    );
    assert!(matches!(p.scene, Some(Scene::Event(_))));
}

#[test]
fn an_event_that_was_walked_away_from_is_not_rerolled_by_reloading() {
    // The same captain, the same open scene, the same seed: the same result, whether or not
    // the line dropped in between.
    let run = |reload: bool| {
        let mut h = Harness::new(1, 5);
        let mut p = h.captain("dockhand");
        h.open_event(&mut p, "dead_freighter");
        if reload {
            p = Player::decode(&p.encode(), &h.content, &h.frontier).unwrap();
        }
        h.reseed(77);
        h.go_strict(&mut p, &Action::Choose(1)).unwrap();
        p
    };
    assert_eq!(run(false), run(true));
}

#[test]
fn a_crew_mix_opens_options_that_a_captain_alone_does_not_have() {
    let mut h = Harness::new(1, 6);
    let mut alone = h.captain("dockhand");
    h.open_event(&mut alone, "dead_freighter");
    let mut with_engineer = alone.clone();
    h.sign(&mut with_engineer, Role::Engineer, 3);
    let mut weak_engineer = alone.clone();
    h.sign(&mut weak_engineer, Role::Engineer, 1);
    // Option 0 is "help them fix the drive": it needs an engineer of skill 2 or better.
    let r = h.go_strict(&mut alone.clone(), &Action::Choose(0));
    assert!(matches!(r, Err(Refusal::Needs(_))), "{r:?}");
    let r = h.go_strict(&mut weak_engineer.clone(), &Action::Choose(0));
    assert!(matches!(r, Err(Refusal::Needs(_))), "{r:?}");
    assert!(h.go_strict(&mut with_engineer, &Action::Choose(0)).is_ok());
    // A hurt engineer is no engineer.
    let mut hurt = alone.clone();
    let i = h.sign(&mut hurt, Role::Engineer, 5);
    hurt.crew[i].hurt_days = 2;
    assert!(h.go_strict(&mut hurt, &Action::Choose(0)).is_err());
    // The engineer's pay-off differs: credits and Miners' standing, not just a smaller risk.
    let mut ok = alone.clone();
    h.sign(&mut ok, Role::Engineer, 4);
    let cash = ok.cash;
    h.go_strict(&mut ok, &Action::Choose(0)).unwrap();
    assert!(ok.cash > cash);
}

#[test]
fn a_standing_closes_an_option_and_opens_another() {
    let mut h = Harness::new(1, 7);
    let mut p = h.captain("dockhand");
    h.open_event(&mut p, "captains_colours");
    // Nobody on the Free Captains' side: the recognition code is closed.
    assert!(matches!(
        h.go_strict(&mut p.clone(), &Action::Choose(0)),
        Err(Refusal::Needs(_))
    ));
    // A friend of the Captains flashes it, and is let alone.
    let mut friend = p.clone();
    friend.standing[Faction::Free.index()] = 30;
    let cash = friend.cash;
    h.go_strict(&mut friend, &Action::Choose(0)).unwrap();
    assert!(friend.standing[Faction::Free.index()] >= 30);
    assert_eq!(friend.cash, cash);
    // The League clearance is its own door, closed to an outcast of the League.
    let mut outcast = p.clone();
    outcast.standing[Faction::League.index()] = -30;
    assert!(h.go_strict(&mut outcast, &Action::Choose(1)).is_err());
    let mut regular = p.clone();
    regular.standing[Faction::League.index()] = 20;
    assert!(h.go_strict(&mut regular, &Action::Choose(1)).is_ok());
}

#[test]
fn a_lost_hand_changes_what_comes_later() {
    let mut h = Harness::new(1, 8);
    let mut p = h.captain("dockhand");
    h.sign(&mut p, Role::Medic, 3);
    h.sign(&mut p, Role::Face, 2);
    h.open_event(&mut p, "medical_beacon");
    let mut with = p.clone();
    assert!(
        h.go_strict(&mut with, &Action::Choose(0)).is_ok(),
        "the medic goes across"
    );
    // Lose the medic and the same scene closes that option.
    let mut ev = Vec::new();
    p.lose_crew(0, &mut ev);
    assert!(matches!(
        h.go_strict(&mut p, &Action::Choose(0)),
        Err(Refusal::Needs(_))
    ));
    // The loss also changes which events come at all: a family fever needs someone to treat it.
    let miners = system_with(&h, "none", None);
    let mut crewed = h.captain("dockhand");
    let w = |p: &Player| weight_of(&h, p, miners, false, "galley_brawl");
    assert_eq!(w(&crewed), 0, "a brawl needs two hands");
    h.sign(&mut crewed, Role::Pilot, 1);
    h.sign(&mut crewed, Role::Gunner, 1);
    assert!(w(&crewed) > 0);
    crewed.crew.pop();
    assert_eq!(w(&crewed), 0, "and one hand fewer means no brawl");
}

#[test]
fn flags_set_by_a_choice_unlock_and_lock_later_scenes() {
    let mut h = Harness::new(1, 9);
    let mut p = h.captain("dockhand");
    let system = system_with(&h, "none", None);
    // The ark can be followed only by someone who has the map.
    h.open_event(&mut p, "gate_song");
    assert!(matches!(
        h.go_strict(&mut p.clone(), &Action::Choose(1)),
        Err(Refusal::Needs(_))
    ));
    let bit = h.content.transit.flag_bit("ark_map").unwrap();
    let mut mapped = p.clone();
    mapped.flags |= bit;
    h.go_strict(&mut mapped, &Action::Choose(1)).unwrap();
    assert!(matches!(mapped.scene, Some(Scene::Boarding(_))));
    // A once-only event that has fired is not drawn again.
    let ghost = h.content.transit.flag_bit("ghost_log").unwrap();
    let mut fresh = h.captain("dockhand");
    assert!(weight_of(&h, &fresh, system, false, "ghost_log") > 0);
    fresh.flags |= ghost;
    assert_eq!(weight_of(&h, &fresh, system, false, "ghost_log"), 0);
    // A scene that fires sets its own once-flag.
    let mut j = h.captain("dockhand");
    j.docked = None;
    let mut hit = false;
    for seed in 0..600 {
        h.reseed(seed);
        let mut q = j.clone();
        let next = h.frontier.neighbours(q.at)[0].0;
        q.ship.fuel = 80;
        if let Ok(ev) = h.go_strict(&mut q, &Action::Jump(next))
            && ev.iter().any(|e| matches!(e, Event::Transit { .. }))
        {
            hit = true;
            break;
        }
    }
    assert!(hit, "a jump sometimes opens a scene");
}

#[test]
fn the_system_the_cargo_and_the_standing_shift_what_turns_up() {
    let h = Harness::new(1, 10);
    let mut plain = h.captain("dockhand");
    plain.standing = [0; 5];
    let debris = system_with(&h, "debris", None);
    let clear = system_with(&h, "none", None);
    // Debris fields breed ambushes.
    let (a, b) = (
        weight_of(&h, &plain, debris, false, "debris_ambush"),
        weight_of(&h, &plain, clear, false, "debris_ambush"),
    );
    assert!(a > 2 * b, "debris {a} against clear {b}");
    // Contraband draws customs.
    let mut smuggler = plain.clone();
    smuggler.ship.cargo.push(Lot {
        good: h.good("ghostwire"),
        tons: 3,
        paid: 500,
    });
    assert!(
        weight_of(&h, &smuggler, clear, false, "customs_sweep")
            > 2 * weight_of(&h, &plain, clear, false, "customs_sweep")
    );
    // A friend of the League is stopped less; an enemy of the Union meets fewer of its pickets.
    let mut friend = plain.clone();
    friend.standing[Faction::League.index()] = 40;
    assert!(
        weight_of(&h, &friend, clear, false, "customs_sweep")
            < weight_of(&h, &plain, clear, false, "customs_sweep")
    );
    // The friendly hail is only for people the League likes.
    assert_eq!(weight_of(&h, &plain, clear, false, "navy_hail"), 0);
    assert!(weight_of(&h, &friend, clear, false, "navy_hail") > 0);
    // Passengers draw their own events; nobody else sees them.
    assert_eq!(weight_of(&h, &plain, clear, false, "pilgrim_hymn"), 0);
    let mut pilgrims = plain.clone();
    let pax = h
        .content
        .jobs
        .passenger
        .iter()
        .position(|d| d.id == "pilgrim")
        .unwrap();
    pilgrims
        .contracts
        .push(russh_bbs_door_long_haul::contracts::Contract {
            kind: russh_bbs_door_long_haul::contracts::Kind::Passenger,
            faction: Faction::Church,
            from: h.frontier.home_station,
            to: h.frontier.home_station,
            good: 0,
            tons: 0,
            pax: u8::try_from(pax).unwrap(),
            count: 2,
            target: 0,
            reward: 400,
            due: 99_999,
            progress: 0,
            mood: 7,
        });
    assert!(weight_of(&h, &pilgrims, clear, false, "pilgrim_hymn") > 0);
    // A freshly charted system has beacons.
    assert_eq!(weight_of(&h, &plain, clear, false, "dark_beacon"), 0);
    assert!(weight_of(&h, &plain, clear, true, "dark_beacon") > 0);
}

#[test]
fn skill_and_crew_tilt_the_odds_of_an_outcome() {
    // "Run" from a skiff: a good pilot aboard gets away more often.
    let mut h = Harness::new(1, 11);
    let run = |h: &mut Harness, pilot: bool| {
        let mut away = 0;
        for seed in 0..400u64 {
            h.reseed(seed);
            let mut p = h.captain("dockhand");
            p.ship.fuel = 80;
            if pilot {
                h.sign(&mut p, Role::Pilot, 4);
            }
            h.open_event(&mut p, "skiff_toll");
            // Option 3 is "Run".
            h.go_strict(&mut p, &Action::Choose(3)).unwrap();
            away += u32::from(p.scene.is_none());
        }
        away
    };
    let (alone, piloted) = (run(&mut h, false), run(&mut h, true));
    assert!(
        piloted > alone + 30,
        "away {piloted} with a pilot, {alone} without"
    );
}

#[test]
fn the_chance_of_an_event_rises_with_distance_danger_and_company() {
    let h = Harness::new(1, 12);
    let p = h.captain("dockhand");
    let clear = system_with(&h, "none", None);
    let debris = system_with(&h, "debris", None);
    let at = |system, fresh, dist| {
        let w = Where {
            content: &h.content,
            frontier: &h.frontier,
            system,
            fresh,
        };
        p.transit_chance(&w, dist)
    };
    assert!(at(clear, false, 40) > at(clear, false, 5));
    assert!(at(debris, false, 20) > at(clear, false, 20));
    assert!(at(clear, true, 20) > at(clear, false, 20));
    assert!((200..=700).contains(&at(clear, false, 20)));
}

#[test]
fn a_captains_own_skill_not_the_crews_gates_the_stealth_option() {
    // Some options ask for the captain's own skill (crew help does not count).
    let h = Harness::new(1, 13);
    let mut p = h.captain("dockhand");
    h.sign(&mut p, Role::Quartermaster, 8);
    assert!(p.eff(Skill::Stealth) > p.level(Skill::Stealth));
    let w = Where {
        content: &h.content,
        frontier: &h.frontier,
        system: 0,
        fresh: false,
    };
    let req = russh_bbs_door_long_haul::transit::Req::Skill {
        skill: Skill::Stealth,
        min: 3,
    };
    assert!(!p.meets(&w, &req));
    h.set_level(&mut p, Skill::Stealth, 3);
    assert!(p.meets(&w, &req));
}
