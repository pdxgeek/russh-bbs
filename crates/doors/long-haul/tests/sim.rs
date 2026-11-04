//! The headless balance run: sixty days of a small shared frontier, with bot captains of
//! several strategies trading in the same markets, the nightly tick running between days.
//!
//! The test asserts the guard rails (prices stay bounded, nobody runs away, information and
//! skill matter) and prints the curves. Run with `--nocapture` to see them:
//! `cargo test -p russh-bbs-door-long-haul --test sim -- --nocapture`.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::print_stdout,
    clippy::many_single_char_names,
    clippy::assert_is_empty,
    clippy::similar_names
)]

mod common;

use std::collections::{BTreeMap, VecDeque};

use common::Harness;
use russh_bbs_door::DoorRng;
use russh_bbs_door_long_haul::content::GoodClass;
use russh_bbs_door_long_haul::engine::{Action, RepairTarget};
use russh_bbs_door_long_haul::events::Event;
use russh_bbs_door_long_haul::market::{depth_at, fill_buy_within, fill_sell, step_mid};
use russh_bbs_door_long_haul::player::Player;
use russh_bbs_door_long_haul::ship::Ship;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    Informed,
    Oracle,
    Smuggler,
    Explorer,
    Random,
}

struct Bot {
    kind: Kind,
    p: Player,
    queue: VecDeque<Action>,
    tally: BTreeMap<&'static str, u32>,
}

fn name_of(e: &Event) -> &'static str {
    match e {
        Event::Seized { .. } => "seized",
        Event::Towed { .. } => "towed",
        Event::Repossessed { .. } => "repossessed",
        Event::Charted { .. } => "charted",
        Event::DriveFault { .. } => "drive fault",
        Event::DueMissed { .. } => "missed payment",
        Event::Sold { .. } => "sales",
        Event::SurveySold { .. } => "survey sales",
        Event::Repaired { .. } => "repairs",
        Event::ModuleBought { .. } => "modules bought",
        Event::Jumped { .. } => "jumps",
        _ => "",
    }
}

struct Plan {
    actions: Vec<Action>,
}

fn path_cost(h: &Harness, p: &Player, path: &[u16]) -> Option<(u16, u32)> {
    let stats = p.stats(&h.content);
    let (mut fuel, mut days) = (0u16, 0u32);
    let mut at = p.at;
    for &next in path {
        let d = h.frontier.lane(at, next)?;
        fuel += Ship::fuel_for(&stats, d);
        days += Ship::days_for(&stats, d);
        at = next;
    }
    Some((fuel, days))
}

