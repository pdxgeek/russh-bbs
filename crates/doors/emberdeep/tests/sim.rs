//! The headless balance run: bots that read the door's text view (the same lines a screen reader
//! gets) play thirty days through the real open, save, tick and flush path. The numbers it prints
//! (`cargo test -p russh-bbs-door-emberdeep --test sim -- --nocapture`) are the ones in the pull
//! request; the assertions only guard the shape of the game, not a balance target.

#![allow(
    clippy::unwrap_used,
    clippy::print_stdout,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::struct_excessive_bools,
    clippy::unnecessary_wraps,
    clippy::cognitive_complexity,
    clippy::collapsible_if
)]

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::limits::Day;
use russh_bbs_core::limits::{Clock, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::sim::{Bot, Simulation};
use russh_bbs_door::{DoorEnv, DoorKey};
use russh_bbs_door_emberdeep::{Emberdeep, Summary, Whereabouts, summarize, summarize_world};

const DAY: i64 = 20_000;
const NODE_KEYS: &str = "123456789abcdef";

/// `[k] label` entries of a screen, in order.
fn entries(screen: &[String]) -> Vec<(char, String)> {
    let mut out = Vec::new();
    for line in screen {
        let mut rest = line.as_str();
        while let Some(at) = rest.find('[') {
            let tail = &rest[at + 1..];
            let Some(close) = tail.find(']') else { break };
            let key = &tail[..close];
            let label_start = &tail[close + 1..];
            let end = label_start.find("   [").unwrap_or(label_start.len());
            let mut chars = key.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                out.push((c, label_start[..end].trim().to_owned()));
            }
            rest = &label_start[end..];
        }
    }
    out
}

fn field(screen: &[String], name: &str) -> Option<(i32, i32)> {
    for line in screen {
        if let Some(at) = line.find(name) {
            let tail = &line[at + name.len()..];
            let word = tail.split_whitespace().next()?;
            let (a, b) = word.split_once('/').unwrap_or((word, "0"));
            return Some((a.parse().ok()?, b.parse().unwrap_or(0)));
        }
    }
    None
}

fn has(screen: &[String], text: &str) -> bool {
    screen.iter().any(|l| l.contains(text))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    /// Goes about one level per three hero levels; plays fights out by hand.
    Careful,
    /// Goes deeper, and quick-resolves fights.
    Greedy,
}

struct Delver {
    calling: char,
    traits: [char; 2],
    attr_key: char,
    style: Style,
    queue: VecDeque<DoorKey>,
    plan: VecDeque<char>,
    attempted: HashSet<(String, String)>,
    descended: bool,
    skills_done: bool,
    skill_cursor: usize,
    want_map: bool,
    created_points: usize,
    /// The map showed nowhere left to go this visit.
    no_route: bool,
    raised: bool,
}

impl Delver {
    fn new(calling: char, traits: [char; 2], attr_key: char, style: Style) -> Self {
        Self {
            calling,
            traits,
            attr_key,
            style,
            queue: VecDeque::new(),
            plan: VecDeque::new(),
            attempted: HashSet::new(),
            descended: false,
            skills_done: false,
            skill_cursor: 0,
            want_map: true,
            created_points: 0,
            no_route: false,
            raised: false,
        }
    }

    fn target_depth(&self, level: i32) -> i32 {
        match self.style {
            Style::Careful => 1 + level / 2,
            Style::Greedy => 2 + level * 2 / 3,
        }
    }

    fn create(&mut self, screen: &[String]) -> Option<DoorKey> {
        if has(screen, "Choose your calling") {
            return Some(DoorKey::Char(self.calling));
        }
        if has(screen, "traits") {
            let chosen = screen.iter().filter(|l| l.contains("]*")).count();
            if chosen >= 2 {
                return Some(DoorKey::Enter);
            }
            return Some(DoorKey::Char(self.traits[chosen.min(1)]));
        }
        if has(screen, "attribute points") {
            let left = field(screen, "points (").map_or(0, |v| v.0);
            let _ = left;
            if self.created_points < 4 {
                self.created_points += 1;
                return Some(DoorKey::Char(self.attr_key));
            }
            return Some(DoorKey::Enter);
        }
        Some(DoorKey::Escape)
    }

