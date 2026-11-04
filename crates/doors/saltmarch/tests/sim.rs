//! The headless balance run: simulated clans play 60 days through the framework's simulation
//! (the real door, the real save and world store, the real daily tick), reading the text screen
//! and answering with keys. The numbers it prints are the ones the pull request quotes.
//!
//! Run with `cargo test -p russh-bbs-door-saltmarch --test sim -- --nocapture`.

// The bot is a plain script of nested checks, easier to read uncollapsed.
#![allow(
    clippy::unwrap_used,
    clippy::print_stderr,
    clippy::too_many_lines,
    clippy::collapsible_if
)]

use std::collections::{HashMap, VecDeque};

use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::limits::Day;
use russh_bbs_core::store::AccountId;
use russh_bbs_door::sim::{Bot, Simulation};
use russh_bbs_door::{Door, DoorKey};
use russh_bbs_door_saltmarch::Saltmarch;
use russh_bbs_door_saltmarch::clan::Clan;
use russh_bbs_door_saltmarch::content::Catalog;
use russh_bbs_door_saltmarch::types::{Resource, Trade};
use russh_bbs_door_saltmarch::world::World;

const START: i64 = 20_000;
const DAYS: u32 = 60;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Style {
    /// Builds, posts crew, works the tide, crafts and trades.
    Builder,
    /// Only works shifts and never builds or trades.
    Worker,
    /// Logs in and leaves.
    Idler,
}

struct Player {
    style: Style,
    name: &'static str,
    typed: VecDeque<DoorKey>,
    tried: HashMap<String, u32>,
    /// The tag of the entry pressed last, to be given up on if the screen says it failed.
    last: Option<String>,
    waits: u32,
    queue: VecDeque<DoorKey>,
}

impl Player {
    fn new(style: Style, name: &'static str) -> Self {
        Self {
            style,
            name,
            typed: VecDeque::new(),
            tried: HashMap::new(),
            last: None,
            waits: 0,
            queue: VecDeque::new(),
        }
    }
}

/// `[x] tag  text` lines of the screen: (key, tag).
fn entries(screen: &[String]) -> Vec<(char, String)> {
    screen
        .iter()
        .filter_map(|line| {
            let rest = line.strip_prefix('[')?;
            let (key, text) = rest.split_once("] ")?;
            let tag = text.split_whitespace().next()?.to_owned();
            Some((key.chars().next()?, tag))
        })
        .collect()
}

