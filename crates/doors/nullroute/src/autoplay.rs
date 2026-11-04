//! A headless player for the balance numbers: it drives [`Player`], [`Run`] and [`World`] through
//! days exactly as a person would through the screen, using only what a person can see.
//!
//! It is part of the crate (not a test helper) so the later slices can rerun the same harness
//! after they change the numbers (`docs/doors/README.md`: each game ships an auto-play harness).
//! It reads no clock and uses no RNG but the one it is given.

use russh_bbs_door::DoorRng;

use crate::content::{Content, Effect, Lock};
use crate::net::Patrol;
use crate::player::{JACK_IN_CYCLES, Player, part};
use crate::run::{Act, Run, Status};
use crate::world::{Contract, World};

/// What to simulate.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// Players sharing one world.
    pub players: usize,
    /// Days to run.
    pub days: u32,
    /// The seed for everything.
    pub seed: u64,
    /// The season (networks are generated from it).
    pub season: u32,
}

/// Runs of one tier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TierStats {
    /// Runs started.
    pub runs: u32,
    /// Objectives done and jacked out.
    pub done: u32,
    /// Burned.
    pub burned: u32,
}

/// What a simulation measured.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Days simulated.
    pub days: u32,
    /// Players.
    pub players: usize,
    /// Per tier (index 0 is tier 1).
    pub tiers: [TierStats; 3],
    /// Runs that ended without the objective and without a burn.
    pub dry: u32,
    /// Marks earned in total by all players.
    pub earned: u64,
    /// Marks on hand at the end, per player.
    pub marks: Vec<u32>,
    /// Heat at the end, per player.
    pub heat: Vec<u32>,
    /// Reputation at the end, per player.
    pub rep: Vec<u32>,
    /// Total rig levels at the end, per player.
    pub rig: Vec<u32>,
    /// Bots at the end, per player.
    pub bots: Vec<u32>,
    /// Scripts owned at the end, per player.
    pub owned: Vec<u32>,
    /// Marks earned by all players on each day.
    pub per_day: Vec<u64>,
    /// Network breaches the world recorded.
    pub breaches: u32,
    /// Contracts on the board at the end.
    pub board: usize,
    /// Days a player had nothing they could take.
    pub idle_days: u32,
    /// The highest trace of every run.
    pub peaks: Vec<u32>,
}

impl Report {
    /// Total runs.
    #[must_use]
    pub fn runs(&self) -> u32 {
        self.tiers.iter().map(|t| t.runs).sum()
    }

    /// Total objectives done.
    #[must_use]
    pub fn done(&self) -> u32 {
        self.tiers.iter().map(|t| t.done).sum()
    }

    /// Total burns.
    #[must_use]
    pub fn burned(&self) -> u32 {
        self.tiers.iter().map(|t| t.burned).sum()
    }
}

enum Want {
    Script(u8),
    Rig(usize),
    Bot,
}

/// What the bot buys, in order.
const WANTS: &[Want] = &[
    Want::Script(24),
    Want::Rig(part::MEMORY),
    Want::Rig(part::CPU),
    Want::Bot,
    Want::Script(25),
    Want::Script(6),
    Want::Script(9),
    Want::Script(12),
    Want::Rig(part::SHIELD),
    Want::Rig(part::COOLING),
    Want::Bot,
    Want::Rig(part::IFACE),
    Want::Script(2),
    Want::Script(18),
    Want::Rig(part::MEMORY),
    Want::Script(14),
    Want::Rig(part::CPU),
    Want::Script(7),
    Want::Script(10),
    Want::Script(21),
    Want::Bot,
    Want::Script(16),
];

fn wanted(content: &Content, p: &Player, want: &Want) -> bool {
    match want {
        Want::Script(n) => !p.owned.contains(n) && content.script(*n).is_some(),
        Want::Rig(i) => p.upgrade_cost(content, *i).is_some(),
        Want::Bot => p.bots < content.rig.hideout.bot_max.min(3),
    }
}

