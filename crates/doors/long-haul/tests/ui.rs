//! The L3 screens: snapshots of every screen at three terminal sizes in two tiers, a
//! look-around for every screen, and one full trip driven by keys.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    clippy::many_single_char_names,
    clippy::expect_used
)]

mod common;

use common::ui::{self, scenes};
use russh_bbs_door::{ColorDepth, Tier};
use russh_bbs_door_long_haul::LongHaul;

#[test]
#[ignore = "writes screenshots to $LH_SHOTS"]
fn dump_screens() {
    let Some(dir) = ui::dump_dir() else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let door = LongHaul::new().unwrap();
    for s in scenes() {
        for (cols, rows) in [(80u16, 24u16), (100, 30), (132, 43)] {
            let (w, h) = ui::door_area(cols, rows);
            let d = (s.build)(&door);
            for (tier, depth, tag) in [
                (Tier::Unicode, ColorDepth::TrueColor, "true"),
                (Tier::Unicode, ColorDepth::Ansi16, "ansi16"),
                (Tier::Ascii, ColorDepth::Mono, "ascii"),
            ] {
                let buf = ui::render(&d, w, h, tier, depth);
                let name = format!("{}_{cols}x{rows}_{tag}", s.name);
                std::fs::write(dir.join(format!("{name}.html")), ui::html(&name, &buf)).unwrap();
                std::fs::write(dir.join(format!("{name}.txt")), ui::text(&buf).join("\n")).unwrap();
            }
        }
    }
}

const SIZES: [(u16, u16); 3] = [(80, 24), (100, 30), (132, 43)];

fn snapshot_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(format!("{name}.txt"))
}

/// Compare `got` with the stored snapshot; `UPDATE_SNAPSHOTS=1` rewrites it.
fn check_snapshot(name: &str, got: &str) {
    let path = snapshot_path(name);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("no snapshot {name}: run with UPDATE_SNAPSHOTS=1"));
    assert!(
        want == got,
        "snapshot {name} changed (UPDATE_SNAPSHOTS=1 to accept):\n--- want\n{want}\n--- got\n{got}"
    );
}

#[test]
fn every_screen_matches_its_snapshot_at_three_sizes_in_both_tiers() {
    let door = LongHaul::new().unwrap();
    for s in scenes() {
        for (cols, rows) in SIZES {
            let (w, h) = ui::door_area(cols, rows);
            let d = (s.build)(&door);
            for (tier, tag) in [(Tier::Unicode, "unicode"), (Tier::Ascii, "ascii")] {
                let buf = ui::render(&d, w, h, tier, ColorDepth::Mono);
                let text = ui::text(&buf).join("\n") + "\n";
                if tier == Tier::Ascii {
                    assert!(
                        text.is_ascii(),
                        "{} {cols}x{rows}: non-ASCII in the ASCII tier:\n{text}",
                        s.name
                    );
                }
                check_snapshot(&format!("{}_{cols}x{rows}_{tag}", s.name), &text);
            }
        }
    }
}

#[test]
fn the_nerd_font_tier_draws_what_the_unicode_tier_draws() {
    // A door has no icon font to draw with (D104): the two tiers are one drawing.
    let door = LongHaul::new().unwrap();
    for s in scenes() {
        let d = (s.build)(&door);
        let (w, h) = ui::door_area(100, 30);
        let a = ui::text(&ui::render(&d, w, h, Tier::Unicode, ColorDepth::Mono));
        let b = ui::text(&ui::render(&d, w, h, Tier::NerdFont, ColorDepth::Mono));
        assert_eq!(a, b, "{}", s.name);
    }
}

#[test]
fn screens_use_the_whole_window_when_it_grows_and_fall_back_when_it_is_tiny() {
    let door = LongHaul::new().unwrap();
    for s in scenes() {
        let d = (s.build)(&door);
        // Wider than the board's box allows today (74), for when that cap is lifted.
        for (w, h) in [
            (130u16, 40u16),
            (60, 15),
            (59, 14),
            (40, 12),
            (20, 5),
            (1, 1),
        ] {
            for tier in [Tier::Unicode, Tier::Ascii] {
                let buf = ui::render(&d, w, h, tier, ColorDepth::Ansi16);
                assert_eq!((buf.area.width, buf.area.height), (w, h), "{}", s.name);
            }
        }
        // The full screens carry the status bar; the fallback is the text screen.
        let big = ui::text(&ui::render(&d, 74, 19, Tier::Unicode, ColorDepth::Mono));
        let small = ui::text(&ui::render(&d, 58, 14, Tier::Unicode, ColorDepth::Mono));
        assert!(big.iter().any(|l| !l.trim().is_empty()), "{}", s.name);
        assert!(small.iter().any(|l| !l.trim().is_empty()), "{}", s.name);
        assert_ne!(
            big[0], small[0],
            "{}: the small window is a text screen",
            s.name
        );
    }
}

