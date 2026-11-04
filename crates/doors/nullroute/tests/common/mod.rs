//! Shared helpers: a small hand-built network and run setup.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::unnecessary_wraps
)]

use russh_bbs_door_nullroute::content::{Content, Effect, Lock};
use russh_bbs_door_nullroute::net::{Link, Network, Node, Patrol};
use russh_bbs_door_nullroute::run::{Run, RunSetup};

pub const PING: u8 = 1;
pub const DEEP_SWEEP: u8 = 2;
pub const DICTIONARY: u8 = 5;
pub const BRUTE: u8 = 6;
pub const BADGE: u8 = 8;
pub const STACK_SMASH: u8 = 11;
pub const VEIL: u8 = 14;
pub const DECOY: u8 = 15;
pub const REROUTE: u8 = 16;
pub const JAMMER: u8 = 18;
pub const FORK: u8 = 20;
pub const PURGE: u8 = 21;
pub const SIPHON: u8 = 23;
pub const IMPLANT: u8 = 24;
pub const BLIND_EYE: u8 = 26;

pub fn content() -> Content {
    Content::try_embedded().unwrap()
}

fn node(id: u8, kind: u8, layer: u8, security: u8, lock: Option<Lock>) -> Node {
    let code = ["GW", "RT", "FW", "BR", "TM", "CM", "VT", "AC", "SR"][usize::from(kind)];
    Node {
        id,
        kind,
        layer,
        security,
        lock,
        patrol: Patrol::None,
        honeypot: false,
        value: 0,
        name: format!("{code}-{id}"),
    }
}

/// 0 GW, 1 RT (open), 2 FW (overflow 2), 3 BR (spoof 2), 4 TM (crack 1), 5 VT (open, 60),
/// 6 AC (spoof 2), 7 VT honeypot, 8 CM (camera). Corporation 0, tier 1.
/// Links: 0-1 0-2 0-3 0-4 1-8, 2-5, 3-5, 3-6, 4-7, and a hidden 1-5 (stealth 3).
pub fn hand_net() -> Network {
    let mut nodes = vec![
        node(0, 0, 0, 1, None),
        node(1, 1, 1, 1, None),
        node(2, 2, 1, 2, Some(Lock::Overflow)),
        node(3, 3, 1, 2, Some(Lock::Spoof)),
        node(4, 4, 1, 1, Some(Lock::Crack)),
        node(5, 6, 2, 2, None),
        node(6, 7, 2, 2, Some(Lock::Spoof)),
        node(7, 6, 2, 2, None),
        node(8, 5, 2, 1, None),
    ];
    nodes[5].value = 60;
    nodes[7].honeypot = true;
    let link = |a, b, stealth| Link { a, b, stealth };
    Network {
        corp: 0,
        tier: 1,
        nodes,
        links: vec![
            link(0, 1, 0),
            link(0, 2, 0),
            link(0, 3, 0),
            link(0, 4, 0),
            link(1, 8, 0),
            link(2, 5, 0),
            link(3, 5, 0),
            link(3, 6, 0),
            link(4, 7, 0),
            link(1, 5, 3),
        ],
    }
}

pub fn setup(net: Network, deck: &[u8]) -> RunSetup {
    RunSetup {
        net,
        holes: Vec::new(),
        alert: 0,
        backdoor: false,
        target: "VT".into(),
        effect: Effect::Copy,
        deck: deck.to_vec(),
        shield: 0,
        iface: 0,
        skill: [0; 5],
        cycles: 40,
        buffer: 0,
    }
}

pub fn start(deck: &[u8]) -> (Content, Run) {
    let content = content();
    let run = Run::new(&content, setup(hand_net(), deck)).unwrap();
    (content, run)
}

use std::collections::BTreeSet;

use russh_bbs_door::DoorKey;
use russh_bbs_door::sim::Bot;

/// A player that reads the door's text view and answers with a key, like a person would: it takes
/// the best job it can, scans, walks toward the deepest open node, breaks the locks its starter
/// deck can break, copies a vault if it finds one, and jacks out. One run per visit.
#[derive(Default)]
pub struct ScreenBot {
    steps: u32,
    visited: BTreeSet<String>,
    scanned: BTreeSet<String>,
    copied: BTreeSet<String>,
    aim: Option<usize>,
    ran: bool,
    last: Option<String>,
    started: bool,
}

fn digit_key(n: usize) -> DoorKey {
    DoorKey::Char(char::from_digit(u32::try_from(n).unwrap_or(1), 10).unwrap_or('1'))
}

