//! The Hallpass screens (M4-H3): every screen rendered at 80x24, 100x30 and 132x43 in each icon
//! tier through the same compositing the board does; the look-around of every screen; and one
//! whole school day played through the keys (lessons, a scheme planned and run, the file, a club
//! meeting).

#![allow(
    clippy::cast_precision_loss,
    clippy::needless_pass_by_value,
    clippy::doc_markdown,
    clippy::similar_names,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::type_complexity,
    clippy::collapsible_if,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::format_push_string,
    clippy::panic
)]

mod common;

use std::sync::Arc;

use common::{Sandbox, Setup, arrange, content, meet, monday, satisfy};
use russh_bbs_core::limits::{Day, ManualClock};
use russh_bbs_door::host::Driver;
use russh_bbs_door::style::{Color, Modifier, Style};
use russh_bbs_door::{
    ColorDepth, Composite, DoorEnv, DoorKey, DoorPalette, DoorSurface, Save, Start, Tier,
};
use russh_bbs_door_hallpass::Hallpass;
use russh_bbs_door_hallpass::commons::{Candidate, Pupil, Rumour};
use russh_bbs_door_hallpass::cond::Cond;
use russh_bbs_door_hallpass::game::Action;
use russh_bbs_door_hallpass::save::encode_player;
use russh_bbs_door_hallpass::state::{
    Active, FileEntry, HallEntry, Headline, Membership, Status, World, YearEntry,
};
use russh_bbs_door_hallpass::wire::encode;

fn clock(day: i64) -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Day::from_raw(day).start()))
}

/// A visit to the door with its own environment.
struct Visit {
    driver: Driver,
    env: DoorEnv,
    last_save: Option<Vec<u8>>,
}

impl Visit {
    fn open(door: &Hallpass, env: DoorEnv, state: &State) -> Self {
        let driver = Driver::open(
            door,
            Start {
                save: match &state.player {
                    Some(bytes) => Save::Loaded { bytes, season: 1 },
                    None => Save::Fresh,
                },
                world: Some(&state.world),
                env: &env,
                today: Day::from_raw(state.day),
                season: 1,
            },
            9,
            clock(state.day),
        );
        let mut v = Self {
            driver,
            env,
            last_save: None,
        };
        v.keys(state.keys);
        v
    }

    fn key(&mut self, key: DoorKey) {
        self.driver.key(key);
        if let Some(bytes) = self.driver.take_effects().save() {
            self.last_save = Some(bytes.to_vec());
        }
    }

    /// Press characters; `^ v < >` are the arrows and `!` is Enter.
    fn keys(&mut self, keys: &str) {
        for c in keys.chars() {
            self.key(match c {
                '^' => DoorKey::Up,
                'v' => DoorKey::Down,
                '<' => DoorKey::Left,
                '>' => DoorKey::Right,
                '!' => DoorKey::Enter,
                '~' => DoorKey::Escape,
                c => DoorKey::Char(c),
            });
        }
    }

    fn look(&self) -> Vec<String> {
        self.driver.look_around(&self.env)
    }

    fn render(&self, cols: u16, rows: u16, depth: ColorDepth) -> Vec<String> {
        let mut surface = DoorSurface::new(cols, rows, DoorPalette::mono());
        surface.draw(|canvas| self.driver.render(canvas, &self.env));
        surface
            .composite(Composite::new(depth, self.env.tier == Tier::Ascii))
            .rows_text()
    }

    /// The screen as `symbol <tab> fg <tab> bg <tab> flags` per cell, one row per line, for the
    /// picture renderer in `docs/qa/m4-h3-screens/render.py`.
    fn dump(&self, cols: u16, rows: u16, palette: DoorPalette, depth: ColorDepth) -> String {
        let mut surface = DoorSurface::new(cols, rows, palette);
        surface.draw(|canvas| self.driver.render(canvas, &self.env));
        let depth = if self.env.settings.color() {
            depth
        } else {
            ColorDepth::Mono
        };
        let clean = surface.composite(Composite::new(depth, self.env.tier == Tier::Ascii));
        let hex = |c: Color| match c {
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            Color::Reset => String::new(),
            other => format!("{other:?}"),
        };
        let mut out = String::new();
        for y in 0..rows {
            for x in 0..cols {
                let cell = &clean.buffer()[(x, y)];
                let m = cell.modifier;
                let mut flags = String::new();
                for (flag, ch) in [
                    (Modifier::BOLD, 'b'),
                    (Modifier::DIM, 'd'),
                    (Modifier::REVERSED, 'r'),
                    (Modifier::UNDERLINED, 'u'),
                ] {
                    if m.contains(flag) {
                        flags.push(ch);
                    }
                }
                out.push_str(&format!(
                    "{}\t{}\t{}\t{flags}\n",
                    cell.symbol(),
                    hex(cell.fg),
                    hex(cell.bg)
                ));
            }
            out.push_str("|\n");
        }
        out
    }
}

