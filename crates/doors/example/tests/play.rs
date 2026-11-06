//! The example door through the framework: replay, saves, migration, the daily tick, the shared
//! world and the headless balance run.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

use std::sync::Arc;

use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::replay::{self, Recorder, ReplayLog};
use russh_bbs_door::sim::{Bot, MemoryDoorStore, Simulation};
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_door::{Door, DoorEnv, DoorKey, Flow};
use russh_bbs_door_example::engine::{DAILY_TOSSES, RELIEF_FLOOR, START_COINS};
use russh_bbs_door_example::save;
use russh_bbs_door_example::world::World;
use russh_bbs_door_example::{CoinToss, DOOR_ID};

const DAY: i64 = 20_000;

fn keys(text: &str) -> Vec<DoorKey> {
    text.chars().map(DoorKey::Char).collect()
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

fn env() -> DoorEnv {
    DoorEnv::for_tests("ann")
}

fn clock(day: i64) -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Day::from_raw(day).start()))
}

#[test]
fn a_recorded_run_reproduces_the_saved_state_exactly() {
    let door = CoinToss::new().unwrap();
    let recorded = log(42, None, "hththhtq");
    let first = replay::run_checked(&door, &recorded, &env()).unwrap();
    let again = replay::run_checked(&door, &recorded, &env()).unwrap();
    assert_eq!(first, again, "the same log gives the same run");
    assert!(first.exited);
    let state = save::decode(first.save.as_deref().unwrap()).unwrap();
    // 5 tosses a day, so three of the seven calls after the fifth were refused.
    assert_eq!(state.left, 0);
    assert_eq!(first.world_edits, usize::from(DAILY_TOSSES));
    // A different seed is a different game (the coin has a side to land on).
    let other = replay::run(&door, &log(43, None, "hththhtq"), &env());
    assert_ne!(first.save, other.save);
    // A log made for another door is refused.
    let mut wrong = recorded;
    wrong.door = "other".into();
    assert!(replay::run_checked(&door, &wrong, &env()).is_err());
}

#[test]
fn the_replay_text_form_pins_a_whole_branch_of_play() {
    let door = CoinToss::new().unwrap();
    let text = "door=coin-toss seed=42 start=1728000000 season=1 save=- keys=h t h t h q";
    let parsed: ReplayLog = text.parse().unwrap();
    assert_eq!(parsed.to_string(), text);
    let run = replay::run_checked(&door, &parsed, &env()).unwrap();
    let state = save::decode(run.save.as_deref().unwrap()).unwrap();
    assert_eq!((state.left, state.day), (0, 20_000));
    // Pinned from this seed: the outcome never changes while the RNG and the rules stand still.
    assert_eq!(
        (state.coins, state.streak, state.best),
        (PINNED_COINS, PINNED_STREAK, PINNED_BEST)
    );
}

const PINNED_COINS: u32 = 4;
const PINNED_STREAK: u16 = 0;
const PINNED_BEST: u16 = 1;

#[test]
fn a_live_session_and_its_recording_agree() {
    let door = CoinToss::new().unwrap();
    let mut recorder = Recorder::new(&door, 7, Day::from_raw(DAY).start().as_raw(), 1, None);
    let mut driver = russh_bbs_door::host::Driver::open(
        &door,
        russh_bbs_door::Start {
            save: russh_bbs_door::Save::Fresh,
            world: None,
            env: &env(),
            today: Day::from_raw(DAY),
            season: 1,
        },
        7,
        clock(DAY),
    );
    let mut last = None;
    for key in keys("hhtth") {
        recorder.key(key);
        driver.key(key);
        let effects = driver.take_effects();
        if let Some(bytes) = effects.save() {
            last = Some(bytes.to_vec());
        }
    }
    let replayed = replay::run(&door, &recorder.into_log(), &env());
    assert_eq!(replayed.save, last);
}

#[tokio::test]
async fn it_saves_and_loads_through_the_store_and_the_allowance_follows_the_day() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = CoinToss::shared().unwrap();
    let clock = clock(DAY);
    let ann = AccountId::from_raw(1);
    let env = env();
    let open = |seed| {
        let clock: Arc<dyn Clock> = clock.clone();
        Running::open(&store, door.clone(), ann, &env, clock, seed)
    };
    let mut run = open(1).await.unwrap();
    for key in keys("hhhhh") {
        run.key(key);
        run.flush(&store).await.unwrap();
    }
    // The sixth call is refused and writes nothing.
    run.key(DoorKey::Char('h'));
    assert!(!run.has_pending());
    assert_eq!(run.key(DoorKey::Char('q')), Flow::Exit);
    let stored = store
        .load_save(&door.info().id, ann)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.revision, Revision::from_raw(5));
    assert_eq!(save::decode(&stored.bytes).unwrap().left, 0);

    // The same day, a new visit: still no tosses.
    let again = open(2).await.unwrap();
    assert!(
        again
            .look_around(&env)
            .join("\n")
            .contains("Tosses left today: 0")
    );
    // Next day: five more, and the relief floor applies.
    clock.set(Day::from_raw(DAY + 1).start());
    let next = open(3).await.unwrap();
    assert!(
        next.look_around(&env)
            .join("\n")
            .contains("Tosses left today: 5")
    );
    // Going back in time does not refill it.
    clock.set(Day::from_raw(DAY - 3).start());
    let past = open(4).await.unwrap();
    assert!(
        past.look_around(&env)
            .join("\n")
            .contains("Tosses left today: 0")
    );
}