    fn town(&mut self, screen: &[String], level: i32) -> Option<DoorKey> {
        let wants_points =
            has(screen, "to spend.") && !has(screen, "0 skill point(s) and 0 attribute");
        if wants_points && !self.skills_done {
            self.skill_cursor = 0;
            return Some(DoorKey::Char('s'));
        }
        if self.descended {
            return None;
        }
        self.descended = true;
        self.want_map = true;
        let td = self.target_depth(level);
        // Take the deepest shortcut that is not deeper than the day's goal.
        let express = entries(screen)
            .into_iter()
            .filter(|(_, label)| label.starts_with("Take the"))
            .filter_map(|(k, label)| {
                let depth: i32 = label
                    .split("level ")
                    .nth(1)?
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()?;
                (depth <= td).then_some((depth, k))
            })
            .max();
        Some(DoorKey::Char(express.map_or('d', |(_, k)| k)))
    }

    fn skills(&mut self, screen: &[String]) -> Option<DoorKey> {
        let points = field(screen, "Skill points").map_or(0, |v| v.0);
        let attrs = screen
            .iter()
            .find_map(|l| l.split("Attribute points ").nth(1))
            .and_then(|t| t.split_whitespace().next()?.parse::<i32>().ok())
            .unwrap_or(0);
        if attrs > 0 && !self.raised {
            self.raised = true;
            let letter = match self.attr_key {
                '1' => 'M',
                '2' => 'F',
                '3' => 'W',
                _ => 'S',
            };
            return Some(DoorKey::Char(letter));
        }
        if points > 0 && self.skill_cursor < NODE_KEYS.len() {
            let key = NODE_KEYS.chars().nth(self.skill_cursor).unwrap();
            self.skill_cursor += 1;
            return Some(DoorKey::Char(key));
        }
        self.skills_done = true;
        Some(DoorKey::Escape)
    }

    fn fight(&mut self, screen: &[String]) -> Option<DoorKey> {
        let (hp, max) = field(screen, " HP ").unwrap_or((10, 10));
        let focus = field(screen, "focus ").map_or(0, |v| v.0);
        let header = screen
            .iter()
            .find(|l| l.starts_with("FIGHT"))
            .cloned()
            .unwrap_or_default();
        let enemies: Vec<&String> = screen.iter().filter(|l| l.contains("next:")).collect();
        let heavy = enemies.iter().any(|l| l.contains("crushing"));
        let incoming = enemies
            .iter()
            .any(|l| l.contains("blow") || l.contains("aim") || l.contains("furnace"));
        if hp * 100 < max * 22
            && max > 0
            && has(screen, "[f] Flee")
            && header.contains("round")
            && !has(screen, "round 1 ")
        {
            return Some(DoorKey::Char('f'));
        }
        if self.style == Style::Greedy && has(screen, "[z] Quick") {
            return Some(DoorKey::Char('z'));
        }
        let want = if heavy {
            "evasive"
        } else if incoming {
            "guarded"
        } else {
            "aggressive"
        };
        if !header.contains(&format!("stance {want}")) {
            return Some(DoorKey::Char(match want {
                "evasive" => 'v',
                "guarded" => 'g',
                _ => 'a',
            }));
        }
        let moves: Vec<(char, String, i32)> = entries(screen)
            .into_iter()
            .filter(|(k, _)| k.is_ascii_digit())
            .filter_map(|(k, label)| {
                let cost: i32 = label
                    .split('(')
                    .nth(1)?
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()?;
                Some((k, label, cost))
            })
            .collect();
        // A wind-up is worth interrupting.
        if heavy {
            if let Some((k, _, _)) = moves.iter().find(|(_, l, c)| {
                *c <= focus
                    && ["Shield Bash", "Cheap Shot", "Snare Shot", "Riposte"]
                        .iter()
                        .any(|n| l.starts_with(n))
            }) {
                return Some(DoorKey::Char(*k));
            }
        }
        if hp * 100 < max * 45 {
            if let Some((k, _, _)) = moves.iter().find(|(_, l, c)| {
                *c <= focus && (l.starts_with("Ember Mend") || l.starts_with("Second Wind"))
            }) {
                return Some(DoorKey::Char(*k));
            }
        }
        // The costliest damaging move we can afford.
        let best = moves
            .iter()
            .filter(|(_, l, c)| {
                *c <= focus
                    && *c >= 2
                    && !["Mend", "Wind", "Hold", "Runic Ward", "Brand"]
                        .iter()
                        .any(|n| l.contains(n))
            })
            .max_by_key(|(_, _, c)| *c);
        if let Some((k, _, _)) = best {
            return Some(DoorKey::Char(*k));
        }
        Some(DoorKey::Char('x'))
    }