/// Spend marks on the first thing on the list that is still wanted. Returns whether it bought.
fn shop(content: &Content, p: &mut Player, market: u32) -> bool {
    for want in WANTS {
        if !wanted(content, p, want) {
            continue;
        }
        if p.fried != 0 {
            let part = (0..crate::player::PARTS).find(|i| p.fried & (1 << i) != 0);
            if let Some(part) = part {
                return p.repair(content, part).is_ok();
            }
        }
        return match want {
            Want::Script(n) => p.buy_script(content, *n, market).is_ok(),
            Want::Rig(i) => p.upgrade(content, *i).is_ok(),
            Want::Bot => p.buy_bot(content).is_ok(),
        };
    }
    false
}

fn best_owned(content: &Content, p: &Player, pick: impl Fn(Effect) -> bool) -> Option<u8> {
    p.owned
        .iter()
        .filter_map(|n| content.script(*n))
        .filter(|s| pick(s.effect))
        .max_by_key(|s| (s.power, std::cmp::Reverse(s.memory)))
        .map(|s| s.num)
}

fn best_break(content: &Content, p: &Player, lock: Lock) -> Option<u8> {
    best_owned(content, p, |e| e.lock() == Some(lock))
}

/// Whether the bot thinks it can take a contract with what it owns.
fn feasible(content: &Content, p: &Player, c: &Contract) -> bool {
    let Some(tmpl) = content.contract(c.tmpl) else {
        return false;
    };
    if p.closed(content, c).is_some() {
        return false;
    }
    if !p
        .owned
        .iter()
        .filter_map(|n| content.script(*n))
        .any(|s| s.effect == tmpl.effect)
    {
        return false;
    }
    let power = Lock::ALL
        .iter()
        .filter_map(|l| best_break(content, p, *l))
        .filter_map(|n| content.script(n))
        .map(|s| s.power)
        .max()
        .unwrap_or(0);
    match c.tier {
        1 => true,
        2 => power >= 4,
        _ => power >= 6,
    }
}

/// Load the deck for a contract: the tool the job needs, a scan, then the breaks the corporation
/// leans on first, then helpers while memory lasts.
fn plan_deck(content: &Content, p: &mut Player, c: &Contract) {
    p.deck.clear();
    let Some(tmpl) = content.contract(c.tmpl) else {
        return;
    };
    let Some(corp) = content.corp(c.corp) else {
        return;
    };
    if let Some(n) = best_owned(content, p, |e| e == tmpl.effect) {
        let _ = p.load(content, n);
    }
    let scan = p
        .owned
        .iter()
        .filter_map(|n| content.script(*n))
        .filter(|s| s.effect == Effect::Scan && (s.memory <= 1 || p.memory() >= 12))
        .max_by_key(|s| s.power)
        .map(|s| s.num);
    if let Some(n) = scan {
        let _ = p.load(content, n);
    }
    let mut locks = vec![corp.favoured];
    locks.extend(Lock::ALL.into_iter().filter(|l| *l != corp.favoured));
    for lock in locks {
        if let Some(n) = best_break(content, p, lock) {
            let _ = p.load(content, n);
        }
    }
    let helpers = [
        Effect::Mask,
        Effect::Jam,
        Effect::Reroute,
        Effect::Purge,
        Effect::Decoy,
    ];
    for effect in helpers {
        if effect == Effect::Purge && c.tier < 2 {
            continue;
        }
        if let Some(n) = best_owned(content, p, |e| e == effect) {
            let _ = p.load(content, n);
        }
    }
}

fn slot_of(run: &Run, content: &Content, pick: impl Fn(Effect, u32) -> bool) -> Option<usize> {
    run.deck.iter().position(|slot| {
        slot.left > 0
            && content
                .script(slot.script)
                .is_some_and(|s| pick(s.effect, s.power))
    })
}

fn bit(id: u8) -> u32 {
    1 << (id & 31)
}

