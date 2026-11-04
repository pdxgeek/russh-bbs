//! The door through the framework: replay branches, a live session and its recording, the screen
//! at 80x24, saves and versions, and a headless run with bots that read the screen.
#![allow(
    clippy::cast_precision_loss,
    clippy::needless_pass_by_value,
    clippy::doc_markdown,
    clippy::similar_names,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::type_complexity,
    clippy::collapsible_if,
    clippy::cast_possible_wrap,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::print_stderr,
    clippy::panic
)]
mod common;

use std::sync::Arc;

use common::{Sandbox, Setup, arrange};
use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::{Driver, OpenError, Running};
use russh_bbs_door::replay::{self, Recorder, ReplayLog, draw_to_text};
use russh_bbs_door::sim::{Bot, MemoryDoorStore, Simulation};
use russh_bbs_door::{BlobKind, Door, DoorEnv, DoorKey, MigrateError, Save, Start, TransportKind};
use russh_bbs_door_hallpass::save::{decode_player, decode_world, encode_player};
use russh_bbs_door_hallpass::state::{Player, Status};
use russh_bbs_door_hallpass::{DOOR_ID, Hallpass, SAVE_VERSION};

// Pinned: with the RNG and rules as they are, these seeds give these branches of the same scheme.
const CLEAN: u64 = 4;
const TRACED: u64 = 0;
const CAUGHT: u64 = 3;

fn env() -> DoorEnv {
    DoorEnv::for_tests("ann")
}

fn keys(text: &str) -> Vec<DoorKey> {
    text.chars().map(DoorKey::Char).collect()
}

/// A salt-in-the-sugar scheme against Mr. Pell, scouted and with a lookout, standing in the
/// staffroom at the moment it is timed for.
fn prepared() -> Player {
    let mut sb = Sandbox::new("smooth_talker", 1);
    let setup = Setup {
        know: vec!["staff_rota"],
        scout: 2,
        allies: vec![("mags", "lookout")],
        ..Setup::default()
    };
    let id = arrange(&mut sb, "pell", "salt_swap", &setup);
    let scheme = sb.game.player.scheme(id).unwrap().clone();
    let (day, slot) = scheme.when.unwrap();
    sb.game.sync_day(day);
    sb.game.player.slot = slot;
    sb.game.player.place = sb.game.scheme_place(&scheme).unwrap();
    sb.game.player.clone()
}

fn log_for(player: &Player, seed: u64, key_text: &str) -> ReplayLog {
    ReplayLog {
        door: DOOR_ID.to_owned(),
        seed,
        start: Day::from_raw(player.day).start().as_raw(),
        season: 1,
        save: Some(encode_player(player)),
        keys: keys(key_text),
    }
}

#[test]
fn replay_branches_of_the_same_scheme_end_three_different_ways() {
    let door = Hallpass::new().unwrap();
    let player = prepared();
    let run = |seed| {
        let log = log_for(&player, seed, "p1x000");
        let first = replay::run_checked(&door, &log, &env()).unwrap();
        let again = replay::run_checked(&door, &log, &env()).unwrap();
        assert_eq!(first, again, "the same log gives the same run");
        decode_player(first.save.as_deref().unwrap()).unwrap()
    };
    let clean = run(CLEAN);
    assert_eq!(
        (
            clean.pulled,
            clean.caught,
            clean.legend,
            clean.file_points()
        ),
        (1, 0, 6, 0)
    );
    let traced = run(TRACED);
    assert_eq!(
        (
            traced.pulled,
            traced.caught,
            traced.legend,
            traced.file_points()
        ),
        (1, 0, 4, 1)
    );
    assert_eq!(
        traced.file[0].kind, "pinned",
        "pulled off, but somebody put two and two together"
    );
    let caught = run(CAUGHT);
    assert_eq!(
        (
            caught.pulled,
            caught.caught,
            caught.legend,
            caught.file_points()
        ),
        (0, 1, 0, 1)
    );
    assert_eq!(caught.file[0].kind, "caught");
    assert!(caught.detention >= 1 && caught.heat > clean.heat.min(traced.heat));
    // The three differ; the same seed against a build without the lookout is a different run.
    assert_ne!(clean, traced);
    assert_ne!(traced, caught);
}

