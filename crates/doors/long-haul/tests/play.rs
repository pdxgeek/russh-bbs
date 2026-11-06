//! The door through the framework: replay, saves, the daily tick, the headless run and the
//! sandbox audit.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::assert_is_empty,
    clippy::similar_names
)]

use std::sync::Arc;

use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::replay::{self, ReplayLog};
use russh_bbs_door::sandbox;
use russh_bbs_door::sim::{Bot, MemoryDoorStore, Simulation};
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_door::{Door, DoorEnv, DoorKey, Flow};
use russh_bbs_door_long_haul::player::Player;
use russh_bbs_door_long_haul::world::World;
use russh_bbs_door_long_haul::{DOOR_ID, LongHaul};

const DAY: i64 = 20_000;

fn door() -> LongHaul {
    LongHaul::new().unwrap()
}

fn keys(text: &str) -> Vec<DoorKey> {
    text.split(' ')
        .filter(|t| !t.is_empty())
        .map(|t| DoorKey::from_token(t).unwrap())
        .collect()
}

fn log(seed: u64, save: Option<Vec<u8>>, text: &str) -> ReplayLog {
    ReplayLog {
        door: DOOR_ID.to_owned(),
        seed,
        start: Day::from_raw(DAY).start().as_raw(),
        season: 1,
        save,
        keys: keys(text),
    }
}

fn player_of(door: &LongHaul, bytes: &[u8]) -> Player {
    let frontier = door.frontier(1);
    Player::decode(bytes, door.content(), &frontier).unwrap()
}

#[test]
fn the_source_and_manifest_pass_the_framework_audit() {
    assert_eq!(sandbox::audit_crate(env!("CARGO_MANIFEST_DIR")), Ok(()));
}

/// A first visit: pick a captain, open the market, buy a hold of whatever is on the first line,
/// leave, refuel, and look at the lanes. The same log must give the same bytes.
const TRADE_RUN: &str = "enter m a esc f j enter";

#[test]
fn a_recorded_run_reproduces_the_saved_state_exactly() {
    let door = door();
    let env = DoorEnv::for_tests("ann");
    let recorded = log(42, None, TRADE_RUN);
    let first = replay::run_checked(&door, &recorded, &env).unwrap();
    let again = replay::run_checked(&door, &recorded, &env).unwrap();
    assert_eq!(first, again, "the same log gives the same run");
    let player = player_of(&door, first.save.as_deref().unwrap());
    assert!(player.stats.jumps == 1 || player.stats.tons_traded > 0);
    assert_eq!(
        player.ledger.net(),
        player.cash,
        "every credit is on the ledger"
    );
    // The text form round-trips, so a log can sit in a test as a string.
    let parsed: ReplayLog = recorded.to_string().parse().unwrap();
    assert_eq!(parsed, recorded);
    // A log for another door is refused.
    let mut other = recorded.clone();
    other.door = "coin-toss".to_owned();
    assert!(replay::run_checked(&door, &other, &env).is_err());
}

#[test]
fn a_saved_captain_loads_and_play_continues_from_it() {
    let door = door();
    let env = DoorEnv::for_tests("ann");
    let first = replay::run(&door, &log(1, None, "enter m a esc"), &env);
    let save = first.save.unwrap();
    let before = player_of(&door, &save);
    assert!(
        before.ship.cargo_tons() > 0,
        "the first line of the market was bought"
    );
    // Second visit, same day, loaded from the save: the hold is still full, nothing re-rolled.
    let second = replay::run(&door, &log(2, Some(save.clone()), "q"), &env);
    assert!(second.exited);
    assert!(
        second.save.is_none(),
        "leaving changes nothing, so nothing is written"
    );
    assert!(second.screen.iter().any(|l| l.contains("Hold:")));
}