/// The best trade within two jumps, or none. `kind` decides what the bot knows and may carry.
fn plan_trade(h: &Harness, bot: &Bot) -> Option<Plan> {
    let p = &bot.p;
    let here = p.docked?;
    let st = h.frontier.station(here)?;
    let stats = p.stats(&h.content);
    let rows = p.market_rows(&h.content, &h.frontier, &h.world);
    let free = u32::from(stats.cargo_cap).saturating_sub(p.ship.cargo_tons());
    // Keep enough back for fuel to the farthest destination considered and a floor for dock fees.
    let budget = (p.cash - 400 - 12 * i64::from(stats.fuel_cap.saturating_sub(p.ship.fuel))).max(0);
    let spread_src = p.spread_at(&h.content, st);
    // Paths of one and two jumps.
    let mut paths: Vec<Vec<u16>> = Vec::new();
    for &(a, _) in h.frontier.neighbours(p.at) {
        paths.push(vec![a]);
        for &(b, _) in h.frontier.neighbours(a) {
            if b != p.at {
                paths.push(vec![a, b]);
            }
        }
    }
    let mut best: Option<(i64, Plan)> = None;
    for path in paths {
        let Some((fuel, days)) = path_cost(h, p, &path) else {
            continue;
        };
        if fuel > stats.fuel_cap.saturating_sub(5) {
            continue;
        }
        let dest_sys = *path.last().unwrap();
        if !h.world.is_charted(&h.frontier, dest_sys) {
            continue;
        }
        for &dest in &h.frontier.system(dest_sys).unwrap().stations {
            let dst = h.frontier.station(dest).unwrap();
            let spread_dst = p.spread_at(&h.content, dst);
            for row in &rows {
                let good = &h.content.goods[usize::from(row.good)];
                let contraband = good.class == GoodClass::Contraband;
                if !row.can_buy || row.buy == 0 || (contraband && bot.kind != Kind::Smuggler) {
                    continue;
                }
                let Some(slot_there) = dst.goods.iter().position(|m| m.good == row.good) else {
                    continue;
                };
                let depth_src =
                    depth_at(&h.content.goods, &h.frontier, here, usize::from(row.slot));
                let stock_here = h.world.stock_of(here, usize::from(row.slot));
                let want = u32::from(u16::try_from(free).unwrap_or(u16::MAX));
                let buy =
                    fill_buy_within(good.base, depth_src, stock_here, want, spread_src, budget);
                if buy.units == 0 {
                    continue;
                }
                let depth_dst = depth_at(&h.content.goods, &h.frontier, dest, slot_there);
                let revenue = if bot.kind == Kind::Oracle || bot.kind == Kind::Smuggler {
                    fill_sell(
                        good.base,
                        depth_dst,
                        h.world.stock_of(dest, slot_there),
                        buy.units,
                        spread_dst,
                    )
                    .total
                } else if let Some(price) = p
                    .intel_at(dest)
                    .and_then(|i| i.prices.get(slot_there))
                    .map(|&m| i64::from(m))
                {
                    // Last seen mid price, a little shaved for the slippage of selling a full hold.
                    i64::from(buy.units) * price * (1000 - spread_dst) / 1000 * 93 / 100
                } else {
                    // Never been there: guess from the economy type, which the chart shows.
                    let eco = &h.content.economies[usize::from(dst.economy)];
                    let guess = if eco.needs.contains(&good.id) {
                        150
                    } else if eco.produces.contains(&good.id) {
                        70
                    } else {
                        100
                    };
                    i64::from(buy.units) * i64::from(good.base) * guess / 100 * (1000 - spread_dst)
                        / 1000
                        * 90
                        / 100
                };
                let fuel_cost = i64::from(fuel) * 9;
                let profit = revenue - buy.total - fuel_cost;
                if profit < buy.total / 20 {
                    continue;
                }
                let score = profit * 100 / i64::from(days + 1);
                if best.as_ref().is_none_or(|(s, _)| score > *s) {
                    let mut actions = vec![Action::Buy {
                        slot: row.slot,
                        tons: u16::try_from(buy.units).unwrap(),
                    }];
                    let need = fuel + 4;
                    if p.ship.fuel < need {
                        actions.insert(0, Action::Refuel(need - p.ship.fuel + 20));
                    }
                    actions.extend(path.iter().map(|&s| Action::Jump(s)));
                    actions.push(Action::Dock(dest));
                    best = Some((score, Plan { actions }));
                }
            }
        }
    }
    best.map(|(_, plan)| plan)
}