/// Play one run with the information a person has. Returns when the run is over.
fn play(content: &Content, run: &mut Run, rng: &mut DoorRng, bail_at: u32) {
    let (target, effect) = {
        let (t, e) = run.objective_target();
        (t.to_owned(), e)
    };
    let mut scanned_at: u32 = 0;
    let mut opening = true;
    for _ in 0..300 {
        if run.status != Status::Active {
            return;
        }
        if run.objective_done() || run.trace >= bail_at || run.cycles <= 2 {
            let _ = run.act(content, Act::JackOut, rng);
            return;
        }
        if opening {
            opening = false;
            for want in [Effect::Mask, Effect::Decoy] {
                if let Some(slot) = slot_of(run, content, |e, _| e == want) {
                    let _ = run.act(content, Act::Use { slot, target: None }, rng);
                }
            }
        }
        if run.trace >= 45
            && let Some(slot) = slot_of(run, content, |e, _| e == Effect::Reroute)
        {
            let _ = run.act(content, Act::Use { slot, target: None }, rng);
            continue;
        }
        let Some(here) = run.view(content, run.at) else {
            return;
        };
        if here.code == target && !here.honeypot {
            if let Some(slot) = slot_of(run, content, |e, _| e == effect) {
                if run
                    .act(content, Act::Use { slot, target: None }, rng)
                    .is_err()
                    && !run.objective_done()
                {
                    let _ = run.act(content, Act::JackOut, rng);
                    return;
                }
                continue;
            }
            let _ = run.act(content, Act::JackOut, rng);
            return;
        }
        if run.feed_live()
            && let Some(slot) = slot_of(run, content, |e, _| e == Effect::Cutfeed)
        {
            let _ = run.act(content, Act::Use { slot, target: None }, rng);
            continue;
        }
        if scanned_at & bit(run.at) == 0 {
            scanned_at |= bit(run.at);
            if let Some(slot) = slot_of(run, content, |e, _| e == Effect::Scan) {
                let _ = run.act(content, Act::Use { slot, target: None }, rng);
                continue;
            }
        }
        let mut cands: Vec<_> = run
            .exits()
            .into_iter()
            .filter(|id| run.visited & bit(*id) == 0)
            .filter_map(|id| run.view(content, id))
            .filter(|v| !v.honeypot)
            .collect();
        cands.sort_by_key(|v| {
            std::cmp::Reverse(
                (i32::from(v.code == target) * 100)
                    + i32::from(v.depth) * 10
                    + i32::from(v.open) * 5,
            )
        });
        let mut acted = false;
        for v in &cands {
            if v.open {
                // A sentry or tracer next door: jam it first if we can.
                if matches!(v.patrol, Patrol::Sentry | Patrol::Tracer)
                    && !v.patrol_off
                    && let Some(slot) = slot_of(run, content, |e, pow| {
                        e == Effect::Jam && pow >= u32::from(v.patrol_level)
                    })
                {
                    acted = run
                        .act(
                            content,
                            Act::Use {
                                slot,
                                target: Some(v.id),
                            },
                            rng,
                        )
                        .is_ok();
                    if acted {
                        break;
                    }
                }
                acted = run.act(content, Act::Move(v.id), rng).is_ok();
            } else if let Some((lock, level)) = v.lock {
                if let Some(slot) = slot_of(run, content, |e, pow| {
                    e.lock() == Some(lock) && pow >= u32::from(level)
                }) {
                    acted = run
                        .act(
                            content,
                            Act::Use {
                                slot,
                                target: Some(v.id),
                            },
                            rng,
                        )
                        .is_ok();
                }
            } else if v.patrol == Patrol::BlackWall
                && let Some(slot) = slot_of(run, content, |e, pow| {
                    e == Effect::Purge && pow >= u32::from(v.patrol_level)
                })
            {
                acted = run
                    .act(
                        content,
                        Act::Use {
                            slot,
                            target: Some(v.id),
                        },
                        rng,
                    )
                    .is_ok();
            }
            if acted {
                break;
            }
        }
        if acted {
            continue;
        }
        // Nowhere new to go: step back toward a visited neighbour with a smaller depth, or leave.
        let back = run
            .exits()
            .into_iter()
            .filter_map(|id| run.view(content, id))
            .filter(|v| v.open && v.depth < here.depth)
            .min_by_key(|v| v.depth);
        match back {
            Some(v) if run.act(content, Act::Move(v.id), rng).is_ok() => {
                // Walking back over visited nodes is allowed; mark so we don't loop on scans.
            }
            _ => {
                let _ = run.act(content, Act::JackOut, rng);
                return;
            }
        }
    }
    let _ = run.act(content, Act::JackOut, rng);
}

