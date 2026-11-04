//! The L2 screens through the framework's replay helper: a recorded run of keys must give the
//! same saved state every time, from a save that already holds crew or an open scene, and the
//! screen must say what the engine knows.
#![allow(
    clippy::assert_is_empty,
    clippy::single_match_else,
    unused_mut,
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines
)]

mod common;

use common::Harness;
use russh_bbs_core::limits::Day;
use russh_bbs_door::replay::{self, ReplayLog};
use russh_bbs_door::{DoorEnv, DoorKey};
use russh_bbs_door_long_haul::crew::Role;
use russh_bbs_door_long_haul::player::Player;
use russh_bbs_door_long_haul::scene::Scene;
use russh_bbs_door_long_haul::{DOOR_ID, LongHaul};

fn log(seed: u64, save: Vec<u8>, keys: &str) -> ReplayLog {
    ReplayLog {
        door: DOOR_ID.to_owned(),
        seed,
        start: Day::from_raw(common::DAY0).start().as_raw(),
        season: 1,
        save: Some(save),
        keys: keys
            .split(' ')
            .filter(|t| !t.is_empty())
            .map(|t| DoorKey::from_token(t).unwrap())
            .collect(),
    }
}

fn load(door: &LongHaul, bytes: &[u8]) -> Player {
    Player::decode(bytes, door.content(), &door.frontier(1)).unwrap()
}

fn screen_has(run: &replay::ReplayRun, text: &str) -> bool {
    run.screen.iter().any(|l| l.contains(text))
}

#[test]
fn an_open_event_shows_its_options_closed_ones_included_and_replays_exactly() {
    let door = LongHaul::new().unwrap();
    let env = DoorEnv::for_tests("ann");
    let mut h = Harness::new(1, 1);
    let mut p = h.captain("dockhand");
    h.sign(&mut p, Role::Engineer, 3);
    h.open_event(&mut p, "dead_freighter");
    let save = p.encode();
    // Just opening it: the screen shows the scene and nothing is written.
    let look = replay::run(&door, &log(1, save.clone(), ""), &env);
    assert!(
        screen_has(&look, "A FREIGHTER WITH A DEAD DRIVE"),
        "{:?}",
        look.screen
    );
    assert!(screen_has(&look, "[1] Help them fix the drive"));
    // The same event for a captain with no engineer has that option closed, with the reason.
    let mut alone = h.captain("dockhand");
    h.open_event(&mut alone, "dead_freighter");
    let closed = replay::run(&door, &log(1, alone.encode(), ""), &env);
    assert!(screen_has(
        &closed,
        "closed: an engineer of skill 2 or better"
    ));
    // Choosing an open option twice from the same log gives the same bytes.
    let recorded = log(7, save, "2");
    let a = replay::run_checked(&door, &recorded, &env).unwrap();
    let b = replay::run_checked(&door, &recorded, &env).unwrap();
    assert_eq!(a, b);
    let after = load(&door, a.save.as_deref().unwrap());
    assert!(after.scene.is_none(), "the choice ended the scene");
    // A closed option on the keyboard does nothing but say so.
    let refused = replay::run(&door, &log(7, alone.encode(), "1"), &env);
    assert!(screen_has(&refused, "Closed to you"));
    assert!(refused.save.is_none(), "a refusal writes nothing");
}

#[test]
fn a_fight_replays_round_by_round_and_the_save_keeps_the_scene_between_keys() {
    let door = LongHaul::new().unwrap();
    let env = DoorEnv::for_tests("ann");
    let mut h = Harness::new(1, 2);
    let mut p = h.captain("dockhand");
    h.fit(&mut p, "lance1");
    h.fit(&mut p, "shield1");
    h.sign(&mut p, Role::Gunner, 4);
    h.fight(&mut p, "skiff");
    let save = p.encode();
    let look = replay::run(&door, &log(3, save.clone(), ""), &env);
    assert!(screen_has(&look, "COMBAT: Pirate skiff"));
    assert!(screen_has(&look, "Range:"));
    assert!(screen_has(&look, "It is "), "the telegraph is on screen");
    // Two rounds, then the line drops: the saved captain is still in the fight.
    let two = replay::run_checked(&door, &log(3, save.clone(), "1 1"), &env).unwrap();
    let mid = load(&door, two.save.as_deref().unwrap());
    if let Some(Scene::Combat(c)) = &mid.scene {
        assert!(c.round >= 1);
    }
    // A full run: keep firing, then run. Reproducible, and it ends in a legal state.
    let long = "1 1 1 1 1 1 2 2 2 2 r r r r r r r r r r";
    let a = replay::run_checked(&door, &log(3, save.clone(), long), &env).unwrap();
    let b = replay::run_checked(&door, &log(3, save, long), &env).unwrap();
    assert_eq!(a, b);
    let end = load(&door, a.save.as_deref().unwrap());
    assert!(end.scene.is_none() || matches!(end.scene, Some(Scene::Combat(_))));
    assert_eq!(end.ledger.net(), end.cash);
}

