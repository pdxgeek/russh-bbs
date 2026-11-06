//! The season balance harness (M4-S5): sixteen clans in four styles (builder, raider, diplomat,
//! trader) play a whole 8-week season through the framework's simulation (the real door, the real
//! store, the real daily tick), reading the text screen and answering with keys. Every style
//! also works a shift or two, goes out on expeditions and takes contracts, as a player would;
//! what differs is what each leans on.
//!
//! It reports, per style: the economy curve (wealth every week), renown, raids sent, won and
//! beaten off, pacts signed and broken, contracts, quest chains, causeways, tolls; and for the
//! board: the ending the season came to, and how much of the scene content the bots reached
//! (branch coverage). `cargo test -p russh-bbs-door-saltmarch --test season_sim -- --nocapture`
//! prints the tables. The assertions are the balance floors this slice tuned the game to.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::print_stderr,
    clippy::cast_precision_loss,
    clippy::many_single_char_names
)]

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;

use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::limits::Day;
use russh_bbs_core::store::AccountId;
use russh_bbs_door::sim::{Bot, Simulation};
use russh_bbs_door::{Door, DoorKey};
use russh_bbs_door_saltmarch::Saltmarch;
use russh_bbs_door_saltmarch::clan::Clan;
use russh_bbs_door_saltmarch::social::pact::State;
use russh_bbs_door_saltmarch::types::Resource;
use russh_bbs_door_saltmarch::world::World;

const START: i64 = 20_000;
/// A season: eight weeks.
const DAYS: u32 = 56;
const PER_STYLE: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
enum Style {
    /// Builds, works shifts, sells surplus, short safe trips, delivers goods to the boards.
    Builder,
    /// Recruits, arms, raids, takes clear contracts and fights what it meets.
    Raider,
    /// Letters, pacts kept, lays causeways, escorts, and takes the road of the sociable answer.
    Diplomat,
    /// Lives on the exchange and the market, salvages for the boards, sells everything.
    Trader,
}