fn env_for(tier: Tier, color: bool) -> DoorEnv {
    let mut env = DoorEnv::for_tests("ann");
    env.tier = tier;
    env.settings = russh_bbs_core::terminal_settings::TerminalSettings::new(
        None,
        None,
        color,
        true,
        Some(tier),
    )
    .unwrap();
    env
}

/// What a screen starts from: a character (none for the enrolment card), the world, the day, and
/// the keys that reach the screen.
struct State {
    name: &'static str,
    player: Option<Vec<u8>>,
    world: Vec<u8>,
    day: i64,
    keys: &'static str,
}

/// A school with some history: a hall of legend, a yearbook, headlines and a rule in force.
fn school(day: i64) -> World {
    let mut w = World::from_bytes(None, day);
    w.term_start = day - 12;
    w.hall = vec![
        HallEntry {
            name: "Wick".into(),
            legend: 74,
        },
        HallEntry {
            name: "Marguerite".into(),
            legend: 61,
        },
        HallEntry {
            name: "Pip".into(),
            legend: 38,
        },
        HallEntry {
            name: "Rafe".into(),
            legend: 22,
        },
    ];
    w.yearbook = vec![
        YearEntry {
            term: 1,
            name: "Wick".into(),
            legend: 88,
            changes: 3,
        },
        YearEntry {
            term: 2,
            name: "Sol".into(),
            legend: 71,
            changes: 1,
        },
    ];
    w.paper = vec![
        Headline {
            day: day - 4,
            text: "Deputy head moved to the Old Wing, say sources close to the Old Wing.".into(),
        },
        Headline {
            day: day - 1,
            text: "Fire drill muddle: nobody knows whose idea it was.".into(),
        },
    ];
    w.active = vec![Active {
        id: content().effects.first().map(|e| e.id.clone()).unwrap(),
        since: day - 2,
        until: Some(day + 3),
        by: "Wick".into(),
    }];
    // The social side: two other pupils (one has opted out), a rumour and a candidate.
    w.commons.roster = vec![
        Pupil {
            name: "Wick".into(),
            last_day: day,
            legend: 38,
            grades: 74,
            ..Pupil::default()
        },
        Pupil {
            name: "Sol".into(),
            last_day: day,
            opt_out: true,
            legend: 22,
            ..Pupil::default()
        },
    ];
    w.commons.rumours = vec![Rumour {
        id: "fair_play".into(),
        about: "tilly".into(),
        by: "Wick".into(),
        day,
    }];
    w.commons.election.candidates = vec![Candidate {
        who: "Wick".into(),
        backing: 7,
    }];
    w
}

/// A pupil mid-term with a file, grades, friends, a club and two schemes, on a Wednesday.
fn pupil() -> (Sandbox, i64) {
    let mut sb = Sandbox::new("smooth_talker", 7);
    sb.game.world = school(monday());
    sb.game.world.term_start = monday() - 10;
    let day = monday() + 2;
    sb.game.sync_day(day);
    let p = &mut sb.game.player;
    p.slot = 3;
    p.place = "quad".into();
    p.heat = 9;
    p.legend = 23;
    p.cash = 18;
    p.notoriety = 4;
    p.pulled = 3;
    p.caught = 2;
    p.pinned = 1;
    for (i, e) in ["caught", "caught", "poor_report", "out_of_bounds"]
        .iter()
        .enumerate()
    {
        p.file.push(FileEntry {
            day: day - 8 + i as i64 * 2,
            kind: (*e).to_owned(),
            sev: 1 + (i as u8 % 2),
        });
    }
    p.detention = 1;
    for (i, s) in sb.game.content.subjects.clone().iter().enumerate() {
        russh_bbs_door_hallpass::state::map_set(
            &mut sb.game.player.grades,
            &s.id,
            [82, 61, 47, 90, 38, 70][i % 6],
        );
    }
    sb.game.player.add_standing("rowing_eight", 3);
    sb.game.player.add_standing("ink_society", 6);
    sb.game.player.add_standing("the_bench", -4);
    sb.game.player.life.add_bond("pell", 20);
    sb.game.player.life.add_bond("marsh", -18);
    sb.game.player.life.add_bond("tilly", 45);
    sb.game.player.life.add_bond("rafe", 5);
    sb.game.player.life.clubs.push(Membership {
        club: "chess".into(),
        rank: 2,
        merit: 6,
        last_meet: day - 1,
    });
    sb.game.player.knowledge.push("diary_page".into());
    arrange(&mut sb, "pell", "salt_swap", &Setup::default());
    (sb, day)
}

