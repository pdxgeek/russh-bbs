//! FW-1: the world read, the render-time clock, world-aware replay, `largest_save` and the
//! reserved look-around key, with a tiny door that exercises each.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use russh_bbs_core::doors::DoorId;
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::{AccountId, UnixSeconds};
use russh_bbs_door::host::{Driver, Running};
use russh_bbs_door::replay::{self, Recorder, WorldLog};
use russh_bbs_door::sim::{Bot, MemoryDoorStore, Simulation};
use russh_bbs_door::{
    Canvas, Ctx, Door, DoorEnv, DoorInfo, DoorKey, DoorRng, DoorSession, DoorStyle, Flow, Save,
    Start,
};

/// `r` saves the world it reads, `w` appends a byte to the world, `z` saves 37 bytes, `+` counts,
/// `t` starts typing (every key is then text) and Enter stops it.
struct Reader {
    info: DoorInfo,
}

struct Session {
    count: u32,
    typing: bool,
    typed: String,
}

impl Door for Reader {
    fn info(&self) -> &DoorInfo {
        &self.info
    }

    fn open(&self, _start: Start<'_>, _rng: &mut DoorRng) -> Box<dyn DoorSession> {
        Box::new(Session {
            count: 0,
            typing: false,
            typed: String::new(),
        })
    }
}

impl DoorSession for Session {
    fn key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        if self.typing {
            match key {
                DoorKey::Enter => self.typing = false,
                DoorKey::Char(c) => self.typed.push(c),
                _ => {}
            }
            return Flow::Stay;
        }
        match key {
            DoorKey::Char('r') => {
                let seen = ctx.world().unwrap_or_default().to_vec();
                ctx.request_save(seen);
            }
            DoorKey::Char('w') => {
                ctx.edit_world(|old| {
                    let mut bytes = old.unwrap_or_default().to_vec();
                    bytes.push(1);
                    Some(bytes)
                });
            }
            DoorKey::Char('z') => ctx.request_save(vec![0; 37]),
            DoorKey::Char('+') => self.count += 1,
            DoorKey::Char('t') => self.typing = true,
            DoorKey::Char('q') => return Flow::Exit,
            _ => {}
        }
        Flow::Stay
    }

    fn render(&self, canvas: &mut Canvas<'_>, _env: &DoorEnv) {
        let line = format!("day {} count {}", canvas.today().as_raw(), self.count);
        canvas.text(0, 0, &line, DoorStyle::Plain);
    }

    fn look_around(&self, _env: &DoorEnv) -> Vec<String> {
        vec![format!("the count is {}", self.count)]
    }

    fn captures_text(&self) -> bool {
        self.typing
    }
}

fn door() -> Arc<dyn Door> {
    Arc::new(Reader {
        info: DoorInfo {
            id: DoorId::parse("reader").unwrap(),
            title: "Reader",
            blurb: "reads",
            save_version: 1,
            world_version: 1,
        },
    })
}

fn env() -> DoorEnv {
    DoorEnv::for_tests("ann")
}

fn clock(day: i64) -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Day::from_raw(day).start()))
}

async fn open(store: &MemoryDoorStore, who: i64, clock: &Arc<ManualClock>) -> Running {
    let clock: Arc<dyn Clock> = clock.clone();
    Running::open(store, door(), AccountId::from_raw(who), &env(), clock, 1)
        .await
        .unwrap()
}

async fn saved(store: &MemoryDoorStore, who: i64) -> Vec<u8> {
    use russh_bbs_core::doors::DoorStore as _;
    store
        .load_save(&DoorId::parse("reader").unwrap(), AccountId::from_raw(who))
        .await
        .unwrap()
        .unwrap()
        .bytes
}

#[tokio::test]
async fn the_world_read_sees_another_players_edit_made_after_open() {
    let store = MemoryDoorStore::new();
    let clock = clock(20_000);
    let mut ann = open(&store, 1, &clock).await;
    let mut bea = open(&store, 2, &clock).await;

    // Bea edits the world after Ann opened.
    bea.key(DoorKey::Char('w'));
    bea.flush(&store).await.unwrap();

    // Within the same clock second Ann's view is not refreshed: it is one edit behind.
    assert!(!ann.world_is_stale());
    ann.refresh_world_if_stale(&store).await.unwrap();
    ann.key(DoorKey::Char('r'));
    ann.flush(&store).await.unwrap();
    assert_eq!(saved(&store, 1).await, Vec::<u8>::new());

    // A second later the board re-reads before the key and Ann sees Bea's edit.
    clock.set(UnixSeconds::from_raw(
        Day::from_raw(20_000).start().as_raw() + 1,
    ));
    assert!(ann.world_is_stale());
    ann.refresh_world_if_stale(&store).await.unwrap();
    ann.key(DoorKey::Char('r'));
    ann.flush(&store).await.unwrap();
    assert_eq!(saved(&store, 1).await, vec![1]);

    // A session sees its own edit straight after its flush, with no clock movement.
    ann.key(DoorKey::Char('w'));
    ann.flush(&store).await.unwrap();
    ann.key(DoorKey::Char('r'));
    ann.flush(&store).await.unwrap();
    assert_eq!(saved(&store, 1).await, vec![1, 1]);
}

