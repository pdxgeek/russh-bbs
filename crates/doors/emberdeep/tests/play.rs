//! Emberdeep through the framework: replay, saves, the daily tick and the screens.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::items_after_statements
)]

use std::sync::Arc;

use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::{Driver, Running};
use russh_bbs_door::replay::{self, Recorder, ReplayLog};
use russh_bbs_door::sim::{MemoryDoorStore, Simulation};
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_door::{BlobKind, Door, DoorEnv, DoorKey, Flow, MigrateError, Save, Start};
use russh_bbs_door_emberdeep::{
    DOOR_ID, Emberdeep, SAVE_VERSION, Summary, Whereabouts, summarize, summarize_world,
};

const DAY: i64 = 20_000;

fn env() -> DoorEnv {
    DoorEnv::for_tests("ann")
}

fn keys(text: &str) -> Vec<DoorKey> {
    text.split_whitespace()
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

fn summary(door: &Emberdeep, bytes: &[u8]) -> Summary {
    summarize(bytes, door.content().unwrap()).unwrap()
}

/// Make a warden: calling 1, traits a and b, 2+1+1 points into might, finesse, wits.
const CREATE_WARDEN: &str = "1 a b enter 1 1 2 3 enter";

#[test]
fn a_pinned_log_reproduces_the_same_hero() {
    let door = Emberdeep::new().unwrap();
    let text = format!("{CREATE_WARDEN} s 1 esc d");
    let run = replay::run_checked(&door, &log(7, None, &text), &env()).unwrap();
    let s = summary(&door, run.save.as_deref().unwrap());
    // Pinned: while the rules and the content stand still, this log never changes.
    assert_eq!(s.name, "ann");
    assert_eq!(s.calling, "warden");
    assert_eq!((s.level, s.nodes, s.turns), (1, 1, 24));
    assert_eq!(s.whereabouts, Whereabouts::Below(1));
    assert_eq!(
        s.max_hp, 59,
        "38 base + 14 might + 3 for level 1 + 4 from Iron Skin"
    );
    let again = replay::run_checked(&door, &log(7, None, &text), &env()).unwrap();
    assert_eq!(run, again, "the same log, the same run");
    // The text form round-trips, so a log can sit in a test file as a string.
    let l = log(7, None, &text);
    assert_eq!(l.to_string().parse::<ReplayLog>().unwrap(), l);
}

/// A long wander: every kind of key, many times. The hero may fight, climb, die or be carried
/// off by the dark; whatever happens must replay identically.
fn wander() -> String {
    let mut out = String::from(CREATE_WARDEN);
    out.push_str(" s 1 2 3 esc d");
    let pattern = "d n 1 x x x g a x e 1 x x 2 s 1 x 3 w u";
    for _ in 0..30 {
        out.push(' ');
        out.push_str(pattern);
    }
    out
}

#[test]
fn a_long_recorded_run_replays_to_exactly_the_same_save() {
    let door = Emberdeep::new().unwrap();
    // Record it live through a driver...
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(Day::from_raw(DAY).start()));
    let env = env();
    let start = Start {
        save: Save::Fresh,
        world: None,
        env: &env,
        today: Day::from_raw(DAY),
        season: 1,
    };
    let mut driver = Driver::open(&door as &dyn Door, start, 99, clock);
    let mut recorder = Recorder::new(&door, 99, Day::from_raw(DAY).start().as_raw(), 1, None);
    let mut last_save = None;
    for token in wander().split(' ') {
        let key = DoorKey::from_token(token).unwrap();
        recorder.key(key);
        let flow = driver.key(key);
        if let Some(bytes) = driver.take_effects().save() {
            last_save = Some(bytes.to_vec());
        }
        if flow == Flow::Exit {
            break;
        }
    }
    let recorded = recorder.into_log();
    // ...and replay it with no store at all.
    let replayed = replay::run_checked(&door, &recorded, &env).unwrap();
    assert_eq!(
        replayed.save, last_save,
        "a recorded run reproduces the saved state exactly"
    );
    let s = summary(&door, replayed.save.as_deref().unwrap());
    assert!(
        s.best_depth >= 1 && s.kills >= 1,
        "the wander fought: {s:?}"
    );
}