fn state_of(name: &'static str, sb: &Sandbox, day: i64, keys: &'static str) -> State {
    State {
        name,
        player: Some(encode_player(&sb.game.player)),
        world: encode(&sb.game.world),
        day,
        keys,
    }
}

/// Every screen as a state, in a fixed order.
fn all_states() -> Vec<State> {
    let (base, day) = pupil();
    let mut out: Vec<State> = Vec::new();
    // The enrolment card.
    out.push(State {
        name: "enrol",
        player: None,
        world: encode(&school(day)),
        day,
        keys: "",
    });
    for (name, keys) in [
        ("day", ""),
        ("map", "g"),
        ("map-moved", "g^<"),
        ("schemes", "p"),
        ("target", "p2"),
        ("method", "p21"),
        ("planner", "p1"),
        ("planner-odds", "p1>"),
        ("twists", "p1t"),
        ("getaway", "p1y"),
        ("schedule", "p1d"),
        ("file", "f"),
        ("file-marks", "f>"),
        ("clubs", "c"),
        ("journal", "j"),
        ("paper", "n"),
        ("hall", "nh"),
        ("yearbook", "ny"),
        ("social", "o"),
        ("classmates", "oc"),
        ("plots", "oc1"),
        ("rumours", "or"),
        ("rumour", "or1"),
        ("election", "oe"),
    ] {
        out.push(state_of(name, &base, day, keys));
    }
    // The library, at lunch: somewhere to study.
    {
        let (mut sb, day) = pupil();
        sb.game.player.place = "library".into();
        sb.game.player.slot = 2;
        out.push(state_of("study", &sb, day, "s"));
    }
    // A shop.
    {
        let (mut sb, day) = pupil();
        sb.game.player.place = "tuck_shop".into();
        sb.game.player.slot = 2;
        out.push(state_of("shop", &sb, day, "b"));
    }
    // A lesson.
    {
        let (mut sb, day) = pupil();
        sb.game.player.slot = 1;
        sb.game.player.place = "room_maths".into();
        out.push(state_of("lesson", &sb, day, "a"));
        // Each subject's own picture: sciences (balance), languages (vocab), history (timeline), art (pattern).
        let by_subject = [
            ("lesson-sciences", "room_sciences", 4usize),
            ("lesson-history", "room_history", 0),
        ];
        for (name, room, slot) in by_subject {
            let (mut sb, _) = pupil();
            sb.game.player.slot = u8::try_from(slot).unwrap();
            sb.game.player.place = room.into();
            // Wednesday: history first period, sciences fifth.
            out.push(state_of(name, &sb, day, "a"));
        }
    }
    // A conversation: a staff member who has noticed you.
    {
        let (mut sb, day) = pupil();
        sb.game.world.term_start = sb.game.player.day - 20;
        satisfy(&mut sb.game, &Cond::parse("wary:marsh>=2").unwrap());
        meet(&mut sb.game, "marsh");
        out.push(state_of("talk", &sb, day, "t"));
        out.push(state_of("scene", &sb, day, "t1"));
    }
    // A scheme being run, one phase done.
    {
        let (mut sb, _) = pupil();
        let id = sb.game.player.schemes.last().unwrap().id;
        let scheme = sb.game.player.scheme(id).unwrap().clone();
        let (d, slot) = scheme.when.unwrap();
        sb.game.sync_day(d);
        sb.game.player.slot = slot;
        sb.game.player.nerve = sb.game.player.nerve_max;
        sb.game.player.place = sb.game.scheme_place(&scheme).unwrap();
        sb.ok(Action::BeginRun(id));
        out.push(state_of("run", &sb, d, ""));
        sb.ok(Action::RunPhase { push: 1 });
        out.push(state_of("run-act", &sb, d, ""));
    }
    // The end of a term.
    {
        let (mut sb, day) = pupil();
        sb.game.finish_term();
        out.push(state_of("over", &sb, day, ""));
    }
    let _ = (Status::New,);
    out
}

fn visit(door: &Hallpass, env: &DoorEnv, s: &State) -> Visit {
    Visit::open(door, env.clone(), s)
}