    fn read_map(&mut self, screen: &[String]) {
        // Rows after the title; each cell is three columns; a row of cells, then a row of links.
        let rows: Vec<&String> = screen.iter().skip(1).take(8).collect();
        let cell = |x: usize, y: usize| -> char {
            rows.get(y * 2)
                .and_then(|r| r.chars().nth(x * 3))
                .unwrap_or(' ')
        };
        let mut here = None;
        let mut goal_exit = None;
        let mut unknown = Vec::new();
        for y in 0..4 {
            for x in 0..5 {
                match cell(x, y) {
                    '@' => here = Some((x, y)),
                    '>' => goal_exit = Some((x, y)),
                    '?' => unknown.push((x, y)),
                    _ => {}
                }
            }
        }
        let Some(start) = here else { return };
        let linked = |a: (usize, usize), b: (usize, usize)| -> bool {
            if a.1 == b.1 {
                let x = a.0.min(b.0);
                rows.get(a.1 * 2).and_then(|r| r.chars().nth(x * 3 + 1)) == Some('-')
            } else {
                let y = a.1.min(b.1);
                rows.get(y * 2 + 1).and_then(|r| r.chars().nth(a.0 * 3)) == Some('|')
            }
        };
        // Breadth-first search over what the map shows.
        let mut prev: std::collections::HashMap<(usize, usize), ((usize, usize), char)> =
            std::collections::HashMap::new();
        let mut seen = HashSet::from([start]);
        let mut queue = VecDeque::from([start]);
        let mut found = None;
        let prefer_exit = self.plan_prefers_exit();
        while let Some(at) = queue.pop_front() {
            let hit = if prefer_exit && goal_exit.is_some() {
                Some(at) == goal_exit
            } else {
                unknown.contains(&at)
            };
            if hit && at != start {
                found = Some(at);
                break;
            }
            for (dx, dy, letter) in [(0i32, -1i32, 'n'), (1, 0, 'e'), (0, 1, 's'), (-1, 0, 'w')] {
                let nx = i32::try_from(at.0).unwrap() + dx;
                let ny = i32::try_from(at.1).unwrap() + dy;
                if !(0..5).contains(&nx) || !(0..4).contains(&ny) {
                    continue;
                }
                let next = (usize::try_from(nx).unwrap(), usize::try_from(ny).unwrap());
                if seen.contains(&next) || cell(next.0, next.1) == ' ' || !linked(at, next) {
                    continue;
                }
                seen.insert(next);
                prev.insert(next, (at, letter));
                queue.push_back(next);
            }
        }
        self.plan.clear();
        self.no_route = found.is_none();
        if let Some(mut at) = found {
            let mut steps = Vec::new();
            while let Some((from, letter)) = prev.get(&at) {
                steps.push(*letter);
                at = *from;
            }
            self.plan = steps.into_iter().rev().collect();
        }
    }

    fn plan_prefers_exit(&self) -> bool {
        self.want_map
    }

    fn delve(&mut self, screen: &[String], level: i32) -> Option<DoorKey> {
        let (hp, max) = field(screen, " HP ").unwrap_or((10, 10));
        let (turns, _) = field(screen, "Turns ").unwrap_or((0, 25));
        let depth: i32 = screen
            .iter()
            .find_map(|l| l.strip_prefix("Level "))
            .and_then(|t| t.split_whitespace().next()?.parse().ok())
            .unwrap_or(1);
        let room = screen
            .iter()
            .find(|l| l.starts_with("Level "))
            .cloned()
            .unwrap_or_default();
        let cost: i32 = entries(screen)
            .iter()
            .find(|(k, _)| *k == 'u')
            .and_then(|(_, l)| {
                l.split("costs ")
                    .nth(1)?
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()
            })
            .unwrap_or(1);
        let td = self.target_depth(level);
        let hostile = has(screen, "bars the way");
        let low = hp * 100 < max * 35;
        let margin = if self.style == Style::Careful { 3 } else { 2 };
        if turns <= cost + 1 || low && !hostile {
            return Some(DoorKey::Char('u'));
        }
        // Deal with the room: options first.
        let menu = entries(screen);
        let mut choices: Vec<(char, String)> = menu
            .iter()
            .filter(|(k, _)| k.is_ascii_digit())
            .cloned()
            .collect();
        if hostile {
            let clever = [
                "Creep",
                "Stare",
                "Lure",
                "Order them",
                "Find the gap",
                "warding flame",
                "lantern",
                "snare",
                "Slip",
                "Feel your way",
            ];
            choices.sort_by_key(|(_, l)| i32::from(!clever.iter().any(|c| l.contains(c))));
        }
        for (k, label) in &choices {
            let key = (room.clone(), label.clone());
            let option_turns: i32 = label
                .rsplit_once('(')
                .and_then(|(_, t)| t.split_whitespace().next()?.parse().ok())
                .unwrap_or(1);
            let is_fight = label.starts_with("Fight") || label.starts_with("Face");
            if !is_fight && self.attempted.contains(&key) {
                continue;
            }
            // On the way down, only the cheap things and the shortcuts are worth the torch.
            let shortcut = ["winch", "lift", "bridge", "span", "line", "Carry"]
                .iter()
                .any(|w| label.contains(w));
            let worth = depth >= td || option_turns <= 1 || shortcut;
            if hostile || (worth && turns - option_turns > cost + margin) {
                self.attempted.insert(key);
                self.plan.clear();
                return Some(DoorKey::Char(*k));
            }
        }
        if hostile {
            return Some(DoorKey::Char('u'));
        }
        // Down, if this is the way down and the day's goal is deeper.
        let can_stairs = entries(screen).iter().any(|(k, _)| *k == '>');
        if can_stairs && depth < td && turns > cost + 4 {
            return Some(DoorKey::Char('>'));
        }
        if depth >= td && turns <= cost + margin + 2 {
            return Some(DoorKey::Char('u'));
        }
        if self.no_route {
            return Some(DoorKey::Char('u'));
        }
        // Move: follow the plan, or read the map for a new one.
        self.want_map = depth < td;
        if let Some(step) = self.plan.pop_front() {
            return Some(DoorKey::Char(step));
        }
        if has(screen, "MAP of level") {
            return Some(DoorKey::Escape);
        }
        self.queue.push_back(DoorKey::Char('m'));
        self.queue.pop_front()
    }
}

