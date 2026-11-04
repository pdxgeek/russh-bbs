//! The Nullroute screens: every screen drawn at the three window sizes in every tier, a look-around
//! for each, and one whole run played through the keys.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::expect_used,
    clippy::filter_next,
    clippy::print_stdout,
    clippy::format_push_string,
    clippy::field_reassign_with_default,
    clippy::unnecessary_debug_formatting
)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::*;
use russh_bbs_core::icons::Tier;
use russh_bbs_core::limits::{Day, ManualClock};
use russh_bbs_door::host::Driver;
use russh_bbs_door::replay;
use russh_bbs_door::sim::Bot;
use russh_bbs_door::{DoorEnv, DoorKey, Flow, Save, Start};
use russh_bbs_door_nullroute::Nullroute;
use russh_bbs_door_nullroute::content::FragKind;
use russh_bbs_door_nullroute::player::Player;
use russh_bbs_door_nullroute::save::{encode, encode_world};
use russh_bbs_door_nullroute::world::{Learned, World};

/// The door areas of an 80x24, a 100x30 and a 132x43 window (the board's box takes 8 columns and
/// 5 rows).
const SIZES: [(u16, u16); 3] = [(72, 19), (92, 25), (124, 38)];

/// A day whose first-visit board has a tier-1 Steal job, which the starter deck can finish.
fn day() -> i64 {
    let c = content();
    (20_000..20_200)
        .find(|d| {
            russh_bbs_door_nullroute::world::first_board(&c, *d)
                .iter()
                .any(|k| k.tier == 1 && c.contract(k.tmpl).is_some_and(|t| t.verb == "Steal"))
        })
        .unwrap()
}

fn env(tier: Tier) -> DoorEnv {
    let mut e = DoorEnv::for_tests("ann");
    e.tier = tier;
    e
}

/// A character with a bit of everything, so every screen has something to show.
fn rich_save() -> Vec<u8> {
    let c = content();
    let mut p = Player::new(&c, 77, "ann", 1, day());
    p.marks = 640;
    p.stash = 120;
    p.rep = 32;
    p.heat = 52;
    p.earned = 4_200;
    p.runs = 31;
    p.jobs = 24;
    p.burns = 3;
    p.bots = 2;
    p.workbench = true;
    p.fence = 1;
    p.rig = [2, 1, 1, 0, 1];
    p.fried = 0b00100;
    p.xp = [14, 33, 5, 61, 2];
    p.standing[0] = 3;
    p.standing[1] = -1;
    p.standing[2] = -4;
    p.standing[3] = 1;
    for kind in [FragKind::Effect, FragKind::Target, FragKind::Modifier] {
        for f in c
            .fragments
            .fragment
            .iter()
            .filter(|f| f.kind == kind)
            .take(4)
        {
            p.fragments.push(f.num);
        }
    }
    // A few shop scripts owned, to fill the deck screen.
    for s in c.scripts.script.iter().filter(|s| s.price > 0).take(9) {
        p.owned.push(s.num);
    }
    encode(&p)
}

/// A world where the corporations have learned some of the scripts, with the market up.
fn learned_world() -> Vec<u8> {
    let c = content();
    let mut w = World::default();
    w.season = 1;
    w.market_pct = 112;
    for (i, s) in c
        .scripts
        .script
        .iter()
        .filter(|s| s.price > 0)
        .take(6)
        .enumerate()
    {
        w.learned.push(Learned {
            corp: u8::try_from(i % 3).unwrap(),
            sig: s.sig,
            uses: if i % 2 == 0 { 24 } else { 12 },
        });
    }
    encode_world(&w)
}

fn open(
    door: &Nullroute,
    env: &DoorEnv,
    d: i64,
    seed: u64,
    save: Option<&[u8]>,
    world: Option<&[u8]>,
) -> Driver {
    Driver::open(
        door,
        Start {
            save: save.map_or(Save::Fresh, |bytes| Save::Loaded { bytes, season: 1 }),
            world,
            env,
            today: Day::from_raw(d),
            season: 1,
        },
        seed,
        Arc::new(ManualClock::new(Day::from_raw(d).start())),
    )
}

fn key_of(ch: char) -> DoorKey {
    match ch {
        '\u{1b}' => DoorKey::Escape,
        '\n' => DoorKey::Enter,
        '^' => DoorKey::Up,
        'v' => DoorKey::Down,
        c => DoorKey::Char(c),
    }
}