/// Sell what the station will buy at no great loss, repair, repay, upgrade: the housekeeping.
fn housekeeping(h: &mut Harness, bot: &mut Bot) {
    let station = bot.p.docked.unwrap();
    let rows = bot.p.market_rows(&h.content, &h.frontier, &h.world);
    for row in rows {
        let held = bot.p.ship.tons_of(row.good);
        if held == 0 || row.sell == 0 {
            continue;
        }
        let lot = bot
            .p
            .ship
            .cargo
            .iter()
            .find(|l| l.good == row.good)
            .unwrap();
        let avg = lot.paid / i64::from(lot.tons.max(1));
        let contraband = h.content.goods[usize::from(row.good)].class == GoodClass::Contraband;
        if row.sell * 100 >= avg * 80 || contraband {
            run(
                h,
                bot,
                &Action::Sell {
                    slot: row.slot,
                    tons: held,
                },
            );
        }
    }
    let (repair, shipyard) = {
        let st = h.frontier.station(station).unwrap();
        (st.repair, st.shipyard)
    };
    if repair
        && (bot.p.worst_wear_pct() >= 40
            || u32::from(bot.p.ship.hull_hp) * 100
                < 65 * u32::from(h.content.hulls[usize::from(bot.p.ship.hull)].hull_points))
    {
        run(h, bot, &Action::Repair(RepairTarget::All));
    }
    if shipyard && bot.p.cash > 12_000 {
        let hull = &h.content.hulls[usize::from(bot.p.ship.hull)];
        let used = |k| bot.p.ship.used_slots(&h.content, k);
        let id = if used(russh_bbs_door_long_haul::content::SlotKind::Cargo)
            < usize::from(hull.slots.cargo)
        {
            Some("pod2")
        } else if used(russh_bbs_door_long_haul::content::SlotKind::Tank)
            < usize::from(hull.slots.tank)
        {
            Some("tank1")
        } else {
            None
        };
        if let Some(id) = id {
            run(h, bot, &Action::BuyModule(h.module(id)));
        }
    }
    if bot.p.cash < 300 && bot.p.debt.principal + 1_500 <= bot.p.credit_limit() {
        // Broke at a dock: the lender will always lend, at a price.
        run(h, bot, &Action::Borrow(1_500));
    }
    if bot.p.cash > 9_000 && bot.p.debt.principal > 0 {
        let pay = (bot.p.cash - 5_000).min(bot.p.debt.principal);
        run(h, bot, &Action::Repay(pay));
    }
}

fn run(h: &mut Harness, bot: &mut Bot, action: &Action) -> bool {
    match h.go(&mut bot.p, action) {
        Ok(events) => {
            for e in &events {
                let n = name_of(e);
                if !n.is_empty() {
                    *bot.tally.entry(n).or_insert(0) += 1;
                }
            }
            true
        }
        Err(_) => false,
    }
}

/// The shortest path (as systems, excluding the start) to the nearest system with a station
/// that satisfies `want`, or none.
fn route_to(h: &Harness, from: u16, want: impl Fn(u16) -> bool) -> Option<Vec<u16>> {
    let mut prev: Vec<Option<u16>> = vec![None; h.frontier.systems.len()];
    let mut seen = vec![false; h.frontier.systems.len()];
    let mut queue = VecDeque::from([from]);
    seen[usize::from(from)] = true;
    while let Some(at) = queue.pop_front() {
        let found = h
            .frontier
            .system(at)
            .unwrap()
            .stations
            .iter()
            .any(|&s| want(s));
        if found && at != from && h.world.is_charted(&h.frontier, at) {
            let mut path = vec![at];
            let mut cur = at;
            while let Some(p) = prev[usize::from(cur)] {
                if p == from {
                    break;
                }
                path.push(p);
                cur = p;
            }
            path.reverse();
            return Some(path);
        }
        for &(next, _) in h.frontier.neighbours(at) {
            if !seen[usize::from(next)] {
                seen[usize::from(next)] = true;
                prev[usize::from(next)] = Some(at);
                queue.push_back(next);
            }
        }
    }
    None
}

fn wander(h: &mut Harness, bot: &mut Bot, prefer_uncharted: bool, rng: &mut DoorRng) {
    let neighbours: Vec<u16> = h
        .frontier
        .neighbours(bot.p.at)
        .iter()
        .map(|&(id, _)| id)
        .collect();
    let stats = bot.p.stats(&h.content);
    let affordable: Vec<u16> = neighbours
        .iter()
        .copied()
        .filter(|&n| {
            Ship::fuel_for(&stats, h.frontier.lane(bot.p.at, n).unwrap()) <= bot.p.ship.fuel
        })
        .collect();
    if affordable.is_empty() {
        if !run(h, bot, &Action::CallTow) {
            // Dock and buy fuel if there is a station here.
            if let Some(&s) = h.frontier.system(bot.p.at).unwrap().stations.first() {
                run(h, bot, &Action::Dock(s));
            }
            run(h, bot, &Action::Refuel(200));
        }
        return;
    }
    let pick = if prefer_uncharted {
        affordable
            .iter()
            .copied()
            .find(|&n| !h.world.is_charted(&h.frontier, n))
    } else {
        None
    };
    let target = pick.unwrap_or_else(|| {
        affordable[usize::try_from(rng.below(u32::try_from(affordable.len()).unwrap())).unwrap()]
    });
    // Keep the tank healthy before a long wander.
    if bot.p.docked.is_some() {
        run(h, bot, &Action::Refuel(200));
    }
    run(h, bot, &Action::Jump(target));
}