#[test]
fn a_save_this_build_cannot_read_is_never_overwritten() {
    let door = door();
    let env = DoorEnv::for_tests("ann");
    let run = replay::run(&door, &log(1, Some(vec![1, 2, 3]), "enter m a esc q"), &env);
    assert!(
        run.save.is_none(),
        "nothing may be written over an unreadable save"
    );
    assert!(run.exited, "any key leaves");
    assert!(run.screen.iter().any(|l| l.contains("cannot read")));
}

#[test]
fn a_new_day_gives_the_drive_allowance_back_and_charges_interest() {
    let door = door();
    let env = DoorEnv::for_tests("ann");
    let day1 = replay::run(&door, &log(3, None, "enter m a esc f"), &env);
    let save = day1.save.unwrap();
    let p1 = player_of(&door, &save);
    let mut next = log(4, Some(save), "n esc");
    next.start = Day::from_raw(DAY + 3).start().as_raw();
    let after = replay::run(&door, &next, &env);
    // Opening on a later day settles it; the next save (any change) carries the new day.
    let mut next2 = next.clone();
    next2.keys = keys("l b esc");
    let after2 = replay::run(&door, &next2, &env);
    let p2 = player_of(&door, after2.save.as_deref().unwrap());
    assert_eq!(p2.last_day, DAY + 3);
    assert_eq!(p2.days_left, 10);
    assert!(
        p2.debt.principal > p1.debt.principal,
        "three nights of interest"
    );
    assert!(
        after
            .screen
            .iter()
            .any(|l| l.contains("Drive-days left: 10"))
    );
}

#[tokio::test]
async fn the_nightly_tick_moves_markets_idempotently_and_catches_up_with_a_cap() {
    let door = Arc::new(door());
    let store = MemoryDoorStore::new();
    let clock = Arc::new(ManualClock::new(Day::from_raw(DAY).start()));
    let policy = TickPolicy::default();
    let report = run_due(&store, door.as_ref(), clock.as_ref(), policy)
        .await
        .unwrap();
    assert_eq!(report.applied.len(), 1, "the first run applies only today");
    let stored = store.load_world(&door.info().id).await.unwrap().unwrap();
    let frontier = door.frontier(1);
    let world = World::decode(&stored.bytes, door.content(), &frontier).unwrap();
    assert_eq!(world.day, DAY);
    // Same day again: nothing to do.
    let again = run_due(&store, door.as_ref(), clock.as_ref(), policy)
        .await
        .unwrap();
    assert!(again.applied.is_empty());
    // Ten days of downtime: seven days applied, three skipped.
    clock.set(Day::from_raw(DAY + 10).start());
    let late = run_due(&store, door.as_ref(), clock.as_ref(), policy)
        .await
        .unwrap();
    assert_eq!(late.applied.len(), 7);
    assert_eq!(late.skipped, 3);
    let stored2 = store.load_world(&door.info().id).await.unwrap().unwrap();
    let world2 = World::decode(&stored2.bytes, door.content(), &frontier).unwrap();
    assert_eq!(world2.day, DAY + 10);
    assert_ne!(world.stock, world2.stock, "stock moved over the week");
    assert!(stored2.bytes.len() < 256 * 1024, "the world fits its cap");
}

#[tokio::test]
async fn the_tick_is_a_pure_function_of_the_world_and_the_day() {
    let make = || async {
        let door = Arc::new(door());
        let store = MemoryDoorStore::new();
        let clock = Arc::new(ManualClock::new(Day::from_raw(DAY).start()));
        for d in 0..5 {
            clock.set(Day::from_raw(DAY + d).start());
            run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
                .await
                .unwrap();
        }
        store
            .load_world(&door.info().id)
            .await
            .unwrap()
            .unwrap()
            .bytes
    };
    assert_eq!(make().await, make().await);
}

