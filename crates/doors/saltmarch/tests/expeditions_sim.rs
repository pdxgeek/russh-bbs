//! The headless expedition run: simulated clans play 60 days through the framework's simulation
//! (the real door, the real save and world store, the real daily tick), read the text screen
//! and answer with keys. The numbers it prints are the ones the pull request quotes.
//!
//! Run with `cargo test -p russh-bbs-door-saltmarch --test expeditions_sim -- --nocapture`.

#![allow(
    clippy::unwrap_used,
    clippy::print_stderr,
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::many_single_char_names
)]

use std::collections::{HashMap, VecDeque};

use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::limits::Day;
use russh_bbs_core::store::AccountId;
use russh_bbs_door::sim::{Bot, Simulation};
use russh_bbs_door::{Door, DoorKey};
use russh_bbs_door_saltmarch::Saltmarch;
use russh_bbs_door_saltmarch::clan::Clan;

const START: i64 = 20_000;
const DAYS: u32 = 60;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Style {
    /// Two crew, light gear, answers scenes safely, home with a margin of four turns.
    Cautious,
    /// All three crew, heavy gear, takes the riskier answers, races the flood to the last turn.
    Greedy,
    /// Two crew who work every patch of ground they pass and come home when the pack is full.
    Gatherer,
    /// All three crew with arms, who take every fight the screen offers and quick-resolve it.
    Brawler,
    /// Logs in and leaves.
    Idler,
}

struct Player {
    style: Style,
    name: &'static str,
    typed: VecDeque<DoorKey>,
    tried: HashMap<String, u32>,
    trips_today: u32,
    legs: u32,
    market_done: bool,
    shifts: u32,
}

impl Player {
    fn new(style: Style, name: &'static str) -> Self {
        Self {
            style,
            name,
            typed: VecDeque::new(),
            tried: HashMap::new(),
            trips_today: 0,
            legs: 0,
            market_done: false,
            shifts: 0,
        }
    }
}

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

/// How much of `what` the "Stores:" line says the clan holds.
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

/// Turns until the ground under the party goes under (99: not soon).
fn danger(screen: &[String]) -> u32 {
    screen
        .iter()
        .find_map(|l| {
            let rest = l.split("goes under at water ").nth(1)?;
            let n = rest.split("in about ").nth(1)?;
            n.split(' ').next()?.parse().ok()
        })
        .unwrap_or(99)
}

fn turns_home(screen: &[String]) -> u32 {
    screen
        .iter()
        .find_map(|l| {
            let at = l.find(" turns home")?;
            l[..at].rsplit(' ').next()?.parse().ok()
        })
        .unwrap_or(99)
}

fn load(screen: &[String]) -> (u32, u32) {
    screen
        .iter()
        .find_map(|l| {
            let rest = l.strip_prefix("Load ")?;
            let (a, b) = rest.split_once('/')?;
            let used = a.parse().ok()?;
            let cap = b
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()?;
            Some((used, cap))
        })
        .unwrap_or((0, 1))
}