fn step(h: &mut Harness, bot: &mut Bot, rng: &mut DoorRng) {
    match bot.kind {
        Kind::Random => {
            // The floor: any legal-looking action, as the economy fuzzer does.
            let neighbours: Vec<u16> = h
                .frontier
                .neighbours(bot.p.at)
                .iter()
                .map(|&(id, _)| id)
                .collect();
            let a = match rng.below(8) {
                0 => Action::Jump(
                    neighbours[usize::try_from(
                        rng.below(u32::try_from(neighbours.len()).unwrap()),
                    )
                    .unwrap()],
                ),
                1 => Action::Dock(
                    h.frontier
                        .system(bot.p.at)
                        .unwrap()
                        .stations
                        .first()
                        .copied()
                        .unwrap_or(0),
                ),
                2 => Action::Buy {
                    slot: u8::try_from(rng.below(10)).unwrap(),
                    tons: u16::try_from(rng.range(1, 40)).unwrap(),
                },
                3 => Action::Sell {
                    slot: u8::try_from(rng.below(10)).unwrap(),
                    tons: 40,
                },
                4 => Action::Refuel(60),
                5 => Action::Repair(RepairTarget::All),
                6 => Action::Skim,
                _ => Action::Undock,
            };
            run(h, bot, &a);
        }
        Kind::Explorer => {
            if let Some(action) = bot.queue.pop_front() {
                if !run(h, bot, &action) {
                    bot.queue.clear();
                }
                return;
            }
            if bot.p.survey.len() >= 3 && bot.p.stats(&h.content).fuel_cap > 0 {
                // Enough data in the hold: take it to the Guild.
                let lab = h.content.economy_ix("research").unwrap();
                let here_lab = bot
                    .p
                    .docked
                    .is_some_and(|s| usize::from(h.frontier.station(s).unwrap().economy) == lab);
                if here_lab {
                    run(h, bot, &Action::SellSurvey);
                    return;
                }
                if let Some(path) = route_to(h, bot.p.at, |s| {
                    usize::from(h.frontier.station(s).unwrap().economy) == lab
                }) {
                    if bot.p.docked.is_some() {
                        run(h, bot, &Action::Refuel(200));
                    }
                    bot.queue.extend(path.iter().map(|&s| Action::Jump(s)));
                    if let Some(&last) = path.last() {
                        let station = h
                            .frontier
                            .system(last)
                            .unwrap()
                            .stations
                            .iter()
                            .copied()
                            .find(|&s| usize::from(h.frontier.station(s).unwrap().economy) == lab)
                            .unwrap();
                        bot.queue.push_back(Action::Dock(station));
                    }
                    return;
                }
            }
            let stats = bot.p.stats(&h.content);
            let low = u32::from(bot.p.ship.fuel) * 100 < 45 * u32::from(stats.fuel_cap);
            if bot.p.docked.is_none() && low {
                // Running low: make for the nearest port rather than the next unknown.
                if let Some(&s) = h.frontier.system(bot.p.at).unwrap().stations.first() {
                    run(h, bot, &Action::Dock(s));
                } else if let Some(path) =
                    route_to(h, bot.p.at, |s| h.frontier.station(s).is_some())
                {
                    if !run(h, bot, &Action::Jump(path[0])) {
                        run(h, bot, &Action::CallTow);
                    }
                    return;
                }
            }
            if bot.p.docked.is_none()
                && !low
                && let Some(&s) = h.frontier.system(bot.p.at).unwrap().stations.first()
            {
                run(h, bot, &Action::Dock(s));
            }
            if bot.p.docked.is_some() {
                run(h, bot, &Action::SellSurvey);
                housekeeping(h, bot);
                run(h, bot, &Action::Refuel(200));
                run(h, bot, &Action::DeepScan);
            } else {
                run(h, bot, &Action::DeepScan);
            }
            wander(h, bot, true, rng);
        }
        _ => {
            if let Some(action) = bot.queue.pop_front() {
                if !run(h, bot, &action) {
                    bot.queue.clear();
                }
                return;
            }
            if bot.p.docked.is_none() {
                match h.frontier.system(bot.p.at).unwrap().stations.first() {
                    Some(&s) if bot.p.cash > 50 => {
                        run(h, bot, &Action::Dock(s));
                    }
                    _ => wander(h, bot, false, rng),
                }
                return;
            }
            housekeeping(h, bot);
            if let Some(plan) = plan_trade(h, bot) {
                bot.queue.extend(plan.actions);
            } else {
                // A smuggler without a run in reach heads for the nearest haven; anyone
                // else wanders and looks.
                let haven = h.content.economy_ix("haven").unwrap();
                let route = (bot.kind == Kind::Smuggler)
                    .then(|| {
                        route_to(h, bot.p.at, |s| {
                            usize::from(h.frontier.station(s).unwrap().economy) == haven
                        })
                    })
                    .flatten();
                match route {
                    Some(path) => {
                        run(h, bot, &Action::Refuel(200));
                        bot.queue.extend(path.iter().map(|&s| Action::Jump(s)));
                        if let Some(&last) = path.last() {
                            let station = h.frontier.system(last).unwrap().stations[0];
                            bot.queue.push_back(Action::Dock(station));
                        }
                    }
                    None => wander(h, bot, false, rng),
                }
            }
        }
    }
}

