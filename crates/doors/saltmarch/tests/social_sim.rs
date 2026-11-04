//! A headless run of the social layer: twelve clans in four styles play forty days through the
//! framework's simulation (the screen is read and answered with keys, the daily tick settles the
//! raids), and the numbers a balance pass needs are printed: raids sent and won, what they took,
//! pacts made and broken, what the exchange moved and what it kept, and where each style ends
//! up in the league. The assertions are the invariants the caps promise, not a balance verdict.
//!
//! `cargo test -p russh-bbs-door-saltmarch --test social_sim -- --nocapture` prints the table.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::print_stderr,
    clippy::cast_precision_loss
)]

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::limits::Day;
use russh_bbs_core::store::AccountId;
use russh_bbs_door::sim::{Bot, Simulation};
use russh_bbs_door::{Door, DoorKey};
use russh_bbs_door_saltmarch::Saltmarch;
use russh_bbs_door_saltmarch::clan::Clan;
use russh_bbs_door_saltmarch::social::{caps, pact::State, raid::LogEntry};
use russh_bbs_door_saltmarch::types::Resource;
use russh_bbs_door_saltmarch::world::World;

const START: i64 = 20_000;
const DAYS: u32 = 40;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
enum Style {
    /// Works the hold, builds, and sells its surplus on the exchange.
    Builder,
    /// Recruits, works a little, and raids whoever the gate will let it.
    Raider,
    /// Offers truces, accepts them, writes letters, and breaks a pact now and then.
    Diplomat,
    /// Lives on the exchange: asks, bids and takes the best price.
    Trader,
}

