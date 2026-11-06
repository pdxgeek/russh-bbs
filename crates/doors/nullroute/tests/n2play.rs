//! N2 through the framework: save and world migration from version 1, the pinned version 2 layouts,
//! and replay logs that cover a coding session and a fight.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

mod common;

use std::sync::Arc;

use common::*;
use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite, WorldWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::{Driver, Running};
use russh_bbs_door::replay::{self, Recorder, ReplayLog};
use russh_bbs_door::sim::{Bot, MemoryDoorStore};
use russh_bbs_door::{BlobKind, Door, DoorEnv, DoorKey, Flow, MigrateError, Save, Start};
use russh_bbs_door_nullroute::code::{Build, CODED_BASE, CODED_MAX};
use russh_bbs_door_nullroute::learn::LEARNED_AT;
use russh_bbs_door_nullroute::player::Player;
use russh_bbs_door_nullroute::save::{decode, decode_world, encode, encode_world};
use russh_bbs_door_nullroute::world::{Learned, World};
use russh_bbs_door_nullroute::{DOOR_ID, Nullroute, SAVE_VERSION, WORLD_VERSION};

const DAY: i64 = 20_000;

/// The v2 tail of a save: a fragment list (count and bytes) and 16 slots of 3 bytes.
fn save_tail(p: &Player) -> usize {
    1 + p.fragments.len() + CODED_MAX * 3
}

/// The bytes M4-N4 (version 3) appends to a save whose social layer is empty.
const TIES_BYTES: usize = 110;
/// The bytes version 3 appends to a world with no price history and an empty social state.
const WORLD_V3_TAIL: usize = 52;

/// What the N2 code (version 2) would have written for `p`: the version 3 bytes without the tail.
fn v2_of(p: &Player) -> Vec<u8> {
    let bytes = encode(p);
    bytes[..bytes.len() - TIES_BYTES].to_vec()
}

fn v1_of(p: &Player) -> Vec<u8> {
    let bytes = v2_of(p);
    bytes[..bytes.len() - save_tail(p)].to_vec()
}

fn env() -> DoorEnv {
    DoorEnv::for_tests("ann")
}

#[test]
fn the_versions_are_three_and_the_migrations_are_pinned() {
    assert_eq!((SAVE_VERSION, WORLD_VERSION), (3, 3));
    let door = Nullroute::new().unwrap();
    assert_eq!(door.info().save_version, 3);
    // Nothing older than version 1 exists, and nothing is invented for it.
    assert_eq!(
        door.migrate(BlobKind::Save, 0, &[]),
        Err(MigrateError::Unsupported(0))
    );
    assert_eq!(
        door.migrate(BlobKind::World, 7, &[]),
        Err(MigrateError::Unsupported(7))
    );
}

#[test]
fn a_version_1_save_is_brought_forward_and_loads_as_the_same_character() {
    let c = content();
    let mut p = Player::new(&c, 77, "ann", 1, DAY);
    p.marks = 321;
    p.heat = 17;
    p.workbench = true;
    p.xp = [5, 14, 0, 31, 2];
    let v1 = v1_of(&p);
    // The N1 decoder would have read exactly these bytes; the N2 one refuses them as they are.
    assert!(decode(&v1).is_none());
    let door = Nullroute::new().unwrap();
    let v2 = door.migrate(BlobKind::Save, 1, &v1).unwrap();
    assert_eq!(v2, v2_of(&p));
    // And on to version 3: the character is the same, with an empty social layer and the
    // protection week behind them (they are not new).
    let moved = door.migrate(BlobKind::Save, 2, &v2).unwrap();
    let back = decode(&moved).unwrap();
    p.ties = russh_bbs_door_nullroute::player::Ties::grandfathered(p.day);
    assert_eq!(back, p);
    assert_eq!(moved, encode(&p));
    assert!(back.fragments.is_empty() && back.coded.iter().all(Option::is_none));
    // Bytes that are not a version 1 save are an error, never guessed at.
    assert_eq!(
        door.migrate(BlobKind::Save, 1, &v1[..v1.len() - 3]),
        Err(MigrateError::Corrupt(1))
    );
    let mut trailing = v1;
    trailing.push(9);
    assert_eq!(
        door.migrate(BlobKind::Save, 1, &trailing),
        Err(MigrateError::Corrupt(1))
    );
}

#[test]
fn a_version_1_world_is_brought_forward_with_nothing_learned() {
    let c = content();
    let mut w = World::default();
    w.nightly(&c, &mut russh_bbs_door::DoorRng::from_seed(1), DAY);
    w.breaches = 3;
    // Version 2 knew no price history.
    w.hist.clear();
    let v3 = encode_world(&w);
    let v2 = &v3[..v3.len() - WORLD_V3_TAIL - w.hist.len()];
    // Version 2 appends a season (4 bytes) and an empty learned list (1 byte).
    let v1 = &v2[..v2.len() - 5];
    assert!(decode_world(v1).is_none());
    let door = Nullroute::new().unwrap();
    assert_eq!(door.migrate(BlobKind::World, 1, v1).unwrap(), v2);
    let moved = door.migrate(BlobKind::World, 2, v2).unwrap();
    assert_eq!(moved, v3);
    let back = decode_world(&moved).unwrap();
    assert_eq!(back, w);
    assert!(back.learned.is_empty() && back.season == 0);
    assert_eq!(
        door.migrate(BlobKind::World, 1, &v1[..10]),
        Err(MigrateError::Corrupt(1))
    );
}

