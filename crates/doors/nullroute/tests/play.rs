//! The door through the framework: screens, replay, saves, the tick, the shared world and the
//! headless harness.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::assert_is_empty,
    clippy::identity_op
)]

mod common;

use std::sync::Arc;

use common::*;
use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::{Driver, Running};
use russh_bbs_door::replay::{self, Recorder, ReplayLog};
use russh_bbs_door::sim::{Bot, MemoryDoorStore, Simulation};
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_door::{Door, DoorEnv, DoorKey, Flow, Save, Start, TransportKind};
use russh_bbs_door_nullroute::save::{decode, decode_world};
use russh_bbs_door_nullroute::world::NIGHTLY_CONTRACTS;
use russh_bbs_door_nullroute::{DOOR_ID, Nullroute};

/// A day whose first-visit board has a tier-1 Steal job, which the starter deck can finish.
fn day() -> i64 {
    let c = content();
    (20_000..20_200)
        .find(|d| {
            russh_bbs_door_nullroute::world::first_board(&c, *d)
                .iter()
                .any(|k| k.tier == 1 && c.contract(k.tmpl).is_some_and(|t| t.verb == "Steal"))
        })
        .unwrap()
}

fn env() -> DoorEnv {
    DoorEnv::for_tests("ann")
}

fn clock(day: i64) -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Day::from_raw(day).start()))
}

fn driver<'a>(door: &'a Nullroute, env: &'a DoorEnv, seed: u64, save: Option<&'a [u8]>) -> Driver {
    Driver::open(
        door,
        Start {
            save: save.map_or(Save::Fresh, |bytes| Save::Loaded { bytes, season: 1 }),
            world: None,
            env,
            today: Day::from_raw(day()),
            season: 1,
        },
        seed,
        clock(day()),
    )
}

