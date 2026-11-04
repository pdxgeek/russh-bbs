//! The L2 balance run: sixty days of a shared frontier with bot captains who hire crew, take
//! jobs, meet events, fight and board, with the nightly tick between days. It prints the crew
//! loyalty, combat and reputation curves and asserts the guard rails.
//!
//! `cargo test -p russh-bbs-door-long-haul --test sim_crew -- --nocapture` shows the tables.
#![allow(
    clippy::assert_is_empty,
    clippy::single_match_else,
    unused_mut,
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::print_stdout,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::cast_precision_loss
)]

mod common;

use std::collections::BTreeMap;

use common::{Harness, orders};
use russh_bbs_door::DoorRng;
use russh_bbs_door_long_haul::boarding::Drill;
use russh_bbs_door_long_haul::combat::Phase;
use russh_bbs_door_long_haul::content::Faction;
use russh_bbs_door_long_haul::contracts::{Kind, board};
use russh_bbs_door_long_haul::crew::{MAX_CREW, Role};
use russh_bbs_door_long_haul::engine::{Action, RepairTarget};
use russh_bbs_door_long_haul::events::Event;
use russh_bbs_door_long_haul::player::Player;
use russh_bbs_door_long_haul::scene::Scene;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Strategy {
    /// Takes jobs, keeps a crew fed and paid, flies carefully.
    Hauler,
    /// Takes jobs with a small crew.
    Courier,
    /// Armed, takes bounties, fights.
    Hunter,
    /// Takes jobs with a crew it then ignores: no rations, no goals.
    Neglect,
    /// Takes jobs alone and runs from every fight.
    Loner,
}

const ALL: [Strategy; 5] = [
    Strategy::Hauler,
    Strategy::Courier,
    Strategy::Hunter,
    Strategy::Neglect,
    Strategy::Loner,
];

struct Bot {
    s: Strategy,
    p: Player,
    rng: DoorRng,
    dest: Option<u16>,
    handled: Option<(u16, i64)>,
    tally: BTreeMap<&'static str, u32>,
}

fn tally(bot: &mut Bot, events: &[Event]) {
    for e in events {
        let key = match e {
            Event::Hired { .. } => "hired",
            Event::Deserted { .. } => "deserted",
            Event::Mutiny { .. } => "mutinies",
            Event::GoalMet { .. } => "goals met",
            Event::CrewHurt { .. } => "hurt",
            Event::CrewLost { .. } => "lost",
            Event::Transit { .. } => "scenes",
            Event::FightBegins { .. } => "fights",
            Event::Wrecked { .. } => "wrecked",
            Event::Ran { .. } => "ran",
            Event::Beaten { .. } => "beaten",
            Event::ContractDone { .. } => "jobs done",
            Event::ContractFailed { .. } => "jobs failed",
            Event::BoardEnd { clean: true, .. } => "boardings",
            Event::BoardEnd { clean: false, .. } => "wiped out",
            Event::WagesMissed { .. } => "wages missed",
            _ => continue,
        };
        *bot.tally.entry(key).or_default() += 1;
    }
}

fn step_towards(h: &Harness, from: u16, to: u16) -> Option<u16> {
    let hops = h.frontier.hops_from(to);
    let here = hops.get(usize::from(from)).copied()?;
    h.frontier
        .neighbours(from)
        .iter()
        .filter(|(n, _)| hops.get(usize::from(*n)).is_some_and(|x| *x < here))
        .min_by_key(|(_, d)| *d)
        .map(|(n, _)| *n)
}

fn act(h: &mut Harness, bot: &mut Bot, action: &Action) -> bool {
    let r = h.go_strict(&mut bot.p, action);
    match r {
        Ok(ev) => {
            tally(bot, &ev);
            true
        }
        Err(_) => false,
    }
}