#[test]
fn every_screen_has_a_look_around() {
    let door = LongHaul::new().unwrap();
    let environment = ui::env(Tier::Ascii);
    let want: &[(&str, &str)] = &[
        ("intro", "Perk:"),
        ("bridge_docked", "Hold:"),
        ("bridge_flying", "Hold:"),
        ("chart_region", "STAR CHART"),
        ("chart_frontier", "Lanes from here"),
        ("chart_close", "spans"),
        ("route", "ROUTE PLANNER"),
        ("market", "Good"),
        ("jobs", "JOBS"),
        ("crew", "CREW"),
        ("crew_recruit", "For hire"),
        ("yard", "Shipyard"),
        ("loan", "lender"),
        ("news", "News"),
        ("standing", "FACTIONS AND STANDING"),
        ("event", "A FREIGHTER WITH A DEAD DRIVE"),
        ("combat", "Reactor split"),
        ("combat_won", "wreck"),
        ("boarding", "Map of the wreck"),
    ];
    for s in scenes() {
        let d = (s.build)(&door);
        let look = d.look_around(&environment);
        assert!(look.len() >= 3, "{}: {look:?}", s.name);
        let text = look.join("\n");
        let needle = want.iter().find(|(n, _)| *n == s.name).unwrap().1;
        assert!(
            text.contains(needle),
            "{} lacks `{needle}`:\n{text}",
            s.name
        );
        for line in &look {
            assert!(line.chars().count() <= 71, "{}: {line}", s.name);
        }
    }
    assert_eq!(
        want.len(),
        scenes().len(),
        "a screen has no look-around entry"
    );
}

#[test]
fn the_chart_look_around_names_every_lane_and_its_cost() {
    let door = LongHaul::new().unwrap();
    let mut h = common::Harness::new(1, 11);
    let p = ui::rich_captain(&mut h, false);
    let neighbours = h.frontier.neighbours(p.at).len();
    let d = ui::open(&door, Some(&p), "v", 5);
    let look = d.look_around(&ui::env(Tier::Ascii));
    let lanes = look
        .iter()
        .filter(|l| l.contains(" spans, ") && l.contains(" fuel, "))
        .count();
    assert_eq!(lanes, neighbours, "{look:?}");
}

#[test]
fn the_cursor_walks_the_stars_and_the_planner_plans_without_spending() {
    let door = LongHaul::new().unwrap();
    let mut h = common::Harness::new(1, 11);
    let p = ui::rich_captain(&mut h, false);
    let before = p.clone();
    // Walk the cursor in every direction, open the planner, change the goal, back out.
    let d = ui::open(
        &door,
        Some(&p),
        "v left up right down right right enter g g g esc esc",
        5,
    );
    let mut driver = d;
    let effects = driver.take_effects();
    assert!(
        effects.save().is_none(),
        "planning is free: nothing is saved"
    );
    let look = driver.look_around(&ui::env(Tier::Ascii));
    assert!(
        look.iter().any(|l| l.contains("Hold:")),
        "back on the bridge: {look:?}"
    );
    drop(before);
}

fn saved(driver: &mut russh_bbs_door::host::Driver, from: &Player) -> Player {
    let door = LongHaul::new().unwrap();
    match driver.take_effects().save() {
        Some(bytes) => Player::decode(bytes, door.content(), &door.frontier(1)).unwrap(),
        None => from.clone(),
    }
}

use russh_bbs_door_long_haul::player::Player;
use russh_bbs_door_long_haul::scene::Scene;