#[test]
fn splitting_a_visit_in_two_changes_nothing() {
    // Reopening a save is not a re-roll: leave in town, come back, and carry on.
    let door = Emberdeep::new().unwrap();
    let whole = replay::run(
        &door,
        &log(5, None, &format!("{CREATE_WARDEN} d u d n 1 x x")),
        &env(),
    );
    let first = replay::run(
        &door,
        &log(5, None, &format!("{CREATE_WARDEN} d u")),
        &env(),
    );
    let second = replay::run(&door, &log(1234, first.save.clone(), "d n 1 x x"), &env());
    assert_eq!(
        whole.save, second.save,
        "a different session seed makes no difference to a saved hero"
    );
}

#[test]
fn different_builds_replay_to_different_heroes_from_the_same_seed() {
    let door = Emberdeep::new().unwrap();
    let run = |create: &str| {
        let run = replay::run(&door, &log(11, None, &format!("{create} d")), &env());
        summary(&door, run.save.as_deref().unwrap())
    };
    let warden = run("1 a b enter 1 1 1 1 enter");
    let thief = run("2 a b enter 2 2 2 2 enter");
    let priest = run("3 a b enter 4 4 4 4 enter");
    assert_eq!(
        [
            warden.calling.as_str(),
            thief.calling.as_str(),
            priest.calling.as_str()
        ],
        ["warden", "cutthroat", "ember-priest"]
    );
    assert!(warden.max_hp > thief.max_hp);
    assert_ne!(warden.max_hp, priest.max_hp);
}

#[test]
fn a_damaged_save_is_never_overwritten() {
    let door = Emberdeep::new().unwrap();
    let junk = vec![1, 2, 3, 4];
    let run = replay::run(&door, &log(1, Some(junk), "x x enter esc"), &env());
    assert!(run.save.is_none(), "nothing was asked to be written");
    assert!(
        run.screen.iter().any(|l| l.contains("could not be read")),
        "{:?}",
        run.screen
    );
    assert!(run.exited);
}

#[test]
fn a_new_season_retires_the_old_hero_and_starts_a_new_one() {
    let door = Emberdeep::new().unwrap();
    let made = replay::run(&door, &log(1, None, CREATE_WARDEN), &env());
    let bytes = made.save.unwrap();
    let env = env();
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(Day::from_raw(DAY).start()));
    // The same bytes, written in season 1, opened by a board that is now in season 2.
    let start = Start {
        save: Save::Loaded {
            bytes: &bytes,
            season: 1,
        },
        world: None,
        env: &env,
        today: Day::from_raw(DAY),
        season: 2,
    };
    let driver = Driver::open(&door as &dyn Door, start, 1, clock);
    let screen = driver.look_around(&env);
    assert!(
        screen.iter().any(|l| l.contains("A new season has begun")),
        "{screen:?}"
    );
    assert!(screen.iter().any(|l| l.contains("Choose your calling")));
}

#[test]
fn the_door_has_a_tick_and_no_older_formats_yet() {
    let door = Emberdeep::new().unwrap();
    assert!(door.has_daily_tick());
    assert_eq!(door.info().save_version, SAVE_VERSION);
    assert_eq!(
        door.migrate(BlobKind::Save, 0, &[]),
        Err(MigrateError::Unsupported(0))
    );
}

async fn world_of(
    store: &MemoryDoorStore,
    id: &russh_bbs_door::DoorId,
) -> russh_bbs_door_emberdeep::WorldSummary {
    summarize_world(&store.load_world(id).await.unwrap().unwrap().bytes).unwrap()
}

#[tokio::test]
async fn the_nightly_tick_runs_once_a_day_and_catches_up_after_downtime() {
    let door = Emberdeep::shared().unwrap();
    let id = door.info().id.clone();
    let store = MemoryDoorStore::new();
    let clock = ManualClock::new(Day::from_raw(DAY).start());
    store.set_now(clock.now());
    let policy = TickPolicy::default();
    let first = run_due(&store, door.as_ref(), &clock, policy)
        .await
        .unwrap();
    assert_eq!(first.applied.len(), 1, "the first run applies today only");
    assert_eq!(world_of(&store, &id).await.day, DAY);
    let again = run_due(&store, door.as_ref(), &clock, policy)
        .await
        .unwrap();
    assert!(again.applied.is_empty(), "idempotent within a day");
    clock.set(Day::from_raw(DAY + 4).start());
    store.set_now(clock.now());
    let catch_up = run_due(&store, door.as_ref(), &clock, policy)
        .await
        .unwrap();
    assert_eq!(
        catch_up.applied.len(),
        4,
        "four missed days replayed oldest first"
    );
    assert_eq!(world_of(&store, &id).await.day, DAY + 4);
}

