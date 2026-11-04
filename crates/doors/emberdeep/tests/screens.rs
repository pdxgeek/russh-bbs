//! Every Emberdeep screen, drawn through the framework's own compositing at the sizes the owner
//! named (80x24, 100x30 and 132x43), in every icon tier and colour depth, from real saves opened
//! with the real door and reached with real keys (`showcase` builds the saves). The tests check
//! the shape of each screen, pin the text of every screen (set `UPDATE_SNAPSHOTS=1` to rewrite
//! `tests/snapshots/*.txt`), check that every screen has a look-around, and walk one whole day.
//! `EMBERDEEP_SHOTS=dir cargo test --test screens dump_screens -- --ignored` writes ANSI captures
//! for pictures.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::similar_names
)]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_door::host::Driver;
use russh_bbs_door::style::{Color, Modifier, Style};
use russh_bbs_door::{
    ColorDepth, Composite, DoorEnv, DoorKey, DoorPalette, DoorSurface, Drew, Save, Start, Tier,
};
use russh_bbs_door_emberdeep::Emberdeep;
use russh_bbs_door_emberdeep::showcase::{DAY, SEASON, Showcase, showcases};

/// The terminal sizes of the owner's bar.
const TERMS: [(u16, u16); 3] = [(80, 24), (100, 30), (132, 43)];
const TIERS: [Tier; 3] = [Tier::Ascii, Tier::Unicode, Tier::NerdFont];
const DEPTHS: [ColorDepth; 4] = [
    ColorDepth::TrueColor,
    ColorDepth::Ansi256,
    ColorDepth::Ansi16,
    ColorDepth::Mono,
];

/// The door's area as the board gives it: the box is at most 76 columns wide (74 inside), the
/// status row and the box edges and the two heading rows come off the height, and a window taller
/// than 24 rows keeps a margin above and below the box (`server::settings::inner_size`).
const fn board_area(cols: u16, rows: u16) -> (u16, u16) {
    let w = if cols > 76 { 74 } else { cols - 2 };
    let content = rows - 1;
    let boxed = if content > 24 { content - 2 } else { content };
    (w, boxed - 2 - 2)
}

fn palette() -> DoorPalette {
    let c = |r, g, b| Style::new().fg(Color::Rgb(r, g, b));
    DoorPalette::new([
        c(214, 210, 200),
        c(128, 128, 140).add_modifier(Modifier::DIM),
        c(240, 236, 226).add_modifier(Modifier::BOLD),
        c(232, 176, 80).add_modifier(Modifier::BOLD),
        c(120, 190, 110),
        c(214, 84, 70).add_modifier(Modifier::BOLD),
        Style::new().add_modifier(Modifier::REVERSED),
    ])
}

fn env_of(tier: Tier) -> DoorEnv {
    let mut env = DoorEnv::for_tests("Ember");
    env.tier = tier;
    env
}

fn keys(text: &str) -> Vec<DoorKey> {
    text.split_whitespace()
        .map(|t| DoorKey::from_token(t).unwrap())
        .collect()
}

fn clock() -> Arc<dyn Clock> {
    Arc::new(ManualClock::new(Day::from_raw(DAY).start()))
}

/// The door opened on a showcase and the keys pressed.
fn open(sc: &Showcase, env: &DoorEnv) -> Driver {
    let door = Emberdeep::new().unwrap();
    let save = if sc.save.is_empty() {
        Save::Fresh
    } else {
        Save::Loaded {
            bytes: &sc.save,
            season: SEASON,
        }
    };
    let mut driver = Driver::open(
        &door,
        Start {
            save,
            world: Some(&sc.world),
            env,
            today: Day::from_raw(DAY),
            season: SEASON,
        },
        7,
        clock(),
    );
    for k in keys(sc.keys) {
        driver.key(k);
    }
    driver
}

fn composite(
    driver: &Driver,
    env: &DoorEnv,
    w: u16,
    h: u16,
    depth: ColorDepth,
) -> (russh_bbs_door::CleanBuffer, Drew) {
    let mut surface = DoorSurface::new(w, h, palette());
    let drew = surface.draw(|canvas| driver.render(canvas, env));
    (
        surface.composite(Composite::new(depth, env.tier == Tier::Ascii)),
        drew,
    )
}

fn text(driver: &Driver, env: &DoorEnv, w: u16, h: u16) -> Vec<String> {
    composite(driver, env, w, h, ColorDepth::Mono).0.rows_text()
}