/// Print every screen: `HALLPASS_SHOW=1 cargo test -p russh-bbs-door-hallpass --test ui show
/// -- --nocapture`. `HALLPASS_SIZE=100x30`, `HALLPASS_TIER=ascii`, `HALLPASS_ONLY=map`,
/// `HALLPASS_DUMP=dir` writes `.cells` files for the picture renderer.
#[test]
fn show() {
    if std::env::var("HALLPASS_SHOW").is_err() && std::env::var("HALLPASS_DUMP").is_err() {
        return;
    }
    let (cols, rows): (u16, u16) = std::env::var("HALLPASS_SIZE")
        .ok()
        .and_then(|s| {
            let (c, r) = s.split_once('x')?;
            Some((c.parse().ok()?, r.parse().ok()?))
        })
        .unwrap_or((80, 24));
    let tier = match std::env::var("HALLPASS_TIER").as_deref() {
        Ok("ascii") => Tier::Ascii,
        Ok("nerd") => Tier::NerdFont,
        _ => Tier::Unicode,
    };
    let color = std::env::var("HALLPASS_COLOR").as_deref() != Ok("0");
    let door = Hallpass::new().unwrap();
    let env = env_for(tier, color);
    let only = std::env::var("HALLPASS_ONLY").ok();
    let dump_dir = std::env::var("HALLPASS_DUMP").ok();
    let depth = if std::env::var("HALLPASS_DEPTH").as_deref() == Ok("16") {
        ColorDepth::Ansi16
    } else {
        ColorDepth::TrueColor
    };
    for s in all_states() {
        if only.as_deref().is_some_and(|o| o != s.name) {
            continue;
        }
        let v = visit(&door, &env, &s);
        if let Some(dir) = &dump_dir {
            // A theme like the board's default: the roles the screens ask for.
            let palette = DoorPalette::new([
                Style::new(),
                Style::new().fg(Color::Rgb(128, 138, 144)),
                Style::new()
                    .fg(Color::Rgb(238, 240, 236))
                    .add_modifier(Modifier::BOLD),
                Style::new().fg(Color::Rgb(116, 200, 214)),
                Style::new().fg(Color::Rgb(124, 200, 120)),
                Style::new().fg(Color::Rgb(226, 100, 84)),
                Style::new()
                    .fg(Color::Rgb(16, 22, 28))
                    .bg(Color::Rgb(116, 200, 214)),
            ]);
            std::fs::create_dir_all(dir).unwrap();
            let file = format!(
                "{dir}/{}-{cols}x{rows}-{}{}.cells",
                s.name,
                match tier {
                    Tier::Ascii => "ascii",
                    Tier::Unicode => "unicode",
                    Tier::NerdFont => "nerd",
                },
                if color { "" } else { "-mono" }
            );
            std::fs::write(file, v.dump(cols, rows, palette, depth)).unwrap();
            continue;
        }
        println!("===== {} {cols}x{rows} {tier:?} color={color}", s.name);
        for line in v.render(cols, rows, ColorDepth::Mono) {
            println!("{line}");
        }
    }
}

const SIZES: [(u16, u16); 3] = [(80, 24), (100, 30), (132, 43)];
const TIERS: [Tier; 3] = [Tier::Ascii, Tier::Unicode, Tier::NerdFont];

fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Ascii => "ascii",
        Tier::Unicode => "unicode",
        Tier::NerdFont => "nerd",
    }
}

/// Every screen at one size as text, in the order of `all_states`.
fn rendered(
    door: &Hallpass,
    env: &DoorEnv,
    (cols, rows): (u16, u16),
) -> Vec<(&'static str, Vec<String>)> {
    all_states()
        .into_iter()
        .map(|s| {
            let v = visit(door, env, &s);
            (s.name, v.render(cols, rows, ColorDepth::Mono))
        })
        .collect()
}