#[test]
fn replay_with_world_reads_reproduces_the_saved_bytes() {
    let d = door();
    let mut recorder = Recorder::new(
        d.as_ref(),
        7,
        Day::from_raw(20_000).start().as_raw(),
        1,
        None,
    )
    .with_world(Some(vec![9]));
    recorder.key(DoorKey::Char('r'));
    recorder.world_read(Some(vec![9, 2]));
    recorder.key(DoorKey::Char('r'));
    let log = recorder.into_world_log();

    let run = replay::run_world(d.as_ref(), &log, &env());
    assert_eq!(run.save.as_deref(), Some(&[9, 2][..]));

    // The one-line text form carries the world and the reads, and replays identically.
    let text = log.to_string();
    assert!(text.contains(" world=09 reads=1:0902 keys="), "{text}");
    let parsed: WorldLog = text.parse().unwrap();
    assert_eq!(parsed, log);
    assert_eq!(
        replay::run_world(d.as_ref(), &parsed, &env()).save,
        run.save
    );

    // Without the world the same keys see nothing: the world is part of what is replayed.
    let blind = replay::run(d.as_ref(), &log.log, &env());
    assert_eq!(blind.save.as_deref(), Some(&[][..]));
}

#[test]
fn render_draws_the_current_day_across_midnight() {
    let d = door();
    let clock = clock(20_000);
    let start = Start {
        save: Save::Fresh,
        world: None,
        env: &env(),
        today: Day::from_raw(20_000),
        season: 1,
    };
    let as_clock: Arc<dyn Clock> = clock.clone();
    let driver = Driver::open(d.as_ref(), start, 1, as_clock);
    let screen = |driver: &Driver| {
        replay::draw_to_text(driver, &env(), 30, 3)[0]
            .trim()
            .to_owned()
    };
    assert_eq!(screen(&driver), "day 20000 count 0");
    // Midnight passes with no key: the next draw shows the new day.
    clock.set(Day::from_raw(20_001).start());
    assert_eq!(screen(&driver), "day 20001 count 0");
}

struct Zapper;

impl Bot for Zapper {
    fn next_key(&mut self, _screen: &[String], _visit: u32, sent: u32) -> Option<DoorKey> {
        match sent {
            0 => Some(DoorKey::Char('z')),
            1 => Some(DoorKey::Char('q')),
            _ => None,
        }
    }
}

#[tokio::test]
async fn the_simulation_reports_the_largest_save() {
    let mut sim = Simulation::new(door(), Day::from_raw(20_000), 3);
    sim.add_player(Box::new(Zapper));
    let report = sim.run_days(2).await.unwrap();
    assert_eq!(report.saves, 2);
    assert_eq!(report.largest_save, 37);
}

#[test]
fn the_look_around_key_shows_the_text_view_and_any_key_returns() {
    let d = door();
    let clock = clock(20_000);
    let start = Start {
        save: Save::Fresh,
        world: None,
        env: &env(),
        today: Day::from_raw(20_000),
        season: 1,
    };
    let as_clock: Arc<dyn Clock> = clock;
    let mut driver = Driver::open(d.as_ref(), start, 1, as_clock);
    let screen = |driver: &Driver| replay::draw_to_text(driver, &env(), 40, 4);

    driver.key(DoorKey::Char('+'));
    assert_eq!(driver.key(DoorKey::Char('?')), Flow::Stay);
    assert!(driver.is_looking());
    let view = screen(&driver);
    assert!(view[0].contains("Look around"), "{view:?}");
    assert!(view[1].contains("the count is 1"), "{view:?}");

    // The key that closes the view is not passed on to the door.
    driver.key(DoorKey::Char('+'));
    assert!(!driver.is_looking());
    assert!(screen(&driver)[0].contains("count 1"));
    // The door never saw `?`.
    driver.key(DoorKey::Char('+'));
    assert!(screen(&driver)[0].contains("count 2"));

    // While the door takes typed text, `?` is text.
    driver.key(DoorKey::Char('t'));
    driver.key(DoorKey::Char('?'));
    assert!(!driver.is_looking());
}