impl Bot for Player {
    fn next_key(&mut self, screen: &[String], _visit: u32, sent: u32) -> Option<DoorKey> {
        if sent == 0 {
            self.tried.clear();
            self.trips_today = 0;
            self.legs = 0;
            self.market_done = false;
            self.shifts = 0;
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
        if self.style == Style::Idler {
            return Some(DoorKey::Escape);
        }
        let list = entries(screen);
        let turns = number_after(screen, "Turns ").unwrap_or(0);
        let has = |tag: &str| list.iter().any(|(_, t)| t.starts_with(tag));
        let press = |tag: &str| {
            list.iter()
                .find(|(_, t)| t.starts_with(tag))
                .map(|(k, _)| DoorKey::Char(*k))
        };
        self.legs += 1;
        if self.legs > 400 {
            return Some(DoorKey::Escape);
        }
        let once = |me: &mut Self, tag: &str, limit: u32| -> Option<DoorKey> {
            for (k, t) in &list {
                if t.starts_with(tag) {
                    let n = me.tried.entry(t.clone()).or_insert(0);
                    if *n < limit {
                        *n += 1;
                        return Some(DoorKey::Char(*k));
                    }
                }
            }
            None
        };
        // Planning.
        if screen.iter().any(|l| l.starts_with("Plan an expedition")) {
            let want = match self.style {
                Style::Greedy | Style::Brawler => 3,
                _ => 2,
            };
            // Nobody goes out short-handed: with fewer than two fit hands, stay and rest.
            if list.iter().filter(|(_, t)| t.starts_with("party:")).count() < 2 {
                return Some(DoorKey::Escape);
            }
            let picked = self
                .tried
                .iter()
                .filter(|(t, n)| t.starts_with("party:") && **n > 0)
                .count();
            if picked < want
                && let Some(k) = once(self, "party:", 1)
            {
                return Some(k);
            }
            let kit = if matches!(self.style, Style::Greedy | Style::Brawler) {
                "kit:heavy"
            } else {
                "kit:standard"
            };
            if let Some(k) = once(self, kit, 1) {
                return Some(k);
            }
            if self.style != Style::Gatherer
                && let Some(k) = once(self, "target:", 1)
            {
                return Some(k);
            }
            return press("depart").or(Some(DoorKey::Escape));
        }
        // On the marsh.
        if screen.iter().any(|l| l.starts_with("Party: ")) {
            if has("choose:") {
                // Read the options the way a player would: the risk and the fight warnings.
                let mut rows: Vec<(usize, u32, char)> = screen
                    .iter()
                    .filter_map(|l| {
                        let rest = l.strip_prefix('[')?;
                        let (key, text) = rest.split_once("] ")?;
                        let tag = text.split_whitespace().next()?;
                        let i: usize = tag.strip_prefix("choose:")?.parse().ok()?;
                        let risk = text
                            .split_once('[')
                            .and_then(|(_, r)| r.split('%').next()?.parse::<u32>().ok())
                            .unwrap_or(0);
                        let fight = if text.contains("(starts a fight)") {
                            1000
                        } else if text.contains("(a bad turn starts a fight)") {
                            200
                        } else {
                            0
                        };
                        Some((i, risk + fight, key.chars().next()?))
                    })
                    .collect();
                rows.sort_by_key(|(i, ..)| *i);
                let pick = if self.style == Style::Greedy {
                    rows.last()
                } else if self.style == Style::Brawler {
                    // Fights first (a sure one, then a likely one), then the safest.
                    rows.iter().max_by_key(|(i, score, _)| {
                        (
                            score / 200 * u32::from(*score >= 200),
                            std::cmp::Reverse(*score),
                            std::cmp::Reverse(*i),
                        )
                    })
                } else {
                    rows.iter().min_by_key(|(i, score, _)| (*score, *i))
                };
                return pick.map(|(_, _, k)| DoorKey::Char(*k));
            }
            if has("quick") {
                return press("quick");
            }
            let home = turns_home(screen);
            let at_hold = screen.iter().any(|l| l.contains(" at the hold."));
            let (used, cap) = load(screen);
            // How far from home the style will go, in turns of the way back.
            let (reserve, range) = match self.style {
                Style::Greedy => (2, 14),
                Style::Brawler => (4, 10),
                Style::Gatherer => (6, 7),
                _ => (8, 7),
            };
            let full = self.style == Style::Gatherer && used + 3 >= cap;
            // The tide: if the ground here will go under before the party can be home with a
            // margin for the style, leave it now.
            let slack = match self.style {
                Style::Greedy => 0,
                Style::Gatherer => 2,
                _ => 3,
            };
            let flooding = !at_hold && danger(screen) <= home.min(12) + slack;
            self.market_done = false;
            if !at_hold && (turns <= home + reserve || full || flooding) && has("home") {
                return press("home");
            }
            let near = home <= range || at_hold;
            if self.style == Style::Gatherer
                && !at_hold
                && !flooding
                && let Some(k) = once(self, "gather", 99)
            {
                self.tried.retain(|t, _| t != "gather");
                return Some(k);
            }
            if has("march") {
                return press("march");
            }
            if let Some(k) = once(self, "go:", 1) {
                return Some(k);
            }
            if self.legs.is_multiple_of(3)
                && !at_hold
                && !flooding
                && let Some(k) = press("gather")
            {
                return Some(k);
            }
            let dirs = ["explore:n", "explore:e", "explore:w", "explore:s"];
            if near {
                for i in 0..4 {
                    if let Some(k) = press(dirs[(usize::try_from(self.legs).unwrap() + i) % 4]) {
                        return Some(k);
                    }
                }
            }
            return press("home")
                .or_else(|| press("camp:3"))
                .or(Some(DoorKey::Escape));
        }
        // The market, between trips.
        if list.iter().any(|(_, t)| t.starts_with("buy:")) {
            // Sell the cargo (keep the food, the peat, the rope and the tools): one turn each.
            for sell in [
                "sell:iron",
                "sell:silt",
                "sell:relic",
                "sell:salt",
                "sell:timber",
                "sell:reed",
                "sell:fish",
            ] {
                if let Some(k) = once(self, sell, 1) {
                    return Some(k);
                }
            }
            if stock(screen, "preserved food") < 14
                && number_after(screen, "Tallies ").unwrap_or(0) >= 12
                && let Some(k) = once(self, "buy:food", 3)
            {
                return Some(k);
            }
            if stock(screen, "peat") < 8
                && number_after(screen, "Tallies ").unwrap_or(0) >= 6
                && let Some(k) = once(self, "buy:peat", 1)
            {
                return Some(k);
            }
            self.market_done = true;
            return Some(DoorKey::Escape);
        }
        // The hold's menu: a real player keeps the hold going too. Three shifts a day, two at
        // the fish weir and one at the peat stack, before going out.
        if screen.iter().any(|l| l.starts_with("Plots ")) {
            if self.shifts < 3 {
                self.shifts += 1;
                let first = if self.shifts <= 2 {
                    "fish-weir"
                } else {
                    "peat-stack"
                };
                if let Some(k) =
                    press(&format!("pair:{first}")).or_else(|| press(&format!("shift:{first}")))
                {
                    return Some(k);
                }
            }
            return Some(DoorKey::Escape);
        }
        // At the hold.
        if self.shifts < 3 && turns >= 12 && has("go:hold") {
            return press("go:hold");
        }
        if !self.market_done && has("go:market") {
            self.market_done = true;
            return press("go:market");
        }
        if has("go:expedition") && turns >= 10 && self.trips_today < 1 {
            self.trips_today += 1;
            return press("go:expedition");
        }
        Some(DoorKey::Escape)
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    clans: u32,
    departures: u32,
    returned: u32,
    cut_off: u32,
    swept: u32,
    routed: u32,
    lost_parties: u32,
    deaths: u32,
    fights: u32,
    fights_won: u32,
    injuries: u32,
    loot: u32,
    tallies: u32,
    crew: u32,
    bonds: u32,
}

async fn play(seed: u64) -> Vec<(Style, &'static str, Clan)> {
    let door: std::sync::Arc<dyn Door> = Saltmarch::shared().unwrap();
    let mut sim = Simulation::new(door.clone(), Day::from_raw(START), seed);
    let roster = [
        (Style::Cautious, "Ashlock"),
        (Style::Cautious, "Brinemere"),
        (Style::Cautious, "Copperfen"),
        (Style::Greedy, "Drystake"),
        (Style::Greedy, "Eelgrass"),
        (Style::Greedy, "Fogbank"),
        (Style::Gatherer, "Gullcry"),
        (Style::Gatherer, "Hollowmere"),
        (Style::Gatherer, "Ironsalt"),
        (Style::Brawler, "Kelpgate"),
        (Style::Brawler, "Lantern"),
        (Style::Brawler, "Mudlark"),
        (Style::Idler, "Nettle"),
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
        out.push((*style, *name, Clan::decode(&stored.bytes).unwrap()));
    }
    out
}

fn tally(rows: &[(Style, &'static str, Clan)], style: Style) -> Tally {
    let mut t = Tally::default();
    for (s, _, c) in rows.iter().filter(|(s, ..)| *s == style) {
        let _ = s;
        let st = &c.stats;
        t.clans += 1;
        t.departures += st.departures;
        t.returned += st.returned;
        t.cut_off += st.cut_off;
        t.swept += st.swept;
        t.routed += st.routed;
        t.lost_parties += st.lost_parties;
        t.deaths += st.lost_on_trips;
        t.fights += st.fights;
        t.fights_won += st.fights_won;
        t.injuries += st.injuries;
        t.loot += st.loot_value;
        t.tallies += c.tallies;
        t.crew += u32::try_from(c.crew.len()).unwrap();
        t.bonds += st.bonds;
    }
    t
}

#[tokio::test]
async fn sixty_days_of_expeditions_show_the_race_against_the_flood_pays_and_costs() {
    let mut all = Vec::new();
    for seed in [2026u64, 99] {
        all.extend(play(seed).await);
    }
    let styles = [
        Style::Cautious,
        Style::Greedy,
        Style::Gatherer,
        Style::Brawler,
        Style::Idler,
    ];
    eprintln!("\nSaltmarch expeditions, headless: {DAYS} days, 13 clans x 2 seeds (26 clans)");
    eprintln!(
        "{:<9} {:>5} {:>5} {:>6} {:>6} {:>5} {:>6} {:>5} {:>6} {:>5} {:>5} {:>6} {:>7} {:>5} {:>5}",
        "style",
        "clans",
        "trips",
        "home%",
        "cutoff",
        "swept",
        "routed",
        "lost",
        "deaths",
        "fight",
        "won%",
        "hurt",
        "loot/tr",
        "crew",
        "bonds"
    );
    for style in styles {
        let t = tally(&all, style);
        let trips = f64::from(t.departures.max(1));
        eprintln!(
            "{:<9} {:>5} {:>5} {:>5.0}% {:>6} {:>5} {:>6} {:>5} {:>6} {:>5} {:>4.0}% {:>6} {:>7.1} {:>5.1} {:>5}",
            format!("{style:?}"),
            t.clans,
            t.departures,
            100.0 * f64::from(t.returned) / trips,
            t.cut_off,
            t.swept,
            t.routed,
            t.lost_parties,
            t.deaths,
            t.fights,
            100.0 * f64::from(t.fights_won) / f64::from(t.fights.max(1)),
            t.injuries,
            f64::from(t.loot) / trips,
            f64::from(t.crew) / f64::from(t.clans),
            t.bonds,
        );
    }
    let (c, g, w, b, i) = (
        tally(&all, Style::Cautious),
        tally(&all, Style::Greedy),
        tally(&all, Style::Gatherer),
        tally(&all, Style::Brawler),
        tally(&all, Style::Idler),
    );
    // The idler never leaves the hold.
    assert_eq!((i.departures, i.fights, i.deaths), (0, 0, 0));
    // The others went out, often.
    for t in [c, g, w, b] {
        assert!(t.departures >= 60, "{t:?}");
    }
    // The race against the flood is a real choice: cutting it fine costs more trips than
    // keeping a margin does, and staying close to home costs fewer still.
    let bad = |t: Tally| f64::from(t.cut_off + t.swept) / f64::from(t.departures);
    assert!(
        bad(g) > bad(c),
        "greedy {:.3} against cautious {:.3}",
        bad(g),
        bad(c)
    );
    assert!(
        bad(c) > bad(w),
        "cautious {:.3} against gatherer {:.3}",
        bad(c),
        bad(w)
    );
    assert!(
        g.returned * 5 > g.departures * 2,
        "even the greedy get home 40% of the time: {g:?}"
    );
    // The brawlers fought, and won more often than not; fights cost wounds and sometimes lives.
    assert!(b.fights >= 20, "{b:?}");
    assert!(f64::from(b.fights_won) > 0.5 * f64::from(b.fights), "{b:?}");
    assert!(b.injuries > c.injuries / 2);
    assert!(b.deaths + b.routed >= 1, "and fights are not free: {b:?}");
    // Nobody's save outgrows the cap.
    assert!(all.iter().all(|(_, _, c)| c.encode().len() < 8_000));
}