impl Bot for Delver {
    fn next_key(&mut self, screen: &[String], visit: u32, keys_this_visit: u32) -> Option<DoorKey> {
        self.decide(screen, visit, keys_this_visit)
    }
}

impl Delver {
    fn decide(&mut self, screen: &[String], visit: u32, keys_this_visit: u32) -> Option<DoorKey> {
        // Every screen any bot ever sees fits the board's box at 80 columns.
        for line in screen {
            assert!(line.chars().count() <= 74, "too wide: {line:?}");
        }
        if keys_this_visit == 0 {
            self.queue.clear();
            self.plan.clear();
            self.attempted.clear();
            self.descended = false;
            self.skills_done = false;
            self.no_route = false;
            self.raised = false;
        }
        let _ = visit;
        if let Some(key) = self.queue.pop_front() {
            return Some(key);
        }
        let first = screen.first().map_or("", String::as_str);
        let level: i32 = screen
            .iter()
            .find(|l| l.contains(" L") && l.contains("HP "))
            .and_then(|l| {
                let at = l.find(" L")?;
                l[at + 2..].split_whitespace().next()?.parse().ok()
            })
            .unwrap_or(1);
        if first.starts_with("EMBERDEEP: a new hero") {
            return self.create(screen);
        }
        if first == "SKILLS" {
            return self.skills(screen);
        }
        if first.starts_with("MAP of level") {
            self.read_map(screen);
            return Some(DoorKey::Escape);
        }
        if first == "HERO SHEET" || first == "THE HALL" {
            return Some(DoorKey::Escape);
        }
        if has(screen, "You are dead") {
            return None;
        }
        if has(screen, "sealed") || has(screen, "could not be read") {
            return None;
        }
        if screen.iter().any(|l| l.starts_with("FIGHT")) {
            return self.fight(screen);
        }
        if has(screen, "Cindermere, under the hill") {
            return self.town(screen, level);
        }
        if screen.iter().any(|l| l.starts_with("Level ")) {
            return self.delve(screen, level);
        }
        Some(DoorKey::Escape)
    }
}

fn roster() -> Vec<(&'static str, Delver)> {
    // calling key, two trait keys (a keen-eyed, b iron gut, c quick, d stout, h stubborn,
    // l nimble fingers, m rune-touched, n pious, o woodsman, p climber, q brawny, w tinker, x sharp-tongued),
    // attribute key, style
    let mut out = Vec::new();
    for (name, calling, traits, attr) in [
        ("warden", '1', ['d', 'q'], '1'),
        ("cutthroat", '2', ['c', 'a'], '2'),
        ("ember-priest", '3', ['n', 'h'], '4'),
        ("runesmith", '4', ['m', 'd'], '3'),
        ("wanderer", '5', ['o', 'p'], '2'),
        ("duelist", '6', ['c', 'h'], '2'),
    ] {
        out.push((name, Delver::new(calling, traits, attr, Style::Careful)));
        out.push((name, Delver::new(calling, traits, attr, Style::Greedy)));
    }
    out
}

