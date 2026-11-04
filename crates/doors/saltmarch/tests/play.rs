//! Saltmarch through the framework: replay, saves, the shared world, the daily tick, a save the
//! door cannot read, season changes and two sessions racing to make the season's world.

#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

use std::sync::Arc;

use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::{Driver, OpenError, Running};
use russh_bbs_door::replay::{self, Recorder, ReplayLog};
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_door::{Door, DoorEnv, DoorKey, Flow, Save, Start};
use russh_bbs_door_saltmarch::clan::Clan;
use russh_bbs_door_saltmarch::types::Resource;
use russh_bbs_door_saltmarch::world::World;
use russh_bbs_door_saltmarch::{DOOR_ID, SAVE_VERSION, Saltmarch};

const DAY: i64 = 20_000;

fn env() -> DoorEnv {
    DoorEnv::for_tests("ann")
}

fn clock(day: i64) -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Day::from_raw(day).start()))
}

fn door() -> Saltmarch {
    Saltmarch::new().unwrap()
}

/// A live session that records what it is given, so a replay can be compared with it.
struct Live {
    driver: Driver,
    recorder: Recorder,
    last_save: Option<Vec<u8>>,
    env: DoorEnv,
}

impl Live {
    fn open(door: &Saltmarch, seed: u64, save: Option<Vec<u8>>) -> Self {
        let env = env();
        let start = Day::from_raw(DAY).start().as_raw();
        let driver = Driver::open(
            door,
            Start {
                save: match &save {
                    Some(bytes) => Save::Loaded { bytes, season: 1 },
                    None => Save::Fresh,
                },
                world: None,
                env: &env,
                today: Day::from_raw(DAY),
                season: 1,
            },
            seed,
            clock(DAY),
        );
        Self {
            driver,
            recorder: Recorder::new(door, seed, start, 1, save),
            last_save: None,
            env,
        }
    }

    fn key(&mut self, key: DoorKey) -> Flow {
        self.recorder.key(key);
        let flow = self.driver.key(key);
        let effects = self.driver.take_effects();
        if let Some(bytes) = effects.save() {
            self.last_save = Some(bytes.to_vec());
        }
        flow
    }

    fn screen(&self) -> Vec<String> {
        self.driver.look_around(&self.env)
    }

    /// Press the menu key of the first entry whose tag starts with `tag`.
    fn choose(&mut self, tag: &str) -> bool {
        let found = self.screen().iter().find_map(|line| {
            let rest = line.strip_prefix('[')?;
            let (key, text) = rest.split_once("] ")?;
            text.starts_with(tag).then(|| key.chars().next())?
        });
        match found {
            Some(c) => {
                self.key(DoorKey::Char(c));
                true
            }
            None => false,
        }
    }

    fn clan(&self) -> Clan {
        Clan::decode(self.last_save.as_deref().unwrap()).unwrap()
    }

    fn said(&self, needle: &str) -> bool {
        self.screen().join("\n").contains(needle)
    }
}

#[test]
fn a_fresh_visit_asks_for_a_name_the_board_accepts_and_founds_a_clan() {
    let door = door();
    let mut live = Live::open(&door, 1, None);
    assert!(live.said("Name your clan") && live.said("Clan name: ann_"));
    // A reserved name is refused and nothing is saved.
    for c in "ann".chars() {
        let _ = c;
        live.key(DoorKey::Backspace);
    }
    for c in "sysop".chars() {
        live.key(DoorKey::Char(c));
    }
    live.key(DoorKey::Enter);
    assert!(live.said("not allowed") && live.last_save.is_none());
    for _ in 0..5 {
        live.key(DoorKey::Backspace);
    }
    for c in "Saltlip".chars() {
        live.key(DoorKey::Char(c));
    }
    live.key(DoorKey::Enter);
    let clan = live.clan();
    assert_eq!(
        (clan.name.as_str(), clan.crew.len(), clan.tallies),
        ("Saltlip", 3, 40)
    );
    assert!(live.said("Turns 30/30") && live.said("Tide: water"));
    // Escape at the name prompt leaves without founding anything.
    let mut quitter = Live::open(&door, 2, None);
    assert_eq!(quitter.key(DoorKey::Escape), Flow::Exit);
    assert!(quitter.last_save.is_none());
}