fn play_scene(h: &mut Harness, bot: &mut Bot) {
    let Some(scene) = bot.p.scene.clone() else {
        return;
    };
    match scene {
        Scene::Event(_) => {
            // A random open option: the crew-gated ones get used when they are open.
            let opts = 5u32;
            let start = bot.rng.below(opts);
            for k in 0..opts {
                let n = u8::try_from((start + k) % opts).unwrap();
                if act(h, bot, &Action::Choose(n)) {
                    return;
                }
            }
            let _ = act(h, bot, &Action::Choose(0));
        }
        Scene::Combat(c) => {
            if c.phase == Phase::Won {
                let board = bot.p.crew.len() >= 2 && bot.s != Strategy::Neglect;
                let _ = act(h, bot, &Action::Wreck { board });
                return;
            }
            let armed = bot.p.firepower(&h.content, c.range) > 0;
            let hull = u32::from(bot.p.ship.hull_hp);
            let max = u32::from(h.content.hulls[usize::from(bot.p.ship.hull)].hull_points);
            if !armed || hull * 100 < max * 40 || bot.s == Strategy::Loner || c.round > 14 {
                let _ = act(h, bot, &Action::Run);
            } else {
                let b = bot.p.gear(&h.content).budget;
                let w = (b * 3).div_ceil(5);
                let _ = act(h, bot, &Action::Fight(orders(w, b - w, 0)));
            }
        }
        Scene::Boarding(b) => {
            let room = b.room(&h.content);
            let def = b.def(&h.content);
            let bit = |r: u8| 1u16 << u32::from(r);
            let foes = b.foes_here(&h.content);
            let hurt = b.party.iter().any(|m| m.up() && m.hp * 2 < m.max);
            let d = if foes > 0 && hurt && b.party.iter().filter(|m| m.up()).count() < 2 {
                Drill::Withdraw
            } else if foes > 0 {
                Drill::Fight
            } else if b.looted & bit(b.at) == 0
                && (room.credits[1] > 0 || room.tons > 0 || room.parts > 0)
            {
                Drill::Take
            } else if room.cache > 0 && b.read & bit(b.at) == 0 {
                Drill::Search
            } else if let Some(&to) = room.exits.iter().find(|&&e| {
                let t = &def.rooms[usize::from(e)];
                b.looted & bit(e) == 0
                    && (t.lock == 0 || b.opened & bit(e) != 0)
                    && (t.credits[1] > 0 || t.tons > 0 || t.parts > 0 || t.cache > 0)
            }) {
                Drill::Move(to)
            } else if let Some(&to) = room
                .exits
                .iter()
                .find(|&&e| def.rooms[usize::from(e)].lock > 0 && b.opened & bit(e) == 0)
            {
                Drill::Breach(to)
            } else {
                Drill::Withdraw
            };
            if !act(h, bot, &Action::Skirmish(d)) {
                let _ = act(h, bot, &Action::Skirmish(Drill::Withdraw));
            }
        }
    }
}