fn send(d: &mut Driver, keys: &str) {
    for ch in keys.chars() {
        d.key(key_of(ch));
    }
}

fn text(d: &Driver, env: &DoorEnv, size: (u16, u16)) -> Vec<String> {
    replay::draw_to_text(d, env, size.0, size.1)
}

/// Keys that reach the moments of a run worth drawing, found by letting the screen bot play: the
/// run some moves in, a fight, and the debrief.
fn bot_moments(door: &Nullroute) -> BTreeMap<&'static str, (i64, Vec<DoorKey>)> {
    let e = env(Tier::Unicode);
    let mut found: BTreeMap<&'static str, (i64, Vec<DoorKey>)> = BTreeMap::new();
    for seed in 20_000..20_060i64 {
        let mut d = open(door, &e, seed, 3, None, None);
        let mut bot = ScreenBot::default();
        let mut keys: Vec<DoorKey> = Vec::new();
        let mut in_run = 0;
        for sent in 0..500 {
            let screen = d.look_around(&e);
            let on_run = screen.get(1).is_some_and(|l| l.starts_with("TRACE"));
            if on_run {
                in_run += 1;
            }
            if on_run && in_run == 7 {
                found.entry("run").or_insert((seed, keys.clone()));
            }
            if screen.iter().any(|l| l.contains("break off")) {
                found.entry("fight").or_insert((seed, keys.clone()));
            }
            if screen.first().is_some_and(|l| l.starts_with("DEBRIEF")) {
                found.entry("debrief").or_insert((seed, keys.clone()));
            }
            let Some(key) = bot.next_key(&screen, 1, sent) else {
                break;
            };
            keys.push(key);
            if d.key(key) == Flow::Exit {
                break;
            }
        }
        if found.len() == 3 {
            break;
        }
    }
    found
}

/// Every screen as `(name, driver)` ready to draw. The text drawn is for `tier`.
fn screens(door: &Nullroute, env: &DoorEnv) -> Vec<(String, Driver)> {
    let rich = rich_save();
    let world = learned_world();
    let mut out: Vec<(String, Driver)> = Vec::new();
    let mut add = |name: &str, keys: &str, rich_player: bool| {
        let mut d = if rich_player {
            open(door, env, day(), 5, Some(&rich), Some(&world))
        } else {
            open(door, env, day(), 5, None, Some(&world))
        };
        send(&mut d, keys);
        out.push((name.to_owned(), d));
    };
    add("home", "", false);
    add("home-rich", "", true);
    add("contracts", "c", false);
    add("contracts-cursor", "cvv", true);
    add("brief", "c1", false);
    add("fixers", "fvv", true);
    add("heat", "t", true);
    add("profile", "p", true);
    add("deck", "dvv", true);
    add("market", "svv", true);
    add("black-market", "b", true);
    add("rig", "r", true);
    add("hideout", "h", true);
    add("code-effect", "hc", true);
    add("code-target", "hc1", true);
    add("code-modifier", "hc11", true);
    add("code-confirm", "hc110", true);
    let moments = bot_moments(door);
    for (name, (seed, keys)) in &moments {
        let mut d = open(door, env, *seed, 3, None, None);
        for k in keys {
            d.key(*k);
        }
        out.push(((*name).to_owned(), d));
    }
    out
}

/// Every screen at every size in one tier, as the text a snapshot file holds.
fn render_all(door: &Nullroute, tier: Tier) -> BTreeMap<(u16, u16), String> {
    let e = env(tier);
    let mut out = BTreeMap::new();
    for size in SIZES {
        let mut text = String::new();
        for (name, d) in screens(door, &e) {
            text.push_str(&format!("=== {name} {}x{}\n", size.0, size.1));
            let rows = self::text(&d, &e, size);
            assert_eq!(rows.len(), usize::from(size.1), "{name}");
            for row in rows {
                assert!(
                    row.chars().count() <= usize::from(size.0),
                    "{name} {size:?}: {row:?}"
                );
                text.push('|');
                text.push_str(&row);
                text.push('\n');
            }
        }
        out.insert(size, text);
    }
    out
}

fn snapshot_path(tier: &str, size: (u16, u16)) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join(format!("{tier}-{}x{}.txt", size.0, size.1))
}