/// One visit by the screen bot, recorded. Returns the log and the last save.
fn bot_visit(door: &Nullroute, seed: u64) -> (ReplayLog, Option<Vec<u8>>) {
    let env = env();
    let mut d = driver(door, &env, seed, None);
    let mut rec = Recorder::new(door, seed, Day::from_raw(day()).start().as_raw(), 1, None);
    let mut bot = ScreenBot::default();
    let mut save = None;
    for sent in 0..400 {
        let screen = d.look_around(&env);
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
    (rec.into_log(), save)
}

#[test]
fn every_screen_fits_the_door_area_with_its_key_hints_in_view() {
    let door = Nullroute::new().unwrap();
    let env = env();
    let mut d = driver(&door, &env, 4, None);
    let rows = |d: &Driver| replay::draw_to_text(d, &env, 72, 19);
    // The last row with anything on it is the key hints: they are never cut.
    let last_has_keys = |text: &[String]| {
        text.iter()
            .rev()
            .find(|r| !r.is_empty())
            .is_some_and(|r| r.contains('['))
    };
    let mut seen = Vec::new();
    for keys in [
        "",
        "c",
        "1",
        "\u{1b}\u{1b}d",
        "\u{1b}s",
        "\u{1b}r",
        "\u{1b}h",
    ] {
        for ch in keys.chars() {
            d.key(if ch == '\u{1b}' {
                DoorKey::Escape
            } else {
                DoorKey::Char(ch)
            });
        }
        let text = rows(&d);
        assert!(text.iter().all(|r| r.chars().count() <= 72));
        assert!(!text[0].is_empty());
        assert!(last_has_keys(&text), "{text:#?}");
        seen.push(text[0].clone());
    }
    assert_eq!(seen.len(), 7);
    // And in a run, where the screen is fullest.
    let (log, _) = bot_visit(&door, 3);
    let mut d = driver(&door, &env, 3, None);
    let mut widest = 0;
    for key in &log.keys {
        d.key(*key);
        let text = rows(&d);
        widest = widest.max(text.iter().map(|r| r.chars().count()).max().unwrap_or(0));
        assert!(text.iter().all(|r| r.chars().count() <= 72), "{text:#?}");
        if text.get(1).is_some_and(|r| r.contains("TRACE")) {
            // The key hints are the last line of the run screen and must be on screen.
            assert!(
                text.iter()
                    .any(|r| r.contains("[o]jack out") || r.contains("Aim ")),
                "{text:#?}"
            );
        }
    }
    assert!(widest > 40);
}

#[test]
fn the_look_around_view_has_everything_and_nothing_hostile() {
    let door = Nullroute::new().unwrap();
    let env = env();
    let (log, _) = bot_visit(&door, 3);
    let mut d = driver(&door, &env, 3, None);
    for key in &log.keys {
        d.key(*key);
        for line in d.look_around(&env) {
            assert!(!line.chars().any(char::is_control), "{line:?}");
            assert!(line.chars().count() <= 140, "{line:?}");
        }
    }
}

#[test]
fn a_recorded_visit_reproduces_the_saved_state_exactly() {
    let door = Nullroute::new().unwrap();
    let (log, live_save) = bot_visit(&door, 3);
    assert!(
        log.keys.len() > 8,
        "the bot actually did something: {}",
        log.keys.len()
    );
    let first = replay::run_checked(&door, &log, &env()).unwrap();
    let again = replay::run_checked(&door, &log, &env()).unwrap();
    assert_eq!(first, again);
    assert_eq!(first.save, live_save);
    assert!(first.save.is_some());
    // The text form round-trips and replays the same.
    let parsed: ReplayLog = log.to_string().parse().unwrap();
    assert_eq!(replay::run(&door, &parsed, &env()).save, live_save);
    // A different seed is a different game.
    let other = replay::run(
        &door,
        &ReplayLog {
            seed: 4,
            ..log.clone()
        },
        &env(),
    );
    assert_ne!(other.save, live_save);
    // Another door's log is refused.
    assert!(
        replay::run_checked(
            &door,
            &ReplayLog {
                door: "other".into(),
                ..log
            },
            &env()
        )
        .is_err()
    );
}

#[test]
fn many_seeds_replay_and_actually_play_runs() {
    let door = Nullroute::new().unwrap();
    let mut runs = 0;
    let mut jobs = 0;
    for seed in 1..=12 {
        let (log, live) = bot_visit(&door, seed);
        assert_eq!(replay::run(&door, &log, &env()).save, live, "seed {seed}");
        let p = decode(live.as_deref().unwrap()).unwrap();
        runs += p.runs;
        jobs += p.jobs;
    }
    assert!(runs >= 8, "{runs}");
    assert!(jobs >= 2, "the starter deck can finish a job: {jobs}");
}

#[test]
fn a_pinned_log_pins_a_whole_branch_of_play() {
    // Recorded from `bot_visit(&door, 3)`; the numbers below never change while the rules, the
    // content and the RNG stand still. If this fails, a rule changed: re-record on purpose.
    let door = Nullroute::new().unwrap();
    let (log, _) = bot_visit(&door, 3);
    let pinned: ReplayLog = PINNED.parse().unwrap();
    assert_eq!(
        log.to_string(),
        PINNED,
        "re-record the pin if you meant to change a rule"
    );
    let run = replay::run_checked(&door, &pinned, &env()).unwrap();
    let p = decode(run.save.as_deref().unwrap()).unwrap();
    assert_eq!(
        (p.runs, p.jobs, p.burns, p.marks, p.cycles, p.heat),
        PINNED_STATE
    );
}

/// scan, step, scan, step, scan, step, copy the vault, jack out: one clean job.
const PINNED: &str =
    "door=nullroute seed=3 start=1728086400 season=1 save=- keys=c 3 j a 1 a 2 a 4 d o enter q";
/// runs, jobs, burns, marks, cycles, heat
const PINNED_STATE: (u32, u32, u32, u32, u32, u32) = (1, 1, 0, 168, 27, 4);

#[test]
fn the_pinned_branch_changes_when_the_build_changes() {
    // The same keys with the dictionary taken out of the deck: the terminal route is shut, and
    // the run (and the save) come out differently. The deck is a choice with consequences.
    let door = Nullroute::new().unwrap();
    let pinned: ReplayLog = PINNED.parse().unwrap();
    let base = replay::run(&door, &pinned, &env()).save.unwrap();
    // Open the deck screen first and unload the dictionary (letter b), then do the same visit.
    let mut keys = vec![DoorKey::Char('d'), DoorKey::Char('b'), DoorKey::Escape];
    keys.extend(pinned.keys.iter().copied());
    let other = replay::run(&door, &ReplayLog { keys, ..pinned }, &env())
        .save
        .unwrap();
    assert_ne!(other, base);
    let p = decode(&other).unwrap();
    assert!(!p.deck.contains(&5), "the dictionary is out of the deck");
}

#[test]
fn the_screen_does_not_depend_on_the_transport() {
    let door = Nullroute::new().unwrap();
    let ssh = env();
    let telnet = DoorEnv {
        transport: TransportKind::Telnet,
        ..env()
    };
    let (log, _) = bot_visit(&door, 3);
    let mut a = driver(&door, &ssh, 3, None);
    let mut b = driver(&door, &telnet, 3, None);
    for key in &log.keys {
        a.key(*key);
        b.key(*key);
        assert_eq!(a.look_around(&ssh), b.look_around(&telnet));
    }
}

#[tokio::test]
async fn it_saves_loads_and_dropping_the_line_in_a_run_costs_heat() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Nullroute::shared().unwrap();
    let clock = clock(day());
    let ann = AccountId::from_raw(1);
    let env = env();
    let open = |seed| {
        let clock: Arc<dyn Clock> = clock.clone();
        Running::open(&store, door.clone(), ann, &env, clock, seed)
    };
    let mut run = open(1).await.unwrap();
    // Contracts, the Steal job, jack in; then the line drops (the session just ends).
    let screen = run.look_around(&env);
    assert!(screen[0].starts_with("NULLROUTE"));
    let mut bot = ScreenBot::default();
    for sent in 0..6 {
        let screen = run.look_around(&env);
        let Some(key) = bot.next_key(&screen, 1, sent) else {
            break;
        };
        run.key(key);
        run.flush(&store).await.unwrap();
        if run
            .look_around(&env)
            .get(1)
            .is_some_and(|l| l.starts_with("TRACE"))
        {
            break;
        }
    }
    assert!(run.look_around(&env)[1].starts_with("TRACE"), "in a run");
    let id = door.info().id.clone();
    let stored = store.load_save(&id, ann).await.unwrap().unwrap();
    let before = decode(&stored.bytes).unwrap();
    assert!(
        before.pending.is_some() && before.cycles == 35,
        "jacking in is saved: cycles are spent"
    );
    drop(run);

    // Open again the same day: the dropped line shows, and the heat is saved with the next key.
    let mut again = open(2).await.unwrap();
    let text = again.look_around(&env).join("\n");
    assert!(text.contains("dropped the line"), "{text}");
    again.key(DoorKey::Char('x'));
    again.flush(&store).await.unwrap();
    let after = decode(&store.load_save(&id, ann).await.unwrap().unwrap().bytes).unwrap();
    assert_eq!(
        (after.heat, after.pending, after.cycles),
        (8, None, 35),
        "no refund, no second penalty"
    );
    let third = open(3).await.unwrap().look_around(&env).join("\n");
    assert!(!third.contains("dropped the line"));
}

