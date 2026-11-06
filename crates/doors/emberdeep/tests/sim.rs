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
use russh_bbs_door_emberdeep::{
    Emberdeep, Summary, Whereabouts, summarize, summarize_town, summarize_world,
};

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
    /// Goes deeper still, and stays below with little health left: it dies, and is rescued.
    Reckless,
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
    /// Town places still to visit this morning, as the keys that open them.
    chores: VecDeque<char>,
    place_steps: u32,
    crafted: u32,
    drank: bool,
    drilled: bool,
    asked: bool,
    cored: bool,
    cores_bought: u32,
    /// The dearest weapon and armour this bot has bought: it only buys better.
    price_floor: (i32, i32),
    weapon_attr: &'static str,
    ranged: bool,
    court: bool,
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
            chores: VecDeque::new(),
            place_steps: 0,
            crafted: 0,
            drank: false,
            drilled: false,
            asked: false,
            cored: false,
            cores_bought: 0,
            price_floor: (0, 0),
            weapon_attr: match calling {
                '1' => "might",
                '3' => "spirit",
                '4' => "wits",
                _ => "finesse",
            },
            ranged: calling == '5',
            court: matches!(calling, '1' | '3' | '4'),
        }
    }

    fn target_depth(&self, level: i32) -> i32 {
        match self.style {
            Style::Careful => 1 + level / 2,
            Style::Greedy => 2 + level * 2 / 3,
            Style::Reckless => 5 + level,
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
        if let Some(chore) = self.chores.pop_front() {
            self.place_steps = 0;
            return Some(DoorKey::Char(chore));
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
        if self.style != Style::Careful && has(screen, "[z] Quick") {
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
        let low = hp * 100 < max * if self.style == Style::Reckless { 8 } else { 35 };
        let margin = match self.style {
            Style::Careful => 3,
            Style::Greedy => 2,
            Style::Reckless => 0,
        };
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

/// `n. text  123c` rows of a market screen.
fn stall_rows(screen: &[String]) -> Vec<(usize, String, i32, bool)> {
    screen
        .iter()
        .filter_map(|l| {
            let t = l.trim_start();
            let (n, rest) = t.split_once(". ")?;
            let n: usize = n.parse().ok()?;
            let sold = rest.ends_with(" SOLD");
            let rest = rest.trim_end_matches(" SOLD");
            let (text, price) = rest.rsplit_once("  ")?;
            let price: i32 = price.strip_suffix('c')?.parse().ok()?;
            Some((n, text.to_owned(), price, sold))
        })
        .collect()
}

/// The average blow of `6-10 might melee`, or the armour of `armor 3`.
fn stat(text: &str) -> Option<(i32, &'static str)> {
    if let Some(at) = text.find("armor ") {
        let v: i32 = text[at + 6..].split_whitespace().next()?.parse().ok()?;
        return Some((v, "armor"));
    }
    for word in text.split_whitespace() {
        if let Some((a, b)) = word.split_once('-')
            && let (Ok(a), Ok(b)) = (a.parse::<i32>(), b.parse::<i32>())
        {
            let attr = ["might", "finesse", "wits", "spirit"]
                .into_iter()
                .find(|w| text.contains(w))?;
            return Some((i32::midpoint(a, b), attr));
        }
    }
    None
}

impl Delver {
    /// One key in a town place: do the next sensible thing, or leave.
    fn place_step(&mut self, first: &str, screen: &[String], level: i32) -> Option<DoorKey> {
        self.place_steps += 1;
        if self.place_steps > 40 {
            return Some(DoorKey::Escape);
        }
        let coin = field(screen, "Coin ").map_or(0, |v| v.0);
        let stash = field(screen, "Stash ").map_or(0, |v| v.0);
        let menu = entries(screen);
        let press = |prefix: &str| {
            menu.iter()
                .find(|(_, l)| l.starts_with(prefix))
                .map(|(k, _)| DoorKey::Char(*k))
        };
        let price_in = |label: &str| -> i32 {
            label
                .rsplit_once('(')
                .and_then(|(_, t)| t.trim_end_matches(')').trim_end_matches('c').parse().ok())
                .unwrap_or(0)
        };
        if first.starts_with("THE FORGE") {
            for (k, label) in &menu {
                if label.starts_with("Repair") && price_in(label) <= coin {
                    return Some(DoorKey::Char(*k));
                }
            }
            for (k, label) in &menu {
                let Some(rest) = label.strip_prefix("Slot ") else {
                    continue;
                };
                let (slot, line_start) = if rest.ends_with("in weapon (8c)") {
                    ("weapon", "Weapon ")
                } else {
                    ("armour", "Armour ")
                };
                let free = screen
                    .iter()
                    .any(|l| l.starts_with(line_start) && l.contains("no core"));
                if free && coin >= 8 && rest.contains(slot) {
                    return Some(DoorKey::Char(*k));
                }
            }
            if self.crafted < 2 && coin >= 90 {
                for (k, label) in &menu {
                    let Some(name) = label.strip_prefix("Make ") else {
                        continue;
                    };
                    let ready = screen.iter().any(|l| {
                        l.trim_start().starts_with(&format!("{name}:")) && !l.contains("(not yet)")
                    });
                    if ready {
                        self.crafted += 1;
                        return Some(DoorKey::Char(*k));
                    }
                }
            }
            return Some(DoorKey::Escape);
        }
        if first.starts_with("THE MARKET") {
            for (n, text, price, sold) in stall_rows(screen) {
                if sold || price + 30 > coin {
                    continue;
                }
                let key = menu
                    .iter()
                    .find(|(_, l)| *l == format!("Buy {n}"))
                    .map(|(k, _)| DoorKey::Char(*k));
                let Some(key) = key else { continue };
                if text.contains("Core") {
                    if self.cores_bought < 1 && coin >= price + 120 {
                        self.cores_bought += 1;
                        return Some(key);
                    }
                    continue;
                }
                let req: i32 = text
                    .rsplit_once(" L")
                    .and_then(|(_, r)| r.split_whitespace().next()?.parse().ok())
                    .unwrap_or(1);
                if req > level {
                    continue;
                }
                let is_armour = text.contains("armor ");
                let ok_weapon = text.contains(self.weapon_attr)
                    && text.contains("ranged") == self.ranged
                    && !is_armour;
                if (is_armour && price > self.price_floor.1)
                    || (ok_weapon && price > self.price_floor.0)
                {
                    if is_armour {
                        self.price_floor.1 = price;
                    } else {
                        self.price_floor.0 = price;
                    }
                    return Some(key);
                }
            }
            return Some(DoorKey::Escape);
        }
        if first == "GEAR" {
            let current = |prefix: &str| {
                screen
                    .iter()
                    .find(|l| l.starts_with(prefix))
                    .and_then(|l| stat(l))
            };
            let (w, a) = (current("Weapon:"), current("Armour:"));
            let ranged_now = screen
                .iter()
                .any(|l| l.starts_with("Weapon:") && l.contains("ranged"));
            for (k, label) in &menu {
                let Some(n) = label.strip_prefix("Wield ") else {
                    continue;
                };
                let row = screen
                    .iter()
                    .find(|l| l.trim_start().starts_with(&format!("{n}. ")));
                let Some(row) = row else { continue };
                let Some((v, kind)) = stat(row) else { continue };
                let req: i32 = row
                    .rsplit_once("  L")
                    .and_then(|(_, r)| r.split_whitespace().next()?.parse().ok())
                    .unwrap_or(1);
                if req > level {
                    continue;
                }
                let better = if kind == "armor" {
                    a.is_some_and(|(cv, _)| v > cv)
                } else {
                    w.is_some_and(|(cv, attr)| v > cv && attr == kind)
                        && row.contains("ranged") == ranged_now
                };
                if better {
                    return Some(DoorKey::Char(*k));
                }
            }
            if let Some(key) = press("Sell 1") {
                return Some(key);
            }
            return Some(DoorKey::Escape);
        }
        if first.starts_with("THE TOWN WORKS") {
            if coin >= 60 {
                if let Some(key) = press("Give up to 25c") {
                    return Some(key);
                }
            }
            if stash > 20 {
                if let Some(key) = press("Give up to 25s") {
                    return Some(key);
                }
            }
            return Some(DoorKey::Escape);
        }
        if first.starts_with("THE GUILD") {
            if !self.drilled && coin >= 100 && level < 8 {
                self.drilled = true;
                if let Some(key) = press("Drill") {
                    return Some(key);
                }
            }
            return Some(DoorKey::Escape);
        }
        if first.starts_with("THE TAVERN") {
            if !self.asked {
                self.asked = true;
                if has(screen, "done: turn it in") {
                    if let Some(key) = press("Turn in") {
                        return Some(key);
                    }
                }
                if let Some(key) = press("Take 1") {
                    return Some(key);
                }
            }
            if self.court && coin >= 90 && !self.cored {
                self.cored = true;
                if let Some(key) = press("Call on") {
                    return Some(key);
                }
            }
            if !self.drank && coin >= 60 {
                self.drank = true;
                if let Some(key) = press("Drink") {
                    return Some(key);
                }
            }
            return Some(DoorKey::Escape);
        }
        Some(DoorKey::Escape)
    }
}

impl Bot for Delver {
    fn next_key(&mut self, screen: &[String], visit: u32, keys_this_visit: u32) -> Option<DoorKey> {
        self.decide(screen, visit, keys_this_visit)
    }
}

impl Delver {
    /// The social places: join and invite, step into the ring (the fighters do) and call
    /// someone out, take the first choice a questline offers, and answer the Furnace.
    fn social_step(&mut self, first: &str, screen: &[String]) -> Option<DoorKey> {
        self.place_steps += 1;
        if self.place_steps > 6 {
            return Some(DoorKey::Escape);
        }
        let menu = entries(screen);
        let pick = |prefixes: &[&str]| {
            prefixes.iter().find_map(|p| {
                menu.iter()
                    .find(|(_, l)| l.starts_with(p))
                    .map(|(k, _)| DoorKey::Char(*k))
            })
        };
        let key = if first == "THE COMPANY" {
            pick(&["Accept", "Invite"])
        } else if first == "THE DUEL RING" {
            if matches!(self.calling, '1' | '2' | '6') {
                pick(&["Step into", "Call out"])
            } else {
                None
            }
        } else if first == "THE COLD FURNACE" {
            // The most committal answer open to this build, to walk every ending.
            menu.last().map(|(k, _)| DoorKey::Char(*k))
        } else {
            // A questline's choice: the last way offered for the cautious, the first for the rest.
            let choices: Vec<_> = menu
                .iter()
                .filter(|(_, l)| !l.starts_with("Write"))
                .collect();
            if self.style == Style::Careful {
                choices.last().map(|(k, _)| DoorKey::Char(*k))
            } else {
                choices.first().map(|(k, _)| DoorKey::Char(*k))
            }
        };
        Some(key.unwrap_or(DoorKey::Escape))
    }

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
            self.chores = "fmigwbcuj".chars().collect();
            self.crafted = 0;
            self.drank = false;
            self.drilled = false;
            self.asked = false;
            self.cored = false;
            self.cores_bought = 0;
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
        if first == "THE COMPANY"
            || first == "THE DUEL RING"
            || first.starts_with("THE JOURNAL")
            || first == "THE COLD FURNACE"
        {
            return self.social_step(first, screen);
        }
        if first.starts_with("THE FORGE")
            || first.starts_with("THE MARKET")
            || first == "GEAR"
            || first.starts_with("THE TOWN WORKS")
            || first.starts_with("THE GUILD")
            || first.starts_with("THE TAVERN")
        {
            return self.place_step(first, screen, level);
        }
        if has(screen, "You are dead") {
            return None;
        }
        if has(screen, "Emberdeep is sealed") || has(screen, "could not be read") {
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
        out.push((name, Delver::new(calling, traits, attr, Style::Reckless)));
    }
    out
}

#[tokio::test]
async fn a_season_of_eighteen_delvers() {
    let door = Emberdeep::shared().unwrap();
    let id = door.info().id.clone();
    let mut sim = Simulation::new(
        door,
        Day::from_raw(DAY),
        std::env::var("EMBERDEEP_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4242),
    );
    let legacy = std::env::var("EMBERDEEP_SIM_LEGACY").is_ok();
    let bots: Vec<_> = roster()
        .into_iter()
        .filter(|(_, d)| !(legacy && d.style == Style::Reckless))
        .collect();
    let labels: Vec<(&str, &str)> = bots
        .iter()
        .map(|(n, d)| {
            (
                *n,
                match d.style {
                    Style::Careful => "careful",
                    Style::Greedy => "greedy",
                    Style::Reckless => "reckless",
                },
            )
        })
        .collect();
    for (_, bot) in bots {
        sim.add_player(Box::new(bot));
    }
    // Thirty days, sampled every five: the curves the pull request reports.
    let content = Emberdeep::new().unwrap();
    sim.tick_now().await.unwrap();
    let mut curve: Vec<String> = Vec::new();
    let mut done_by_day: Vec<(u32, usize)> = Vec::new();
    let mut gear_curve: Vec<(f64, f64)> = Vec::new();
    for day in 1..=(if legacy { 30 } else { SEASON_DAYS }) {
        sim.play_day().await.unwrap();
        sim.advance_day().await.unwrap();
        if day % 6 == 0 || day == 1 {
            let mut sums = (0i32, 0i32, 0i32, 0u32, 0u32);
            let mut n = 0;
            for i in 0..labels.len() {
                let account = AccountId::from_raw(i64::try_from(i).unwrap() + 1);
                if let Some(stored) = sim.store().load_save(&id, account).await.unwrap() {
                    let s = summarize(&stored.bytes, content.content().unwrap()).unwrap();
                    sums.0 += s.damage_avg;
                    sums.1 += s.armor;
                    sums.2 += i32::from(s.level);
                    sums.3 += s.given.0;
                    sums.4 += s.given.1;
                    n += 1;
                }
            }
            let w = sim.store().load_world(&id).await.unwrap().unwrap();
            let town = summarize_town(&w.bytes, content.content().unwrap()).unwrap();
            done_by_day.push((day, town.done.len()));
            gear_curve.push((
                f64::from(sums.0) / f64::from(n),
                f64::from(sums.1) / f64::from(n),
            ));
            curve.push(format!(
                "day {day:>2}: avg blow {:>5.1}  avg armor {:>4.1}  avg level {:>4.1}  given {:>5}c {:>4}s  projects done {}/12",
                f64::from(sums.0) / f64::from(n),
                f64::from(sums.1) / f64::from(n),
                f64::from(sums.2) / f64::from(n),
                sums.3,
                sums.4,
                town.done.len()
            ));
        }
    }
    let report = sim.report();
    println!("\nEMBERDEEP SEASON SIM: {report:?}");
    for line in &curve {
        println!("{line}");
    }
    println!(
        "{:<13} {:<8} {:>3} {:>5} {:>5} {:>6} {:>5} {:>7} {:>5} {:>6} {:>7} {:>6}  gear",
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
            stored.bytes.len(),
        );
        println!(
            "{:>66}{} / {}  blow {} armor {} wear {}%/{}% cores {} spares {} gave {}c {}s deals {} debt {} bond {} recipes {}",
            "",
            s.weapon,
            s.armour,
            s.damage_avg,
            s.armor,
            s.wear.0,
            s.wear.1,
            s.cores,
            s.pack,
            s.given.0,
            s.given.1,
            s.contracts_done,
            s.debt,
            s.bond,
            s.recipes
        );
        summaries.push(s);
    }
    let world =
        summarize_world(&sim.store().load_world(&id).await.unwrap().unwrap().bytes).unwrap();
    println!("world: {world:?}");
    balance_report(&labels, &summaries, &world);
    let total = |f: fn(&Summary) -> u32| summaries.iter().map(f).sum::<u32>();
    println!(
        "totals: deaths {} kills {} relics {} deepest {}",
        total(|s| u32::from(s.deaths)),
        total(|s| s.kills),
        total(|s| u32::from(s.relics)),
        summaries.iter().map(|s| s.best_depth).max().unwrap()
    );
    assert_eq!(report.cut_off, 0, "no visit ran away");
    // The town: projects progress across all players and finish, and gear gets better.
    let town = summarize_town(
        &sim.store().load_world(&id).await.unwrap().unwrap().bytes,
        content.content().unwrap(),
    )
    .unwrap();
    assert!(
        town.done.len() >= 3,
        "a dozen heroes finish several town projects in thirty days: {:?}",
        town.done
    );
    assert!(
        done_by_day.windows(2).all(|w| w[0].1 <= w[1].1),
        "a finished project never un-finishes: {done_by_day:?}"
    );
    let (first, last) = (gear_curve[0], gear_curve[gear_curve.len() - 1]);
    assert!(
        last.0 > first.0 && last.1 > first.1,
        "gear improves: {gear_curve:?}"
    );
    assert!(
        summaries.iter().all(|s| s.debt == 0),
        "the bots never gamble, so no one owes the crew"
    );
    assert!(
        summaries.iter().any(|s| s.given.0 > 0),
        "heroes gave coin to the works"
    );
    let gave: u32 = summaries.iter().map(|s| s.given.0).sum();
    let pledged: u32 = world.pledged.iter().map(|(_, c, _)| *c).sum();
    assert_eq!(
        gave, pledged,
        "what heroes say they gave is exactly what the world holds: no coin counted twice or lost"
    );
    let gave_s: u32 = summaries.iter().map(|s| s.given.1).sum();
    let pledged_s: u32 = world.pledged.iter().map(|(_, _, s)| *s).sum();
    assert_eq!(gave_s, pledged_s, "and the same for salvage");
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
    let ways = summarize_town(
        &sim.store().load_world(&id).await.unwrap().unwrap().bytes,
        content.content().unwrap(),
    )
    .unwrap()
    .ways_down;
    assert_eq!(express.len(), ways.len().min(9), "{town:?}");
    assert!(ways.len() >= world.shortcuts.len());
    println!("newcomer sees: {express:?}");
    let _ = running.key(DoorKey::Char('1'));
    let below = running.look_around(&env);
    assert!(below.iter().any(|l| l.starts_with("Level ")), "{below:?}");
}

/// The numbers the pull request reports: by calling, the economy, the social layer and the
/// questlines.
fn balance_report(
    labels: &[(&str, &str)],
    summaries: &[Summary],
    world: &russh_bbs_door_emberdeep::WorldSummary,
) {
    println!("\nBY CALLING (three heroes each: careful, greedy, reckless)");
    println!(
        "{:<13} {:>6} {:>8} {:>9} {:>7} {:>9} {:>8}",
        "calling", "level", "bestdep", "died(h)", "deaths", "coin", "quests"
    );
    let mut names: Vec<&str> = labels.iter().map(|(n, _)| *n).collect();
    names.dedup();
    for name in &names {
        let mine: Vec<&Summary> = labels
            .iter()
            .zip(summaries)
            .filter(|((n, _), _)| n == name)
            .map(|(_, s)| s)
            .collect();
        let n = mine.len() as f64;
        let sum = |f: &dyn Fn(&Summary) -> f64| mine.iter().map(|s| f(s)).sum::<f64>();
        println!(
            "{:<13} {:>6.1} {:>8.1} {:>6}/{:<2} {:>7} {:>9.0} {:>8.1}",
            name,
            sum(&|s| f64::from(s.level)) / n,
            sum(&|s| f64::from(s.best_depth)) / n,
            mine.iter().filter(|s| s.deaths > 0).count(),
            mine.len(),
            mine.iter().map(|s| u32::from(s.deaths)).sum::<u32>(),
            sum(&|s| f64::from(s.coin)) / n,
            sum(&|s| s.quest_steps as f64) / n
        );
    }
    println!("\nBY STYLE");
    for style in ["careful", "greedy", "reckless"] {
        let mine: Vec<&Summary> = labels
            .iter()
            .zip(summaries)
            .filter(|((_, st), _)| *st == style)
            .map(|(_, s)| s)
            .collect();
        println!(
            "{style:<9} level {:>4.1}  best depth {:>4.1}  deaths {:>2}  heroes dead at least once {}/{}",
            mine.iter().map(|s| f64::from(s.level)).sum::<f64>() / mine.len() as f64,
            mine.iter().map(|s| f64::from(s.best_depth)).sum::<f64>() / mine.len() as f64,
            mine.iter().map(|s| u32::from(s.deaths)).sum::<u32>(),
            mine.iter().filter(|s| s.deaths > 0).count(),
            mine.len()
        );
    }
    let carried: u16 = summaries.iter().map(|s| s.rescues.0).sum();
    let rescued: u16 = summaries.iter().map(|s| s.rescues.1).sum();
    let deaths: u32 = summaries.iter().map(|s| u32::from(s.deaths)).sum();
    println!(
        "\nRESCUES: {deaths} deaths, {carried} heroes carried others home, {rescued} were carried home, {} epitaphs for the unfound (world says {} carried home)",
        world.epitaphs, world.carried_home
    );
    println!(
        "COMPANIES: {:?}\nRING: {} duels, ladder {:?}",
        world.companies, world.duels, world.ladder
    );
    let flags: std::collections::BTreeSet<&String> =
        summaries.iter().flat_map(|s| s.flags.iter()).collect();
    println!(
        "QUESTS: {} steps done by 18 heroes; choices made: {flags:?}",
        summaries.iter().map(|s| s.quest_steps).sum::<usize>()
    );
    println!("ENDINGS: {:?}", world.chronicle);
    let coin: u64 = summaries.iter().map(|s| u64::from(s.coin)).sum();
    let given: u64 = summaries.iter().map(|s| u64::from(s.given.0)).sum();
    println!(
        "ECONOMY: {coin} coin in purses, {given} given to the works, {} duels' stakes included in pledges",
        world.duels
    );
}

const SEASON_DAYS: u32 = 42;