#[test]
fn a_recorded_session_replays_to_exactly_the_saved_state_on_every_branch() {
    let door = door();
    let mut live = Live::open(&door, 77, None);
    live.key(DoorKey::Enter); // found "ann"
    let day_one = live.clan();
    assert_eq!(day_one.name, "ann");
    // Hold: start a hut, labour until it is built (the wader speeds it).
    live.choose("go:hold");
    assert!(live.choose("build:hut"));
    let mut labours = 0;
    while live.choose("labour:shelter") {
        labours += 1;
        assert!(labours < 10);
    }
    assert!(labours >= 1);
    assert_eq!(live.clan().hold.count("hut"), 1, "the hut stands");
    // Branch: a second track in the same visit (work), with the crew posted overnight.
    assert!(live.choose("build:peat-stack"));
    while live.choose("labour:work") {}
    live.key(DoorKey::Escape);
    live.choose("go:crew");
    assert!(live.choose("post:"));
    live.key(DoorKey::Escape);
    // Branch: chart the marsh, sell, buy.
    live.choose("survey");
    let charted_after_survey = live.clan().charted_count();
    assert!(charted_after_survey > day_one.charted_count());
    live.choose("go:market");
    assert!(live.choose("buy:food"));
    let spent = live.clan();
    assert!(
        spent.turns_used >= 5 && spent.tallies < 40,
        "{} turns, {} tallies",
        spent.turns_used,
        spent.tallies
    );
    let replayed = replay::run(&door, &live.recorder.into_log(), &live.env);
    assert_eq!(
        replayed.save, live.last_save,
        "the log reproduces the save byte for byte"
    );
    assert!(
        replayed.world_edits >= 2,
        "founding publishes the world and a purchase adds to it"
    );
    // The log is text, can be pinned, parses back and runs the same.
    let mut again = Live::open(&door, 77, None);
    again.key(DoorKey::Enter);
    let log = again.recorder.into_log();
    let text = log.to_string();
    assert!(text.starts_with("door=saltmarch seed=77 "));
    let parsed: ReplayLog = text.parse().unwrap();
    assert_eq!(
        replay::run_checked(&door, &parsed, &env()).unwrap().save,
        again.last_save
    );
    let mut wrong = parsed;
    wrong.door = "coin-toss".into();
    assert!(replay::run_checked(&door, &wrong, &env()).is_err());
}

#[test]
fn the_seed_decides_the_people_who_come_to_the_gate() {
    let door = door();
    let found = |seed| {
        let mut live = Live::open(&door, seed, None);
        live.key(DoorKey::Enter);
        live.clan()
    };
    assert_eq!(found(5), found(5));
    assert_ne!(found(5).crew, found(6).crew);
}

#[test]
fn every_screen_draws_at_80_by_24_without_trouble() {
    let door = door();
    let mut live = Live::open(&door, 3, None);
    let draw = |live: &Live| replay::draw_to_text(&live.driver, &live.env, 80, 24);
    assert!(draw(&live).iter().any(|l| l.contains("Name your clan")));
    live.key(DoorKey::Enter);
    for menu in ["go:hold", "go:crew", "go:market", "go:tides"] {
        live.choose(menu);
        let rows = draw(&live);
        assert_eq!(rows.len(), 24);
        assert!(rows.iter().any(|l| l.contains("Turns")), "{menu}");
        let text = live.screen().join("\n");
        assert!(!text.contains('\u{1b}') && text.contains("Turns"), "{menu}");
        live.key(DoorKey::Escape);
    }
    assert!(live.screen().iter().any(|l| l.contains("[a] go:hold")));
}