#[test]
fn the_version_2_layouts_are_pinned() {
    let c = content();
    let mut p = Player::new(&c, 1, "a", 1, DAY);
    p.fragments = vec![14, 3];
    p.workbench = true;
    p.owned.push(CODED_BASE);
    p.coded[0] = Some(Build {
        effect: 1,
        target: 14,
        modifier: 23,
    });
    let bytes = encode(&p);
    let v2 = v2_of(&p);
    let tail = &v2[v2.len() - (1 + 2 + CODED_MAX * 3)..];
    assert_eq!(&tail[..3], [2, 14, 3], "count, then the fragments");
    assert_eq!(&tail[3..6], [1, 14, 23], "slot 0: effect, target, modifier");
    assert!(tail[6..].iter().all(|b| *b == 0), "empty slots are zeros");
    assert_eq!(decode(&bytes).unwrap(), p);
    // A save never grows beyond a few hundred bytes, far under the framework's cap.
    let mut full = p.clone();
    full.fragments = vec![30; 40];
    full.coded.fill(Some(Build {
        effect: 10,
        target: 18,
        modifier: 30,
    }));
    assert!(encode(&full).len() < 1024);
    // Every corrupt tail is refused.
    let mut short = bytes.clone();
    short.pop();
    assert!(decode(&short).is_none());
}

#[test]
fn the_world_with_learning_stays_small_and_round_trips() {
    let mut w = World {
        season: 4,
        ..World::default()
    };
    for i in 0..96u16 {
        w.learned.push(Learned {
            corp: (i % 5) as u8,
            sig: 1000 + i,
            uses: 30,
        });
    }
    let bytes = encode_world(&w);
    assert!(bytes.len() < 1024, "{}", bytes.len());
    assert_eq!(decode_world(&bytes).unwrap(), w);
    // More rows than the cap is not a world.
    let mut bad = bytes;
    let at = bad.len() - WORLD_V3_TAIL - 96 * 4 - 1;
    bad[at] = 97;
    assert!(decode_world(&bad).is_none());
}

#[tokio::test]
async fn an_n1_save_and_world_in_the_store_open_in_n2() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Nullroute::shared().unwrap();
    let ann = AccountId::from_raw(1);
    let c = content();
    let mut p = Player::new(&c, 5, "ann", 1, DAY);
    p.marks = 777;
    let id = door.info().id.clone();
    store
        .write_save(
            &id,
            ann,
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: 1,
                bytes: &v1_of(&p),
            },
        )
        .await
        .unwrap();
    let mut world = World::default();
    world.nightly(&c, &mut russh_bbs_door::DoorRng::from_seed(2), DAY);
    let v3 = encode_world(&world);
    store
        .write_world(
            &id,
            &WorldWrite {
                expected: Revision::NONE,
                version: 1,
                bytes: &v3[..v3.len() - WORLD_V3_TAIL - world.hist.len() - 5],
                tick: None,
            },
        )
        .await
        .unwrap();
    let clock: Arc<ManualClock> = Arc::new(ManualClock::new(Day::from_raw(DAY).start()));
    let env = env();
    let run = Running::open(&store, door, ann, &env, clock as Arc<dyn Clock>, 4)
        .await
        .unwrap();
    let text = run.look_around(&env).join("\n");
    assert!(
        text.contains("Marks 777"),
        "the old character is intact: {text}"
    );
}

// ---- replay ----

fn driver(door: &Nullroute, env: &DoorEnv, day: i64, seed: u64, save: Option<&[u8]>) -> Driver {
    Driver::open(
        door,
        Start {
            save: save.map_or(Save::Fresh, |bytes| Save::Loaded { bytes, season: 1 }),
            world: None,
            env,
            today: Day::from_raw(day),
            season: 1,
        },
        seed,
        Arc::new(ManualClock::new(Day::from_raw(day).start())),
    )
}

