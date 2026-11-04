//! Replay: a recorded run must reproduce the saved state exactly.
//!
//! A game is a pure engine (state plus an action gives a new state plus events) driven by a
//! seeded RNG and the injected clock, so the same *log* always gives the same result. A
//! [`ReplayLog`] is that log: the door, the seed, the clock, the starting save and the keys. The
//! test helpers here run a log with no store and no I/O and return what the door asked to save,
//! so a game's tests can pin whole branches of play:
//!
//! ```ignore
//! let log: ReplayLog = "door=coin-toss seed=7 start=1700000000 season=1 save=- keys=f f f".parse()?;
//! let run = replay::run(&CoinToss::new(), &log, &DoorEnv::for_tests("ann"));
//! assert_eq!(run.save.as_deref(), Some(&expected_bytes[..]));
//! ```
//!
//! [`Recorder`] makes a log from a live session, so a bug found by hand becomes a regression
//! test. The text form is stable and one line, so logs can sit in a test file as strings.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use russh_bbs_core::limits::{Day, ManualClock};
use russh_bbs_core::store::UnixSeconds;

use crate::canvas::{Canvas, DoorPalette};
use crate::door::{Door, DoorEnv, Flow, Save, Start};
use crate::host::Driver;
use crate::key::DoorKey;

/// Everything needed to rerun a visit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayLog {
    /// The door's id, as a check that the log belongs to the door it is run on.
    pub door: String,
    /// The session RNG seed.
    pub seed: u64,
    /// The clock, in Unix seconds, fixed for the whole run.
    pub start: i64,
    /// The door's season.
    pub season: u32,
    /// The save the visit started from (`None`: a first visit).
    pub save: Option<Vec<u8>>,
    /// The keys, in order.
    pub keys: Vec<DoorKey>,
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

impl fmt::Display for ReplayLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let save = self.save.as_deref().map_or_else(|| "-".to_owned(), hex);
        let keys: Vec<String> = self.keys.iter().map(|k| k.token()).collect();
        write!(
            f,
            "door={} seed={} start={} season={} save={} keys={}",
            self.door,
            self.seed,
            self.start,
            self.season,
            save,
            keys.join(" ")
        )
    }
}

/// Why a log did not parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a replay log: {0}")]
pub struct LogParseError(&'static str);

impl FromStr for ReplayLog {
    type Err = LogParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (head, keys) = text
            .split_once(" keys=")
            .or_else(|| text.strip_suffix(" keys=").map(|head| (head, "")))
            .ok_or(LogParseError("no keys field"))?;
        let mut door = None;
        let mut seed = None;
        let mut start = None;
        let mut season = None;
        let mut save = None;
        for part in head.split(' ') {
            let (name, value) = part.split_once('=').ok_or(LogParseError("bad field"))?;
            match name {
                "door" => door = Some(value.to_owned()),
                "seed" => seed = value.parse().ok(),
                "start" => start = value.parse().ok(),
                "season" => season = value.parse().ok(),
                "save" => {
                    save = Some(if value == "-" {
                        None
                    } else {
                        Some(unhex(value).ok_or(LogParseError("bad save"))?)
                    });
                }
                _ => return Err(LogParseError("unknown field")),
            }
        }
        let keys = keys
            .split(' ')
            .filter(|token| !token.is_empty())
            .map(|token| DoorKey::from_token(token).ok_or(LogParseError("bad key")))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            door: door.ok_or(LogParseError("no door"))?,
            seed: seed.ok_or(LogParseError("no seed"))?,
            start: start.ok_or(LogParseError("no start"))?,
            season: season.ok_or(LogParseError("no season"))?,
            save: save.ok_or(LogParseError("no save"))?,
            keys,
        })
    }
}

/// What a replay produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayRun {
    /// The last save the door asked for (`None` if it never asked).
    pub save: Option<Vec<u8>>,
    /// How many world edits the door asked for.
    pub world_edits: usize,
    /// Whether the door ended the visit.
    pub exited: bool,
    /// How many keys were used (a door that exits early leaves the rest unused).
    pub keys_used: usize,
    /// The door's text view after the last key used.
    pub screen: Vec<String>,
}