#[tokio::test]
async fn it_saves_through_the_store_and_the_nights_run_when_the_player_returns() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Arc::new(door());
    let clock = clock(DAY);
    let ann = AccountId::from_raw(1);
    let env = env();
    let open = |seed| {
        let clock: Arc<dyn Clock> = clock.clone();
        Running::open(&store, door.clone(), ann, &env, clock, seed)
    };
    let mut run = open(1).await.unwrap();
    run.key(DoorKey::Enter);
    run.flush(&store).await.unwrap();
    for key in ['a', 'a'] {
        // Main -> hold -> first build.
        run.key(DoorKey::Char(key));
        run.flush(&store).await.unwrap();
    }
    assert_eq!(run.key(DoorKey::Escape), Flow::Stay);
    assert_eq!(run.key(DoorKey::Escape), Flow::Exit);
    let id = door.info().id.clone();
    let stored = store.load_save(&id, ann).await.unwrap().unwrap();
    assert_eq!(stored.version, SAVE_VERSION);
    let before = Clan::decode(&stored.bytes).unwrap();
    assert_eq!(before.last_day, DAY);
    // The same day, a new visit: no report, same state.
    let same = open(2).await.unwrap();
    assert!(
        !same
            .look_around(&env)
            .join("\n")
            .contains("While you were away")
    );
    // Three days later the nights have run: the crew ate, the report says so.
    clock.set(Day::from_raw(DAY + 3).start());
    let mut later = open(3).await.unwrap();
    let screen = later.look_around(&env).join("\n");
    assert!(screen.contains("While you were away"), "{screen}");
    later.key(DoorKey::Char(' '));
    later.flush(&store).await.unwrap();
    let after = Clan::decode(&store.load_save(&id, ann).await.unwrap().unwrap().bytes).unwrap();
    assert_eq!(after.last_day, DAY + 3);
    assert!(after.stock.get(Resource::Food) <= before.stock.get(Resource::Food) - 9);
    assert_eq!(after.turns_used, 0);
    // Going back in time changes nothing.
    clock.set(Day::from_raw(DAY - 5).start());
    let past = open(4).await.unwrap();
    assert!(
        !past
            .look_around(&env)
            .join("\n")
            .contains("While you were away")
    );
}

#[tokio::test]
async fn the_daily_tick_never_invents_a_world_then_moves_prices_once_per_day() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Arc::new(door());
    let clock = clock(DAY);
    let id = door.info().id.clone();
    run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    let stored = store.load_world(&id).await.unwrap().unwrap();
    assert!(
        stored.bytes.is_empty(),
        "no player has made the season's world yet"
    );
    // The first player makes it, and sells some iron to the Tidecourt.
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
    for key in [DoorKey::Enter, DoorKey::Char('c')] {
        run.key(key);
        run.flush(&store).await.unwrap();
    }
    let world = World::decode(&store.load_world(&id).await.unwrap().unwrap().bytes).unwrap();
    assert_eq!((world.season, world.clans.len()), (1, 1));
    assert_eq!(world.clans[0].name, "ann");
    // Sell all the timber: the net sold shows in the world at once.
    assert!(
        run.look_around(&env())
            .iter()
            .any(|l| l.contains("sell:timber"))
    );
    let line = run
        .look_around(&env())
        .into_iter()
        .find(|l| l.contains("sell:timber"))
        .unwrap();
    run.key(DoorKey::Char(line.chars().nth(1).unwrap()));
    run.flush(&store).await.unwrap();
    let world = World::decode(&store.load_world(&id).await.unwrap().unwrap().bytes).unwrap();
    assert_eq!(world.net_sold[Resource::Timber.index()], 12);
    // Tomorrow's tick turns that into a lower price and clears the tally.
    let base = world.prices[Resource::Timber.index()];
    clock.set(Day::from_raw(DAY + 1).start());
    run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    let next = World::decode(&store.load_world(&id).await.unwrap().unwrap().bytes).unwrap();
    assert!(
        next.prices[Resource::Timber.index()] < base + base / 50,
        "selling pushed it down"
    );
    assert_eq!(next.net_sold, [0; 13]);
    assert_eq!((next.last_tick, next.seed), (DAY + 1, world.seed));
    // Idempotent per day.
    let report = run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.applied.len(), 0);
    // Downtime: a week away is caught up, capped by the policy.
    clock.set(Day::from_raw(DAY + 20).start());
    let report = run_due(
        &store,
        door.as_ref(),
        clock.as_ref(),
        TickPolicy { max_catch_up: 5 },
    )
    .await
    .unwrap();
    assert_eq!((report.applied.len(), report.skipped), (5, 14));
}

#[tokio::test]
async fn a_save_it_cannot_read_is_never_overwritten() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Arc::new(door());
    let ann = AccountId::from_raw(1);
    let id = door.info().id.clone();
    let junk = b"not a clan".to_vec();
    store
        .write_save(
            &id,
            ann,
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: SAVE_VERSION,
                bytes: &junk,
            },
        )
        .await
        .unwrap();
    let clock: Arc<dyn Clock> = clock(DAY);
    let mut run = Running::open(&store, door.clone(), ann, &env(), clock, 1)
        .await
        .unwrap();
    let screen = run.look_around(&env()).join("\n");
    assert!(screen.contains("cannot read"), "{screen}");
    for key in [DoorKey::Enter, DoorKey::Char('a'), DoorKey::Char('x')] {
        run.key(key);
        assert!(!run.has_pending(), "nothing is written for {key:?}");
    }
    assert_eq!(run.key(DoorKey::Escape), Flow::Exit);
    let stored = store.load_save(&id, ann).await.unwrap().unwrap();
    assert_eq!(
        (stored.bytes, stored.revision),
        (junk, Revision::from_raw(1))
    );
}