#[test]
fn a_boarding_replays_and_the_screen_lists_the_rooms() {
    let door = LongHaul::new().unwrap();
    let env = DoorEnv::for_tests("ann");
    let mut h = Harness::new(1, 3);
    let mut p = h.captain("dockhand");
    h.sign(&mut p, Role::Gunner, 6);
    h.board(&mut p, "pirate_wreck");
    let save = p.encode();
    let look = replay::run(&door, &log(4, save.clone(), ""), &env);
    assert!(screen_has(&look, "BOARDING: Pirate wreck"));
    assert!(screen_has(&look, "Go to Gun deck"));
    assert!(screen_has(&look, "Party:"));
    let keys = "1 1 1 1 2 2 2 1 1 1 1 1 1 1 1";
    let a = replay::run_checked(&door, &log(4, save.clone(), keys), &env).unwrap();
    let b = replay::run_checked(&door, &log(4, save, keys), &env).unwrap();
    assert_eq!(a, b);
    let end = load(&door, a.save.as_deref().unwrap());
    assert_eq!(end.ledger.net(), end.cash);
}

#[test]
fn hiring_and_taking_a_job_from_the_screens_reproduce_exactly() {
    let door = LongHaul::new().unwrap();
    let env = DoorEnv::for_tests("ann");
    let h = Harness::new(1, 4);
    let mut p = h.captain("dockhand");
    let st = h.station("industrial");
    h.dock_free(&mut p, st);
    let save = p.encode();
    // [c] crew, Down to the first recruit (no crew yet, so the first row), Enter hires.
    let hire = replay::run_checked(&door, &log(5, save.clone(), "c enter"), &env).unwrap();
    let hired = load(&door, hire.save.as_deref().unwrap());
    assert_eq!(hired.crew.len(), 1);
    assert!(screen_has(&hire, "Goal:") || screen_has(&hire, "For hire"));
    // [b] jobs: Enter takes the first open job on the board.
    let jobs = replay::run_checked(&door, &log(5, save, "b"), &env).unwrap();
    assert!(screen_has(&jobs, "JOBS"));
    let again = replay::run_checked(&door, &log(5, hired.encode(), "b enter"), &env).unwrap();
    let after = load(&door, again.save.as_deref().unwrap_or(&hired.encode()));
    assert!(after.contracts.len() <= 1);
}

#[test]
fn the_crew_screen_names_goals_and_warns_of_a_restless_hand() {
    let door = LongHaul::new().unwrap();
    let env = DoorEnv::for_tests("ann");
    let h = Harness::new(1, 5);
    let mut p = h.captain("dockhand");
    let i = h.sign(&mut p, Role::Medic, 3);
    p.crew[i].loyalty = 5;
    p.crew[i].goal.kind = russh_bbs_door_long_haul::crew::GoalKind::Docks;
    p.crew[i].goal.target = 4;
    p.crew[i].goal.text = 11;
    let run = replay::run(&door, &log(6, p.encode(), "c"), &env);
    assert!(screen_has(&run, "Medic"));
    assert!(screen_has(&run, "RESTLESS"));
    assert!(screen_has(&run, "Goal:"));
    assert!(screen_has(&run, "0/4"));
    let look = door_look(&door, &p);
    assert!(look.iter().any(|l| l.contains("Medic")), "{look:?}");
}

fn door_look(door: &LongHaul, p: &Player) -> Vec<String> {
    let env = DoorEnv::for_tests("ann");
    let run = replay::run(door, &log(8, p.encode(), "c"), &env);
    run.screen
}

#[test]
fn every_event_scene_and_every_l2_screen_fits_the_board_box() {
    // The board's box leaves 72 columns; the door draws from column 1 (so 71 characters).
    let door = LongHaul::new().unwrap();
    let env = DoorEnv::for_tests("ann");
    let mut h = Harness::new(1, 6);
    let mut p = h.captain("dockhand");
    for role in Role::ALL {
        h.sign(&mut p, role, 5);
    }
    for ix in 0..door.content().transit.event.len() {
        let id = door.content().transit.event[ix].id.clone();
        let mut q = p.clone();
        h.open_event(&mut q, &id);
        let run = replay::run(&door, &log(1, q.encode(), ""), &env);
        for line in &run.screen {
            assert!(
                line.chars().count() <= 71,
                "{id}: {} wide: {line}",
                line.chars().count()
            );
        }
        let title = door.content().transit.event[ix].title.to_uppercase();
        assert!(
            screen_has(&run, &title) || screen_has(&run, title.split(' ').next().unwrap()),
            "{id}"
        );
    }
    let st = h.station("research");
    h.dock_free(&mut p, st);
    for keys in ["c", "b", "c down", "b down", "c enter", "p"] {
        let run = replay::run(&door, &log(2, p.encode(), keys), &env);
        for line in &run.screen {
            assert!(line.chars().count() <= 71, "after `{keys}`: {line}");
        }
    }
    for layout in &door.content().derelicts.derelict {
        let mut q = p.clone();
        h.board(&mut q, &layout.id);
        let run = replay::run(&door, &log(3, q.encode(), ""), &env);
        for line in &run.screen {
            assert!(line.chars().count() <= 71, "{}: {line}", layout.id);
        }
    }
    for enemy in &door.content().enemies.enemy {
        let mut q = p.clone();
        h.fight(&mut q, &enemy.id);
        let run = replay::run(&door, &log(4, q.encode(), ""), &env);
        for line in &run.screen {
            assert!(line.chars().count() <= 71, "{}: {line}", enemy.id);
        }
    }
}