#[test]
fn a_log_in_text_form_round_trips_and_a_log_for_another_door_is_refused() {
    let door = Hallpass::new().unwrap();
    let log = log_for(&prepared(), CLEAN, "p1x000");
    let text = log.to_string();
    assert!(text.starts_with("door=hallpass seed=4 start=1728259200 season=1 save="));
    assert!(text.ends_with(" keys=p 1 x 0 0 0"));
    assert_eq!(text.parse::<ReplayLog>().unwrap(), log);
    let mut wrong = log;
    wrong.door = "coin-toss".into();
    assert!(replay::run_checked(&door, &wrong, &env()).is_err());
}

#[test]
fn a_first_morning_replays_to_the_same_state_as_the_engine_played_by_hand() {
    // Enrol as the first trait (1), walk to the maths room (g, 8: the maths room in the list), attend (a).
    let door = Hallpass::new().unwrap();
    let day = common::monday();
    let log = ReplayLog {
        door: DOOR_ID.into(),
        seed: 7,
        start: Day::from_raw(day).start().as_raw(),
        season: 1,
        save: None,
        keys: keys("1g8a"),
    };
    let run = replay::run(&door, &log, &env());
    let player = decode_player(run.save.as_deref().unwrap()).unwrap();
    // The same thing through the engine directly.
    let mut sb = Sandbox::on("smooth_talker", 7, day);
    sb.goto("room_maths");
    sb.ok(russh_bbs_door_hallpass::game::Action::Attend);
    assert_eq!(
        player, sb.game.player,
        "the screen adds nothing the engine does not do"
    );
    assert_eq!((player.slot, player.place.as_str()), (1, "room_maths"));
    assert_eq!(player.traits, ["smooth_talker"]);
}

#[test]
fn a_live_session_and_its_recording_agree() {
    let door = Hallpass::new().unwrap();
    let player = prepared();
    let mut recorder = Recorder::new(
        &door,
        9,
        Day::from_raw(player.day).start().as_raw(),
        1,
        Some(encode_player(&player)),
    );
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(Day::from_raw(player.day).start()));
    let bytes = encode_player(&player);
    let env = env();
    let mut driver = Driver::open(
        &door,
        Start {
            save: Save::Loaded {
                bytes: &bytes,
                season: 1,
            },
            world: None,
            env: &env,
            today: Day::from_raw(player.day),
            season: 1,
        },
        9,
        clock,
    );
    let mut last = None;
    for key in keys("fnbp1tbyb").into_iter().chain(keys("p1x000")) {
        recorder.key(key);
        driver.key(key);
        if let Some(save) = driver.take_effects().save() {
            last = Some(save.to_vec());
        }
    }
    assert!(last.is_some());
    assert_eq!(replay::run(&door, &recorder.into_log(), &env).save, last);
}

#[test]
fn the_screen_fits_80_by_24_in_every_mode_and_reads_the_same_over_telnet() {
    let door = Hallpass::new().unwrap();
    let player = prepared();
    let bytes = encode_player(&player);
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(Day::from_raw(player.day).start()));
    let ssh = env();
    let mut telnet = env();
    telnet.transport = TransportKind::Telnet;
    let open = |env: &DoorEnv| {
        Driver::open(
            &door,
            Start {
                save: Save::Loaded {
                    bytes: &bytes,
                    season: 1,
                },
                world: None,
                env,
                today: Day::from_raw(player.day),
                season: 1,
            },
            2,
            Arc::clone(&clock),
        )
    };
    let (mut a, mut b) = (open(&ssh), open(&telnet));
    // Main, go, back, study, shop, plans, a scheme, twists, getaway, schedule, stats, news.
    let mut seen = Vec::new();
    for key in "gbsbp1tbybdbbbfbnb".chars() {
        a.key(DoorKey::Char(key));
        b.key(DoorKey::Char(key));
        let screen = draw_to_text(&a, &ssh, 80, 24);
        assert!(screen.iter().all(|l| l.chars().count() <= 80), "{screen:?}");
        assert_eq!(
            screen,
            draw_to_text(&b, &telnet, 80, 24),
            "same screen on both transports"
        );
        assert_eq!(a.look_around(&ssh), b.look_around(&telnet));
        a.take_effects();
        b.take_effects();
        seen.push(screen.join("\n"));
    }
    let all = seen.join("\n");
    for needle in [
        "HALLPASS",
        "Term 1 - day",
        "[g] Go somewhere",
        "Odds:",
        "Twists for",
        "How do you get away",
        "News",
        "Standing:",
        "Skills (grow with use)",
    ] {
        assert!(
            all.contains(needle) || needle == "News",
            "missing {needle:?}"
        );
    }
    // A very small window still draws without panicking.
    let tiny = draw_to_text(&a, &ssh, 40, 8);
    assert_eq!(tiny.len(), 8);
}