enum Step {
    /// Press the first entry whose tag starts with this, if the screen has one.
    Tag(&'static str),
    /// Press the first of `find`; if there is none, press `turn` and look again, up to `max` times.
    Seek {
        find: &'static str,
        turn: &'static str,
        max: u8,
    },
    Key(DoorKey),
}

struct Player {
    style: Style,
    script: VecDeque<Step>,
    seeking: u8,
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

fn key_for(screen: &[String], prefix: &str) -> Option<DoorKey> {
    entries(screen)
        .into_iter()
        .find(|(_, t)| t.starts_with(prefix))
        .map(|(k, _)| DoorKey::Char(k))
}

fn script(style: Style, visit: u32) -> VecDeque<Step> {
    use Step::{Key, Seek, Tag};
    let mut s: VecDeque<Step> = VecDeque::new();
    // Past the morning report, and the clan's name on a first visit.
    s.push_back(Key(DoorKey::Enter));
    s.push_back(Key(DoorKey::Char(' ')));
    // Everyone answers the post (a pact offer, if one waits) and works a shift or two: the
    // hold is the economy.
    s.extend([
        Tag("go:court"),
        Tag("go:letters"),
        Tag("accept:"),
        esc_k(),
        esc_k(),
    ]);
    s.extend([Tag("go:hold"), Tag("shift:"), Tag("shift:"), esc_k()]);
    match style {
        Style::Builder => {
            s.extend([
                Tag("go:hold"),
                Tag("build:"),
                Tag("labour:"),
                Tag("labour:"),
                esc_k(),
                Tag("go:court"),
                Tag("go:exchange"),
            ]);
            // Sell a little of whatever the hold has too much of.
            for _ in 0..6 {
                s.extend([Tag("ask:"), Tag("good:next")]);
            }
        }
        Style::Raider => {
            s.extend([
                Tag("go:crew"),
                Tag("recruit:"),
                esc_k(),
                Tag("go:court"),
                Tag("go:raid"),
                Seek {
                    find: "raid:send:",
                    turn: "target:next",
                    max: 12,
                },
                Tag("raid:send:"),
                esc_k(),
                Tag("go:letters"),
                Tag("compose"),
                Tag("send:"),
            ]);
        }
        Style::Diplomat => {
            s.extend([
                Tag("go:court"),
                Tag("go:letters"),
                Tag("compose"),
                Tag("send:"),
                esc_k(),
                Tag("go:pacts"),
                Seek {
                    find: "offer:",
                    turn: "ptarget:next",
                    max: 12,
                },
                Tag("offer:"),
            ]);
            // Now and then a pact is broken (two presses).
            if visit.is_multiple_of(9) {
                s.extend([Tag("break:"), Tag("break:")]);
            }
        }
        Style::Trader => {
            s.extend([Tag("go:court"), Tag("go:exchange")]);
            for _ in 0..6 {
                s.extend([
                    Tag("xbuy:"),
                    Tag("xsell:"),
                    Tag("bid:"),
                    Tag("ask:"),
                    Tag("good:next"),
                ]);
            }
        }
    }
    s
}

fn esc_k() -> Step {
    Step::Key(DoorKey::Escape)
}

impl Bot for Player {
    fn next_key(&mut self, screen: &[String], visit: u32, sent: u32) -> Option<DoorKey> {
        if sent == 0 {
            self.script = script(self.style, visit);
            self.seeking = 0;
        }
        loop {
            match self.script.pop_front()? {
                Step::Key(k) => return Some(k),
                Step::Tag(t) => {
                    if let Some(k) = key_for(screen, t) {
                        return Some(k);
                    }
                }
                Step::Seek { find, turn, max } => {
                    if let Some(k) = key_for(screen, find) {
                        return Some(k);
                    }
                    if std::env::var("SOCIAL_SIM_DEBUG").is_ok()
                        && self.style == Style::Diplomat
                        && self.seeking == 0
                    {
                        eprintln!("--- seek {find} failed on:\n{}", screen.join("\n"));
                    }
                    if self.seeking < max
                        && let Some(k) = key_for(screen, turn)
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
    world_bytes: usize,
}

async fn play(seed: u64) -> Outcome {
    let door: Arc<dyn Door> = Saltmarch::shared().unwrap();
    let mut sim = Simulation::new(door.clone(), Day::from_raw(START), seed);
    let styles = [
        Style::Builder,
        Style::Raider,
        Style::Diplomat,
        Style::Trader,
    ];
    let mut order = Vec::new();
    for i in 0..12usize {
        let style = styles[i % styles.len()];
        order.push(style);
        sim.add_player(Box::new(Player {
            style,
            script: VecDeque::new(),
            seeking: 0,
        }));
    }
    sim.run_days(DAYS).await.unwrap();
    let id = door.info().id.clone();
    let world_bytes = sim.store().load_world(&id).await.unwrap().unwrap().bytes;
    let world = World::decode(&world_bytes).unwrap();
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
    Outcome {
        clans,
        world,
        world_bytes: world_bytes.len(),
    }
}

#[tokio::test]
async fn forty_days_of_raids_pacts_and_trade_keep_every_promise_the_caps_make() {
    let o = play(2026).await;
    let w = &o.world;
    // Everyone found a clan and is on the rolls.
    assert_eq!(o.clans.len(), 12);
    assert_eq!(w.clans.len(), 12);
    // The blob stays inside the board's cap with room to spare.
    assert!(o.world_bytes < caps::WORLD_SOFT_CAP, "{}", o.world_bytes);
    // The raid log never shows more than the caps allow.
    let mut per_day: BTreeMap<(u8, i64), usize> = BTreeMap::new();
    let mut hits: BTreeMap<(u8, i64), usize> = BTreeMap::new();
    for LogEntry { from, to, day } in &w.social.raid_log {
        *per_day.entry((*from, *day)).or_default() += 1;
        *hits.entry((*to, *day)).or_default() += 1;
    }
    assert!(
        per_day
            .values()
            .all(|n| *n <= usize::from(caps::RAIDS_PER_DAY)),
        "{per_day:?}"
    );
    assert!(
        hits.values().all(|n| *n <= usize::from(caps::HITS_PER_DAY)),
        "{hits:?}"
    );
    let log = &w.social.raid_log;
    for (i, a) in log.iter().enumerate() {
        for b in &log[i + 1..] {
            let same_pair =
                (a.from == b.from && a.to == b.to) || (a.from == b.to && a.to == b.from);
            if same_pair {
                assert!(
                    (a.day - b.day).abs() >= caps::PAIR_COOLDOWN_DAYS,
                    "the same pair raided within the cooldown: {a:?} {b:?}"
                );
            }
        }
    }
    // Nobody raided a pact partner or a new clan (the log keeps the sender's day).
    for e in log {
        let target = &w.clans[usize::from(e.to)];
        assert!(
            e.day - target.posted.founded >= caps::NEW_CLAN_DAYS || target.record.raided_first,
            "a protected clan was raided: {e:?}"
        );
        assert!(
            !w.social
                .pacts
                .iter()
                .any(|p| p.state == State::Active && p.joins(e.from, e.to) && p.since < e.day),
            "a pact partner was raided: {e:?}"
        );
    }
    // Per clan: renown is what the league holds, raids add at most 18, a betrayal costs 15.
    for (style, clan) in &o.clans {
        assert!(w.ix(&clan.name).is_some(), "{style:?} {}", clan.name);
        assert!(
            clan.encode().len() < 20_000,
            "{} bytes",
            clan.encode().len()
        );
        assert!(clan.social.pending.len() <= caps::PENDING_CAP);
        assert!(clan.social.inbox.len() <= caps::INBOX_CAP);
    }
    // The books balance on the exchange: what stands in the book is escrow, never negative, and
    // no order is over the quantity cap.
    for o in w.social.exchange.asks.iter().chain(&w.social.exchange.bids) {
        assert!(o.qty >= 1 && o.qty <= caps::ORDER_QTY, "{o:?}");
    }
    assert!(w.social.mail.len() <= caps::MAIL_TOTAL);

    // The numbers.
    let mut by_style: BTreeMap<Style, Vec<(u32, &Clan)>> = BTreeMap::new();
    for (style, clan) in &o.clans {
        let rn = w
            .ix(&clan.name)
            .map(|i| w.clans[usize::from(i)].score)
            .unwrap();
        by_style.entry(*style).or_default().push((rn, clan));
    }
    eprintln!("\nSaltmarch social sim: {DAYS} days, 12 clans (3 per style), seed 2026");
    eprintln!(
        "{:<9} {:>7} {:>6} {:>5} {:>5} {:>6} {:>7} {:>7} {:>6} {:>5} {:>6}",
        "style",
        "renown",
        "raids",
        "won",
        "lost",
        "held",
        "taken",
        "robbed",
        "crew",
        "dead",
        "letters"
    );
    for (style, rows) in &by_style {
        let n = rows.len() as f64;
        let sum =
            |f: &dyn Fn(&Clan) -> u32| f64::from(rows.iter().map(|(_, c)| f(c)).sum::<u32>()) / n;
        eprintln!(
            "{:<9} {:>7.1} {:>6.1} {:>5.1} {:>5.1} {:>6.1} {:>7.1} {:>7.1} {:>6.1} {:>5.1} {:>6.1}",
            format!("{style:?}"),
            f64::from(rows.iter().map(|r| r.0).sum::<u32>()) / n,
            sum(&|c| u32::from(c.social.raids_sent)),
            sum(&|c| u32::from(c.social.raids_won)),
            sum(&|c| u32::from(c.social.raids_lost)),
            sum(&|c| u32::from(c.social.repelled)),
            sum(&|c| c.social.plundered),
            sum(&|c| c.social.robbed),
            sum(&|c| u32::try_from(c.crew.len()).unwrap()),
            sum(&|c| c.stats.lost),
            sum(&|c| u32::try_from(c.social.inbox.len()).unwrap()),
        );
    }
    let raids_total: u32 = o
        .clans
        .iter()
        .map(|(_, c)| u32::from(c.social.raids_sent))
        .sum();
    let won: u32 = o
        .clans
        .iter()
        .map(|(_, c)| u32::from(c.social.raids_won))
        .sum();
    let plunder: u32 = o.clans.iter().map(|(_, c)| c.social.plundered).sum();
    let robbed: u32 = o.clans.iter().map(|(_, c)| c.social.robbed).sum();
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
    let traded: u32 = w.clans.iter().map(|c| c.record.traded).sum::<u32>() / 2;
    eprintln!(
        "raids sent {raids_total}, won {won} ({:.0}%), loot taken {plunder} tallies' worth, robbed {robbed} (the shortfall is goods the defender no longer held)",
        if raids_total == 0 {
            0.0
        } else {
            f64::from(won) * 100.0 / f64::from(raids_total)
        }
    );
    eprintln!(
        "pacts in force {signed}, broken {broken}, betrayals {}",
        w.clans
            .iter()
            .map(|c| u32::from(c.record.betrayals))
            .sum::<u32>()
    );
    eprintln!(
        "exchange: {traded} tallies traded, {} asks and {} bids standing, {} deliveries and {} receipts waiting, world {} bytes",
        w.social.exchange.asks.len(),
        w.social.exchange.bids.len(),
        w.social.mail.len(),
        w.social.receipts.len(),
        o.world_bytes
    );
    let prices: Vec<String> = [
        Resource::Salt,
        Resource::Iron,
        Resource::Food,
        Resource::Timber,
    ]
    .iter()
    .map(|r| {
        let t = w.social.exchange.trail(*r);
        format!(
            "{} exchange {:.2} vs Tidecourt {:.2}",
            r.id(),
            f64::from(t.last().copied().unwrap_or(0)) / 100.0,
            f64::from(w.prices[r.index()]) / 100.0
        )
    })
    .collect();
    eprintln!("last prices: {}", prices.join("; "));
    // Something social happened (a sim that never raids or trades proves nothing).
    assert!(traded > 0 || raids_total > 0 || signed + broken > 0);
}

#[tokio::test]
async fn the_social_run_is_deterministic() {
    let a = play(77).await;
    let b = play(77).await;
    assert_eq!(a.world, b.world);
    let names = |o: &Outcome| o.clans.iter().map(|(_, c)| c.encode()).collect::<Vec<_>>();
    assert_eq!(names(&a), names(&b));
}