/// Compare with the snapshot file, or write it when `UPDATE_SNAPSHOTS` is set.
fn check_snapshot(tier: &str, size: (u16, u16), got: &str) {
    let path = snapshot_path(tier, size);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!("no snapshot {path:?}: run with UPDATE_SNAPSHOTS=1 to write it")
    });
    if want != got {
        let line = want
            .lines()
            .zip(got.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        panic!(
            "snapshot {path:?} differs from line {}: want {:?}, got {:?}\n(run with UPDATE_SNAPSHOTS=1 to accept)",
            line + 1,
            want.lines().nth(line),
            got.lines().nth(line)
        );
    }
}

#[test]
fn every_screen_matches_its_snapshot_at_every_size_in_every_tier() {
    let door = Nullroute::new().unwrap();
    let unicode = render_all(&door, Tier::Unicode);
    let nerd = render_all(&door, Tier::NerdFont);
    let ascii = render_all(&door, Tier::Ascii);
    for size in SIZES {
        // The door cannot use a private-use glyph, so the Nerd Font tier draws what Unicode does.
        assert_eq!(nerd[&size], unicode[&size], "{size:?}");
        check_snapshot("unicode", size, &unicode[&size]);
        check_snapshot("ascii", size, &ascii[&size]);
        // The ASCII tier is ASCII, whatever was drawn.
        for (n, row) in ascii[&size].lines().enumerate() {
            assert!(row.is_ascii(), "ascii {size:?} line {}: {row:?}", n + 1);
        }
    }
}

#[test]
fn every_screen_is_full_enough_to_be_worth_the_panels() {
    let door = Nullroute::new().unwrap();
    let e = env(Tier::Unicode);
    for (name, d) in screens(&door, &e) {
        for size in SIZES {
            let rows = text(&d, &e, size);
            let used = rows.iter().filter(|r| !r.is_empty()).count();
            assert!(used >= 5, "{name} {size:?} draws {used} rows: {rows:#?}");
            // Every screen shows its keys, in brackets, on one of the last rows.
            let tail = rows.iter().rev().take(3).any(|r| r.contains('['));
            assert!(tail, "{name} {size:?} shows no key hints: {rows:#?}");
        }
    }
}

#[test]
fn a_window_too_small_for_the_panels_gets_the_text() {
    let door = Nullroute::new().unwrap();
    let e = env(Tier::Unicode);
    let d = open(&door, &e, day(), 5, None, None);
    let rows = text(&d, &e, (50, 12));
    assert!(rows[0].starts_with(" NULLROUTE"), "{rows:#?}");
    assert!(rows.iter().any(|r| r.contains("Cycles")), "{rows:#?}");
}

#[test]
#[ignore = "prints the screens for a person to look at: cargo test --test ui dump -- --ignored --nocapture"]
fn dump() {
    let door = Nullroute::new().unwrap();
    let tier = match std::env::var("TIER").as_deref() {
        Ok("ascii") => Tier::Ascii,
        _ => Tier::Unicode,
    };
    let only = std::env::var("ONLY").ok();
    let size = std::env::var("SIZE")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let e = env(tier);
    for (name, d) in screens(&door, &e) {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        println!("=== {name} {:?}", SIZES[size]);
        for row in text(&d, &e, SIZES[size]) {
            println!("|{row}");
        }
    }
}

#[test]
fn every_screen_has_a_look_around() {
    let door = Nullroute::new().unwrap();
    let e = env(Tier::Ascii);
    let mut heads = Vec::new();
    for (name, d) in screens(&door, &e) {
        let lines = d.look_around(&e);
        assert!(lines.len() >= 3, "{name} has no look-around: {lines:?}");
        assert!(
            !lines[0].trim().is_empty(),
            "{name}: the first line names the screen"
        );
        assert!(
            lines.iter().any(|l| l.contains('[')),
            "{name} says what the keys are: {lines:?}"
        );
        for line in &lines {
            assert!(
                !line.chars().any(|c| c.is_control() || c == '\u{1b}'),
                "{name}: {line:?}"
            );
        }
        heads.push((name, lines));
    }
    // A map screen describes the map in words: where you are, the ways out and their state.
    let run = &heads.iter().find(|(n, _)| n == "run").unwrap().1.join("\n");
    assert!(run.contains("At "), "{run}");
    assert!(run.contains("TRACE"), "{run}");
    assert!(run.contains("1) "), "{run}");
    let fight = &heads
        .iter()
        .find(|(n, _)| n == "fight")
        .unwrap()
        .1
        .join("\n");
    assert!(fight.contains("It will"), "{fight}");
    assert!(fight.contains("dmg"), "{fight}");
    // The new screens each have their own header.
    for (name, head) in [
        ("fixers", "STANDINGS"),
        ("heat", "HEAT"),
        ("profile", "CHARACTER"),
        ("black-market", "BLACK MARKET"),
    ] {
        let lines = &heads.iter().find(|(n, _)| n == name).unwrap().1;
        assert!(lines[0].starts_with(head), "{name}: {lines:?}");
    }
}