fn business(h: &mut Harness, bot: &mut Bot, here: u16) {
    let shipyard = h.frontier.station(here).unwrap().shipyard;
    // Fuel and wear.
    act(h, bot, &Action::Refuel(u16::MAX));
    if bot.p.worst_wear_pct() > 40 || bot.p.ship.hull_hp < 70 {
        act(h, bot, &Action::Repair(RepairTarget::All));
    }
    let careful = matches!(
        bot.s,
        Strategy::Hauler | Strategy::Hunter | Strategy::Courier
    );
    // Crew.
    let want_crew = match bot.s {
        Strategy::Hauler | Strategy::Hunter | Strategy::Neglect => 3,
        Strategy::Courier => 2,
        Strategy::Loner => 0,
    };
    if bot.p.crew.len() < want_crew && bot.p.cash > 900 {
        let list = bot.p.recruit_list(&h.content, &h.frontier, here, h.today);
        let wanted = list.iter().find(|(_, c)| match bot.s {
            Strategy::Hunter => matches!(c.role, Role::Gunner | Role::Medic | Role::Engineer),
            Strategy::Hauler => matches!(
                c.role,
                Role::Pilot | Role::Engineer | Role::Medic | Role::Quartermaster
            ),
            _ => true,
        });
        if let Some((slot, _)) = wanted {
            if bot.p.berths_used() >= bot.p.berths(&h.content) && bot.p.cash > 2200 {
                // Buy a cabin if the yard sells one.
                if shipyard {
                    let cabin = h.module("cabin1");
                    act(h, bot, &Action::BuyModule(cabin));
                }
            }
            act(h, bot, &Action::Hire(*slot));
        }
    }
    // Rations and weapons for those who care.
    if careful && !bot.p.crew.is_empty() && bot.p.rations_until < h.today + 3 {
        act(h, bot, &Action::Provision);
    }
    if bot.s == Strategy::Hunter && shipyard && bot.p.cash > 4000 {
        if bot.p.stats(&h.content).power < 4 {
            let lance = h.module("lance1");
            act(h, bot, &Action::BuyModule(lance));
        } else if bot.p.stats(&h.content).shield_pct == 0 && bot.p.cash > 4500 {
            let sh = h.module("shield1");
            act(h, bot, &Action::BuyModule(sh));
        }
    }
    // Jobs.
    if bot.p.contracts.len() < 3 {
        let offers = bot.p.board_here_for(&h.content, &h.frontier, h.today);
        let mut pick = None;
        for (i, o) in offers.iter().enumerate() {
            let c = &o.contract;
            let ok = o.locked.is_none()
                && match bot.s {
                    Strategy::Hunter => {
                        matches!(c.kind, Kind::Bounty | Kind::Escort | Kind::Delivery)
                    }
                    Strategy::Loner => matches!(c.kind, Kind::Delivery | Kind::Survey),
                    _ => matches!(
                        c.kind,
                        Kind::Delivery | Kind::Passenger | Kind::Survey | Kind::Escort
                    ),
                };
            if ok {
                pick = Some(i);
                break;
            }
        }
        if let Some(i) = pick {
            act(h, bot, &Action::Accept(u8::try_from(i).unwrap()));
        }
    }
    bot.dest = None;
    // A job, if there is one, sets the destination.
    if let Some(c) = bot.p.contracts.iter().min_by_key(|c| c.due) {
        bot.dest = Some(c.to);
        // A survey or bounty target is somewhere else first.
        if c.kind == Kind::Survey && c.progress == 0 {
            // Go to the target system's nearest station, or just to the target.
            bot.dest = h
                .frontier
                .system(c.target)
                .and_then(|s| s.stations.first().copied())
                .or(Some(c.to));
            if let Some(s) = h.frontier.system(c.target)
                && s.stations.is_empty()
            {
                bot.dest = Some(c.to);
            }
        }
    }
    // With no job to carry, wander to a nearby market and read its board.
    if bot.dest.is_none() {
        let sys = h.frontier.station(here).unwrap().system;
        let hops = h.frontier.hops_from(sys);
        let near: Vec<u16> = h
            .frontier
            .stations
            .iter()
            .filter(|s| {
                s.id != here
                    && (1..=3).contains(&hops[usize::from(s.system)])
                    && !h.frontier.systems[usize::from(s.system)].uncharted
            })
            .map(|s| s.id)
            .collect();
        if !near.is_empty() {
            bot.dest = Some(near[bot.rng.below(u32::try_from(near.len()).unwrap()) as usize]);
        }
    }
}

trait BoardFor {
    fn board_here_for(
        &self,
        content: &russh_bbs_door_long_haul::content::Content,
        frontier: &russh_bbs_door_long_haul::frontier::Frontier,
        today: i64,
    ) -> Vec<russh_bbs_door_long_haul::contracts::Offer>;
}

