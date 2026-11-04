//! The framework against an in-memory store: saves, migrations, conflicts, caps, the world and
//! the daily tick, with a tiny counting door.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

use std::sync::Arc;

use russh_bbs_core::doors::{DoorId, DoorStore, MAX_SAVE_BYTES, Revision};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::{AccountId, UnixSeconds};
use russh_bbs_door::host::{FlushError, OpenError, Running};
use russh_bbs_door::sim::{Bot, MemoryDoorStore, Simulation};
use russh_bbs_door::tick::{TickCtx, TickError, TickPolicy, run_due};
use russh_bbs_door::{
    BlobKind, Canvas, Ctx, Door, DoorEnv, DoorInfo, DoorKey, DoorRng, DoorSession, DoorStyle, Flow,
    MigrateError, Save, Start,
};

/// A door whose save is `[version 2 layout] count: u32 le, big: u8` and whose world is a day
/// counter. `+` adds one and saves, `B` saves a blob over the cap, `w` bumps the world, `q`
/// leaves.
struct Counter {
    info: DoorInfo,
    fail_tick_on: Option<i64>,
}

impl Counter {
    fn new() -> Self {
        Self {
            info: DoorInfo {
                id: DoorId::parse("counter").unwrap(),
                title: "Counter",
                blurb: "counts",
                save_version: 2,
                world_version: 1,
            },
            fail_tick_on: None,
        }
    }
}

struct Session {
    count: u32,
    world: u32,
    seen_season: u32,
}

fn le(bytes: &[u8]) -> u32 {
    bytes
        .get(..4)
        .and_then(|b| b.try_into().ok())
        .map_or(0, u32::from_le_bytes)
}

impl Door for Counter {
    fn info(&self) -> &DoorInfo {
        &self.info
    }

    fn open(&self, start: Start<'_>, rng: &mut DoorRng) -> Box<dyn DoorSession> {
        let _ = rng.next_u64();
        let (count, seen_season) = match start.save {
            Save::Fresh => (0, start.season),
            Save::Loaded { bytes, season } => (le(bytes), season),
        };
        Box::new(Session {
            count,
            world: start.world.map_or(0, le),
            seen_season,
        })
    }

    fn migrate(&self, kind: BlobKind, from: u32, bytes: &[u8]) -> Result<Vec<u8>, MigrateError> {
        match (kind, from) {
            // v1 saves were one byte; v2 is a little-endian u32.
            (BlobKind::Save, 1) => match bytes {
                [n] => Ok(u32::from(*n).to_le_bytes().to_vec()),
                _ => Err(MigrateError::Corrupt(1)),
            },
            _ => Err(MigrateError::Unsupported(from)),
        }
    }

    fn has_daily_tick(&self) -> bool {
        true
    }

    fn daily_tick(&self, tick: &mut TickCtx<'_>) -> Result<(), TickError> {
        if self.fail_tick_on == Some(tick.day().as_raw()) {
            return Err(TickError::Door("boom".into()));
        }
        let next = tick.world().map_or(0, le) + 1;
        tick.set_world(next.to_le_bytes().to_vec());
        Ok(())
    }
}

impl DoorSession for Session {
    fn key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        match key {
            DoorKey::Char('+') => {
                self.count += 1;
                ctx.request_save(self.count.to_le_bytes().to_vec());
            }
            DoorKey::Char('B') => ctx.request_save(vec![0; MAX_SAVE_BYTES + 1]),
            DoorKey::Char('w') => {
                ctx.edit_world(|world| Some((world.map_or(0, le) + 100).to_le_bytes().to_vec()));
            }
            DoorKey::Char('q') => return Flow::Exit,
            _ => {}
        }
        Flow::Stay
    }

    fn render(&self, canvas: &mut Canvas<'_>, _env: &DoorEnv) {
        canvas.text(0, 0, &format!("count {}", self.count), DoorStyle::Plain);
    }

    fn look_around(&self, _env: &DoorEnv) -> Vec<String> {
        vec![format!(
            "count {} world {} season {}",
            self.count, self.world, self.seen_season
        )]
    }
}

fn id(n: i64) -> AccountId {
    AccountId::from_raw(n)
}

fn clock(day: i64) -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Day::from_raw(day).start()))
}

fn env() -> DoorEnv {
    DoorEnv::for_tests("ann")
}

async fn open(
    store: &MemoryDoorStore,
    door: &Arc<dyn Door>,
    who: i64,
    clock: &Arc<ManualClock>,
) -> Result<Running, OpenError> {
    let clock: Arc<dyn Clock> = clock.clone();
    Running::open(store, Arc::clone(door), id(who), &env(), clock, 1).await
}