#[tokio::test]
async fn a_hero_below_wakes_to_a_hard_morning_when_the_day_turns() {
    // Through the real save path: descend, leave the door, come back tomorrow.
    let door = Emberdeep::shared().unwrap();
    let store = MemoryDoorStore::new();
    let clock = Arc::new(ManualClock::new(Day::from_raw(DAY).start()));
    let dyn_clock: Arc<dyn Clock> = clock.clone();
    store.set_now(clock.now());
    let env = env();
    let account = AccountId::from_raw(1);
    let mut running = Running::open(
        &store,
        Arc::clone(&door),
        account,
        &env,
        Arc::clone(&dyn_clock),
        3,
    )
    .await
    .unwrap();
    for token in format!("{CREATE_WARDEN} d").split(' ') {
        let _ = running.key(DoorKey::from_token(token).unwrap());
        running.flush(&store).await.unwrap();
    }
    let _ = running.key(DoorKey::Escape);
    running.flush(&store).await.unwrap();
    let id = door.info().id.clone();
    let content = Emberdeep::new().unwrap();
    let before = summarize(
        &store.load_save(&id, account).await.unwrap().unwrap().bytes,
        content.content().unwrap(),
    )
    .unwrap();
    assert_eq!(before.whereabouts, Whereabouts::Below(1));
    // A day later.
    clock.set(Day::from_raw(DAY + 1).start());
    store.set_now(clock.now());
    let mut running = Running::open(&store, Arc::clone(&door), account, &env, dyn_clock, 4)
        .await
        .unwrap();
    let screen = running.look_around(&env);
    assert!(
        screen.iter().any(|l| l.contains("spent the night below")),
        "{screen:?}"
    );
    let _ = running.key(DoorKey::Char('u'));
    running.flush(&store).await.unwrap();
    let after = summarize(
        &store.load_save(&id, account).await.unwrap().unwrap().bytes,
        content.content().unwrap(),
    )
    .unwrap();
    assert_eq!(after.whereabouts, Whereabouts::Town);
    assert!(
        after.turns < 25,
        "the torches burned keeping watch: {}",
        after.turns
    );
}

#[test]
fn every_screen_fits_eighty_by_twenty_four_in_plain_ascii() {
    let door = Emberdeep::new().unwrap();
    let env = env();
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(Day::from_raw(DAY).start()));
    let start = Start {
        save: Save::Fresh,
        world: None,
        env: &env,
        today: Day::from_raw(DAY),
        season: 1,
    };
    let mut driver = Driver::open(&door as &dyn Door, start, 21, clock);
    let script = format!("{CREATE_WARDEN} s esc h esc r esc d m esc c esc 1 x x x");
    for token in script.split(' ') {
        driver.key(DoorKey::from_token(token).unwrap());
        for row in replay::draw_to_text(&driver, &env, 80, 24) {
            assert!(row.chars().count() <= 80, "too wide: {row:?}");
            assert!(
                row.chars().all(|c| c == ' ' || c.is_ascii_graphic()),
                "not ASCII: {row:?}"
            );
        }
        for line in driver.look_around(&env) {
            assert!(
                line.chars().all(|c| c == ' ' || c.is_ascii_graphic()),
                "{line:?}"
            );
            assert!(
                line.chars().count() <= 74,
                "too wide for the board's box: {line:?}"
            );
        }
    }
}

#[tokio::test]
async fn the_same_walk_by_two_players_shares_one_dungeon_and_one_world() {
    // Two players, one door, thirty days: both live in the same seeded dungeon, so the town's
    // shortcut list is the same for each, and the board's world holds what either changed.
    let door = Emberdeep::shared().unwrap();
    let mut sim = Simulation::new(door, Day::from_raw(DAY), 8);
    struct Walk(Vec<DoorKey>);
    impl russh_bbs_door::sim::Bot for Walk {
        fn next_key(&mut self, _s: &[String], _v: u32, k: u32) -> Option<DoorKey> {
            self.0.get(usize::try_from(k).unwrap()).copied()
        }
    }
    let script: Vec<DoorKey> = format!("{CREATE_WARDEN} d u")
        .split(' ')
        .map(|t| DoorKey::from_token(t).unwrap())
        .collect();
    sim.add_player(Box::new(Walk(script.clone())));
    sim.add_player(Box::new(Walk(script)));
    let report = sim.run_days(1).await.unwrap();
    assert_eq!(report.visits, 2);
    assert_eq!(
        report.saves,
        2 * 3,
        "creating, descending and climbing each saved"
    );
}