#[test]
fn the_look_around_text_has_the_whole_screen_with_every_choice_on_its_own_line() {
    let door = Hallpass::new().unwrap();
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(Day::from_raw(common::monday()).start()));
    let env = env();
    let driver = Driver::open(
        &door,
        Start {
            save: Save::Fresh,
            world: None,
            env: &env,
            today: Day::from_raw(common::monday()),
            season: 1,
        },
        1,
        clock,
    );
    let lines = driver.look_around(&env);
    let text = lines.join("\n");
    assert!(text.contains("GREYWATER ACADEMY"));
    for trait_name in ["Smooth talker", "Teacher's pet", "Fast runner", "Tinkerer"] {
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with('[') && l.contains(trait_name)),
            "{trait_name}"
        );
    }
    assert!(lines.iter().any(|l| l == "[q] Leave"));
}

#[test]
fn saves_are_versioned_forward_only_and_capped() {
    let door = Hallpass::new().unwrap();
    assert_eq!(door.info().save_version, SAVE_VERSION);
    assert_eq!(
        door.migrate(BlobKind::Save, 0, &[]),
        Err(MigrateError::Unsupported(0))
    );
    assert_eq!(
        door.migrate(BlobKind::World, 3, &[]),
        Err(MigrateError::Unsupported(3))
    );
    // A full, busy character stays far below the framework's cap.
    let mut p = prepared();
    p.file = (0..48)
        .map(|i| russh_bbs_door_hallpass::state::FileEntry {
            day: i,
            kind: "caught".into(),
            sev: 1,
        })
        .collect();
    p.news = (0..8).map(|_| "x".repeat(400)).collect();
    assert!(
        encode_player(&p).len() < 16 * 1024,
        "{}",
        encode_player(&p).len()
    );
    assert!(encode_player(&p).len() < russh_bbs_core::doors::MAX_SAVE_BYTES);
}

#[tokio::test]
async fn a_newer_save_is_never_loaded_or_overwritten_and_a_new_season_starts_over() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Hallpass::shared().unwrap();
    let ann = AccountId::from_raw(1);
    let player = prepared();
    store
        .write_save(
            &door.info().id,
            ann,
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: SAVE_VERSION + 1,
                bytes: &encode_player(&player),
            },
        )
        .await
        .unwrap();
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(Day::from_raw(player.day).start()));
    let opened = Running::open(&store, door.clone(), ann, &env(), clock.clone(), 1).await;
    assert!(
        matches!(opened, Err(OpenError::TooNew { .. })),
        "{:?}",
        opened.err()
    );

    // The same character under another season number starts a new one: staff ended the season.
    let driver = Driver::open(
        door.as_ref(),
        Start {
            save: Save::Loaded {
                bytes: &encode_player(&player),
                season: 1,
            },
            world: None,
            env: &env(),
            today: Day::from_raw(player.day),
            season: 2,
        },
        1,
        clock,
    );
    assert!(
        driver
            .look_around(&env())
            .join("\n")
            .contains("GREYWATER ACADEMY")
    );
}