fn price_index(h: &Harness) -> (i64, i64, i64) {
    let (mut sum, mut n, mut lo, mut hi) = (0i64, 0i64, i64::MAX, 0i64);
    for st in &h.frontier.stations {
        for (slot, ms) in st.goods.iter().enumerate() {
            let g = &h.content.goods[usize::from(ms.good)];
            let depth = depth_at(&h.content.goods, &h.frontier, st.id, slot);
            let mid =
                step_mid(g.base, depth, h.world.stock_of(st.id, slot)) * 1000 / i64::from(g.base);
            sum += mid;
            n += 1;
            lo = lo.min(mid);
            hi = hi.max(mid);
        }
    }
    (sum / n, lo, hi)
}

fn median(v: &mut [i64]) -> i64 {
    v.sort_unstable();
    v[v.len() / 2]
}

struct Outcome {
    curves: BTreeMap<Kind, Vec<i64>>,
    index: Vec<(i64, i64, i64)>,
    tallies: BTreeMap<Kind, BTreeMap<&'static str, u32>>,
    ledgers: Vec<(Kind, String)>,
    final_nw: BTreeMap<Kind, Vec<i64>>,
    debt: BTreeMap<Kind, Vec<i64>>,
    traded: i64,
}

fn simulate(season: u32, seed: u64, days: usize) -> Outcome {
    let mut h = Harness::new(season, seed);
    let mut rng = DoorRng::from_seed(seed ^ 0x5151);
    let roster = [
        (Kind::Informed, "dockhand", 4),
        (Kind::Informed, "washout", 2),
        (Kind::Oracle, "dockhand", 3),
        (Kind::Smuggler, "runner", 3),
        (Kind::Explorer, "dropout", 2),
        (Kind::Random, "dockhand", 2),
    ];
    let mut bots: Vec<Bot> = Vec::new();
    for (kind, bg, n) in roster {
        for _ in 0..n {
            bots.push(Bot {
                kind,
                p: h.captain(bg),
                queue: VecDeque::new(),
                tally: BTreeMap::new(),
            });
        }
    }
    let mut curves: BTreeMap<Kind, Vec<i64>> = BTreeMap::new();
    let mut index = Vec::new();
    for day in 0..=days {
        let mut by_kind: BTreeMap<Kind, Vec<i64>> = BTreeMap::new();
        for b in &bots {
            by_kind
                .entry(b.kind)
                .or_default()
                .push(b.p.net_worth(&h.content));
        }
        for (k, mut v) in by_kind {
            curves.entry(k).or_default().push(median(&mut v));
        }
        index.push(price_index(&h));
        if day == days {
            break;
        }
        for b in &mut bots {
            for e in h.settle(&mut b.p) {
                let n = name_of(&e);
                if !n.is_empty() {
                    *b.tally.entry(n).or_insert(0) += 1;
                }
            }
            let mut guard = 0;
            while b.p.days_left > 0 && guard < 60 {
                guard += 1;
                step(&mut h, b, &mut rng);
            }
            assert!(b.p.cash >= 0 && b.p.ledger.net() == b.p.cash);
        }
        h.next_day();
    }
    let mut tallies: BTreeMap<Kind, BTreeMap<&'static str, u32>> = BTreeMap::new();
    let mut final_nw: BTreeMap<Kind, Vec<i64>> = BTreeMap::new();
    let mut debt: BTreeMap<Kind, Vec<i64>> = BTreeMap::new();
    let mut ledgers = Vec::new();
    for b in &bots {
        let l = &b.p.ledger;
        ledgers.push((
            b.kind,
            format!(
                "trade +{} -{} fuel -{} repair -{} dock -{} tow -{} fine -{} module -{} +{} pay -{} loan +{} jumps {} tons {}",
                l.income[1], l.spend[1], l.spend[2], l.spend[3], l.spend[11], l.spend[10], l.spend[9], l.spend[4], l.income[4], l.spend[7], l.income[6], b.p.stats.jumps, b.p.stats.tons_traded
            ),
        ));
        for (k, v) in &b.tally {
            *tallies.entry(b.kind).or_default().entry(k).or_insert(0) += v;
        }
        final_nw
            .entry(b.kind)
            .or_default()
            .push(b.p.net_worth(&h.content));
        debt.entry(b.kind).or_default().push(b.p.debt.principal);
    }
    Outcome {
        curves,
        index,
        tallies,
        ledgers,
        final_nw,
        debt,
        traded: h.world.traded_tons,
    }
}