#[tokio::test]
async fn a_save_from_a_newer_version_is_refused_not_loaded() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Arc::new(door());
    let ann = AccountId::from_raw(1);
    store
        .write_save(
            &door.info().id,
            ann,
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: SAVE_VERSION + 1,
                bytes: &[1, 2, 3],
            },
        )
        .await
        .unwrap();
    let clock: Arc<dyn Clock> = clock(DAY);
    let result = Running::open(&store, door, ann, &env(), clock, 1).await;
    assert!(
        matches!(result, Err(OpenError::TooNew { .. })),
        "{:?}",
        result.err()
    );
}

#[test]
fn a_new_season_keeps_the_hall_and_gives_a_fresh_start() {
    let door = door();
    let mut live = Live::open(&door, 9, None);
    live.key(DoorKey::Enter);
    let mut old = live.clan();
    old.hall.push(russh_bbs_door_saltmarch::clan::HallEntry {
        season: 1,
        text: "Old Tam. Remembered.".into(),
    });
    old.tallies = 999;
    let bytes = old.encode();
    let env = env();
    let driver = Driver::open(
        &door,
        Start {
            save: Save::Loaded {
                bytes: &bytes,
                season: 1,
            },
            world: None,
            env: &env,
            today: Day::from_raw(DAY + 60),
            season: 2,
        },
        4,
        clock(DAY + 60),
    );
    let screen = driver.look_around(&env).join("\n");
    assert!(
        screen.contains("season 2") && screen.contains("Tallies 40"),
        "{screen}"
    );
    // The next key saves the new season's clan: the hall and the count of seasons carried over.
    let mut driver = driver;
    driver.key(DoorKey::Char(' '));
    let saved = Clan::decode(driver.take_effects().save().unwrap()).unwrap();
    assert_eq!(
        (saved.season, saved.seasons_played, saved.tallies),
        (2, 2, 40)
    );
    assert_eq!(saved.hall, old.hall);
}

#[tokio::test]
async fn two_sessions_racing_to_make_the_world_end_up_on_one_map() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Arc::new(door());
    let clock = clock(DAY);
    let id = door.info().id.clone();
    let (ann, bob) = (AccountId::from_raw(1), AccountId::from_raw(2));
    let open = |who: AccountId, seed, handle: &str| {
        let clock: Arc<dyn Clock> = clock.clone();
        let env = DoorEnv::for_tests(handle);
        let door = door.clone();
        let store = &store;
        async move {
            Running::open(store, door, who, &env, clock, seed)
                .await
                .unwrap()
        }
    };
    // Both open before either has written a world, each proposing its own seed.
    let mut a = open(ann, 1, "ann").await;
    let mut b = open(bob, 2, "bob").await;
    a.key(DoorKey::Enter);
    a.flush(&store).await.unwrap();
    b.key(DoorKey::Enter);
    b.flush(&store).await.unwrap();
    let world = World::decode(&store.load_world(&id).await.unwrap().unwrap().bytes).unwrap();
    assert_eq!(
        world.clans.len(),
        2,
        "both holds are registered in the one world"
    );
    let load = |who| {
        let store = &store;
        let id = id.clone();
        async move { Clan::decode(&store.load_save(&id, who).await.unwrap().unwrap().bytes).unwrap() }
    };
    let seeds = [load(ann).await.seed, load(bob).await.seed];
    assert_ne!(
        seeds[0], seeds[1],
        "the loser still holds the seed it proposed"
    );
    // The loser joins the world's map the next time it opens.
    let mut b2 = open(bob, 3, "bob").await;
    b2.key(DoorKey::Char(' '));
    b2.flush(&store).await.unwrap();
    let fixed = load(bob).await;
    assert_eq!(fixed.seed, world.seed);
    assert!(fixed.charted_count() > 20, "and re-charts around its hold");
}

#[test]
fn the_door_is_registered_under_its_id_and_has_a_tick() {
    let door = door();
    assert_eq!(door.info().id.as_str(), DOOR_ID);
    assert!(door.has_daily_tick());
    assert_eq!(
        (door.info().save_version, door.info().world_version),
        (1, 1)
    );
}