#[test]
fn every_screen_fits_and_is_legible_at_every_size_in_every_tier_with_and_without_colour() {
    let door = Hallpass::new().unwrap();
    for tier in TIERS {
        for color in [true, false] {
            let env = env_for(tier, color);
            for (cols, rows) in SIZES {
                for (name, lines) in rendered(&door, &env, (cols, rows)) {
                    let text = lines.join("\n");
                    let ctx = format!("{name} {cols}x{rows} {tier:?} colour={color}\n{text}");
                    assert_eq!(lines.len(), usize::from(rows), "{ctx}");
                    assert!(
                        lines.iter().all(|l| l.chars().count() <= usize::from(cols)),
                        "{ctx}"
                    );
                    assert!(text.chars().any(char::is_alphabetic), "{ctx}");
                    assert!(!text.contains('\u{FFFD}'), "a neutralized glyph\n{ctx}");
                    if tier == Tier::Ascii {
                        assert!(text.is_ascii(), "the ASCII tier is ASCII only\n{ctx}");
                    }
                    assert!(
                        name == "enrol" || lines[0].contains("GREYWATER"),
                        "the masthead names the school\n{ctx}"
                    );
                    assert!(
                        name != "enrol"
                            || text.contains("G R E Y W A T E R")
                            || text.contains("╔═╗"),
                        "the banner\n{ctx}"
                    );
                    let used = lines.iter().filter(|l| !l.trim().is_empty()).count();
                    assert!(
                        used * 2 >= usize::from(rows) || name == "enrol",
                        "{used} used rows\n{ctx}"
                    );
                }
            }
        }
    }
}

#[test]
fn every_screen_also_fits_the_74_by_20_a_door_gets_inside_an_80_by_24_board() {
    let door = Hallpass::new().unwrap();
    for tier in [Tier::Ascii, Tier::Unicode] {
        let env = env_for(tier, true);
        for (name, lines) in rendered(&door, &env, (74, 20)) {
            let text = lines.join("\n");
            assert_eq!(lines.len(), 20, "{name}");
            assert!(
                lines.iter().all(|l| l.chars().count() <= 74),
                "{name}\n{text}"
            );
            assert!(
                name == "enrol" || lines[0].contains("GREYWATER"),
                "{name}\n{text}"
            );
            // The strip with the periods is on every screen but the card and the report.
            if name != "enrol" && name != "over" {
                assert!(
                    lines[1].contains("Maths") || lines[1].contains("Hist"),
                    "{name}: the timetable strip\n{text}"
                );
            }
        }
    }
    // Under the smallest size the plain text screen takes over rather than a broken picture.
    let env = env_for(Tier::Ascii, true);
    for (name, lines) in rendered(&door, &env, (56, 15)) {
        assert!(lines.iter().all(|l| l.chars().count() <= 56), "{name}");
        assert!(
            lines.join("\n").contains("HALLPASS") || name == "enrol",
            "{name}"
        );
    }
}

#[test]
fn the_nerd_font_tier_draws_what_the_unicode_tier_draws() {
    // The board neutralizes private-use glyphs in a door's cells (D104), so a door cannot use the
    // Nerd Font glyphs; the two tiers must therefore be the same picture, with no private-use
    // glyph in it.
    let door = Hallpass::new().unwrap();
    for size in SIZES {
        let unicode = rendered(&door, &env_for(Tier::Unicode, true), size);
        let nerd = rendered(&door, &env_for(Tier::NerdFont, true), size);
        assert_eq!(unicode, nerd, "{size:?}");
        for (name, lines) in &unicode {
            assert!(
                lines
                    .iter()
                    .flat_map(|l| l.chars())
                    .all(|c| !('\u{E000}'..='\u{F8FF}').contains(&c)),
                "{name}"
            );
        }
    }
}

/// Snapshots: one file per size and tier, every screen in it. `HALLPASS_BLESS=1` rewrites them.
#[test]
fn render_snapshots_of_every_screen_at_every_size_in_the_ascii_and_unicode_tiers() {
    let door = Hallpass::new().unwrap();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let bless = std::env::var("HALLPASS_BLESS").is_ok();
    for tier in [Tier::Ascii, Tier::Unicode] {
        for (cols, rows) in SIZES {
            let mut text = String::new();
            for (name, lines) in rendered(&door, &env_for(tier, true), (cols, rows)) {
                text.push_str(&format!("===== {name} {cols}x{rows} {}\n", tier_name(tier)));
                for line in lines {
                    text.push_str(line.trim_end());
                    text.push('\n');
                }
            }
            let file = dir.join(format!("{}-{cols}x{rows}.txt", tier_name(tier)));
            if bless {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(&file, &text).unwrap();
                continue;
            }
            let want = std::fs::read_to_string(&file)
                .unwrap_or_else(|_| panic!("no snapshot {file:?}; run with HALLPASS_BLESS=1"));
            if want != text {
                let first = want
                    .lines()
                    .zip(text.lines())
                    .enumerate()
                    .find(|(_, (a, b))| a != b);
                panic!(
                    "{file:?} changed (HALLPASS_BLESS=1 rewrites it); first difference: {first:?}"
                );
            }
        }
    }
}

// ------------------------------------------------------------------------------------------
// The look-around.