fn report(o: &Outcome, days: usize) {
    println!("\nmedian net worth (credits) by strategy, every 10 days");
    print!("{:<10}", "day");
    for d in (0..=days).step_by(10) {
        print!("{d:>9}");
    }
    println!();
    for (k, curve) in &o.curves {
        print!("{:<10}", format!("{k:?}"));
        for d in (0..=days).step_by(10) {
            print!("{:>9}", curve[d]);
        }
        println!();
    }
    println!("\nprice index (mean mid/base, per-mille), min and max across all markets");
    for d in (0..=days).step_by(10) {
        let (mean, lo, hi) = o.index[d];
        println!("day {d:>3}: mean {mean:>5}  min {lo:>5}  max {hi:>5}");
    }
    println!("\nevents by strategy (all captains of that kind)");
    for (k, t) in &o.tallies {
        println!("{k:?}: {t:?}");
    }
    println!("\nfinal net worth per captain, and debt");
    for (k, v) in &o.final_nw {
        println!("{k:?}: nw {v:?} debt {:?}", o.debt[k]);
    }
    println!("\nledger per captain (credits)");
    for (k, l) in &o.ledgers {
        println!("{k:?}: {l}");
    }
    println!("tons traded by every captain: {}", o.traded);
}

#[test]
fn sixty_days_of_trade_stay_inside_the_guard_rails_and_skill_and_information_pay() {
    let days = 60;
    let o = simulate(1, 2024, days);
    report(&o, days);
    for (mean, lo, hi) in &o.index {
        assert!(
            (700..=1800).contains(mean),
            "the price level drifted to {mean}"
        );
        assert!(
            *lo >= 400 && *hi <= 3000,
            "prices left their band: {lo}..{hi}"
        );
    }
    let informed = *o.curves[&Kind::Informed].last().unwrap();
    let oracle = *o.curves[&Kind::Oracle].last().unwrap();
    let random = *o.curves[&Kind::Random].last().unwrap();
    let start = o.curves[&Kind::Informed][0];
    assert!(informed > start, "trading must pay: {start} -> {informed}");
    assert!(
        informed > random,
        "a plan beats random flailing: {informed} vs {random}"
    );
    assert!(
        oracle >= informed * 8 / 10,
        "information is worth something, not everything"
    );
    let growth = (informed - start) / i64::try_from(days).unwrap();
    assert!(growth < 20_000, "{growth} credits a day is a runaway");
    assert!(growth > 100, "{growth} credits a day is not a living");
}