/// Run `log` on `door` with no store: a fixed clock, the log's seed, the log's starting save.
///
/// # Panics
/// Never; a log for a different door is reported by [`run_checked`].
#[must_use]
pub fn run(door: &dyn Door, log: &ReplayLog, env: &DoorEnv) -> ReplayRun {
    let clock = Arc::new(ManualClock::new(UnixSeconds::from_raw(log.start)));
    let start = Start {
        save: match &log.save {
            Some(bytes) => Save::Loaded {
                bytes,
                season: log.season,
            },
            None => Save::Fresh,
        },
        world: None,
        env,
        today: Day::of(UnixSeconds::from_raw(log.start)),
        season: log.season,
    };
    let mut driver = Driver::open(door, start, log.seed, clock);
    let mut save = None;
    let mut world_edits = 0;
    let mut exited = false;
    let mut used = 0;
    for key in &log.keys {
        used += 1;
        let flow = driver.key(*key);
        let effects = driver.take_effects();
        world_edits += effects.world.len();
        if let Some(bytes) = effects.save {
            save = Some(bytes);
        }
        if flow == Flow::Exit {
            exited = true;
            break;
        }
    }
    ReplayRun {
        save,
        world_edits,
        exited,
        keys_used: used,
        screen: driver.look_around(env),
    }
}

/// [`run`], refusing a log that names another door.
///
/// # Errors
/// [`LogParseError`] when the log's door is not `door`'s id.
pub fn run_checked(
    door: &dyn Door,
    log: &ReplayLog,
    env: &DoorEnv,
) -> Result<ReplayRun, LogParseError> {
    if log.door != door.info().id.as_str() {
        return Err(LogParseError("the log is for another door"));
    }
    Ok(run(door, log, env))
}

/// Draw the door's current screen to plain text rows (`cols` by `rows`), for tests that check
/// what is on screen.
#[must_use]
pub fn draw_to_text(driver: &Driver, env: &DoorEnv, cols: u16, rows: u16) -> Vec<String> {
    let area = Rect::new(0, 0, cols, rows);
    let mut buf = Buffer::empty(area);
    let palette = DoorPalette::mono();
    driver.render(&mut Canvas::new(&mut buf, area, &palette), env);
    (0..rows)
        .map(|y| {
            let row: String = (0..cols).map(|x| buf[(x, y)].symbol().to_owned()).collect();
            row.trim_end().to_owned()
        })
        .collect()
}

/// Records the keys of a live (or test) session into a [`ReplayLog`].
#[derive(Debug, Clone)]
pub struct Recorder {
    log: ReplayLog,
}

impl Recorder {
    /// Start a recording with the conditions the session started under.
    #[must_use]
    pub fn new(door: &dyn Door, seed: u64, start: i64, season: u32, save: Option<Vec<u8>>) -> Self {
        Self {
            log: ReplayLog {
                door: door.info().id.as_str().to_owned(),
                seed,
                start,
                season,
                save,
                keys: Vec::new(),
            },
        }
    }

    /// Note a key.
    pub fn key(&mut self, key: DoorKey) {
        self.log.keys.push(key);
    }

    /// The finished log.
    #[must_use]
    pub fn into_log(self) -> ReplayLog {
        self.log
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_round_trips_through_its_text_form() {
        let log = ReplayLog {
            door: "coin-toss".into(),
            seed: 18_446_744_073_709_551_615,
            start: 1_700_000_000,
            season: 3,
            save: Some(vec![0, 1, 254, 255]),
            keys: vec![
                DoorKey::Char('f'),
                DoorKey::Char(' '),
                DoorKey::Char('é'),
                DoorKey::Enter,
                DoorKey::Escape,
                DoorKey::Backspace,
                DoorKey::Up,
                DoorKey::Down,
                DoorKey::Left,
                DoorKey::Right,
            ],
        };
        let text = log.to_string();
        assert_eq!(text.parse::<ReplayLog>().unwrap(), log);
        let fresh = ReplayLog {
            save: None,
            keys: Vec::new(),
            ..log
        };
        assert_eq!(fresh.to_string().parse::<ReplayLog>().unwrap(), fresh);
    }

    #[test]
    fn a_malformed_log_is_refused_not_guessed_at() {
        for bad in [
            "",
            "door=x seed=1",
            "door=x seed=one start=0 season=1 save=- keys=",
            "door=x seed=1 start=0 season=1 save=zz keys=",
            "door=x seed=1 start=0 season=1 save=- keys=nonsense!",
            "door=x seed=1 start=0 season=1 save=- extra=1 keys=",
        ] {
            assert!(bad.parse::<ReplayLog>().is_err(), "{bad:?}");
        }
    }
}