impl BoardFor for Player {
    fn board_here_for(
        &self,
        content: &russh_bbs_door_long_haul::content::Content,
        frontier: &russh_bbs_door_long_haul::frontier::Frontier,
        today: i64,
    ) -> Vec<russh_bbs_door_long_haul::contracts::Offer> {
        board(
            content,
            frontier,
            self.docked.unwrap(),
            today,
            &self.standing,
        )
        .into_iter()
        .filter(|o| !self.taken.contains(&o.key))
        .collect()
    }
}

fn play_day(h: &mut Harness, bot: &mut Bot) {
    h.settle(&mut bot.p);
    for _ in 0..60 {
        if bot.p.scene.is_some() {
            play_scene(h, bot);
            continue;
        }
        if bot.p.days_left < 1 {
            return;
        }
        if let Some(here) = bot.p.docked {
            if bot.handled != Some((here, h.today)) {
                bot.handled = Some((here, h.today));
                business(h, bot, here);
            }
            // With nothing to do, wait out the day; otherwise leave for the destination.
            if bot.dest.is_none() || !act(h, bot, &Action::Undock) {
                return;
            }
            continue;
        }
        // In space.
        let here_sys = bot.p.at;
        let dest_sys = bot
            .dest
            .and_then(|d| h.frontier.station(d))
            .map(|s| s.system);
        match dest_sys {
            Some(ds) if ds == here_sys => {
                let st = bot.dest.unwrap();
                if !act(h, bot, &Action::Dock(st)) {
                    bot.dest = None;
                }
            }
            Some(ds) => {
                let Some(next) = step_towards(h, here_sys, ds) else {
                    bot.dest = None;
                    continue;
                };
                if !act(h, bot, &Action::Jump(next)) {
                    // Out of fuel or barred: skim, or call for a tow.
                    if !act(h, bot, &Action::Skim) && !act(h, bot, &Action::CallTow) {
                        bot.dest = None;
                    }
                }
            }
            None => {
                // No plan: dock at the nearest station here, or hop to a neighbour with one.
                let here_st = h
                    .frontier
                    .system(here_sys)
                    .and_then(|s| s.stations.first().copied());
                if let Some(st) = here_st {
                    if !act(h, bot, &Action::Dock(st)) {
                        let n = h.frontier.neighbours(here_sys);
                        let pick = n[bot.rng.below(u32::try_from(n.len()).unwrap()) as usize].0;
                        let _ = act(h, bot, &Action::Jump(pick)) || act(h, bot, &Action::CallTow);
                    }
                } else {
                    let n = h.frontier.neighbours(here_sys);
                    let pick = n[bot.rng.below(u32::try_from(n.len()).unwrap()) as usize].0;
                    let _ = act(h, bot, &Action::Jump(pick))
                        || act(h, bot, &Action::Skim)
                        || act(h, bot, &Action::CallTow);
                }
            }
        }
    }
}

fn median(v: &mut [i64]) -> i64 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[v.len() / 2]
}

struct Sample {
    net: i64,
    crew: usize,
    loyalty: f64,
    sworn: usize,
    standing: [f64; 5],
}

fn sample(h: &Harness, bots: &[&Bot]) -> Sample {
    let mut nets: Vec<i64> = bots.iter().map(|b| b.p.net_worth(&h.content)).collect();
    let crew: usize = bots.iter().map(|b| b.p.crew.len()).sum();
    let loy: f64 = bots
        .iter()
        .flat_map(|b| b.p.crew.iter())
        .map(|c| f64::from(c.loyalty))
        .sum();
    let sworn = bots
        .iter()
        .flat_map(|b| b.p.crew.iter())
        .filter(|c| c.sworn)
        .count();
    let mut standing = [0.0; 5];
    for b in bots {
        for (i, s) in b.p.standing.iter().enumerate() {
            standing[i] += f64::from(*s) / bots.len() as f64;
        }
    }
    Sample {
        net: median(&mut nets),
        crew,
        loyalty: if crew == 0 { 0.0 } else { loy / crew as f64 },
        sworn,
        standing,
    }
}