enum Step {
    Tag(&'static str),
    Key(DoorKey),
    Seek {
        find: &'static str,
        turn: &'static str,
        max: u8,
    },
    /// Take the first contract on the board whose line holds this word.
    Take(&'static str),
    /// Go out on an expedition if there is time.
    Trip,
}

struct Player {
    style: Style,
    name: String,
    salt: u64,
    typed: VecDeque<DoorKey>,
    script: VecDeque<Step>,
    tried: HashMap<String, u32>,
    seeking: u8,
    legs: u32,
    trips_today: u32,
    shifts: u32,
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

fn esc() -> Step {
    Step::Key(DoorKey::Escape)
}

fn script(style: Style, visit: u32, salt: u64) -> VecDeque<Step> {
    use Step::{Seek, Tag, Trip};
    let mut s: VecDeque<Step> = VecDeque::new();
    // The post, then the hold's two shifts: the hold is the economy.
    s.extend([
        Tag("go:court"),
        Tag("go:letters"),
        Tag("accept:"),
        esc(),
        esc(),
    ]);
    s.extend([Tag("go:hold"), Tag("shift:"), Tag("shift:"), esc()]);
    // The boards: every style takes the contract that suits it (by the word in the line).
    s.extend([
        Tag("go:court"),
        Tag("go:board"),
        Step::Take(match style {
            Style::Builder => "(deliver",
            Style::Raider => "(clear",
            Style::Diplomat => "(escort",
            Style::Trader => "(salvage",
        }),
        Tag("hand-over"),
        esc(),
        esc(),
    ]);
    // Everyone keeps building a little: a hold is how a clan is measured.
    s.extend([Tag("go:hold"), Tag("build:"), Tag("labour:")]);
    if style == Style::Builder {
        s.extend([Tag("build:"), Tag("labour:"), Tag("labour:"), Tag("labour:")]);
    }
    s.push_back(esc());
    match style {
        Style::Builder => {
            s.extend([Trip, Tag("go:court"), Tag("go:exchange")]);
            for _ in 0..4 {
                s.extend([Tag("ask:"), Tag("good:next")]);
            }
        }
        Style::Raider => {
            // A raider recruits when the crew is thin, not every day, goes out with what is
            // left of the crew, and sends the raid last (the raiders are away until the report).
            if visit % 5 == 1 {
                s.extend([Tag("go:crew"), Tag("recruit:"), esc()]);
            }
            s.extend([
                Trip,
                Tag("go:court"),
                Tag("go:raid"),
                Seek {
                    find: "raid:send:",
                    turn: "target:next",
                    max: 12,
                },
                Tag("raid:send:"),
                esc(),
                esc(),
            ]);
        }
        Style::Diplomat => {
            s.extend([
                Tag("go:court"),
                Tag("go:letters"),
                Tag("compose"),
                Tag("send:"),
                esc(),
                Tag("go:pacts"),
                Seek {
                    find: "offer:",
                    turn: "ptarget:next",
                    max: 12,
                },
                Tag("offer:"),
                esc(),
                esc(),
                Trip,
            ]);
            // A pact is broken now and then (two presses): the diplomat is not a saint.
            // Only one diplomat in four ever does.
            if visit == 40 && salt % 4 == 1 {
                s.extend([Tag("go:court"), Tag("go:pacts"), Tag("break:"), Tag("break:")]);
            }
        }
        Style::Trader => {
            s.extend([Trip, Tag("go:court"), Tag("go:exchange")]);
            for _ in 0..4 {
                s.extend([Tag("xbuy:"), Tag("xsell:"), Tag("bid:"), Tag("ask:"), Tag("good:next")]);
            }
        }
    }
    s.extend([Tag("go:market")]);
    s
}

impl Player {
    fn new(style: Style, name: String, salt: u64) -> Self {
        Self {
            style,
            name,
            salt,
            typed: VecDeque::new(),
            script: VecDeque::new(),
            tried: HashMap::new(),
            seeking: 0,
            legs: 0,
            trips_today: 0,
            shifts: 0,
        }
    }

    fn pick_option(&self, screen: &[String]) -> Option<DoorKey> {
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
        if rows.is_empty() {
            return None;
        }
        // The builder is careful; the raider takes the fight when there is one; the trader and
        // the diplomat choose by temperament (a stable hash of who they are and where they are),
        // so across a season they walk different branches.
        let pick = match self.style {
            Style::Builder => rows.iter().min_by_key(|(i, score, _)| (*score, *i)),
            Style::Raider => rows
                .iter()
                .max_by_key(|(i, score, _)| (*score >= 200, std::cmp::Reverse(*score), std::cmp::Reverse(*i))),
            Style::Diplomat | Style::Trader => {
                let h = self
                    .salt
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .wrapping_add(u64::from(self.legs).wrapping_mul(0xD1B5_4A32_D192_ED03));
                let safe: Vec<_> = rows.iter().filter(|(_, s, _)| *s < 200).collect();
                let pool = if safe.is_empty() {
                    rows.iter().collect()
                } else {
                    safe
                };
                pool.get(usize::try_from((h >> 33) % u64::try_from(pool.len()).unwrap()).unwrap())
                    .copied()
            }
        };
        pick.map(|(_, _, k)| DoorKey::Char(*k))
    }
}

impl Bot for Player {
    fn next_key(&mut self, screen: &[String], visit: u32, sent: u32) -> Option<DoorKey> {
        let k = self.next_key_inner(screen, visit, sent);
        if std::env::var("SEASON_SIM_DEBUG").is_ok_and(|w| w == self.name) && (2..=3).contains(&visit) {
            eprintln!("[{} v{visit} #{sent}] {:?}\n  top: {}", self.name, k, screen.iter().take(3).cloned().collect::<Vec<_>>().join(" | "));
        }
        k
    }
}

impl Player {
    fn next_key_inner(&mut self, screen: &[String], visit: u32, sent: u32) -> Option<DoorKey> {
        if sent == 0 {
            self.script = script(self.style, visit, self.salt);
            self.tried.clear();
            self.seeking = 0;
            self.legs = 0;
            self.trips_today = 0;
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
        let list = entries(screen);
        let turns = number_after(screen, "Turns ").unwrap_or(0);
        let has = |tag: &str| list.iter().any(|(_, t)| t.starts_with(tag));
        let press = |tag: &str| {
            list.iter()
                .find(|(_, t)| t.starts_with(tag))
                .map(|(k, _)| DoorKey::Char(*k))
        };
        self.legs += 1;
        if self.legs > 500 {
            return Some(DoorKey::Escape);
        }
        // Planning an expedition.
        if screen.iter().any(|l| l.starts_with("Plan an expedition")) {
            let want = match self.style {
                Style::Raider => 3,
                _ => 2,
            };
            if list.iter().filter(|(_, t)| t.starts_with("party:")).count() < 2 {
                return Some(DoorKey::Escape);
            }
            let picked = self
                .tried
                .iter()
                .filter(|(t, n)| t.starts_with("party:") && **n > 0)
                .count();
            if picked < want {
                // A diplomat wants hands that can lay a causeway.
                let builders: Vec<char> = screen
                    .iter()
                    .filter_map(|l| {
                        let rest = l.strip_prefix('[')?;
                        let (key, text) = rest.split_once("] ")?;
                        (text.starts_with("party:")
                            && (text.contains("wader") || text.contains("boatwright")))
                        .then(|| key.chars().next().unwrap())
                    })
                    .collect();
                if self.style == Style::Diplomat {
                    for k in builders {
                        let tag = format!("builder-{k}");
                        if !self.tried.contains_key(&tag) {
                            self.tried.insert(tag, 1);
                            self.tried.insert(format!("party:picked-{k}"), 1);
                            return Some(DoorKey::Char(k));
                        }
                    }
                }
                for (k, t) in &list {
                    if t.starts_with("party:") {
                        let n = self.tried.entry(t.clone()).or_insert(0);
                        if *n == 0 {
                            *n += 1;
                            return Some(DoorKey::Char(*k));
                        }
                    }
                }
            }
            let kit = match self.style {
                Style::Raider | Style::Diplomat => "kit:heavy",
                _ => "kit:standard",
            };
            for (k, t) in &list {
                if t == kit {
                    let n = self.tried.entry(t.clone()).or_insert(0);
                    if *n == 0 {
                        *n += 1;
                        return Some(DoorKey::Char(*k));
                    }
                }
            }
            // Different styles aim at different ground, so the season visits all of it.
            let key = format!("target-{}", self.legs % 3);
            if self.style != Style::Builder && !self.tried.contains_key(&key) {
                for (k, t) in &list {
                    if t.starts_with("target:") {
                        self.tried.insert(key, 1);
                        return Some(DoorKey::Char(*k));
                    }
                }
            }
            return press("depart").or(Some(DoorKey::Escape));
        }
        // On the marsh.
        if screen.iter().any(|l| l.starts_with("Party: ")) {
            if has("choose:") {
                return self.pick_option(screen);
            }
            if has("quick") {
                return press("quick");
            }
            let home = turns_home(screen);
            let at_hold = screen.iter().any(|l| l.contains(" at the hold."));
            let (reserve, range) = match self.style {
                Style::Raider => (4, 10),
                Style::Builder => (8, 6),
                Style::Diplomat => (6, 8),
                Style::Trader => (5, 9),
            };
            let flooding = !at_hold && danger(screen) <= home.min(12) + 3;
            if !at_hold && (turns <= home + reserve || flooding) && has("home") {
                return press("home");
            }
            // The diplomat lays a causeway when it can, once or twice a season.
            if self.style == Style::Diplomat && !at_hold && !flooding && self.legs % 4 == 0 {
                if let Some(k) = press("lay") {
                    return Some(k);
                }
            }
            if has("march") {
                return press("march");
            }
            if has("go:")
                && let Some((k, t)) = list.iter().find(|(_, t)| t.starts_with("go:"))
                && self.tried.get(t).copied().unwrap_or(0) < 1
            {
                self.tried.insert(t.clone(), 1);
                return Some(DoorKey::Char(*k));
            }
            if self.legs.is_multiple_of(3)
                && !at_hold
                && !flooding
                && let Some(k) = press("gather")
            {
                return Some(k);
            }
            let near = home <= range || at_hold;
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
        // The market: sell what the trip brought, keep the food and the peat.
        if list.iter().any(|(_, t)| t.starts_with("buy:")) {
            for sell in [
                "sell:iron",
                "sell:silt",
                "sell:relic",
                "sell:salt",
                "sell:timber",
                "sell:reed",
                "sell:fish",
            ] {
                for (k, t) in &list {
                    if t.starts_with(sell) {
                        let n = self.tried.entry(t.clone()).or_insert(0);
                        if *n < 1 {
                            *n += 1;
                            return Some(DoorKey::Char(*k));
                        }
                    }
                }
            }
            if stock(screen, "preserved food") < 14
                && number_after(screen, "Tallies ").unwrap_or(0) >= 12
                && let Some((k, t)) = list.iter().find(|(_, t)| t.starts_with("buy:food"))
                && self.tried.get(t).copied().unwrap_or(0) < 3
            {
                *self.tried.entry(t.clone()).or_insert(0) += 1;
                return Some(DoorKey::Char(*k));
            }
            return Some(DoorKey::Escape);
        }
        // The script.
        loop {
            match self.script.pop_front()? {
                Step::Key(k) => return Some(k),
                Step::Tag(t) => {
                    if let Some(k) = press(t) {
                        // A tag that is tried over and over (shifts) is limited by the screen.
                        return Some(k);
                    }
                }
                Step::Take(word) => {
                    // The line is "[k] take:<id> Take: <title> (<kind> ...".
                    if let Some(k) = screen.iter().find_map(|l| {
                        let rest = l.strip_prefix('[')?;
                        let (key, text) = rest.split_once("] ")?;
                        (text.starts_with("take:") && text.contains(word))
                            .then(|| DoorKey::Char(key.chars().next().unwrap()))
                    }) {
                        return Some(k);
                    }
                    return press("take:");
                }
                Step::Trip => {
                    if turns >= 12 && self.trips_today < 1 && has("go:expedition") {
                        self.trips_today += 1;
                        return press("go:expedition");
                    }
                }
                Step::Seek { find, turn, max } => {
                    if let Some(k) = press(find) {
                        return Some(k);
                    }
                    if self.seeking < max
                        && let Some(k) = press(turn)
                    {
                        self.seeking += 1;
                        self.script.push_front(Step::Seek { find, turn, max });
                        return Some(k);
                    }
                }
            }
        }
    }
}

struct Outcome {
    clans: Vec<(Style, Clan)>,
    world: World,
    /// Mean wealth (tallies plus stores at base price) of each style at the end of each week.
    curve: BTreeMap<Style, Vec<f64>>,
    world_bytes: usize,
}

fn wealth(c: &Clan) -> u32 {
    c.tallies
        + Resource::ALL
            .iter()
            .map(|r| c.stock.get(*r) * u32::from(r.base_price()))
            .sum::<u32>()
}

const NAMES: [&str; 16] = [
    "Ashlock", "Brinemere", "Copperfen", "Drystake", "Eelgrass", "Fogbank", "Gullcry",
    "Hollowmere", "Ironsalt", "Kelpgate", "Lantern", "Mudlark", "Nettlebed", "Ospreyfen",
    "Peatgate", "Quillreed",
];

async fn clans_of(
    sim: &Simulation,
    door: &Arc<dyn Door>,
    order: &[Style],
) -> Vec<(Style, Clan)> {
    let id = door.info().id.clone();
    let mut clans = Vec::new();
    for (i, style) in order.iter().enumerate() {
        let save = sim
            .store()
            .load_save(&id, AccountId::from_raw(i64::try_from(i).unwrap() + 1))
            .await
            .unwrap()
            .map(|s| s.bytes);
        if let Some(c) = save.as_deref().and_then(Clan::decode) {
            clans.push((*style, c));
        }
    }
    clans
}

async fn play(seed: u64) -> Outcome {
    let door: Arc<dyn Door> = Saltmarch::shared().unwrap();
    let mut sim = Simulation::new(door.clone(), Day::from_raw(START), seed);
    let styles = [Style::Builder, Style::Raider, Style::Diplomat, Style::Trader];
    let mut order = Vec::new();
    for i in 0..PER_STYLE * styles.len() {
        let style = styles[i % styles.len()];
        order.push(style);
        sim.add_player(Box::new(Player::new(
            style,
            NAMES[i].to_owned(),
            seed ^ (i as u64 + 1),
        )));
    }
    let mut curve: BTreeMap<Style, Vec<f64>> = BTreeMap::new();
    for _ in 0..DAYS / 7 {
        sim.run_days(7).await.unwrap();
        let now = clans_of(&sim, &door, &order).await;
        for style in styles {
            let rows: Vec<f64> = now
                .iter()
                .filter(|(s, _)| *s == style)
                .map(|(_, c)| f64::from(wealth(c)))
                .collect();
            curve
                .entry(style)
                .or_default()
                .push(rows.iter().sum::<f64>() / rows.len().max(1) as f64);
        }
    }
    let id = door.info().id.clone();
    let world_bytes = sim.store().load_world(&id).await.unwrap().unwrap().bytes;
    let world = World::decode(&world_bytes).unwrap();
    let clans = clans_of(&sim, &door, &order).await;
    Outcome {
        clans,
        world,
        curve,
        world_bytes: world_bytes.len(),
    }
}

fn run_seeds(seeds: &[u64]) -> Vec<Outcome> {
    std::thread::scope(|s| {
        let handles: Vec<_> = seeds
            .iter()
            .map(|seed| {
                let seed = *seed;
                s.spawn(move || {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap()
                        .block_on(play(seed))
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

fn mean(rows: &[&Clan], f: &dyn Fn(&Clan) -> f64) -> f64 {
    rows.iter().map(|c| f(c)).sum::<f64>() / rows.len().max(1) as f64
}

#[test]
fn a_season_of_four_styles_is_balanced_and_the_content_is_reached() {
    let seeds = [2026u64, 99, 7, 314];
    let outs = run_seeds(&seeds);
    let cat = Saltmarch::new().unwrap();
    let cat = cat.catalog();
    let styles = [Style::Builder, Style::Raider, Style::Diplomat, Style::Trader];

    eprintln!(
        "\nSaltmarch season sim: {DAYS} days, {} clans per seed ({PER_STYLE} per style), seeds {seeds:?}",
        PER_STYLE * 4
    );
    // Economy curves.
    eprintln!("\nEconomy: mean wealth (tallies + stores at base price) at the end of each week");
    eprint!("{:<9}", "style");
    for w in 1..=DAYS / 7 {
        eprint!(" {:>6}", format!("wk{w}"));
    }
    eprintln!();
    for style in styles {
        eprint!("{:<9}", format!("{style:?}"));
        for w in 0..(DAYS / 7) as usize {
            let v: f64 = outs.iter().map(|o| o.curve[&style][w]).sum::<f64>() / outs.len() as f64;
            eprint!(" {v:>6.0}");
        }
        eprintln!();
    }
    // Per style table.
    eprintln!("\nPer style, mean per clan over all seeds");
    eprintln!(
        "{:<9} {:>6} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>6}",
        "style", "renown", "raids", "won", "lost", "held", "crew", "dead", "bld", "cntr", "fail",
        "chain", "laid", "tollP", "tollE"
    );
    let mut renown: BTreeMap<Style, f64> = BTreeMap::new();
    let mut extra: Vec<String> = Vec::new();
    let mut raids_won = 0u32;
    let mut raids_sent = 0u32;
    for style in styles {
        let rows: Vec<(&Clan, u32)> = outs
            .iter()
            .flat_map(|o| {
                o.clans.iter().filter(|(s, _)| *s == style).map(|(_, c)| {
                    let score = o
                        .world
                        .ix(&c.name)
                        .map_or(0, |i| o.world.clans[usize::from(i)].score);
                    (c, score)
                })
            })
            .collect();
        let cl: Vec<&Clan> = rows.iter().map(|(c, _)| *c).collect();
        let r = f64::from(rows.iter().map(|(_, s)| *s).sum::<u32>()) / rows.len() as f64;
        renown.insert(style, r);
        let chains = |c: &Clan| cat.quests_done(|m| c.has_mark(m));
        extra.push(format!(
            "{:<9} trips {:>4.1} home {:>4.1} lost-on-trips {:>4.1} fights {:>4.1} won {:>4.1} loot/trip {:>5.1} plundered {:>5.1} robbed {:>5.1}",
            format!("{style:?}"),
            mean(&cl, &|c| f64::from(c.stats.departures)),
            mean(&cl, &|c| f64::from(c.stats.returned)),
            mean(&cl, &|c| f64::from(c.stats.lost_on_trips)),
            mean(&cl, &|c| f64::from(c.stats.fights)),
            mean(&cl, &|c| f64::from(c.stats.fights_won)),
            mean(&cl, &|c| f64::from(c.stats.loot_value) / f64::from(c.stats.returned.max(1))),
            mean(&cl, &|c| f64::from(c.social.plundered)),
            mean(&cl, &|c| f64::from(c.social.robbed)),
        ));
        raids_won += cl.iter().map(|c| u32::from(c.social.raids_won)).sum::<u32>();
        raids_sent += cl.iter().map(|c| u32::from(c.social.raids_sent)).sum::<u32>();
        eprintln!(
            "{:<9} {:>6.1} {:>5.1} {:>5.1} {:>5.1} {:>5.1} {:>5.1} {:>5.1} {:>5.1} {:>5.1} {:>5.1} {:>5.1} {:>5.1} {:>5.1} {:>6.1}",
            format!("{style:?}"),
            r,
            mean(&cl, &|c| f64::from(c.social.raids_sent)),
            mean(&cl, &|c| f64::from(c.social.raids_won)),
            mean(&cl, &|c| f64::from(c.social.raids_lost)),
            mean(&cl, &|c| f64::from(c.social.repelled)),
            mean(&cl, &|c| c.crew.len() as f64),
            mean(&cl, &|c| f64::from(c.stats.lost)),
            mean(&cl, &|c| c.hold.built(cat).count() as f64),
            mean(&cl, &|c| f64::from(c.story.done)),
            mean(&cl, &|c| f64::from(c.story.failed)),
            mean(&cl, &|c| chains(c) as f64),
            mean(&cl, &|c| f64::from(c.story.laid)),
            mean(&cl, &|c| f64::from(c.story.tolls_paid)),
            mean(&cl, &|c| f64::from(c.story.toll_income)),
        );
    }
    for line in &extra {
        eprintln!("{line}");
    }
    // Raids, pacts and endings per seed.
    eprintln!("\nPer seed: raids, pacts, trade, ending");
    let mut endings: BTreeMap<String, u32> = BTreeMap::new();
    let mut signed_total = 0;
    let mut broken_total = 0;
    for (seed, o) in seeds.iter().zip(&outs) {
        let w = &o.world;
        let signed = w
            .social
            .pacts
            .iter()
            .filter(|p| p.state == State::Active)
            .count();
        let broken = w
            .social
            .pacts
            .iter()
            .filter(|p| p.state == State::Broken)
            .count();
        signed_total += signed;
        broken_total += broken;
        let ending = russh_bbs_door_saltmarch::social::hall::which_ending(w);
        *endings.entry(ending.name().to_owned()).or_default() += 1;
        let tally = russh_bbs_door_saltmarch::social::legacy::Tally::of(w);
        eprintln!(
            "seed {seed:>5}: raids won {:>2}, pacts in force {signed}, broken {broken}, traded {}, deep {}, chains {}, contracts {}, causeways {}, world {} B -> {}",
            tally.raids, tally.traded, tally.deep, tally.quests, tally.contracts, tally.laid,
            o.world_bytes, ending.name()
        );
        assert!(o.world_bytes < russh_bbs_door_saltmarch::social::caps::WORLD_SOFT_CAP);
    }
    let pact_total = signed_total + broken_total;
    eprintln!(
        "raids: {raids_sent} sent, {raids_won} won ({:.0}%); pacts: {signed_total} in force, {broken_total} broken ({:.0}% of all signed)",
        f64::from(raids_won) * 100.0 / f64::from(raids_sent.max(1)),
        broken_total as f64 * 100.0 / pact_total.max(1) as f64
    );
    eprintln!("endings this run: {endings:?}");
    // Branch coverage: which scene branches did any clan take?
    let bits = cat.branch_count();
    let mut taken = vec![false; bits];
    for o in &outs {
        for (_, c) in &o.clans {
            for (b, t) in taken.iter_mut().enumerate() {
                *t |= c.story.branch_seen(b);
            }
        }
    }
    let hit = taken.iter().filter(|t| **t).count();
    let mut scenes_hit = 0;
    let mut never: Vec<&str> = Vec::new();
    for (ei, e) in cat.events.iter().enumerate() {
        let any = (0..e.options.len())
            .any(|oi| taken[cat.branch_bit(ei, oi, false)] || taken[cat.branch_bit(ei, oi, true)]);
        if any {
            scenes_hit += 1;
        } else {
            never.push(&e.id);
        }
    }
    eprintln!(
        "\nBranch coverage: {hit} of {bits} option outcomes taken; {scenes_hit} of {} scenes met",
        cat.events.len()
    );
    let q_total = cat.quests.len();
    let mut q_started = 0;
    let mut ends_seen: BTreeMap<String, u32> = BTreeMap::new();
    for q in &cat.quests {
        let mut started = false;
        for o in &outs {
            for (_, c) in &o.clans {
                started |= c.has_mark(&q.steps[0].mark);
                for e in &q.ends {
                    if c.has_mark(&e.mark) {
                        *ends_seen.entry(e.mark.clone()).or_default() += 1;
                    }
                }
            }
        }
        q_started += usize::from(started);
    }
    eprintln!(
        "Quests: {q_started} of {q_total} chains begun by some clan, {} of {} ends reached",
        ends_seen.len(),
        cat.quests.iter().map(|q| q.ends.len()).sum::<usize>()
    );
    let unseen_ends: Vec<&str> = cat
        .quests
        .iter()
        .flat_map(|q| q.ends.iter().map(|e| e.mark.as_str()))
        .filter(|m| !ends_seen.contains_key(*m))
        .collect();
    eprintln!("Ends never reached: {unseen_ends:?}");
    eprintln!("Scenes never met ({}): {}", never.len(), never.join(", "));

    // Floors this slice tuned for: every style is viable.
    for style in styles {
        assert!(renown[&style] > 8.0, "{style:?} is not viable: {renown:?}");
    }
    let lo = renown.values().copied().fold(f64::MAX, f64::min);
    let hi = renown.values().copied().fold(0.0, f64::max);
    assert!(hi < lo * 2.6, "one style runs away with the league: {renown:?}");
    assert!(raids_sent > 0);
}

#[test]
fn the_season_run_is_deterministic() {
    let a = run_seeds(&[55]);
    let b = run_seeds(&[55]);
    assert_eq!(a[0].world, b[0].world);
    let bytes = |o: &Outcome| o.clans.iter().map(|(_, c)| c.encode()).collect::<Vec<_>>();
    assert_eq!(bytes(&a[0]), bytes(&b[0]));
}