#[test]
fn every_screen_has_a_look_around_that_describes_it() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Ascii, true);
    let mut seen = std::collections::BTreeSet::new();
    for s in all_states() {
        let v = visit(&door, &env, &s);
        let lines = v.look();
        let at = lines
            .iter()
            .position(|l| l.starts_with("Looking around:"))
            .unwrap_or_else(|| panic!("{}: no look-around\n{lines:?}", s.name));
        let described = &lines[at..];
        assert!(
            described.len() >= 2 && described.iter().all(|l| !l.trim().is_empty()),
            "{}: {described:?}",
            s.name
        );
        assert!(
            described.iter().map(String::len).sum::<usize>() >= 80,
            "{}: {described:?}",
            s.name
        );
        // The odds and file tabs are the same screen to a reader: the text covers every tab.
        let tab = matches!(s.name, "planner-odds" | "file-marks");
        assert!(
            seen.insert(described.join("\n")) || tab,
            "{} describes the same as another screen",
            s.name
        );
        // The look-around is the same in every tier and every size: it describes the screen.
        let nerd = Visit::open(&door, env_for(Tier::Unicode, false), &s).look();
        assert_eq!(lines, nerd, "{}", s.name);
    }
}

#[test]
fn the_map_describes_where_you_are_what_is_shut_and_who_is_about() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Ascii, true);
    let states = all_states();
    let map = states.iter().find(|s| s.name == "map").unwrap();
    let text = visit(&door, &env, map).look().join("\n");
    for needle in [
        "The school map",
        "You are in the The Quad",
        "You can walk to:",
        "Locked to you:",
        "Shut at this hour:",
        "Exits:",
        "Arrow keys move the cursor",
    ] {
        assert!(text.contains(needle), "lacks {needle:?}\n{text}");
    }
    // The cursor moved, and the description follows it.
    let moved = states.iter().find(|s| s.name == "map-moved").unwrap();
    let after = visit(&door, &env, moved).look().join("\n");
    assert_ne!(
        text.lines().find(|l| l.contains("The cursor is on")),
        after.lines().find(|l| l.contains("The cursor is on")),
        "{after}"
    );
}

#[test]
fn the_planner_and_the_file_say_their_numbers_in_words() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Ascii, true);
    let states = all_states();
    let find = |n: &str| {
        let s = states.iter().find(|s| s.name == n).unwrap();
        visit(&door, &env, s).look().join("\n")
    };
    let planner = find("planner");
    for needle in ["The planner for scheme", "Odds:", "Why sneak is", "percent"] {
        assert!(planner.contains(needle), "{needle:?}\n{planner}");
    }
    let file = find("file");
    for needle in [
        "Your file in the headmistress's office",
        "of 12 points",
        "Grades:",
    ] {
        assert!(file.contains(needle), "{needle:?}\n{file}");
    }
    let run = find("run");
    assert!(run.contains("A scheme is running, stage 1 of 3"), "{run}");
}

// ------------------------------------------------------------------------------------------
// What the screens show, and the keys.

fn screen_of(door: &Hallpass, env: &DoorEnv, name: &str, size: (u16, u16)) -> String {
    let states = all_states();
    let s = states.iter().find(|s| s.name == name).unwrap();
    visit(door, env, s)
        .render(size.0, size.1, ColorDepth::Mono)
        .join("\n")
}

#[test]
fn the_day_shows_the_timetable_strip_the_place_the_people_and_the_orders() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Unicode, true);
    let day = screen_of(&door, &env, "day", (100, 30));
    for needle in [
        "Maths", "Hist", "Lunch", "Night", "The Quad", "Here:", "heat", "file", "nerve", "Go",
        "Schemes", "Clubs", "File", "Today",
    ] {
        assert!(day.contains(needle), "lacks {needle:?}\n{day}");
    }
    // The period now is lit with its own brackets.
    assert!(day.contains('▐') && day.contains('▌'), "{day}");
}

#[test]
fn the_map_shows_the_places_their_state_and_a_detail_for_the_one_under_the_cursor() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Unicode, true);
    let map = screen_of(&door, &env, "map", (132, 43));
    for needle in [
        "The Quad",
        "Library",
        "Great",
        "Hall",
        "Chapel",
        "Old Wing",
        "East Wing",
        "Cellars",
        "locked",
        "you are here",
        "hours",
        "doors",
    ] {
        assert!(map.contains(needle), "lacks {needle:?}\n{map}");
    }
    // Locked doors are red-marked by a glyph as well as colour.
    assert!(map.contains('⊘'), "{map}");
    let ascii = screen_of(&door, &env_for(Tier::Ascii, false), "map", (80, 24));
    assert!(ascii.contains('X') && ascii.contains('@'), "{ascii}");
}

