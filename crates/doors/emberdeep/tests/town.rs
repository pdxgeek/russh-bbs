//! Emberdeep's town through the framework: migration of E1 data, a recorded walk through every
//! place, the screens at 80x24, and the town shared between players through the store.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::items_after_statements
)]

use std::sync::Arc;

use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite, WorldWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::{Driver, Running};
use russh_bbs_door::replay::{self, Recorder};
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::{BlobKind, Door, DoorEnv, DoorKey, Flow, Save, Start};
use russh_bbs_door_emberdeep::{
    Emberdeep, Whereabouts, summarize, summarize_town, summarize_world,
};

const DAY: i64 = 20_000;

/// A hero and a world written by the E1 engine (a runesmith with 137 coin and 42 salvage in the
/// store, a lift on level 3), pinned as bytes.
const V1_HERO: &str = "065465737465720972756e65736d6974680b090d090382000000010038000000001900000089000000000000002a000000020000020c72756e652d746f75636865640573746f757401046c696d70010467617368030b72756e652d68616d6d65720b736d6974682d6170726f6e00000000000000e320a0cb6c6f4fd7b2d4e4215868db5b2daaf8471779e65c7503c2a19a45d3b20000204e0000000000000000010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
const V1_WORLD: &str = "010000000000204e000000000000010300000001034164610400021d416461206f70656e65642061206c696674206f6e206c6576656c20332e22426f20646964206e6f7420636f6d65206261636b2066726f6d206c6576656c20322e";

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn env() -> DoorEnv {
    DoorEnv::for_tests("ann")
}

fn clock(day: i64) -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Day::from_raw(day).start()))
}

fn migrated_hero(door: &Emberdeep) -> Vec<u8> {
    door.migrate(BlobKind::Save, 1, &unhex(V1_HERO)).unwrap()
}

/// Press keys on a running door, flushing after each as the board does.
async fn press(running: &mut Running, store: &MemoryDoorStore, text: &str) {
    for token in text.split_whitespace() {
        let _ = running.key(DoorKey::from_token(token).unwrap());
        running.flush(store).await.unwrap();
    }
}

#[tokio::test]
async fn an_e1_hero_and_world_open_through_the_host_and_are_saved_as_version_two() {
    let door = Emberdeep::shared().unwrap();
    let store = MemoryDoorStore::new();
    let id = door.info().id.clone();
    let ann = AccountId::from_raw(1);
    store
        .write_save(
            &id,
            ann,
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: 1,
                bytes: &unhex(V1_HERO),
            },
        )
        .await
        .unwrap();
    store
        .write_world(
            &id,
            &WorldWrite {
                expected: Revision::NONE,
                version: 1,
                bytes: &unhex(V1_WORLD),
                tick: None,
            },
        )
        .await
        .unwrap();
    let clock: Arc<dyn Clock> = clock(DAY);
    let env = env();
    let mut running = Running::open(&store, Arc::clone(&door), ann, &env, clock, 5)
        .await
        .unwrap();
    let screen = running.look_around(&env).join("\n");
    assert!(screen.contains("Tester L3 Runesmith"), "{screen}");
    assert!(screen.contains("Coin 137  Salvage 0  Stash 42  Relics 2"), "{screen}");
    // The E1 world's lift on level 3 is still on offer.
    assert!(screen.contains("lift down to level 3"), "{screen}");
    // The calling's recipe is there at once: forge it, which saves.
    press(&mut running, &store, "f 1 esc").await;
    let stored = store.load_save(&id, ann).await.unwrap().unwrap();
    assert_eq!(stored.version, 2, "the next write is version 2");
    let content = Emberdeep::new().unwrap();
    let s = summarize(&stored.bytes, content.content().unwrap()).unwrap();
    assert_eq!(s.whereabouts, Whereabouts::Town);
    assert!(s.coin < 137 && s.stash < 42, "{s:?}");
}

/// A walk through every place with the migrated hero.
const WALK: &str = "m 1 esc i 1 esc f 1 esc b 4 4 esc w 1 2 esc t 1 esc g 1 esc l 1 esc s esc h esc d u";

#[test]
fn a_recorded_walk_through_every_place_replays_to_exactly_the_same_save() {
    let door = Emberdeep::new().unwrap();
    let start = Day::from_raw(DAY).start().as_raw();
    let save = migrated_hero(&door);
    let clk: Arc<dyn Clock> = clock(DAY);
    let env = env();
    let begin = Start {
        save: Save::Loaded {
            bytes: &save,
            season: 1,
        },
        world: None,
        env: &env,
        today: Day::from_raw(DAY),
        season: 1,
    };
    let mut driver = Driver::open(&door as &dyn Door, begin, 31, clk);
    let mut recorder = Recorder::new(&door, 31, start, 1, Some(save.clone()));
    let mut last = None;
    for token in WALK.split(' ') {
        let key = DoorKey::from_token(token).unwrap();
        recorder.key(key);
        let flow = driver.key(key);
        let fx = driver.take_effects();
        if let Some(bytes) = fx.save() {
            last = Some(bytes.to_vec());
        }
        assert_ne!(flow, Flow::Exit, "at {token}");
    }
    let log = recorder.into_log();
    let replayed = replay::run_checked(&door, &log, &env).unwrap();
    assert_eq!(replayed.save, last, "the walk reproduces the saved state exactly");
    assert!(replayed.world_edits >= 1, "the gift reached the world");
    // And it did what it said: spent coin and salvage, made something, gave to the works.
    let s = summarize(replayed.save.as_deref().unwrap(), door.content().unwrap()).unwrap();
    assert!(s.coin < 137, "coin was spent: {s:?}");
    assert!(s.stash < 42, "salvage was spent: {s:?}");
    assert_eq!(s.whereabouts, Whereabouts::Town, "down and up again");
    // The text form is a test fixture.
    assert!(log.to_string().parse::<replay::ReplayLog>().is_ok());
}