#[test]
fn colours_are_the_palettes_and_the_accents_and_degrade_to_nothing() {
    use russh_bbs_door::style::Color;
    use russh_bbs_door::{ColorDepth, Composite, DoorPalette, DoorSurface};
    let door = Nullroute::new().unwrap();
    let e = env(Tier::Unicode);
    let (_, run) = screens(&door, &e)
        .into_iter()
        .find(|(n, _)| n == "run")
        .unwrap();
    let paint = |depth: ColorDepth| {
        let mut surface = DoorSurface::new(92, 25, DoorPalette::mono());
        surface.draw(|canvas| run.render(canvas, &e));
        surface.composite(Composite::new(depth, false))
    };
    let colours = |depth| {
        let clean = paint(depth);
        clean
            .buffer()
            .content()
            .iter()
            .flat_map(|c| [c.fg, c.bg])
            .collect::<Vec<_>>()
    };
    // Truecolor keeps the accents (signal green, warning red, cold cyan).
    let true_colours = colours(ColorDepth::TrueColor);
    for want in [Color::Rgb(0x39, 0xff, 0x8f), Color::Rgb(0x4d, 0xd2, 0xff)] {
        assert!(true_colours.contains(&want), "{want:?} is drawn");
    }
    // Sixteen colours: no Rgb or Indexed is left.
    assert!(
        colours(ColorDepth::Ansi16)
            .iter()
            .all(|c| !matches!(c, Color::Rgb(..) | Color::Indexed(_)))
    );
    // No colour: the picture is still there (glyphs and weights), the colour is not.
    assert!(colours(ColorDepth::Mono).iter().all(|c| *c == Color::Reset));
    let mono = paint(ColorDepth::Mono).rows_text();
    assert!(mono.iter().any(|r| r.contains("TRACE")), "{mono:#?}");
    assert!(mono.iter().any(|r| r.contains("NODE MAP")), "{mono:#?}");
}

#[test]
fn a_hostile_handle_reaches_no_cell() {
    use russh_bbs_door::{ColorDepth, Composite, DoorPalette, DoorSurface};
    let door = Nullroute::new().unwrap();
    let mut e = env(Tier::Unicode);
    e.handle = "an\u{1b}[31mn\u{202e}\u{e000}\u{301a}x".to_owned();
    let mut d = open(&door, &e, day(), 5, None, None);
    for keys in ["", "p", "\u{1b}c"] {
        send(&mut d, keys);
        let mut surface = DoorSurface::new(92, 25, DoorPalette::mono());
        surface.draw(|canvas| d.render(canvas, &e));
        let clean = surface.composite(Composite::new(ColorDepth::TrueColor, false));
        for cell in clean.buffer().content() {
            assert!(
                cell.symbol()
                    .chars()
                    .all(|c| !c.is_control() && c != '\u{202e}' && c != '\u{e000}'),
                "{:?}",
                cell.symbol()
            );
        }
    }
}