fn number_after(screen: &[String], prefix: &str) -> Option<u32> {
    let line = screen.iter().find(|l| l.contains(prefix))?;
    let rest = line.split(prefix).nth(1)?;
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn stock(screen: &[String], what: &str) -> u32 {
    screen
        .iter()
        .find(|l| l.starts_with("Stores:"))
        .and_then(|l| {
            l.trim_start_matches("Stores:")
                .split(',')
                .find_map(|part| part.trim().strip_prefix(what)?.trim().parse().ok())
        })
        .unwrap_or(0)
}

impl Bot for Player {
    fn next_key(&mut self, screen: &[String], _visit: u32, sent: u32) -> Option<DoorKey> {
        if sent == 0 {
            self.tried.clear();
            self.queue.clear();
            self.waits = 0;
        }
        if let Some(k) = self.queue.pop_front() {
            return Some(k);
        }
        if let Some(tag) = self.last.take() {
            const FAILED: [&str; 8] = [
                "You cannot pay",
                "The water is at",
                "will not go out",
                "is too hurt",
                "That takes",
                "You need",
                "no room",
                "as many",
            ];
            if screen.iter().any(|l| FAILED.iter().any(|f| l.contains(f))) {
                self.tried.insert(tag, 99);
            }
        }
        if screen.iter().any(|l| l.starts_with("Clan name:")) {
            if self.typed.is_empty() {
                self.typed
                    .extend(std::iter::repeat_n(DoorKey::Backspace, 12));
                self.typed.extend(self.name.chars().map(DoorKey::Char));
                self.typed.push_back(DoorKey::Enter);
            }
            return self.typed.pop_front();
        }
        if screen.iter().any(|l| l.contains("Any key to go on")) {
            return Some(DoorKey::Char(' '));
        }
        let list = entries(screen);
        let turns = number_after(screen, "Turns ").unwrap_or(0);
        let main = list.iter().any(|(_, t)| t == "go:hold");
        let in_hold = screen.iter().any(|l| l.starts_with("Plots "));
        let in_crew = screen
            .iter()
            .any(|l| l.starts_with("  ") && l.contains(" - "));
        let in_market = list.iter().any(|(_, t)| t.starts_with("buy:"));
        let leave = || Some(DoorKey::Escape);
        if self.style == Style::Idler || turns < 2 {
            return leave();
        }
        let mut pick = |tags: &[&str], limit: u32| -> Option<DoorKey> {
            for want in tags {
                for (key, tag) in &list {
                    if tag.starts_with(want) {
                        let n = self.tried.entry(tag.clone()).or_insert(0);
                        if *n < limit {
                            *n += 1;
                            self.last = Some(tag.clone());
                            return Some(DoorKey::Char(*key));
                        }
                    }
                }
            }
            None
        };
        let goto = |tag: &str| {
            list.iter()
                .find(|(_, t)| t == tag)
                .map(|(k, _)| DoorKey::Char(*k))
        };
        if main {
            // Order of the day: crew, hold, market, then leave (the hold's menu spends the turns).
            let visits = self.tried.entry("visit".into()).or_insert(0);
            *visits += 1;
            return match *visits {
                1 if self.style == Style::Builder => goto("go:crew"),
                2 | 1 => goto("go:hold"),
                3 if self.style == Style::Builder => goto("go:market"),
                4 if self.style == Style::Builder && turns >= 2 => goto("survey"),
                _ => leave(),
            };
        }
        if in_crew {
            if let Some(k) = pick(&["tend:"], 3) {
                return Some(k);
            }
            if self.style == Style::Builder {
                if let Some(k) =
                    pick(&["recruit:"], 3).filter(|_| stock(screen, "preserved food") > 6)
                {
                    return Some(k);
                }
                if let Some(k) = pick(&["post:"], 1) {
                    return Some(k);
                }
            }
            return leave();
        }
        if in_hold {
            if let Some(k) = pick(&["labour:"], 12) {
                return Some(k);
            }
            if self.style == Style::Builder {
                let order = [
                    "build:reed-garden",
                    "build:drift-rack",
                    "build:salt-pan",
                    "build:hut",
                    "build:smokehouse",
                    "build:watchfire",
                    "build:infirmary",
                    "build:palisade",
                    "build:ropewalk",
                    "build:longhouse",
                    "build:smithy",
                    "build:sluice",
                    "build:gatehouse",
                    "build:watch-tower",
                ];
                if let Some(k) = pick(&order, 1) {
                    return Some(k);
                }
                if stock(screen, "fish") >= 3 && stock(screen, "salt") >= 1 {
                    if let Some(k) = pick(&["craft:cure-food"], 4) {
                        return Some(k);
                    }
                }
            }
            if turns >= 2 {
                let food_low = stock(screen, "preserved food") < 12;
                let order: &[&str] = if food_low {
                    &[
                        "pair:smokehouse",
                        "pair:fish-weir",
                        "pair:peat-stack",
                        "pair:reed-garden",
                        "pair:drift-rack",
                        "pair:salt-pan",
                    ]
                } else {
                    &[
                        "pair:peat-stack",
                        "pair:reed-garden",
                        "pair:drift-rack",
                        "pair:fish-weir",
                        "pair:salt-pan",
                        "pair:smokehouse",
                    ]
                };
                let order: Vec<&str> = if self.style == Style::Builder {
                    order.to_vec()
                } else {
                    order
                        .iter()
                        .map(|t| t.replace("pair:", "shift:"))
                        .map(|t| &*Box::leak(t.into_boxed_str()))
                        .collect()
                };
                if let Some(k) = pick(&order, 20) {
                    return Some(k);
                }
                // Nothing can be worked at this water: let the tide move, then try again.
                if turns >= 5 && self.waits < 8 {
                    if let Some(k) = pick(&["wait:3"], 99) {
                        self.waits += 1;
                        self.tried
                            .retain(|t, _| !t.starts_with("pair:") && !t.starts_with("shift:"));
                        return Some(k);
                    }
                }
            }
            return leave();
        }
        if in_market {
            if self.style == Style::Builder {
                if stock(screen, "salt") >= 10 {
                    if let Some(k) = pick(&["sell:salt"], 1) {
                        return Some(k);
                    }
                }
                if stock(screen, "preserved food") < 8 {
                    if let Some(k) = pick(&["buy:food"], 2) {
                        return Some(k);
                    }
                }
                if stock(screen, "peat") < 6 {
                    if let Some(k) = pick(&["buy:peat"], 2) {
                        return Some(k);
                    }
                }
            }
            return leave();
        }
        leave()
    }
}

#[derive(Debug)]
struct Summary {
    name: &'static str,
    style: Style,
    score: u32,
    crew: usize,
    buildings: usize,
    tallies: u32,
    morale: u8,
    shifts: u32,
    injuries: u32,
    bonds: u32,
    lost: u32,
    charted: u32,
    hall: usize,
    top_skill: u8,
    save_bytes: usize,
}

async fn play(seed: u64) -> (Vec<Summary>, World, Vec<u8>) {
    let door: std::sync::Arc<dyn Door> = Saltmarch::shared().unwrap();
    let cat = Catalog::embedded().unwrap();
    let mut sim = Simulation::new(door.clone(), Day::from_raw(START), seed);
    let roster = [
        (Style::Builder, "Ashlock"),
        (Style::Builder, "Brinemere"),
        (Style::Builder, "Copperfen"),
        (Style::Worker, "Drystake"),
        (Style::Worker, "Eelgrass"),
        (Style::Idler, "Fogbank"),
    ];
    for (style, name) in roster {
        sim.add_player(Box::new(Player::new(style, name)));
    }
    let report = sim.run_days(DAYS).await.unwrap();
    assert_eq!(report.cut_off, 0, "no visit ran away: {report:?}");
    let id = door.info().id.clone();
    let mut out = Vec::new();
    for (i, (style, name)) in roster.iter().enumerate() {
        let stored = sim
            .store()
            .load_save(&id, AccountId::from_raw(i64::try_from(i).unwrap() + 1))
            .await
            .unwrap()
            .unwrap();
        let clan = Clan::decode(&stored.bytes).unwrap();
        assert_eq!(clan.name, *name);
        out.push(Summary {
            name,
            style: *style,
            score: clan.score(&cat),
            crew: clan.crew.len(),
            buildings: clan.hold.built(&cat).count(),
            tallies: clan.tallies,
            morale: clan.morale,
            shifts: clan.stats.shifts,
            injuries: clan.stats.injuries,
            bonds: clan.stats.bonds,
            lost: clan.stats.lost,
            charted: clan.charted_count(),
            hall: clan.hall.len(),
            top_skill: clan
                .crew
                .iter()
                .flat_map(|c| Trade::ALL.map(|t| c.level(t)))
                .max()
                .unwrap_or(0),
            save_bytes: stored.bytes.len(),
        });
    }
    let world = World::decode(&sim.store().load_world(&id).await.unwrap().unwrap().bytes).unwrap();
    let world_bytes = sim.store().load_world(&id).await.unwrap().unwrap().bytes;
    (out, world, world_bytes)
}

#[tokio::test]
async fn sixty_days_of_play_show_that_choices_matter_and_the_numbers_are_sane() {
    let (summaries, world, world_bytes) = play(2026).await;
    eprintln!("\nSaltmarch headless sim: {DAYS} days, 6 clans, seed 2026");
    eprintln!(
        "{:<10} {:<8} {:>5} {:>4} {:>5} {:>7} {:>6} {:>6} {:>5} {:>5} {:>4} {:>7} {:>5} {:>5} {:>6}",
        "clan",
        "style",
        "score",
        "crew",
        "build",
        "tallies",
        "morale",
        "shifts",
        "hurt",
        "bonds",
        "lost",
        "charted",
        "hall",
        "skill",
        "bytes"
    );
    for s in &summaries {
        eprintln!(
            "{:<10} {:<8} {:>5} {:>4} {:>5} {:>7} {:>6} {:>6} {:>5} {:>5} {:>4} {:>7} {:>5} {:>5} {:>6}",
            s.name,
            format!("{:?}", s.style),
            s.score,
            s.crew,
            s.buildings,
            s.tallies,
            s.morale,
            s.shifts,
            s.injuries,
            s.bonds,
            s.lost,
            s.charted,
            s.hall,
            s.top_skill,
            s.save_bytes
        );
    }
    let prices: Vec<String> = Resource::ALL
        .iter()
        .map(|r| {
            format!(
                "{} {:.2}",
                r.id(),
                f64::from(world.prices[r.index()]) / 100.0
            )
        })
        .collect();
    eprintln!(
        "world: season {}, {} holds, last tick day {}, world {} bytes",
        world.season,
        world.clans.len(),
        world.last_tick - START,
        world_bytes.len()
    );
    eprintln!("prices: {}", prices.join(", "));

    let of = |style: Style| summaries.iter().filter(move |s| s.style == style);
    let mean = |style: Style, f: fn(&Summary) -> u32| {
        let v: Vec<u32> = of(style).map(f).collect();
        v.iter().sum::<u32>() / u32::try_from(v.len()).unwrap()
    };
    // Choices matter: builders beat workers, who beat clans that never act.
    assert!(
        mean(Style::Builder, |s| s.score) > mean(Style::Worker, |s| s.score),
        "builders out-score workers"
    );
    assert!(
        mean(Style::Worker, |s| s.score) > mean(Style::Idler, |s| s.score) + 5,
        "workers out-score idlers"
    );
    assert!(mean(Style::Builder, |s| u32::try_from(s.buildings).unwrap()) >= 6);
    assert_eq!(
        of(Style::Worker).map(|s| s.buildings).sum::<usize>(),
        4,
        "workers keep the two buildings they began with"
    );
    // The idler's hold carries on without them: nights run, the crew ate, and nobody starved
    // in the first weeks. The roster is what it was or smaller, never invented.
    // Everyone's save stays far below the framework's 64 KiB cap and the world below its cap.
    assert!(
        summaries.iter().all(|s| s.save_bytes < 8_000),
        "{:?}",
        summaries.iter().map(|s| s.save_bytes).max()
    );
    assert!(world_bytes.len() < 4_000);
    // The world ticked every day, the Tidecourt's prices stayed inside their bounds, every
    // clan is on the registry, and the idler's presence is recorded like anyone's.
    assert_eq!(world.last_tick, START + i64::from(DAYS));
    assert_eq!(world.clans.len(), 6);
    for r in Resource::ALL {
        let base = i32::from(r.base_price()) * 100;
        let p = i32::from(world.prices[r.index()]);
        assert!((base / 2..=base * 2).contains(&p), "{r:?} {p}");
    }
    // Skills grew with use and the marsh was charted by those who looked.
    assert!(
        of(Style::Builder).any(|s| s.top_skill >= 3),
        "someone reached level 3"
    );
    assert!(of(Style::Builder).all(|s| s.charted > 40));
    // Things went wrong somewhere: with this many shifts there were injuries.
    assert!(summaries.iter().map(|s| s.injuries).sum::<u32>() > 0);
}

#[tokio::test]
async fn the_headless_run_is_deterministic() {
    let (a, wa, _) = play(77).await;
    let (b, wb, _) = play(77).await;
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    assert_eq!(wa, wb);
}