#[test]
fn a_coding_session_through_the_screen_replays_and_spends_what_it_should() {
    let c = content();
    let door = Nullroute::new().unwrap();
    let mut p = Player::new(&c, 9, "ann", 1, DAY);
    p.workbench = true;
    p.fragments = vec![1, 14, 23, 5];
    let start = encode(&p);
    // Hideout, code: effect 1 (crack-core is first in the case), target 1 (warden-lens), modifier 1
    // (piercing), then confirm.
    let keys = "hc111";
    let mut all: Vec<DoorKey> = keys.chars().map(DoorKey::Char).collect();
    all.push(DoorKey::Enter);
    let log = ReplayLog {
        door: DOOR_ID.into(),
        seed: 3,
        start: Day::from_raw(DAY).start().as_raw(),
        season: 1,
        save: Some(start.clone()),
        keys: all,
    };
    let run = replay::run_checked(&door, &log, &env()).unwrap();
    let after = decode(run.save.as_deref().unwrap()).unwrap();
    assert!(after.owned.contains(&CODED_BASE));
    assert_eq!(after.fragments, vec![5], "the three fragments were burned");
    assert_eq!(after.cycles, p.cycles - 4, "and four cycles");
    assert!(
        run.screen
            .join("\n")
            .contains("Coded crack-warden-piercing"),
        "{:?}",
        run.screen
    );
    // The same log, the text form, and a second run are all the same.
    assert_eq!(replay::run(&door, &log, &env()), run);
    let parsed: ReplayLog = log.to_string().parse().unwrap();
    assert_eq!(replay::run(&door, &parsed, &env()).save, run.save);
    // A different choice at the editor is a different save: leave the modifier out.
    let mut none = log.clone();
    none.keys = "hc110"
        .chars()
        .map(DoorKey::Char)
        .chain([DoorKey::Enter])
        .collect();
    let other = replay::run(&door, &none, &env());
    assert_ne!(other.save, run.save);
}

#[test]
fn the_editor_refuses_what_does_not_fit_and_says_why() {
    let door = Nullroute::new().unwrap();
    let c = content();
    let mut p = Player::new(&c, 9, "ann", 1, DAY);
    p.workbench = true;
    p.fragments = vec![1, 18, 29];
    let keys: Vec<DoorKey> = "hc111"
        .chars()
        .map(DoorKey::Char)
        .chain([DoorKey::Enter])
        .collect();
    let log = ReplayLog {
        door: DOOR_ID.into(),
        seed: 1,
        start: Day::from_raw(DAY).start().as_raw(),
        season: 1,
        save: Some(encode(&p)),
        keys,
    };
    let run = replay::run(&door, &log, &env());
    let text = run.screen.join("\n");
    assert!(text.contains("memory budget"), "{text}");
    let after = run
        .save
        .as_deref()
        .map_or_else(|| p.clone(), |b| decode(b).unwrap());
    assert_eq!(after.fragments, p.fragments, "nothing was burned");
}

/// Record one visit by the screen bot and say whether it fought.
fn visit(door: &Nullroute, day: i64, seed: u64) -> (ReplayLog, Option<Vec<u8>>, bool) {
    let env = env();
    let mut d = driver(door, &env, day, seed, None);
    let mut rec = Recorder::new(door, seed, Day::from_raw(day).start().as_raw(), 1, None);
    let mut bot = ScreenBot::default();
    let mut save = None;
    let mut fought = false;
    for sent in 0..500 {
        let screen = d.look_around(&env);
        fought |= screen.iter().any(|l| l.contains("break off"));
        let Some(key) = bot.next_key(&screen, 1, sent) else {
            break;
        };
        rec.key(key);
        let flow = d.key(key);
        if let Some(bytes) = d.take_effects().save() {
            save = Some(bytes.to_vec());
        }
        if flow == Flow::Exit {
            break;
        }
    }
    (rec.into_log(), save, fought)
}

#[test]
fn a_recorded_visit_with_a_fight_in_it_replays_exactly() {
    let door = Nullroute::new().unwrap();
    let mut with_fight = 0;
    for day in DAY..DAY + 60 {
        let (log, live, fought) = visit(&door, day, 3);
        assert_eq!(replay::run(&door, &log, &env()).save, live, "day {day}");
        with_fight += u32::from(fought);
    }
    assert!(with_fight >= 3, "fights were replayed: {with_fight}");
}

#[test]
fn the_fight_screen_fits_the_door_area_with_its_hints_in_view() {
    let door = Nullroute::new().unwrap();
    let env = env();
    let mut shown = false;
    for day in DAY..DAY + 60 {
        let mut d = driver(&door, &env, day, 3, None);
        let mut bot = ScreenBot::default();
        for sent in 0..500 {
            let screen = d.look_around(&env);
            if screen.iter().any(|l| l.contains("break off")) {
                let text = replay::draw_to_text(&d, &env, 72, 19);
                assert!(text.iter().all(|r| r.chars().count() <= 72), "{text:#?}");
                assert!(
                    text.iter()
                        .rev()
                        .find(|r| !r.is_empty())
                        .is_some_and(|r| r.contains("[x]break off")),
                    "{text:#?}"
                );
                assert!(text.iter().any(|r| r.contains("INTEGRITY")));
                shown = true;
                break;
            }
            let Some(key) = bot.next_key(&screen, 1, sent) else {
                break;
            };
            if d.key(key) == Flow::Exit {
                break;
            }
        }
        if shown {
            break;
        }
    }
    assert!(shown, "no seed produced a fight to look at");
    let _ = LEARNED_AT;
}