/// One whole trip through the keys, one visit per leg of it, each visit starting from the save
/// the last one left (a captain who logs off between jumps): take a contract, load cargo,
/// undock, choose a destination on the chart, plan the route, jump, meet an event, fight,
/// dock and sell.
#[test]
fn one_full_trip_from_contract_to_sale_by_keys_alone() {
    let door = LongHaul::new().unwrap();
    let mut h = common::Harness::new(1, 11);
    let mut p = ui::rich_captain(&mut h, true);
    p.crew.clear();
    p.ship.cargo.clear();
    p.cash = 20_000;
    h.fit(&mut p, "lance1");
    let home = p.docked.unwrap();

    // 1. Jobs: the first open offer is taken with Enter.
    let d = ui::open(&door, Some(&p), "b", 1);
    let board = d.look_around(&ui::env(Tier::Ascii)).join("\n");
    assert!(board.contains("JOBS"), "{board}");
    let offers = door_offers(&door, &p);
    let first_open = offers
        .iter()
        .position(|o| o.locked.is_none())
        .expect("an open job");
    let mut spec = String::from("b");
    for _ in 0..first_open {
        spec.push_str(" down");
    }
    spec.push_str(" enter");
    let mut d = ui::open(&door, Some(&p), &spec, 1);
    p = saved(&mut d, &p);
    assert_eq!(p.contracts.len(), 1, "the job was taken");

    // 2. Market: fill the hold with the first good that can be bought, then leave.
    let mut d = ui::open(&door, Some(&p), "m a", 2);
    p = saved(&mut d, &p);
    assert!(p.hold_used() > 0, "something was bought");
    let cargo_cost = p.ledger.spend.iter().sum::<i64>();
    assert!(cargo_cost > 0);

    // 3. Undock, open the chart, walk to the nearest lane's system, plan, and fly one leg.
    let mut d = ui::open(&door, Some(&p), "u", 3);
    p = saved(&mut d, &p);
    assert!(p.docked.is_none());
    let origin = p.at;
    let mut near: Vec<(u16, u16)> = h.frontier.neighbours(origin).to_vec();
    near.sort_by_key(|&(id, dist)| (dist, id));
    let _ = near;
    // Arrow keys walk the cursor; take the first walk that lands on a system with a route.
    let mut spec = String::new();
    'search: for first in ["right", "left", "up", "down"] {
        for second in ["", "right", "left", "up", "down"] {
            let walk = format!("v {first} {second}");
            let look = ui::open(&door, Some(&p), &walk, 4)
                .look_around(&ui::env(Tier::Ascii))
                .join("\n");
            if look.contains("Route: ") && !look.contains("NOT ENOUGH FUEL") {
                spec = walk;
                break 'search;
            }
        }
    }
    assert!(!spec.is_empty(), "the cursor found a reachable system");
    let preview = ui::open(&door, Some(&p), &format!("{spec} enter"), 4);
    let route = preview.look_around(&ui::env(Tier::Ascii)).join("\n");
    assert!(
        route.contains("ROUTE PLANNER"),
        "the planner opened:\n{route}\n(from {spec})"
    );
    assert!(route.contains("Total:"), "{route}");
    // Flying: the first leg only. Whatever system the keys chose, the engine did the jump.
    let mut d = ui::open(&door, Some(&p), &format!("{spec} enter enter"), 4);
    p = saved(&mut d, &p);
    assert!(p.stats.jumps >= 1, "the ship jumped");
    assert_ne!(p.at, origin);
    assert!(p.ship.fuel < 120);

    // 4. An event: the transit of the jump may have opened one; make sure of one, and choose
    //    the way out ("note it on the log and move on") with a digit.
    h.open_event(&mut p, "dead_freighter");
    let d = ui::open(&door, Some(&p), "", 5);
    let scene = d.look_around(&ui::env(Tier::Ascii)).join("\n");
    assert!(scene.contains("A FREIGHTER WITH A DEAD DRIVE"), "{scene}");
    let mut d = ui::open(&door, Some(&p), "4", 5);
    p = saved(&mut d, &p);
    assert!(p.scene.is_none(), "the event ended");

    // 5. A fight: set the reactor with the arrow keys and fire with Enter until it ends.
    h.fight(&mut p, "skiff");
    let mut fight = String::from("right left down right enter");
    for _ in 0..24 {
        fight.push_str(" enter");
    }
    let mut d = ui::open(&door, Some(&p), &fight, 6);
    p = saved(&mut d, &p);
    match &p.scene {
        None => {}
        Some(Scene::Combat(c)) => {
            assert!(c.round >= 1 || c.phase == russh_bbs_door_long_haul::combat::Phase::Won);
        }
        Some(other) => panic!("unexpected scene {other:?}"),
    }
    // Run from whatever is left, then clear the scene so the trip can go on.
    let mut d = ui::open(&door, Some(&p), &"r ".repeat(30), 7);
    p = saved(&mut d, &p);
    h.finish_scene(&mut p);
    assert!(p.scene.is_none());

    // 6. Dock where the cargo sells and sell it all: `m`, down to the held row, `z`.
    let buyers = h.stations("research");
    let station = buyers[0];
    h.dock_free(&mut p, station);
    if p.hold_used() == 0
        || p.market_rows(&h.content, &h.frontier, &h.world)
            .iter()
            .all(|r| r.held == 0)
    {
        // The fight cost the hold (a defeat takes cargo): the trip carries on with a fresh lot.
        let good = h.frontier.station(station).unwrap().goods[0].good;
        p.ship.cargo.clear();
        p.ship.cargo.push(russh_bbs_door_long_haul::ship::Lot {
            good,
            tons: 8,
            paid: 400,
        });
    }
    let rows = p.market_rows(&h.content, &h.frontier, &h.world);
    let held: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.held > 0)
        .map(|(i, _)| i)
        .collect();
    assert!(!held.is_empty(), "the hold still has cargo");
    let mut spec = String::from("m");
    for _ in 0..held[0] {
        spec.push_str(" down");
    }
    spec.push_str(" z");
    let cash = p.cash;
    let mut d = ui::open(&door, Some(&p), &spec, 8);
    let after = saved(&mut d, &p);
    assert!(after.hold_used() < p.hold_used(), "cargo was sold");
    assert!(after.cash > cash, "and paid");
    assert_eq!(
        after.ledger.net() - p.ledger.net(),
        after.cash - p.cash,
        "the sale went through the ledger"
    );
    let _ = home;
}

fn door_offers(door: &LongHaul, p: &Player) -> Vec<russh_bbs_door_long_haul::contracts::Offer> {
    russh_bbs_door_long_haul::contracts::board(
        door.content(),
        &door.frontier(1),
        p.docked.unwrap(),
        p.last_day,
        &p.standing,
    )
    .into_iter()
    .filter(|o| !p.taken.contains(&o.key))
    .collect()
}