#[test]
fn the_planner_shows_target_method_odds_and_what_is_missing() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Unicode, true);
    let p = screen_of(&door, &env, "planner", (100, 30));
    for needle in [
        "target",
        "method",
        "getaway",
        "when",
        "scout",
        "The odds",
        "sneak",
        "act",
        "escape",
        "traced",
        "why sneak",
    ] {
        assert!(p.contains(needle), "lacks {needle:?}\n{p}");
    }
    assert!(p.contains("READY") || p.contains("NOT READY"), "{p}");
    // Left and right look at another phase's reasons.
    let next = screen_of(&door, &env, "planner-odds", (100, 30));
    assert!(next.contains("why act"), "{next}");
}

#[test]
fn the_file_fills_with_entries_and_stamps_what_it_has_reached() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Unicode, true);
    let f = screen_of(&door, &env, "file", (132, 43));
    for needle in [
        "OFFICE OF THE HEADMISTRESS",
        "CONFIDENTIAL",
        "thick",
        "DETENTION",
        "Marks and skills",
        "Standing",
    ] {
        assert!(f.contains(needle), "lacks {needle:?}\n{f}");
    }
    assert!(f.contains("Mr. Marsh writes you up"), "tier text\n{f}");
}

#[test]
fn each_subject_draws_its_own_small_picture() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Unicode, true);
    let maths = screen_of(&door, &env, "lesson", (100, 30));
    assert!(
        maths.contains("Continue the sequence:") && maths.contains('Σ'),
        "{maths}"
    );
    assert!(maths.contains("Your answer"), "{maths}");
    let history = screen_of(&door, &env, "lesson-history", (100, 30));
    assert!(
        history.contains('⌛') && history.contains("History"),
        "{history}"
    );
    let sciences = screen_of(&door, &env, "lesson-sciences", (100, 30));
    assert!(sciences.contains('⚗'), "{sciences}");
}

#[test]
fn a_shut_choice_in_a_conversation_names_what_it_wants() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Unicode, false);
    let scene = screen_of(&door, &env, "scene", (100, 30));
    assert!(scene.contains("Marsh"), "{scene}");
    assert!(scene.contains("needs"), "{scene}");
    assert!(scene.contains("excuse yourself"), "{scene}");
}

// ------------------------------------------------------------------------------------------
// Colour, markers and the sixteen colours the board composites to.

#[test]
fn no_reverse_bold_marker_style_and_the_accents_survive_sixteen_colours() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Unicode, true);
    let mut colours = std::collections::BTreeSet::new();
    for s in all_states() {
        let v = visit(&door, &env, &s);
        let dump = v.dump(100, 30, DoorPalette::mono(), ColorDepth::Ansi16);
        for line in dump.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 4 {
                continue;
            }
            let flags = parts[3];
            assert!(
                !(flags.contains('b') && flags.contains('r')),
                "bold plus reverse is the reserved marker style ({})",
                s.name
            );
            colours.insert(parts[1].to_owned());
            colours.insert(parts[2].to_owned());
        }
    }
    // At sixteen colours the board's palette is all names, no rgb values.
    assert!(
        colours.iter().all(|c| !c.starts_with('#')),
        "rgb survived the sixteen-colour composite: {colours:?}"
    );
    // The ink, chalk, red and lamplight accents are all there, and apart.
    for want in ["LightBlue", "Red", "LightYellow"] {
        assert!(colours.contains(want), "{want} in {colours:?}");
    }
}

#[test]
fn with_colour_off_nothing_is_coloured_and_every_cue_is_a_word_or_a_glyph() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Ascii, false);
    for s in all_states() {
        let v = visit(&door, &env, &s);
        let dump = v.dump(80, 24, DoorPalette::mono(), ColorDepth::TrueColor);
        assert!(
            dump.lines()
                .filter_map(|l| l.split('\t').nth(1))
                .all(str::is_empty),
            "{}: a foreground colour with colour off",
            s.name
        );
    }
    // The file's stamps and the readiness verdict are words.
    let file = screen_of(&door, &env, "file", (80, 24));
    assert!(file.contains("DETENTION"), "{file}");
    let planner = screen_of(&door, &env, "planner", (80, 24));
    assert!(planner.contains("READY"), "{planner}");
}

// ------------------------------------------------------------------------------------------
// Keys.