#[test]
fn the_same_seed_gives_the_same_sixty_days() {
    let a = simulate(2, 7, 30);
    let b = simulate(2, 7, 30);
    assert_eq!(a.curves, b.curves);
    assert_eq!(a.index, b.index);
}

fn trace(kind: Kind, background: &str, days: usize) {
    let mut h = Harness::new(1, 2024);
    let mut rng = DoorRng::from_seed(0x07e8 ^ 0x5151);
    let mut bot = Bot {
        kind,
        p: h.captain(background),
        queue: VecDeque::new(),
        tally: BTreeMap::new(),
    };
    for day in 0..days {
        h.settle(&mut bot.p);
        let mut guard = 0;
        while bot.p.days_left > 0 && guard < 60 {
            guard += 1;
            let before = (bot.p.cash, bot.p.at, bot.p.docked, bot.p.ship.fuel);
            step(&mut h, &mut bot, &mut rng);
            println!(
                "day {day} {before:?} -> cash {} at {} docked {:?} fuel {} days {} cargo {} survey {}",
                bot.p.cash,
                bot.p.at,
                bot.p.docked,
                bot.p.ship.fuel,
                bot.p.days_left,
                bot.p.ship.cargo_tons(),
                bot.p.survey.len()
            );
        }
        h.next_day();
    }
}

#[test]
#[ignore = "debugging aid: prints one washout's first days"]
fn trace_a_washout() {
    trace(Kind::Informed, "washout", 8);
}

#[test]
#[ignore = "debugging aid: prints one explorer's first days"]
fn trace_an_explorer() {
    trace(Kind::Explorer, "dropout", 12);
}

#[test]
fn across_several_seasons_a_plan_beats_flailing_and_prices_hold() {
    let (mut oracle_total, mut informed_total) = (0i64, 0i64);
    for seed in 1..=4u64 {
        let o = simulate(u32::try_from(seed).unwrap(), seed, 60);
        let last = |k: Kind| *o.curves[&k].last().unwrap();
        assert!(
            last(Kind::Informed) > last(Kind::Random) + 20_000,
            "seed {seed}"
        );
        oracle_total += last(Kind::Oracle);
        informed_total += last(Kind::Informed);
        for (mean, lo, hi) in &o.index {
            assert!(
                (800..=1500).contains(mean) && *lo >= 400 && *hi <= 3000,
                "seed {seed}: {mean} {lo} {hi}"
            );
        }
    }
    // Live prices are worth more than a good memory, on average over the seasons.
    assert!(
        oracle_total > informed_total,
        "{oracle_total} vs {informed_total}"
    );
}

#[test]
#[ignore = "exploration: final medians across seeds"]
fn medians_across_seeds() {
    for seed in 1..=8u64 {
        let o = simulate(u32::try_from(seed % 4).unwrap() + 1, seed, 60);
        let last = |k: Kind| *o.curves[&k].last().unwrap();
        println!(
            "seed {seed}: informed {} oracle {} smuggler {} explorer {} random {} index {:?}",
            last(Kind::Informed),
            last(Kind::Oracle),
            last(Kind::Smuggler),
            last(Kind::Explorer),
            last(Kind::Random),
            o.index.last().unwrap()
        );
    }
}