fn counter() -> Arc<dyn Door> {
    Arc::new(Counter::new())
}

#[tokio::test]
async fn a_visit_saves_through_the_store_and_the_next_visit_loads_it() {
    let store = MemoryDoorStore::new();
    let door = counter();
    let clock = clock(100);
    let mut run = open(&store, &door, 1, &clock).await.unwrap();
    assert_eq!(run.look_around(&env())[0], "count 0 world 0 season 1");
    for _ in 0..3 {
        run.key(DoorKey::Char('+'));
        run.flush(&store).await.unwrap();
    }
    assert_eq!(run.key(DoorKey::Char('q')), Flow::Exit);
    let stored = store
        .load_save(&door.info().id, id(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (le(&stored.bytes), stored.version, stored.revision),
        (3, 2, Revision::from_raw(3))
    );

    let again = open(&store, &door, 1, &clock).await.unwrap();
    assert_eq!(again.look_around(&env())[0], "count 3 world 0 season 1");
    // Another player has their own save.
    let other = open(&store, &door, 2, &clock).await.unwrap();
    assert_eq!(other.look_around(&env())[0], "count 0 world 0 season 1");
}

#[tokio::test]
async fn an_old_save_is_migrated_forward_and_a_newer_one_is_never_touched() {
    let store = MemoryDoorStore::new();
    let door = counter();
    let clock = clock(100);
    let door_id = door.info().id.clone();
    store
        .write_save(
            &door_id,
            id(1),
            &russh_bbs_core::doors::SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: 1,
                bytes: &[7],
            },
        )
        .await
        .unwrap();
    let mut run = open(&store, &door, 1, &clock).await.unwrap();
    assert_eq!(run.look_around(&env())[0], "count 7 world 0 season 1");
    run.key(DoorKey::Char('+'));
    run.flush(&store).await.unwrap();
    let stored = store.load_save(&door_id, id(1)).await.unwrap().unwrap();
    assert_eq!(
        (stored.version, le(&stored.bytes)),
        (2, 8),
        "written at the current version"
    );

    // A save from the future: refused, and the stored bytes are left exactly as they were.
    store
        .write_save(
            &door_id,
            id(2),
            &russh_bbs_core::doors::SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: 3,
                bytes: &[9, 9],
            },
        )
        .await
        .unwrap();
    let refused = open(&store, &door, 2, &clock).await;
    assert!(matches!(
        refused,
        Err(OpenError::TooNew {
            found: 3,
            supported: 2
        })
    ));
    let kept = store.load_save(&door_id, id(2)).await.unwrap().unwrap();
    assert_eq!((kept.version, kept.bytes), (3, vec![9, 9]));

    // A corrupt old save is an error, not a fresh game.
    store
        .write_save(
            &door_id,
            id(3),
            &russh_bbs_core::doors::SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: 1,
                bytes: &[1, 2, 3],
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        open(&store, &door, 3, &clock).await,
        Err(OpenError::Migrate(MigrateError::Corrupt(1)))
    ));
}