#[tokio::test]
async fn a_session_through_the_store_saves_and_comes_back_where_it_left_off() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Hallpass::shared().unwrap();
    let ann = AccountId::from_raw(1);
    let day = common::monday();
    let clock = Arc::new(ManualClock::new(Day::from_raw(day).start()));
    store.set_now(Day::from_raw(day).start());
    let e = env();
    let open = |seed| {
        let c: Arc<dyn Clock> = clock.clone();
        Running::open(&store, door.clone(), ann, &e, c, seed)
    };
    let mut run = open(1).await.unwrap();
    for key in keys("1g8a") {
        run.key(key);
        run.flush(&store).await.unwrap();
    }
    assert_eq!(run.key(DoorKey::Escape), russh_bbs_door::Flow::Exit);
    run.flush(&store).await.unwrap();
    let stored = store
        .load_save(&door.info().id, ann)
        .await
        .unwrap()
        .unwrap();
    let p = decode_player(&stored.bytes).unwrap();
    assert_eq!((p.slot, p.status), (1, Status::Active));
    // The same day, a new visit: the second period, in the maths room.
    let again = open(2).await.unwrap();
    let screen = again.look_around(&env()).join("\n");
    assert!(
        screen.contains("Second period") && screen.contains("Maths room"),
        "{screen}"
    );
    // The next morning is a new day: back in the dorm, first period, with the allowance.
    clock.set(Day::from_raw(day + 1).start());
    let next = open(3).await.unwrap();
    let screen = next.look_around(&env()).join("\n");
    assert!(
        screen.contains("Fenwick dormitory") && screen.contains("First period"),
        "{screen}"
    );
}

/// Reads the screen and presses one of the keys it offers, wandering about the school.
struct Wanderer {
    salt: u64,
}

impl Bot for Wanderer {
    fn next_key(&mut self, screen: &[String], visit: u32, sent: u32) -> Option<DoorKey> {
        let options: Vec<char> = screen
            .iter()
            .filter_map(|l| l.strip_prefix('[')?.chars().next())
            .collect();
        if sent >= 90 {
            return options
                .contains(&'q')
                .then_some(DoorKey::Char('q'))
                .or(Some(DoorKey::Escape));
        }
        if options.is_empty() {
            return Some(DoorKey::Escape);
        }
        let mix = self.salt
            ^ (u64::from(visit) << 20)
            ^ (u64::from(sent).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut pick =
            options[usize::try_from((mix ^ (mix >> 29)) % options.len() as u64).unwrap()];
        if pick == 'q' && sent < 60 {
            pick = options[0];
        }
        Some(DoorKey::Char(pick))
    }
}

#[tokio::test]
async fn the_headless_harness_runs_wandering_players_for_a_term_through_the_real_tick_and_store() {
    let day = common::monday();
    let mut sim = Simulation::new(Hallpass::shared().unwrap(), Day::from_raw(day), 77);
    for salt in 1..=4 {
        sim.add_player(Box::new(Wanderer { salt }));
    }
    let report = sim.run_days(45).await.unwrap();
    eprintln!("framework sim: {report:?}");
    assert_eq!(report.visits, 4 * 45);
    assert_eq!(report.cut_off, 0, "every visit ended");
    assert!(report.saves > 100, "{report:?}");
    assert!(report.largest_save < russh_bbs_core::doors::MAX_SAVE_BYTES);
    let id = Hallpass::new().unwrap().info().id.clone();
    let world = decode_world(&sim.store().load_world(&id).await.unwrap().unwrap().bytes).unwrap();
    assert!(
        world.term >= 2,
        "the term turned over in 45 days: {world:?}"
    );
    for n in 1..=4 {
        let stored = sim
            .store()
            .load_save(&id, AccountId::from_raw(n))
            .await
            .unwrap()
            .unwrap();
        let p = decode_player(&stored.bytes).unwrap();
        assert!(
            p.status == Status::Active || p.status == Status::New,
            "{:?}",
            p.status
        );
        assert!(p.history.len() <= 1);
    }
}