fn exits(screen: &[String]) -> Vec<(usize, String)> {
    screen
        .iter()
        .filter_map(|l| {
            let t = l.trim_start();
            let (n, rest) = t.split_once(") ")?;
            let n: usize = n.parse().ok()?;
            (l.starts_with("  ") && !l.starts_with("   ")).then(|| (n, rest.to_owned()))
        })
        .collect()
}

fn exits_numbered(screen: &[String]) -> Vec<(usize, String)> {
    screen
        .iter()
        .filter_map(|l| {
            let (n, rest) = l.split_once(") ")?;
            Some((n.parse().ok()?, rest.to_owned()))
        })
        .collect()
}

fn depth(text: &str) -> u32 {
    text.split(" d")
        .nth(1)
        .and_then(|r| r.chars().next())
        .and_then(|c| c.to_digit(10))
        .unwrap_or(0)
}

impl ScreenBot {
    pub fn pick(&mut self, screen: &[String]) -> Option<DoorKey> {
        let head = screen.first().map_or("", String::as_str);
        let key = if head.starts_with("NULLROUTE") {
            self.started = false;
            if self.ran {
                return None;
            }
            Some(DoorKey::Char('c'))
        } else if head.starts_with("CONTRACTS") {
            // The first job the deck can do (no note in brackets) that the starter deck can finish.
            let pick = exits_numbered(screen)
                .into_iter()
                .find(|(_, t)| t.starts_with("Steal") && !t.contains('('));
            if let Some((n, _)) = pick {
                Some(digit_key(n))
            } else {
                self.ran = true;
                Some(DoorKey::Escape)
            }
        } else if head.starts_with("DEBRIEF") {
            self.ran = true;
            Some(DoorKey::Enter)
        } else if screen.iter().any(|l| l.starts_with("Target:")) {
            // A briefing: jack in. If we are still here after trying, the deck cannot do the job.
            if self.started {
                self.started = false;
                Some(DoorKey::Escape)
            } else {
                self.started = true;
                Some(DoorKey::Char('j'))
            }
        } else if screen.get(1).is_some_and(|l| l.starts_with("TRACE")) {
            self.run_key(screen)
        } else {
            Some(DoorKey::Escape)
        };
        if let Some(k) = key {
            self.last = Some(k.token());
        }
        key
    }

    fn run_key(&mut self, screen: &[String]) -> Option<DoorKey> {
        self.steps += 1;
        if let Some(n) = self.aim.take()
            && screen.last().is_some_and(|l| l.starts_with("Aim "))
        {
            return Some(digit_key(n));
        }
        let trace: u32 = screen[1]
            .split("] ")
            .nth(1)
            .and_then(|r| r.split('/').next())
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(0);
        let at = screen
            .iter()
            .find(|l| l.starts_with("At "))
            .map_or("", String::as_str);
        let name = at
            .trim_start_matches("At ")
            .split(' ')
            .next()
            .unwrap_or("")
            .to_owned();
        if screen[0].contains("DONE") || trace >= 85 || self.steps > 45 {
            return Some(DoorKey::Char('o'));
        }
        if at.contains("Data vault") && self.copied.insert(name.clone()) {
            return Some(DoorKey::Char('d'));
        }
        if self.scanned.insert(name.clone()) {
            return Some(DoorKey::Char('a'));
        }
        self.visited.insert(name);
        let list = exits(screen);
        let mut open: Vec<_> = list
            .iter()
            .filter(|(_, t)| t.contains(", open") && !t.contains("HONEYPOT"))
            .filter(|(_, t)| !self.visited.contains(t.split(' ').next().unwrap_or("")))
            .collect();
        open.sort_by_key(|(_, t)| std::cmp::Reverse(depth(t)));
        if let Some((n, _)) = open.first() {
            return Some(digit_key(*n));
        }
        for (n, t) in &list {
            if self.visited.contains(t.split(' ').next().unwrap_or("")) {
                continue;
            }
            let slot = if t.contains("password lock") {
                Some('b')
            } else if t.contains("badge lock") {
                Some('c')
            } else {
                None
            };
            if let Some(slot) = slot
                && !t.contains("lock 3")
            {
                self.aim = Some(*n);
                return Some(DoorKey::Char(slot));
            }
        }
        Some(DoorKey::Char('o'))
    }
}

impl Bot for ScreenBot {
    fn next_key(&mut self, screen: &[String], visit: u32, sent: u32) -> Option<DoorKey> {
        if sent == 0 {
            *self = Self::default();
            let _ = visit;
        }
        if screen.first().is_some_and(|l| l.starts_with("NULLROUTE")) && self.ran {
            return Some(DoorKey::Char('q'));
        }
        self.pick(screen)
    }
}