#[tokio::test]
async fn a_barred_player_cannot_open_and_their_save_is_kept() {
    let store = MemoryDoorStore::new();
    let door = counter();
    let clock = clock(100);
    let mut run = open(&store, &door, 1, &clock).await.unwrap();
    run.key(DoorKey::Char('+'));
    run.flush(&store).await.unwrap();
    let audit = russh_bbs_core::audit::events::door_player_barred(
        russh_bbs_core::audit::Actor::System,
        id(1),
        "counter",
    );
    store
        .bar_player(&door.info().id, id(1), id(9), &audit)
        .await
        .unwrap();
    assert!(matches!(
        open(&store, &door, 1, &clock).await,
        Err(OpenError::Barred)
    ));
    assert!(
        store
            .load_save(&door.info().id, id(1))
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn two_sessions_of_one_player_cannot_overwrite_each_other() {
    let store = MemoryDoorStore::new();
    let door = counter();
    let clock = clock(100);
    let mut first = open(&store, &door, 1, &clock).await.unwrap();
    let mut second = open(&store, &door, 1, &clock).await.unwrap();
    first.key(DoorKey::Char('+'));
    first.flush(&store).await.unwrap();
    second.key(DoorKey::Char('+'));
    second.key(DoorKey::Char('+'));
    assert!(matches!(
        second.flush(&store).await,
        Err(FlushError::Conflict)
    ));
    let stored = store
        .load_save(&door.info().id, id(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(le(&stored.bytes), 1, "the first session's save stands");
}

#[tokio::test]
async fn a_save_over_the_cap_is_refused_and_nothing_is_written() {
    let store = MemoryDoorStore::new();
    let door = counter();
    let clock = clock(100);
    let mut run = open(&store, &door, 1, &clock).await.unwrap();
    run.key(DoorKey::Char('B'));
    assert!(matches!(run.flush(&store).await, Err(FlushError::TooLarge)));
    assert!(
        store
            .load_save(&door.info().id, id(1))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_world_edit_is_retried_against_the_latest_world() {
    let store = MemoryDoorStore::new();
    let door = counter();
    let clock = clock(100);
    let mut a = open(&store, &door, 1, &clock).await.unwrap();
    let mut b = open(&store, &door, 2, &clock).await.unwrap();
    a.key(DoorKey::Char('w'));
    b.key(DoorKey::Char('w'));
    a.flush(&store).await.unwrap();
    b.flush(&store).await.unwrap();
    let world = store.load_world(&door.info().id).await.unwrap().unwrap();
    assert_eq!(
        le(&world.bytes),
        200,
        "both edits landed, neither overwrote the other"
    );
}

#[tokio::test]
async fn the_first_tick_applies_today_only_and_a_second_run_does_nothing() {
    let store = MemoryDoorStore::new();
    let door = counter();
    let clock = clock(20_000);
    let first = run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert_eq!(first.applied, vec![Day::from_raw(20_000)]);
    let again = run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert!(again.applied.is_empty() && !again.lost_race);
    let world = store.load_world(&door.info().id).await.unwrap().unwrap();
    assert_eq!(
        (le(&world.bytes), world.last_tick),
        (1, Some(Day::from_raw(20_000)))
    );
    assert_eq!(
        world.revision,
        Revision::from_raw(1),
        "the second run wrote nothing"
    );
}

#[tokio::test]
async fn downtime_is_caught_up_oldest_first_with_a_cap_and_the_skip_is_reported() {
    let store = MemoryDoorStore::new();
    let door = counter();
    let clock = clock(20_000);
    run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    // Three days of downtime, within the cap.
    clock.set(Day::from_raw(20_003).start());
    let report = run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert_eq!(
        report.applied,
        (20_001..=20_003).map(Day::from_raw).collect::<Vec<_>>()
    );
    assert_eq!(report.skipped, 0);
    // A month of downtime with a cap of 5: only the last five days run, the rest are reported.
    clock.set(Day::from_raw(20_033).start());
    let report = run_due(
        &store,
        door.as_ref(),
        clock.as_ref(),
        TickPolicy { max_catch_up: 5 },
    )
    .await
    .unwrap();
    assert_eq!(report.applied.len(), 5);
    assert_eq!(report.applied.last(), Some(&Day::from_raw(20_033)));
    assert_eq!(report.skipped, 25);
    let world = store.load_world(&door.info().id).await.unwrap().unwrap();
    assert_eq!(le(&world.bytes), 1 + 3 + 5);
}

#[tokio::test]
async fn a_failing_tick_writes_nothing_and_the_next_run_retries_the_same_days() {
    let store = MemoryDoorStore::new();
    let mut counter = Counter::new();
    let clock = clock(20_000);
    run_due(&store, &counter, clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    counter.fail_tick_on = Some(20_002);
    clock.set(Day::from_raw(20_003).start());
    let failed = run_due(&store, &counter, clock.as_ref(), TickPolicy::default()).await;
    assert!(matches!(failed, Err(TickError::Door(_))));
    let world = store.load_world(&counter.info.id).await.unwrap().unwrap();
    assert_eq!(
        (le(&world.bytes), world.last_tick),
        (1, Some(Day::from_raw(20_000)))
    );
    counter.fail_tick_on = None;
    let retried = run_due(&store, &counter, clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert_eq!(retried.applied.len(), 3);
}

#[tokio::test]
async fn a_clock_that_goes_backwards_never_applies_a_day_twice() {
    let store = MemoryDoorStore::new();
    let door = counter();
    let clock = clock(20_005);
    run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    clock.set(Day::from_raw(20_001).start());
    let report = run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.applied.len(), 0);
    clock.set(Day::from_raw(20_006).start());
    let report = run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert_eq!(report.applied, vec![Day::from_raw(20_006)]);
}

#[tokio::test]
async fn two_racing_ticks_apply_a_day_exactly_once() {
    let store = Arc::new(MemoryDoorStore::new());
    let door = counter();
    let clock = clock(20_000);
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let store = Arc::clone(&store);
        let door = Arc::clone(&door);
        let clock = Arc::clone(&clock);
        tasks.push(tokio::spawn(async move {
            run_due(
                store.as_ref(),
                door.as_ref(),
                clock.as_ref(),
                TickPolicy::default(),
            )
            .await
            .unwrap()
        }));
    }
    let mut applied = 0;
    for task in tasks {
        applied += task.await.unwrap().applied.len();
    }
    assert_eq!(applied, 1);
    let world = store.load_world(&door.info().id).await.unwrap().unwrap();
    assert_eq!(le(&world.bytes), 1);
}

#[tokio::test]
async fn the_scheduler_job_has_the_door_s_name_and_runs_the_tick() {
    let store = Arc::new(MemoryDoorStore::new());
    let door = counter();
    let clock = clock(20_000);
    let job = russh_bbs_door::tick::job(
        Arc::clone(&store),
        Arc::clone(&door),
        clock.clone(),
        TickPolicy::default(),
    )
    .unwrap();
    assert_eq!(job.name().as_str(), "door-counter");
    let mut scheduler = russh_bbs_core::scheduler::Scheduler::new(NoRuns, clock.clone());
    scheduler.register(job).unwrap();
    let reports = scheduler.tick().await;
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].outcome,
        russh_bbs_core::scheduler::RunOutcome::Ok
    );
    assert!(store.load_world(&door.info().id).await.unwrap().is_some());
}

/// A scheduler store that remembers nothing (every job is always due).
struct NoRuns;

#[allow(clippy::unused_async_trait_impl)]
impl russh_bbs_core::scheduler::SchedulerStore for NoRuns {
    async fn last_run(
        &self,
        _job: &russh_bbs_core::access::Slug,
    ) -> Result<Option<russh_bbs_core::scheduler::LastRun>, russh_bbs_core::store::StoreError> {
        Ok(None)
    }

    async fn record_run(
        &self,
        _job: &russh_bbs_core::access::Slug,
        _run: russh_bbs_core::scheduler::LastRun,
    ) -> Result<(), russh_bbs_core::store::StoreError> {
        Ok(())
    }
}

struct Plus;

impl Bot for Plus {
    fn next_key(&mut self, _screen: &[String], _visit: u32, sent: u32) -> Option<DoorKey> {
        match sent {
            0 | 1 => Some(DoorKey::Char('+')),
            2 => Some(DoorKey::Char('q')),
            _ => None,
        }
    }
}

struct Forever;

impl Bot for Forever {
    fn next_key(&mut self, _screen: &[String], _visit: u32, _sent: u32) -> Option<DoorKey> {
        Some(DoorKey::Char('x'))
    }
}

#[tokio::test]
async fn the_headless_harness_runs_n_days_through_the_real_save_and_tick_path() {
    let mut sim = Simulation::new(counter(), Day::from_raw(20_000), 5);
    sim.add_player(Box::new(Plus));
    sim.add_player(Box::new(Plus));
    let report = sim.run_days(30).await.unwrap();
    assert_eq!((report.days, report.visits, report.exits), (30, 60, 60));
    assert_eq!(report.keys, 60 * 3);
    assert_eq!(report.saves, 60 * 2);
    assert_eq!(report.ticks, 31, "today at the start, then one a day");
    let door_id = DoorId::parse("counter").unwrap();
    let ann = sim
        .store()
        .load_save(&door_id, id(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(le(&ann.bytes), 60);
    let world = sim.store().load_world(&door_id).await.unwrap().unwrap();
    assert_eq!(le(&world.bytes), 31);
}

#[tokio::test]
async fn a_bot_that_never_stops_is_cut_off() {
    let mut sim = Simulation::new(counter(), Day::from_raw(20_000), 5);
    sim.add_player(Box::new(Forever));
    sim.play_day().await.unwrap();
    let report = sim.report();
    assert_eq!(report.cut_off, 1);
    assert_eq!(
        report.keys,
        u64::from(russh_bbs_door::sim::MAX_KEYS_PER_VISIT)
    );
}

#[tokio::test]
async fn downtime_in_the_simulation_uses_the_same_catch_up_cap() {
    let mut sim = Simulation::new(counter(), Day::from_raw(20_000), 5);
    sim.set_policy(TickPolicy { max_catch_up: 3 });
    sim.tick_now().await.unwrap();
    let report = sim.advance_days(10).await.unwrap();
    assert_eq!((report.applied.len(), report.skipped), (3, 7));
    let _ = UnixSeconds::from_raw(0);
}