/// An ANSI capture (truecolor) of a composited screen, for pictures.
fn ansi(clean: &russh_bbs_door::CleanBuffer) -> String {
    let buf = clean.buffer();
    let mut out = String::new();
    for y in 0..buf.area.height {
        let mut last = Style::new();
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            let style = cell.style();
            if style != last {
                out.push_str("\x1b[0m");
                let mut codes: Vec<String> = Vec::new();
                if let Some(Color::Rgb(r, g, b)) = style.fg {
                    codes.push(format!("38;2;{r};{g};{b}"));
                }
                if let Some(Color::Rgb(r, g, b)) = style.bg {
                    codes.push(format!("48;2;{r};{g};{b}"));
                }
                if style.add_modifier.contains(Modifier::BOLD) {
                    codes.push("1".to_owned());
                }
                if style.add_modifier.contains(Modifier::DIM) {
                    codes.push("2".to_owned());
                }
                if style.add_modifier.contains(Modifier::REVERSED) {
                    codes.push("7".to_owned());
                }
                if !codes.is_empty() {
                    let _ = write!(out, "\x1b[{}m", codes.join(";"));
                }
                last = style;
            }
            out.push_str(cell.symbol());
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

#[test]
#[ignore = "writes captures when EMBERDEEP_SHOTS is set"]
fn dump_screens() {
    let Some(dir) = std::env::var_os("EMBERDEEP_SHOTS") else {
        return;
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    for sc in showcases() {
        for (cols, rows) in TERMS {
            let (w, h) = board_area(cols, rows);
            for (tier, tag) in [(Tier::Unicode, "uni"), (Tier::Ascii, "ascii")] {
                let env = env_of(tier);
                let driver = open(&sc, &env);
                let (clean, _) = composite(&driver, &env, w, h, ColorDepth::TrueColor);
                std::fs::write(
                    dir.join(format!("{}-{cols}x{rows}-{tag}.ansi", sc.name)),
                    ansi(&clean),
                )
                .unwrap();
            }
            // As a window that did not cap the box at 76 columns would show it.
            let env = env_of(Tier::Unicode);
            let driver = open(&sc, &env);
            let (clean, _) = composite(&driver, &env, cols - 2, rows - 4, ColorDepth::TrueColor);
            std::fs::write(
                dir.join(format!("{}-{cols}x{rows}-wide.ansi", sc.name)),
                ansi(&clean),
            )
            .unwrap();
        }
    }
}

// ---- shape ----

#[test]
fn every_screen_draws_cleanly_at_every_size_tier_and_depth() {
    let all = showcases();
    assert!(all.len() >= 28, "the showcase covers every screen");
    for sc in &all {
        for tier in TIERS {
            let env = env_of(tier);
            let driver = open(sc, &env);
            for (cols, rows) in TERMS {
                let sizes = [
                    board_area(cols, rows),
                    // And the whole window, as the board would give a roomier box.
                    (cols - 2, rows - 4),
                ];
                for (w, h) in sizes {
                    for depth in DEPTHS {
                        let (clean, drew) = composite(&driver, &env, w, h, depth);
                        let rows_text = clean.rows_text();
                        let ctx = format!(
                            "{} {w}x{h} {tier:?} {depth:?}\n{}",
                            sc.name,
                            rows_text.join("\n")
                        );
                        assert_eq!(drew, Drew::Done, "{ctx}");
                        assert_eq!(usize::from(clean.height()), usize::from(h), "{ctx}");
                        assert_eq!(rows_text.len(), usize::from(h), "{ctx}");
                        assert!(
                            rows_text
                                .iter()
                                .all(|l| l.chars().count() <= usize::from(w)),
                            "{ctx}"
                        );
                        // Something is drawn, with its keys.
                        let joined = rows_text.join("\n");
                        assert!(joined.contains('['), "key chips: {ctx}");
                        assert!(joined.contains("EMBERDEEP"), "the header: {ctx}");
                        // No cell carries the reserved marker style.
                        assert!(
                            clean.buffer().content.iter().all(|c| {
                                !c.modifier.contains(Modifier::BOLD | Modifier::REVERSED)
                            }),
                            "{ctx}"
                        );
                        if tier == Tier::Ascii {
                            assert!(joined.is_ascii(), "the ASCII tier is ASCII only\n{ctx}");
                        }
                        if depth == ColorDepth::Mono {
                            let colored = clean
                                .buffer()
                                .content
                                .iter()
                                .any(|c| c.fg != Color::Reset || c.bg != Color::Reset);
                            assert!(!colored, "mono has no colour\n{ctx}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_roomier_window_shows_more_than_the_board_box() {
    // The same screens at the box's 74 by 19 and at a window with room for the wide layouts.
    for name in ["town", "delve-lift-runesmith", "fight-four", "map"] {
        let sc = showcases().into_iter().find(|s| s.name == name).unwrap();
        let env = env_of(Tier::Unicode);
        let driver = open(&sc, &env);
        let small = text(&driver, &env, 74, 19).join("\n");
        let large = text(&driver, &env, 130, 39).join("\n");
        assert!(
            large.lines().filter(|l| !l.trim().is_empty()).count()
                > small.lines().filter(|l| !l.trim().is_empty()).count(),
            "{name}: more on screen when there is room"
        );
    }
}

#[test]
fn a_tiny_window_falls_back_to_the_text_screen() {
    let sc = showcases().into_iter().find(|s| s.name == "town").unwrap();
    let env = env_of(Tier::Ascii);
    let driver = open(&sc, &env);
    let rows = text(&driver, &env, 50, 12);
    assert!(rows.iter().any(|l| l.contains("[d] Descend")));
}

// ---- the look-around ----

#[test]
fn every_screen_has_a_look_around_a_screen_reader_can_use() {
    for sc in showcases() {
        let env = env_of(Tier::Ascii);
        let driver = open(&sc, &env);
        let lines = driver.look_around(&env);
        assert!(lines.len() >= 4, "{}: {lines:?}", sc.name);
        for l in &lines {
            assert!(
                l.chars().all(|c| c == ' ' || c.is_ascii_graphic()),
                "{}: {l:?}",
                sc.name
            );
            assert!(l.chars().count() <= 74, "{}: too wide: {l:?}", sc.name);
        }
        let joined = lines.join("\n");
        assert!(joined.contains('['), "{}: names its keys", sc.name);
        // Every map and board screen says what is there in words.
        if sc.name.starts_with("delve") || sc.name == "map" {
            assert!(joined.contains("rooms explored"), "{}: {joined}", sc.name);
            assert!(joined.contains("way down"), "{}: {joined}", sc.name);
            assert!(joined.contains("stand in"), "{}: {joined}", sc.name);
        }
        if sc.name.starts_with("fight") {
            assert!(
                joined.contains("next:"),
                "{}: the enemies' intents",
                sc.name
            );
        }
        if sc.name.starts_with("hall") {
            assert!(joined.contains("Deepest heroes"), "{}", sc.name);
        }
    }
}

#[test]
fn two_builds_see_different_options_and_the_screen_says_why() {
    let all = showcases();
    let get = |n: &str| all.iter().find(|s| s.name == n).unwrap();
    let env = env_of(Tier::Unicode);
    let smith = text(&open(get("delve-lift-runesmith"), &env), &env, 74, 19).join("\n");
    let warden = text(&open(get("delve-lift-warden"), &env), &env, 74, 19).join("\n");
    assert!(smith.contains("[1] ◆ Re-seat the winch"), "{smith}");
    assert!(!warden.contains("[1] ◆ Re-seat"), "{warden}");
    assert!(warden.contains("[1] ◆ Haul the chain"), "{warden}");
    // What the warden cannot do is shown, with the reason.
    assert!(
        warden.contains('✗') && warden.contains("(needs"),
        "{warden}"
    );
    let look = open(get("delve-lift-warden"), &env)
        .look_around(&env)
        .join("\n");
    assert!(
        look.contains("Closed to you: Re-seat the winch with rune-work"),
        "{look}"
    );
}

// ---- the snapshots ----

fn snapshot_text(tier: Tier, terms: &[(u16, u16)]) -> String {
    let mut out = String::new();
    for sc in showcases() {
        let env = env_of(tier);
        let driver = open(&sc, &env);
        for &(cols, rows) in terms {
            let (w, h) = board_area(cols, rows);
            let _ = writeln!(out, "=== {} {cols}x{rows} (door {w}x{h})", sc.name);
            for line in text(&driver, &env, w, h) {
                let _ = writeln!(out, "|{line}");
            }
        }
    }
    out
}

fn check_snapshot(file: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(file);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{file} is missing; run with UPDATE_SNAPSHOTS=1"));
    if expected != actual {
        let first = expected
            .lines()
            .zip(actual.lines())
            .enumerate()
            .find(|(_, (a, b))| a != b)
            .map_or_else(
                || "the length differs".to_owned(),
                |(i, (a, b))| format!("line {}:\n  expected {a:?}\n  actual   {b:?}", i + 1),
            );
        panic!("{file} differs ({first}); run with UPDATE_SNAPSHOTS=1 to accept");
    }
}

#[test]
fn the_screens_match_their_pinned_text_in_the_unicode_tier() {
    check_snapshot("unicode.txt", &snapshot_text(Tier::Unicode, &TERMS));
}

#[test]
fn the_screens_match_their_pinned_text_in_the_ascii_tier() {
    check_snapshot("ascii.txt", &snapshot_text(Tier::Ascii, &TERMS[..1]));
}

// ---- one whole day ----

/// What a screen reader would see, as one string.
fn read(driver: &Driver, env: &DoorEnv) -> String {
    driver.look_around(env).join("\n")
}

fn press(driver: &mut Driver, env: &DoorEnv, tokens: &str, seen: &mut Vec<String>) {
    for k in keys(tokens) {
        driver.key(k);
        // Every screen on the way draws, in both tiers, at the board's smallest box.
        for tier in [Tier::Unicode, Tier::Ascii] {
            let e = DoorEnv {
                tier,
                ..env.clone()
            };
            let (w, h) = board_area(80, 24);
            let (clean, drew) = composite(driver, &e, w, h, ColorDepth::Ansi16);
            assert_eq!(drew, Drew::Done);
            if tier == Tier::Ascii {
                assert!(clean.rows_text().join("").is_ascii());
            }
            seen.push(clean.rows_text().join("\n"));
        }
    }
}

#[test]
fn a_whole_day_town_descent_event_fight_and_back_up_draws_every_step() {
    let env = env_of(Tier::Unicode);
    let door = Emberdeep::new().unwrap();
    let mut driver = Driver::open(
        &door,
        Start {
            save: Save::Fresh,
            world: None,
            env: &env,
            today: Day::from_raw(DAY),
            season: SEASON,
        },
        21,
        clock(),
    );
    let mut seen: Vec<String> = Vec::new();
    // A new hero: a warden with two traits and four points.
    press(
        &mut driver,
        &env,
        "down 1 a b enter 1 1 2 3 enter",
        &mut seen,
    );
    assert!(
        read(&driver, &env).contains("Descend"),
        "{}",
        read(&driver, &env)
    );
    // Morning in town: the places, each opened and left.
    for open_key in ["m", "i", "f", "t", "g", "b", "l", "w", "s", "h", "r"] {
        press(&mut driver, &env, &format!("{open_key} esc"), &mut seen);
    }
    // Down the stair.
    press(&mut driver, &env, "d", &mut seen);
    let mut fought = false;
    let mut event = false;
    let mut steps = 0;
    while steps < 140 {
        steps += 1;
        let screen = read(&driver, &env);
        if screen.contains("FIGHT") {
            fought = true;
            // Fight until it ends (a quick-resolve is not offered after round one).
            press(&mut driver, &env, "x", &mut seen);
            continue;
        }
        if fought && event {
            break;
        }
        // Take the first option of a room that is not a fight, once, as the event.
        let opts: Vec<&str> = screen.lines().filter(|l| l.starts_with("[1] ")).collect();
        let hostile = screen.contains("A pack bars the way");
        if !opts.is_empty() && (hostile || !event) {
            if !hostile {
                event = true;
            }
            press(&mut driver, &env, "1", &mut seen);
            continue;
        }
        // Otherwise walk toward something unexplored, else the stairs.
        let go = ["n", "e", "s", "w"]
            .iter()
            .find(|d| {
                screen
                    .lines()
                    .any(|l| l.starts_with(&format!("[{d}] ")) && l.contains("unexplored"))
            })
            .copied();
        match go {
            Some(d) => press(&mut driver, &env, d, &mut seen),
            None => {
                if screen.contains("[>] Take the stairs") {
                    press(&mut driver, &env, ">", &mut seen);
                } else {
                    // Nothing new here: any exit.
                    let any = ["n", "e", "s", "w"]
                        .iter()
                        .find(|d| screen.lines().any(|l| l.starts_with(&format!("[{d}] "))))
                        .copied()
                        .unwrap_or("u");
                    press(&mut driver, &env, any, &mut seen);
                }
            }
        }
    }
    assert!(event, "met an event in a room");
    assert!(fought, "fought something");
    // Back up to town.
    for _ in 0..4 {
        if read(&driver, &env).contains("Descend into the Emberdeep") {
            break;
        }
        press(&mut driver, &env, "u", &mut seen);
    }
    let home = read(&driver, &env);
    assert!(home.contains("Descend"), "back in Cindermere:\n{home}");
    let all = seen.join("\n");
    for word in [
        "Cindermere",
        "The Market",
        "The Forge",
        "Skills",
        "Hero sheet",
        "The Hall",
        "Fight ·",
        "Floor ",
    ] {
        assert!(all.contains(word), "saw {word}");
    }
}