#[test]
fn arrows_move_the_lightbar_and_enter_presses_the_item_it_is_on() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Unicode, true);
    let states = all_states();
    let s = states.iter().find(|s| s.name == "day").unwrap();
    let mut v = visit(&door, &env, s);
    let before = v.render(80, 24, ColorDepth::Mono).join("\n");
    v.keys("v");
    assert_ne!(
        before,
        v.render(80, 24, ColorDepth::Mono).join("\n"),
        "the bar moved"
    );
    // The first item is Go: Enter on it opens the map. Down then Up returns to it.
    v.keys("^!");
    assert!(v.look().join("\n").contains("The school map"));
    // On the map the arrows move a cursor, Enter walks, Esc comes back.
    v.keys("v");
    let moved = v.look().join("\n");
    assert!(moved.contains("The cursor is on"), "{moved}");
    v.keys("~");
    assert!(v.look().join("\n").contains("the day screen"));
}

#[test]
fn enter_on_the_map_walks_to_a_place_you_can_reach_and_refuses_one_you_cannot() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Unicode, true);
    let states = all_states();
    let s = states.iter().find(|s| s.name == "day").unwrap();
    let mut v = visit(&door, &env, s);
    v.keys("g");
    // Far to the right and down: the East Wing or the cellars are not reachable from the Quad.
    v.keys(">>>>>vvvv!");
    let text = v.look().join("\n");
    assert!(
        text.contains("out of reach") || text.contains("walk"),
        "{text}"
    );
    // The place next door: any reachable place walks.
    let mut w = visit(&door, &env, s);
    w.keys("g");
    let here = w.look().join("\n");
    assert!(here.contains("You are in the The Quad"), "{here}");
    w.keys("v!");
    let after = w.look().join("\n");
    assert!(
        after.contains("the day screen") || after.contains("out of reach"),
        "{after}"
    );
}

// ------------------------------------------------------------------------------------------
// One whole day through the keys.

#[test]
fn one_whole_day_lessons_a_scheme_planned_and_run_the_file_and_a_club_meeting() {
    let door = Hallpass::new().unwrap();
    let env = env_for(Tier::Unicode, true);
    // A fresh pupil on a Monday: the enrolment card, then the day.
    let monday = monday();
    let state = State {
        name: "fresh",
        player: None,
        world: encode(&World::from_bytes(None, monday)),
        day: monday,
        keys: "",
    };
    let mut v = visit(&door, &env, &state);
    let card = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(card.contains("Who do you want to be known as?"), "{card}");
    v.keys("1");
    let day = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(day.contains("Fenwick dormitory"), "{day}");
    // First period is maths on a Monday: walk there and take the lesson.
    assert!(v.look().join("\n").contains("[g]"), "go is on offer");
    v.keys("g");
    // Pick the maths room on the map by walking the cursor: or use the numbered list.
    let list: Vec<String> = v.look();
    let maths = list
        .iter()
        .find_map(|l| l.strip_prefix('[').filter(|_| l.contains("Maths room")))
        .map(|l| l.chars().next().unwrap());
    // The numbered keys still work on the map: they press the same items.
    let key = maths.expect("the maths room is on the list");
    v.keys(&key.to_string());
    assert!(v.look().join("\n").contains("Maths room"), "walked there");
    v.keys("a");
    let lesson = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(lesson.contains("puzzle 1 of 3"), "{lesson}");
    // Answer by Enter on the lightbar three times: the first open answer.
    for _ in 0..3 {
        v.keys("!");
    }
    let after = v.look().join("\n");
    assert!(!after.contains("puzzle"), "the lesson is over\n{after}");
    // The file and the journal are always a key away.
    v.keys("f");
    let file = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(file.contains("OFFICE OF THE HEADMISTRESS"), "{file}");
    v.keys("b");
    // A scheme: plan one.
    v.keys("p");
    assert!(
        v.render(80, 24, ColorDepth::Mono)
            .join("\n")
            .contains("Your schemes")
    );
    v.keys("1");
    let target = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(target.contains("Aimed at"), "{target}");
    v.keys("1");
    v.keys("1");
    let planner = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(planner.contains("The odds"), "{planner}");
    assert!(planner.contains("NOT READY"), "{planner}");
    // Clubs, then home.
    v.keys("~~~");
    v.keys("c");
    let clubs = v.render(80, 24, ColorDepth::Mono).join("\n");
    assert!(clubs.contains("Chess Circle"), "{clubs}");
    v.keys("b");
    // The save went out after each real action.
    assert!(v.last_save.is_some());
}