#[tokio::test]
async fn a_version_one_save_is_migrated_when_opened() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = CoinToss::shared().unwrap();
    let ann = AccountId::from_raw(1);
    let mut v1 = Vec::new();
    v1.extend_from_slice(&9u32.to_le_bytes());
    v1.push(4);
    v1.extend_from_slice(&DAY.to_le_bytes());
    store
        .write_save(
            &door.info().id,
            ann,
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: 1,
                bytes: &v1,
            },
        )
        .await
        .unwrap();
    let clock: Arc<dyn Clock> = clock(DAY);
    let run = Running::open(&store, door.clone(), ann, &env(), clock, 1)
        .await
        .unwrap();
    let screen = run.look_around(&env()).join("\n");
    assert!(
        screen.contains("Coins: 9") && screen.contains("Tosses left today: 4"),
        "{screen}"
    );
}

#[tokio::test]
async fn the_daily_tick_rolls_the_shared_world_and_sessions_feed_it() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = CoinToss::shared().unwrap();
    let clock = clock(DAY);
    run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    let id = door.info().id.clone();
    let dyn_clock: Arc<dyn Clock> = clock.clone();
    let mut run = Running::open(
        &store,
        door.clone(),
        AccountId::from_raw(1),
        &env(),
        dyn_clock,
        5,
    )
    .await
    .unwrap();
    for key in keys("hhh") {
        run.key(key);
        run.flush(&store).await.unwrap();
    }
    let world = World::decode(&store.load_world(&id).await.unwrap().unwrap().bytes).unwrap();
    assert_eq!((world.days, world.today, world.yesterday), (1, 3, 0));
    clock.set(Day::from_raw(DAY + 1).start());
    run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    let world = World::decode(&store.load_world(&id).await.unwrap().unwrap().bytes).unwrap();
    assert_eq!((world.days, world.today, world.yesterday), (2, 0, 3));
    // Idempotent: again the same day changes nothing.
    let report = run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.applied.len(), 0);
}

/// Tosses heads until the day's allowance or the purse is spent, then leaves.
struct Greedy;

impl Bot for Greedy {
    fn next_key(&mut self, screen: &[String], _visit: u32, sent: u32) -> Option<DoorKey> {
        let done = screen
            .iter()
            .any(|line| line.contains("Tosses left today: 0") || line.contains("Coins: 0 "));
        match (done, sent) {
            (true, _) => Some(DoorKey::Char('q')),
            (false, n) if n < 20 => Some(DoorKey::Char('h')),
            _ => None,
        }
    }
}

#[tokio::test]
async fn the_headless_harness_plays_a_season_and_the_numbers_make_sense() {
    let mut sim = Simulation::new(CoinToss::shared().unwrap(), Day::from_raw(DAY), 2024);
    for _ in 0..4 {
        sim.add_player(Box::new(Greedy));
    }
    let report = sim.run_days(100).await.unwrap();
    assert_eq!(report.visits, 400);
    assert_eq!(
        report.exits, 400,
        "every visit ended by leaving, none cut off"
    );
    assert_eq!(report.cut_off, 0);
    let id = CoinToss::new().unwrap().info().id.clone();
    let mut total_best = 0u32;
    for n in 1..=4 {
        let stored = sim
            .store()
            .load_save(&id, AccountId::from_raw(n))
            .await
            .unwrap()
            .unwrap();
        let state = save::decode(&stored.bytes).unwrap();
        assert!(state.coins >= RELIEF_FLOOR.min(START_COINS) || state.coins > 0 || state.left == 0);
        assert!(
            state.best >= 3,
            "100 days of tosses show a streak: {}",
            state.best
        );
        total_best += u32::from(state.best);
    }
    assert!(total_best > 0);
    // The board counted every toss each player made: 5 a day for 100 days, 4 players.
    let world = World::decode(&sim.store().load_world(&id).await.unwrap().unwrap().bytes).unwrap();
    assert_eq!(world.days, 101);
    assert_eq!(world.yesterday, 20, "four players, five tosses");
}

#[test]
fn a_question_mark_typed_into_the_name_prompt_is_text_not_the_look_around_view() {
    use russh_bbs_door::host::Driver;
    use russh_bbs_door::{Save, Start};
    let door = CoinToss::new().unwrap();
    let start = Start {
        save: Save::Fresh,
        world: None,
        env: &env(),
        today: Day::from_raw(DAY),
        season: 1,
    };
    let mut driver = Driver::open(&door, start, 1, clock(DAY));
    driver.key(DoorKey::Char('n'));
    driver.key(DoorKey::Char('?'));
    assert!(!driver.is_looking());
    assert!(driver.look_around(&env()).join("\n").contains('?'));
}