#[tokio::test]
async fn a_trade_through_the_real_save_path_changes_the_shared_world() {
    let door: Arc<dyn Door> = Arc::new(door());
    let store = MemoryDoorStore::new();
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(Day::from_raw(DAY).start()));
    let env = DoorEnv::for_tests("ann");
    let account = AccountId::from_raw(1);
    let mut running = Running::open(
        &store,
        Arc::clone(&door),
        account,
        &env,
        Arc::clone(&clock),
        5,
    )
    .await
    .unwrap();
    for key in keys("enter m a esc") {
        assert_eq!(running.key(key), Flow::Stay);
        running.flush(&store).await.unwrap();
    }
    let world = store.load_world(&door.info().id).await.unwrap().unwrap();
    assert!(world.bytes.len() < 256 * 1024);
    let save = store
        .load_save(&door.info().id, account)
        .await
        .unwrap()
        .unwrap();
    assert!(save.bytes.len() < 64 * 1024);
    // A second captain sees the first one's purchase in the stock they open with.
    let mut other = Running::open(
        &store,
        Arc::clone(&door),
        AccountId::from_raw(2),
        &env,
        Arc::clone(&clock),
        6,
    )
    .await
    .unwrap();
    for key in keys("enter m") {
        other.key(key);
    }
    let seen = other.look_around(&env).join("\n");
    assert!(seen.contains("Stock"), "{seen}");
}

/// A bot that reads the look-around text: it signs on, buys a hold, sells it where it can, and
/// goes round again. It exists to run the real open/flush/tick path for many days.
struct Hauler {
    step: u32,
}

impl Bot for Hauler {
    fn next_key(&mut self, screen: &[String], _visit: u32, sent: u32) -> Option<DoorKey> {
        let text = screen.join("\n");
        if sent > 40 {
            return None;
        }
        self.step += 1;
        if text.contains("Who were you") {
            return Some(DoorKey::Enter);
        }
        // An open scene (event, fight, boarding) takes numbered choices; keep picking until it ends.
        if text.contains("[1] ") {
            let n = 1 + self.step % 3;
            return Some(DoorKey::Char(char::from_digit(n, 10).unwrap_or('1')));
        }
        if text.contains("Stock") {
            return Some(match self.step % 3 {
                0 => DoorKey::Char('z'),
                1 => DoorKey::Char('a'),
                _ => DoorKey::Down,
            });
        }
        Some(match self.step % 4 {
            0 => DoorKey::Char('m'),
            1 => DoorKey::Char('f'),
            2 => DoorKey::Char('j'),
            _ => DoorKey::Enter,
        })
    }
}

#[tokio::test]
async fn a_headless_run_of_bots_keeps_every_save_and_the_world_inside_their_caps() {
    let door: Arc<dyn Door> = Arc::new(door());
    let mut sim = Simulation::new(Arc::clone(&door), Day::from_raw(DAY), 11);
    for _ in 0..4 {
        sim.add_player(Box::new(Hauler { step: 0 }));
    }
    for _ in 0..12 {
        sim.play_day().await.unwrap();
        sim.advance_day().await.unwrap();
    }
    let report = sim.report();
    assert_eq!(report.days, 12);
    assert!(report.saves > 20 && report.ticks >= 12);
    assert!(
        report.largest_save < 64 * 1024,
        "largest save {}",
        report.largest_save
    );
    let _: &MemoryDoorStore = sim.store();
}

#[test]
fn every_screen_fits_the_eighty_column_box_and_shows_its_essentials() {
    // The board's box leaves 72 columns inside; the door draws from column 1.
    let door = door();
    let env = DoorEnv::for_tests("ann");
    for keys_text in [
        "", "enter", "enter j", "enter y", "enter l", "enter n", "enter m", "enter d",
    ] {
        let run = replay::run(&door, &log(9, None, keys_text.trim()), &env);
        for line in &run.screen {
            assert!(
                line.chars().count() <= 71,
                "after `{keys_text}` a line is {} wide: {line}",
                line.chars().count()
            );
        }
    }
    // The bridge says what the station wants, so a captain knows where to carry what.
    let run = replay::run(&door, &log(9, None, "enter"), &env);
    let text = run.screen.join("\n");
    assert!(text.contains("They want"), "{text}");
    assert!(text.contains("[m] market"), "{text}");
}