/// Simulate `cfg.players` bots sharing a world for `cfg.days` days.
#[must_use]
pub fn simulate(content: &Content, cfg: Config) -> Report {
    let mut rng = DoorRng::from_seed(cfg.seed);
    let day0: i64 = 20_000;
    let mut world = World::default();
    let mut players: Vec<Player> = (0..cfg.players)
        .map(|i| {
            Player::new(
                content,
                rng.next_u64() ^ i as u64,
                &format!("bot{i}"),
                cfg.season,
                day0,
            )
        })
        .collect();
    let mut report = Report {
        days: cfg.days,
        players: cfg.players,
        ..Report::default()
    };
    for d in 0..i64::from(cfg.days) {
        let today = day0 + d + 1;
        let mut night = DoorRng::from_label("nullroute-sim-tick", today);
        world.nightly(content, &mut night, today);
        let mut earned_today: u64 = 0;
        for (idx, p) in players.iter_mut().enumerate() {
            let before = p.earned;
            let _ = p.rollover(content, today);
            while shop(content, p, world.market_pct) {}
            if p.heat >= 50 {
                let _ = p.lay_low(content);
            }
            let mut took = 0;
            loop {
                if p.cycles < JACK_IN_CYCLES + 12 {
                    break;
                }
                let mut offers: Vec<Contract> = world
                    .board
                    .iter()
                    .copied()
                    .filter(|c| feasible(content, p, c))
                    .collect();
                offers.sort_by_key(|c| std::cmp::Reverse(c.reward));
                let Some(contract) = offers.first().copied() else {
                    break;
                };
                plan_deck(content, p, &contract);
                let Ok(mut run) = p.begin_run(content, cfg.season, &world, &contract) else {
                    break;
                };
                took += 1;
                play(
                    content,
                    &mut run,
                    &mut rng,
                    if idx % 3 == 0 { 99 } else { 80 },
                );
                report.peaks.push(run.result().peak);
                let result = run.result();
                let tier = usize::from(contract.tier.clamp(1, 3) - 1);
                report.tiers[tier].runs += 1;
                match result.status {
                    Status::Burned => report.tiers[tier].burned += 1,
                    _ if result.objective => report.tiers[tier].done += 1,
                    _ => report.dry += 1,
                }
                let _ = p.finish_run(content, &contract, &result, &mut rng);
                world.apply_run(contract.corp, contract.tier, &result);
                while shop(content, p, world.market_pct) {}
            }
            if took == 0 {
                report.idle_days += 1;
            }
            earned_today += p.earned - before;
        }
        report.per_day.push(earned_today);
    }
    report.earned = players.iter().map(|p| p.earned).sum();
    report.marks = players.iter().map(|p| p.marks + p.stash).collect();
    report.heat = players.iter().map(|p| p.heat).collect();
    report.rep = players.iter().map(|p| p.rep).collect();
    report.rig = players
        .iter()
        .map(|p| p.rig.iter().map(|l| u32::from(*l)).sum())
        .collect();
    report.bots = players.iter().map(|p| u32::from(p.bots)).collect();
    report.owned = players
        .iter()
        .map(|p| u32::try_from(p.owned.len()).unwrap_or(0))
        .collect();
    report.breaches = world.breaches;
    report.board = world.board.len();
    report
}