#[tokio::test]
async fn a_save_from_another_season_starts_the_new_season() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Nullroute::shared().unwrap();
    let ann = AccountId::from_raw(1);
    let mut old = russh_bbs_door_nullroute::player::Player::new(&content(), 1, "ann", 1, day());
    old.marks = 999;
    old.heat = 40;
    store
        .write_save(
            &door.info().id,
            ann,
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: 2,
                bytes: &russh_bbs_door_nullroute::save::encode(&old),
            },
        )
        .await
        .unwrap();
    // Staff end the season: the door is opened for season 2.
    let d = Nullroute::new().unwrap();
    let bytes = russh_bbs_door_nullroute::save::encode(&old);
    let env = env();
    let session = d.open(
        Start {
            save: Save::Loaded {
                bytes: &bytes,
                season: 1,
            },
            world: None,
            env: &env,
            today: Day::from_raw(day()),
            season: 2,
        },
        &mut russh_bbs_door::DoorRng::from_seed(1),
    );
    let text = session.look_around(&env).join("\n");
    assert!(
        text.contains("Marks 90") && text.contains("Heat 0"),
        "{text}"
    );
    let same = d.open(
        Start {
            save: Save::Loaded {
                bytes: &bytes,
                season: 1,
            },
            world: None,
            env: &env,
            today: Day::from_raw(day()),
            season: 1,
        },
        &mut russh_bbs_door::DoorRng::from_seed(1),
    );
    assert!(same.look_around(&env).join("\n").contains("Marks 999"));
}