#[tokio::test]
async fn thirty_days_of_twelve_delvers() {
    let door = Emberdeep::shared().unwrap();
    let id = door.info().id.clone();
    let mut sim = Simulation::new(door, Day::from_raw(DAY), 4242);
    let bots = roster();
    let labels: Vec<(&str, &str)> = bots
        .iter()
        .map(|(n, d)| {
            (
                *n,
                if d.style == Style::Careful {
                    "careful"
                } else {
                    "greedy"
                },
            )
        })
        .collect();
    for (_, bot) in bots {
        sim.add_player(Box::new(bot));
    }
    let report = sim.run_days(30).await.unwrap();
    println!("\nEMBERDEEP 30-DAY SIM: {report:?}");
    println!(
        "{:<13} {:<8} {:>3} {:>5} {:>5} {:>6} {:>5} {:>7} {:>5} {:>6} {:>7} {:>6}",
        "calling",
        "style",
        "lvl",
        "best",
        "dead",
        "kills",
        "nodes",
        "relics",
        "coin",
        "scars",
        "where",
        "bytes"
    );
    let mut summaries: Vec<Summary> = Vec::new();
    for (n, (name, style)) in labels.iter().enumerate() {
        let account = AccountId::from_raw(i64::try_from(n).unwrap() + 1);
        let stored = sim.store().load_save(&id, account).await.unwrap().unwrap();
        let door = Emberdeep::new().unwrap();
        let s = summarize(&stored.bytes, door.content().unwrap()).unwrap();
        println!(
            "{name:<13} {style:<8} {:>3} {:>5} {:>5} {:>6} {:>5} {:>7} {:>5} {:>6} {:>7} {:>6}",
            s.level,
            s.best_depth,
            s.deaths,
            s.kills,
            s.nodes,
            s.relics,
            s.coin,
            s.scars,
            match s.whereabouts {
                Whereabouts::Town => "town".to_owned(),
                Whereabouts::Below(d) => format!("L{d}"),
                Whereabouts::Dead(d) => format!("dead@{d}"),
            },
            stored.bytes.len()
        );
        summaries.push(s);
    }
    let world =
        summarize_world(&sim.store().load_world(&id).await.unwrap().unwrap().bytes).unwrap();
    println!("world: {world:?}");
    let total = |f: fn(&Summary) -> u32| summaries.iter().map(f).sum::<u32>();
    println!(
        "totals: deaths {} kills {} relics {} deepest {}",
        total(|s| u32::from(s.deaths)),
        total(|s| s.kills),
        total(|s| u32::from(s.relics)),
        summaries.iter().map(|s| s.best_depth).max().unwrap()
    );
    assert_eq!(report.cut_off, 0, "no visit ran away");
    assert!(
        summaries.iter().all(|s| s.level >= 2),
        "everyone gained a level in thirty days"
    );
    assert!(
        summaries.iter().any(|s| s.best_depth >= 3),
        "someone got below the first levels"
    );
    assert!(
        !world.shortcuts.is_empty(),
        "heroes opened a shortcut for everyone"
    );
    assert!(
        world.deepest.iter().any(|(_, depth)| *depth >= 3),
        "the hall records the deepest"
    );
    assert!(
        report.largest_save < 4_000,
        "saves stay small: {}",
        report.largest_save
    );

    // The world outlives the heroes who changed it: a brand new hero who opens the door the next
    // morning finds the shortcuts they opened in the town's list, and can ride one down.
    let door = Emberdeep::shared().unwrap();
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(sim.clock().now()));
    let env = DoorEnv::for_tests("newcomer");
    let mut running = Running::open(sim.store(), door, AccountId::from_raw(99), &env, clock, 5)
        .await
        .unwrap();
    for c in "1ab\n1123\n".chars() {
        let key = if c == '\n' {
            DoorKey::Enter
        } else {
            DoorKey::Char(c)
        };
        let _ = running.key(key);
    }
    let town = running.look_around(&env);
    let express: Vec<&String> = town.iter().filter(|l| l.contains("Take the")).collect();
    assert_eq!(express.len(), world.shortcuts.len(), "{town:?}");
    println!("newcomer sees: {express:?}");
    let _ = running.key(DoorKey::Char('1'));
    let below = running.look_around(&env);
    assert!(below.iter().any(|l| l.starts_with("Level ")), "{below:?}");
}