struct Run {
    curves: BTreeMap<Strategy, Vec<Sample>>,
    totals: BTreeMap<Strategy, BTreeMap<&'static str, u32>>,
    bots: usize,
    final_bytes: Vec<Vec<u8>>,
}

fn run(season: u32, seed: u64, days: i64) -> Run {
    let mut h = Harness::new(season, seed);
    let plan: Vec<Strategy> = [
        (Strategy::Hauler, 6),
        (Strategy::Courier, 3),
        (Strategy::Hunter, 3),
        (Strategy::Neglect, 2),
        (Strategy::Loner, 2),
    ]
    .iter()
    .flat_map(|(s, n)| std::iter::repeat_n(*s, *n))
    .collect();
    let backgrounds = ["dockhand", "washout", "dropout", "runner"];
    let mut bots: Vec<Bot> = plan
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut p = h.captain(backgrounds[i % 4]);
            // Every captain begins with a berth or two to fill.
            if *s != Strategy::Loner {
                h.fit(&mut p, "cabin1");
            }
            if *s == Strategy::Hunter {
                h.fit(&mut p, "lance1");
                h.fit(&mut p, "shield1");
            }
            Bot {
                s: *s,
                p,
                rng: DoorRng::from_seed(seed ^ (i as u64 * 7919 + 1)),
                dest: None,
                handled: None,
                tally: BTreeMap::new(),
            }
        })
        .collect();
    let mut curves: BTreeMap<Strategy, Vec<Sample>> = BTreeMap::new();
    for day in 0..=days {
        if day % 10 == 0 {
            for s in ALL {
                let group: Vec<&Bot> = bots.iter().filter(|b| b.s == s).collect();
                curves.entry(s).or_default().push(sample(&h, &group));
            }
        }
        if day == days {
            break;
        }
        h.next_day();
        for bot in &mut bots {
            bot.handled = None;
            play_day(&mut h, bot);
            // The invariants, every bot, every day.
            let p = &bot.p;
            assert_eq!(p.ledger.net(), p.cash, "ledger, day {day}");
            assert!(p.cash >= 0);
            let max = h.content.hulls[usize::from(p.ship.hull)].hull_points;
            assert!((1..=max).contains(&p.ship.hull_hp));
            assert!(p.crew.len() <= MAX_CREW && p.berths_used() <= p.berths(&h.content));
            assert!(p.hold_used() <= u32::from(p.stats(&h.content).cargo_cap));
            let bytes = p.encode();
            assert_eq!(&Player::decode(&bytes, &h.content, &h.frontier).unwrap(), p);
        }
    }
    let mut totals: BTreeMap<Strategy, BTreeMap<&'static str, u32>> = BTreeMap::new();
    for b in &bots {
        let t = totals.entry(b.s).or_default();
        for (k, v) in &b.tally {
            *t.entry(k).or_default() += v;
        }
    }
    Run {
        curves,
        totals,
        bots: bots.len(),
        final_bytes: bots.iter().map(|b| b.p.encode()).collect(),
    }
}

fn count(run: &Run, s: Strategy, key: &str) -> u32 {
    run.totals
        .get(&s)
        .and_then(|t| t.get(key))
        .copied()
        .unwrap_or(0)
}

fn row<T>(c: &[Sample], width: usize, f: impl Fn(&Sample) -> T) -> String
where
    T: std::fmt::Display,
{
    c.iter()
        .map(|x| format!("{:>width$}", f(x)))
        .collect::<Vec<_>>()
        .concat()
}

fn print(run: &Run) {
    println!("\n== crew loyalty, combat and reputation over sixty days ==");
    for s in ALL {
        let c = &run.curves[&s];
        println!("\n{s:?}");
        println!(
            "day        {}",
            (0..c.len())
                .map(|i| format!("{:>8}", i * 10))
                .collect::<Vec<_>>()
                .concat()
        );
        println!("net worth  {}", row(c, 8, |x| x.net.to_string()));
        println!("crew       {}", row(c, 8, |x| x.crew.to_string()));
        println!("loyalty    {}", row(c, 8, |x| format!("{:.1}", x.loyalty)));
        println!("sworn      {}", row(c, 8, |x| x.sworn.to_string()));
        for (i, f) in Faction::ALL.iter().enumerate() {
            println!(
                "{:<10} {}",
                f.short(),
                row(c, 8, |x| format!("{:.1}", x.standing[i]))
            );
        }
        let t = &run.totals[&s];
        let line: Vec<String> = t.iter().map(|(k, v)| format!("{k} {v}")).collect();
        println!("totals     {}", line.join(", "));
    }
}