/// Play one whole run through the keys, drawing every screen on the way: pick a job, plan the
/// deck, jack in, meet a countermeasure, and extract (or burn).
#[test]
fn a_whole_run_from_the_board_to_the_debrief() {
    let door = Nullroute::new().unwrap();
    let e = env(Tier::Unicode);
    // A day whose job the starter deck can finish and whose network has a machine in the way.
    let moments = bot_moments(&door);
    let (fight_day, _) = moments.get("fight").expect("a fight was found").clone();
    let mut d = open(&door, &e, fight_day, 3, None, None);
    let size = (92, 25);
    let screen = |d: &Driver| text(d, &e, size).join("\n");

    // The home screen.
    assert!(screen(&d).contains("MENU"), "{}", screen(&d));
    // Plan the deck: the last loaded script out, and back in.
    send(&mut d, "d");
    let deck = screen(&d);
    assert!(
        deck.contains("SCRIPTS") && deck.contains("memory"),
        "{deck}"
    );
    let lines = d.look_around(&e);
    let last_loaded = lines
        .iter()
        .filter(|l| l.starts_with('[') && l.chars().nth(3) == Some('*'))
        .next_back()
        .and_then(|l| l.chars().nth(1))
        .expect("a loaded script");
    send(&mut d, &last_loaded.to_string());
    assert!(screen(&d).contains("DECK") || screen(&d).contains("SCRIPTS"));
    let unloaded = d.look_around(&e).join("\n");
    send(&mut d, &last_loaded.to_string());
    assert_ne!(unloaded, d.look_around(&e).join("\n"), "it loads again");
    send(&mut d, "\u{1b}");
    // Pick a job: walk the cursor to the Steal job the starter deck can do.
    send(&mut d, "c");
    assert!(screen(&d).contains("JOBS"), "{}", screen(&d));
    let lines = d.look_around(&e);
    let pick = lines
        .iter()
        .find(|l| l.contains(") Steal") && !l.contains('('))
        .and_then(|l| l.split(')').next())
        .and_then(|n| n.trim().parse::<usize>().ok())
        .expect("a job the deck can do");
    send(&mut d, &"v".repeat(pick - 1));
    send(&mut d, "\n");
    let brief = screen(&d);
    assert!(
        brief.contains("THE JOB") && brief.contains("YOUR DECK AGAINST THEM"),
        "{brief}"
    );
    // Jack in and let the screen bot work the node map, drawing every frame.
    send(&mut d, "j");
    let mut bot = ScreenBot::default();
    let (mut saw_map, mut saw_fight, mut saw_debrief) = (false, false, false);
    for sent in 0..400 {
        let drawn = text(&d, &e, size);
        let joined = drawn.join("\n");
        assert!(drawn.iter().all(|r| r.chars().count() <= 92), "{joined}");
        saw_map |= joined.contains("NODE MAP") && joined.contains("TRACE");
        saw_fight |= joined.contains("COUNTERMEASURE") && joined.contains("IT WILL");
        if joined.contains("DEBRIEF") {
            saw_debrief = true;
            assert!(
                joined.contains("E X T R A C T E D")
                    || joined.contains("B U R N E D")
                    || joined.contains("J A C K E D"),
                "{joined}"
            );
            break;
        }
        let look = d.look_around(&e);
        // The bot reads the same screen a person with a screen reader does.
        let Some(key) = bot.next_key(&look, 1, sent + 1_000) else {
            break;
        };
        d.key(key);
    }
    assert!(saw_map, "the node map was drawn");
    assert!(saw_fight, "a countermeasure was met");
    assert!(saw_debrief, "the run ended in a debrief");
    // Back home with the run counted and saved.
    send(&mut d, "\n");
    assert!(screen(&d).contains("MENU"));
    let saved = d.take_effects().save().map(<[u8]>::to_vec).expect("a save");
    let player = russh_bbs_door_nullroute::save::decode(&saved).unwrap();
    assert_eq!(player.runs, 1);
}

/// A reckless player burns: scripts that make noise until the trace reaches 100.
#[test]
fn a_reckless_run_ends_burned_and_the_debrief_says_so() {
    let door = Nullroute::new().unwrap();
    let e = env(Tier::Unicode);
    let mut burned = false;
    for seed in 20_000..20_040i64 {
        let mut d = open(&door, &e, seed, 3, None, None);
        send(&mut d, "c");
        let lines = d.look_around(&e);
        let Some(pick) = lines
            .iter()
            .find(|l| l.contains(") Steal") && !l.contains('('))
            .and_then(|l| l.split(')').next())
            .and_then(|n| n.trim().parse::<usize>().ok())
        else {
            continue;
        };
        send(&mut d, &pick.to_string());
        send(&mut d, "j");
        // Sweep, sweep, sweep, and take every road.
        for i in 0..120 {
            let key = match i % 4 {
                0 => 'a',
                1 => '1',
                2 => 'e',
                _ => '2',
            };
            d.key(DoorKey::Char(key));
            if d.look_around(&e)
                .first()
                .is_some_and(|l| l.starts_with("DEBRIEF"))
            {
                break;
            }
        }
        if text(&d, &e, (72, 19)).join("\n").contains("B U R N E D") {
            burned = true;
            break;
        }
    }
    assert!(burned, "a noisy run burned and the debrief said BURNED");
}