#[test]
fn every_place_fits_the_board_box_in_plain_ascii_with_a_full_purse() {
    let door = Emberdeep::new().unwrap();
    let save = migrated_hero(&door);
    let env = env();
    let clk: Arc<dyn Clock> = clock(DAY);
    let begin = Start {
        save: Save::Loaded {
            bytes: &save,
            season: 1,
        },
        world: None,
        env: &env,
        today: Day::from_raw(DAY),
        season: 1,
    };
    let mut driver = Driver::open(&door as &dyn Door, begin, 2, clk);
    for token in "m esc i esc f esc b esc t esc g esc l esc w esc".split(' ') {
        driver.key(DoorKey::from_token(token).unwrap());
        for row in replay::draw_to_text(&driver, &env, 80, 24) {
            assert!(row.chars().count() <= 80, "too wide: {row:?}");
            assert!(row.chars().all(|c| c == ' ' || c.is_ascii_graphic()), "{row:?}");
        }
        let lines = driver.look_around(&env);
        for line in &lines {
            assert!(line.chars().all(|c| c == ' ' || c.is_ascii_graphic()), "{line:?}");
            assert!(line.chars().count() <= 74, "too wide for the box: {line:?}");
        }
        assert!(lines.len() <= 24, "{token}: {} lines do not fit a screen:\n{}", lines.len(), lines.join("\n"));
    }
}

#[tokio::test]
async fn a_finished_project_is_seen_by_the_next_player_and_never_counted_twice() {
    let door = Emberdeep::shared().unwrap();
    let content = Emberdeep::new().unwrap();
    let store = MemoryDoorStore::new();
    let id = door.info().id.clone();
    let (ann, bea) = (AccountId::from_raw(1), AccountId::from_raw(2));
    store
        .write_save(
            &id,
            ann,
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: 2,
                bytes: &migrated_hero(&content),
            },
        )
        .await
        .unwrap();
    let clk: Arc<dyn Clock> = clock(DAY);
    let (env_a, env_b) = (DoorEnv::for_tests("Tester"), DoorEnv::for_tests("bea"));
    // Ann (137 coin, 42 salvage) pays for the new bellows: 120 coin and 40 salvage.
    let mut a = Running::open(&store, Arc::clone(&door), ann, &env_a, Arc::clone(&clk), 1)
        .await
        .unwrap();
    press(&mut a, &store, "w 1 1 1 1 1 2 2").await;
    let screen = a.look_around(&env_a).join("\n");
    assert!(screen.contains("1 of 11 finished"), "{screen}");
    let town = |bytes: &[u8]| summarize_town(bytes, content.content().unwrap()).unwrap();
    let w = store.load_world(&id).await.unwrap().unwrap();
    assert_eq!(town(&w.bytes).done, vec!["new-bellows".to_owned()]);
    assert_eq!(town(&w.bytes).levels[0], 2, "the forge is level 2");
    assert_eq!(
        summarize_world(&w.bytes).unwrap().pledged,
        vec![("new-bellows".to_owned(), 120, 40)]
    );
    let hero = summarize(&store.load_save(&id, ann).await.unwrap().unwrap().bytes, content.content().unwrap()).unwrap();
    assert_eq!((hero.coin, hero.stash), (17, 2), "exactly what was given left the purse");
    // Ann leaves and returns twice; each open tells the world again what she gave. Nothing doubles.
    for seed in [2, 3] {
        let mut again = Running::open(&store, Arc::clone(&door), ann, &env_a, Arc::clone(&clk), seed)
            .await
            .unwrap();
        press(&mut again, &store, "w esc").await;
    }
    let w2 = store.load_world(&id).await.unwrap().unwrap();
    assert_eq!(
        summarize_world(&w2.bytes).unwrap().pledged,
        vec![("new-bellows".to_owned(), 120, 40)]
    );
    // Bea, a stranger, makes a hero and walks into the forge: the town is different for her too.
    let mut b = Running::open(&store, Arc::clone(&door), bea, &env_b, Arc::clone(&clk), 4)
        .await
        .unwrap();
    press(&mut b, &store, "1 a b enter 1 1 2 3 enter f").await;
    let screen = b.look_around(&env_b).join("\n");
    assert!(screen.contains("THE FORGE (level 2)"), "{screen}");
    press(&mut b, &store, "esc w").await;
    let screen = b.look_around(&env_b).join("\n");
    assert!(screen.contains("1 of 11 finished"), "{screen}");
    assert!(!screen.contains("New Bellows"), "a finished project is not asked for again: {screen}");
}