#[test]
fn sixty_days_of_crews_fights_and_reputations() {
    // Several seeds, so a single lucky run does not decide the guard rails.
    let runs: Vec<Run> = [(1u32, 11u64), (2, 22), (3, 33), (4, 44)]
        .iter()
        .map(|(season, seed)| run(*season, *seed, 60))
        .collect();
    print(&runs[0]);
    let sum = |s, k| runs.iter().map(|r| count(r, s, k)).sum::<u32>();
    // Everything in the design happens: events, fights, boardings, jobs, desertions.
    let any = |k| ALL.iter().map(|s| sum(*s, k)).sum::<u32>();
    for key in ["scenes", "hired", "fights", "jobs done", "goals met"] {
        assert!(any(key) > 0, "nothing ever produced `{key}`");
    }
    assert!(
        any("wrecked") + any("ran") + any("beaten") > 0,
        "no fight ended"
    );
    // Crew that is fed and paid stays; crew that is ignored leaves.
    let leaving = |s| {
        f64::from(sum(s, "deserted") + 2 * sum(s, "mutinies")) / f64::from(sum(s, "hired").max(1))
    };
    let (careful, neglected) = (leaving(Strategy::Hauler), leaving(Strategy::Neglect));
    println!("\nlosses per hire: hauler {careful:.2}, neglect {neglected:.2}");
    assert!(
        neglected > careful + 0.1,
        "neglect {neglected:.2} against care {careful:.2}"
    );
    assert!(sum(Strategy::Neglect, "wages missed") + sum(Strategy::Neglect, "deserted") > 0);
    // Loyalty ends higher on a ship that feeds and pays its people.
    let end_loyalty = |s| {
        let v: Vec<f64> = runs
            .iter()
            .map(|r| *r.curves[&s].last().map(|x| &x.loyalty).unwrap())
            .filter(|l| *l > 0.0)
            .collect();
        v.iter().sum::<f64>() / v.len().max(1) as f64
    };
    println!(
        "end loyalty: hauler {:.1}, neglect {:.1}",
        end_loyalty(Strategy::Hauler),
        end_loyalty(Strategy::Neglect)
    );
    assert!(end_loyalty(Strategy::Hauler) > end_loyalty(Strategy::Neglect));
    // Armed hunters win fights; unarmed loners never do.
    assert!(sum(Strategy::Hunter, "wrecked") > 0);
    assert_eq!(sum(Strategy::Loner, "wrecked"), 0);
    // Reputation moves: job-takers earn standing with someone, and nobody is outside the range.
    let moved: f64 = runs
        .iter()
        .map(|r| {
            r.curves[&Strategy::Hauler]
                .last()
                .unwrap()
                .standing
                .iter()
                .map(|s| s.abs())
                .sum::<f64>()
        })
        .sum();
    assert!(moved > 20.0, "haulers' standings barely moved: {moved}");
    for r in &runs {
        for c in r.curves.values().flatten() {
            assert!(c.standing.iter().all(|s| (-100.0..=100.0).contains(s)));
        }
        assert_eq!(r.bots, 16);
    }
}

#[test]
fn the_same_seed_plays_the_same_sixty_days() {
    let a = run(2, 99, 30);
    let b = run(2, 99, 30);
    assert_eq!(a.final_bytes, b.final_bytes);
    let c = run(2, 100, 30);
    assert_ne!(a.final_bytes, c.final_bytes);
}