#[test]
fn bytes_the_door_cannot_read_start_a_fresh_character_instead_of_guessing() {
    let door = Nullroute::new().unwrap();
    let env = env();
    let session = door.open(
        Start {
            save: Save::Loaded {
                bytes: &[1, 2, 3],
                season: 1,
            },
            world: None,
            env: &env,
            today: Day::from_raw(day()),
            season: 1,
        },
        &mut russh_bbs_door::DoorRng::from_seed(1),
    );
    assert!(session.look_around(&env).join("\n").contains("Marks 90"));
    // Version 1 is the first: there is nothing older to migrate from.
    assert!(
        door.migrate(russh_bbs_door::BlobKind::Save, 0, &[])
            .is_err()
    );
}

#[tokio::test]
async fn the_daily_tick_posts_the_board_patches_and_catches_up_with_a_cap() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Nullroute::shared().unwrap();
    let clock = clock(day());
    let id = door.info().id.clone();
    let report = run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.applied.len(), 1);
    let world = decode_world(&store.load_world(&id).await.unwrap().unwrap().bytes).unwrap();
    assert_eq!((world.day, world.board.len()), (day(), NIGHTLY_CONTRACTS));
    // Idempotent within the day.
    assert_eq!(
        run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
            .await
            .unwrap()
            .applied
            .len(),
        0
    );
    // A fortnight of downtime applies seven nights, not fourteen.
    clock.set(Day::from_raw(day() + 14).start());
    let report = run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert_eq!((report.applied.len(), report.skipped), (7, 7));
    let world = decode_world(&store.load_world(&id).await.unwrap().unwrap().bytes).unwrap();
    assert_eq!(world.day, day() + 14);
    assert!(world.board.iter().all(|k| k.expires >= day() + 14));
    // A session opened now sees the board the tick posted.
    let env = env();
    let dyn_clock: Arc<dyn Clock> = clock.clone();
    let run = Running::open(
        &store,
        door.clone(),
        AccountId::from_raw(1),
        &env,
        dyn_clock,
        1,
    )
    .await
    .unwrap();
    let text = run.look_around(&env).join("\n");
    assert!(text.contains("NULLROUTE"), "{text}");
}

#[tokio::test]
async fn thirty_days_of_headless_play_through_the_framework_with_numbers() {
    let mut sim = Simulation::new(Nullroute::shared().unwrap(), Day::from_raw(day()), 2026);
    for _ in 0..6 {
        sim.add_player(Box::new(ScreenBot::default()));
    }
    let report = sim.run_days(30).await.unwrap();
    assert_eq!(report.visits, 180);
    assert_eq!(report.cut_off, 0, "no visit hit the key cap");
    assert_eq!(report.exits, 180, "every visit ended by leaving");
    assert_eq!(report.ticks, 31);
    let door_id = Nullroute::new().unwrap().info().id.clone();
    assert_eq!(door_id.as_str(), DOOR_ID);
    let (mut runs, mut jobs, mut marks, mut burns) = (0, 0, 0, 0);
    for n in 1..=6 {
        let stored = sim
            .store()
            .load_save(&door_id, AccountId::from_raw(n))
            .await
            .unwrap()
            .unwrap();
        let p = decode(&stored.bytes).unwrap();
        runs += p.runs;
        jobs += p.jobs;
        marks += p.marks + p.stash;
        burns += p.burns;
        assert!(p.cycles <= p.max_cycles());
    }
    let world = decode_world(
        &sim.store()
            .load_world(&door_id)
            .await
            .unwrap()
            .unwrap()
            .bytes,
    )
    .unwrap();
    eprintln!(
        "framework sim: visits {} keys {} saves {} | runs {runs} jobs {jobs} burns {burns} marks {marks} | world breaches {} nets {} board {}",
        report.visits,
        report.keys,
        report.saves,
        world.breaches,
        world.nets.len(),
        world.board.len()
    );
    assert!(runs >= 30 && jobs >= 10, "{runs} {jobs}");
    assert!(world.breaches > 0, "breaches reach the shared world");
    assert_eq!(world.day, day() + 30);
}
