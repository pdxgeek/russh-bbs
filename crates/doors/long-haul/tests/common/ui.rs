//! Helpers for the UI tests: scenarios (a captain in a state, some keys), a driver, and a
//! render to cleaned cells in a tier at a size, with text and ANSI/HTML exports.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::format_push_string,
    missing_docs
)]

use std::sync::Arc;

use russh_bbs_core::limits::{Day, ManualClock};
use russh_bbs_door::buffer::Buffer;
use russh_bbs_door::host::Driver;
use russh_bbs_door::style::{Color, Modifier, Style};
use russh_bbs_door::{
    ColorDepth, Composite, DoorEnv, DoorKey, DoorPalette, DoorSurface, Save, Start, Tier,
};
use russh_bbs_door_long_haul::LongHaul;
use russh_bbs_door_long_haul::crew::Role;
use russh_bbs_door_long_haul::player::{Intel, Player};
use russh_bbs_door_long_haul::ship::Lot;

use super::{DAY0, Harness};

pub fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Day::from_raw(DAY0).start()))
}

/// The door's area for a terminal of `cols` by `rows`, as the board's box leaves it (the box is
/// at most 76 wide, takes a border, a status row, a heading and a blank line, and keeps a row
/// off each edge of a tall screen).
pub fn door_area(cols: u16, rows: u16) -> (u16, u16) {
    let width = cols.min(76) - 2;
    let content = rows - 1;
    let boxed = if content > 24 { content - 2 } else { content };
    (width, boxed - 2 - 2)
}

pub fn env(tier: Tier) -> DoorEnv {
    DoorEnv {
        tier,
        ..DoorEnv::for_tests("ann")
    }
}

pub fn keys(spec: &str) -> Vec<DoorKey> {
    spec.split(' ')
        .filter(|t| !t.is_empty())
        .map(|t| {
            let mut chars = t.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if !c.is_ascii_alphanumeric() => DoorKey::Char(c),
                _ => DoorKey::from_token(t).unwrap(),
            }
        })
        .collect()
}

pub fn open(door: &LongHaul, save: Option<&Player>, spec: &str, seed: u64) -> Driver {
    let bytes = save.map(Player::encode);
    let environment = env(Tier::Unicode);
    let start = Start {
        save: match &bytes {
            Some(b) => Save::Loaded {
                bytes: b,
                season: 1,
            },
            None => Save::Fresh,
        },
        world: None,
        env: &environment,
        today: Day::from_raw(DAY0),
        season: 1,
    };
    let mut driver = Driver::open(door, start, seed, clock());
    for key in keys(spec) {
        driver.key(key);
    }
    driver
}

/// A captain with a crew, guns, cargo and a little price memory, docked at an industrial
/// station.
pub fn rich_captain(h: &mut Harness, docked: bool) -> Player {
    let mut p = h.captain("dockhand");
    h.fit(&mut p, "lance1");
    h.fit(&mut p, "shield1");
    for (role, skill) in [
        (Role::Pilot, 4),
        (Role::Engineer, 3),
        (Role::Gunner, 5),
        (Role::Medic, 2),
    ] {
        h.sign(&mut p, role, skill);
    }
    p.crew[0].sworn = true;
    p.crew[1].mood = [3, -2, -6, 4];
    p.crew[3].loyalty = 9;
    p.crew[2].hurt_days = 3;
    p.ship.cargo.push(Lot {
        good: 0,
        tons: 12,
        paid: 600,
    });
    p.ship.cargo.push(Lot {
        good: 3,
        tons: 6,
        paid: 900,
    });
    p.standing = [32, -12, 8, 27, -45];
    p.cash = 18_450;
    let st = h.station("industrial");
    if docked {
        h.dock_free(&mut p, st);
    } else {
        h.orbit(&mut p, st);
    }
    // Remember prices at a handful of stations, so the sparkline has a landscape.
    let ids: Vec<u16> = h.frontier.stations.iter().map(|s| s.id).take(9).collect();
    for (n, id) in ids.into_iter().enumerate() {
        let st = h.frontier.station(id).unwrap();
        let prices: Vec<u32> = st
            .goods
            .iter()
            .enumerate()
            .map(|(k, m)| {
                let base = h.content.goods[usize::from(m.good)].base;
                base * (70 + ((n * 37 + k * 11) % 90) as u32) / 100
            })
            .collect();
        p.intel.push(Intel {
            station: id,
            day: DAY0 - (n as i64 % 6),
            prices,
        });
    }
    p
}

pub fn depth_palette() -> DoorPalette {
    DoorPalette::new([
        Style::new(),
        Style::new().fg(Color::DarkGray),
        Style::new().add_modifier(Modifier::BOLD),
        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        Style::new().fg(Color::Green),
        Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        Style::new().add_modifier(Modifier::REVERSED),
    ])
}

/// Draw the door at `cols` by `rows` in `tier` with `depth` colours.
pub fn render(driver: &Driver, cols: u16, rows: u16, tier: Tier, depth: ColorDepth) -> Buffer {
    let environment = env(tier);
    let mut surface = DoorSurface::new(cols, rows, depth_palette());
    surface.draw(|canvas| driver.render(canvas, &environment));
    surface
        .composite(Composite::new(depth, tier == Tier::Ascii))
        .into_buffer()
}

pub fn text(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_owned())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn rgb(c: Color, default: (u8, u8, u8)) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (0, 0, 0),
        Color::Red => (205, 49, 49),
        Color::Green => (13, 188, 121),
        Color::Yellow => (229, 229, 16),
        Color::Blue => (36, 114, 200),
        Color::Magenta => (188, 63, 188),
        Color::Cyan => (17, 168, 205),
        Color::Gray => (204, 204, 204),
        Color::DarkGray => (118, 118, 118),
        Color::LightRed => (241, 76, 76),
        Color::LightGreen => (35, 209, 139),
        Color::LightYellow => (245, 245, 67),
        Color::LightBlue => (59, 142, 234),
        Color::LightMagenta => (214, 112, 214),
        Color::LightCyan => (41, 184, 219),
        Color::White => (255, 255, 255),
        Color::Indexed(i) => {
            let g = 8 + (i.saturating_sub(232)) * 10;
            (g, g, g)
        }
        Color::Reset => default,
    }
}

/// The buffer as a standalone HTML page (a terminal look), for screenshots.
pub fn html(title: &str, buf: &Buffer) -> String {
    let (fg0, bg0) = ((0xd8, 0xdc, 0xe0), (0x0b, 0x0f, 0x17));
    let mut out = format!(
        "<!doctype html><meta charset=utf-8><title>{title}</title><body style=\"margin:0;background:#{:02x}{:02x}{:02x}\">\
<pre style=\"margin:12px;font:15px/1.15 'DejaVu Sans Mono',Menlo,monospace;color:#{:02x}{:02x}{:02x}\">",
        bg0.0, bg0.1, bg0.2, fg0.0, fg0.1, fg0.2
    );
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let c = &buf[(x, y)];
            let rev = c.modifier.contains(Modifier::REVERSED);
            let (mut f, mut b) = (rgb(c.fg, fg0), rgb(c.bg, bg0));
            if rev {
                std::mem::swap(&mut f, &mut b);
            }
            let sym = c.symbol().replace('&', "&amp;").replace('<', "&lt;");
            let dim = if c.modifier.contains(Modifier::DIM) {
                "opacity:.6;"
            } else {
                ""
            };
            let bold = if c.modifier.contains(Modifier::BOLD) {
                "font-weight:bold;"
            } else {
                ""
            };
            out.push_str(&format!(
                "<span style=\"color:#{:02x}{:02x}{:02x};background:#{:02x}{:02x}{:02x};{dim}{bold}\">{sym}</span>",
                f.0, f.1, f.2, b.0, b.1, b.2
            ));
        }
        out.push('\n');
    }
    out.push_str("</pre>");
    out
}

pub struct Scene {
    pub name: &'static str,
    pub build: Box<dyn Fn(&LongHaul) -> Driver>,
}

/// Every screen of the game, each a deterministic driver.
pub fn scenes() -> Vec<Scene> {
    fn scene(name: &'static str, f: impl Fn(&LongHaul) -> Driver + 'static) -> Scene {
        Scene {
            name,
            build: Box::new(f),
        }
    }
    let docked = |spec: &'static str| {
        move |door: &LongHaul| {
            let mut h = Harness::new(1, 11);
            let p = rich_captain(&mut h, true);
            open(door, Some(&p), spec, 5)
        }
    };
    let flying = |spec: &'static str| {
        move |door: &LongHaul| {
            let mut h = Harness::new(1, 11);
            let p = rich_captain(&mut h, false);
            open(door, Some(&p), spec, 5)
        }
    };
    vec![
        scene("intro", |door| open(door, None, "down", 5)),
        scene("bridge_docked", docked("")),
        scene("bridge_flying", flying("")),
        scene("chart_region", flying("v")),
        scene("chart_frontier", flying("v + +")),
        scene("chart_close", flying("v +")),
        scene("route", flying("v right enter")),
        scene("market", docked("m down down")),
        scene("jobs", docked("b down")),
        scene("crew", docked("c down")),
        scene("crew_recruit", docked("c down down down down")),
        scene("yard", docked("y down")),
        scene("loan", docked("l")),
        scene("news", docked("n")),
        scene("standing", docked("a")),
        scene("society_convoy", docked("o")),
        scene("society_piracy", docked("o right")),
        scene("society_outposts", docked("o right right")),
        scene("society_hall", docked("o right right right")),
        scene("society_journal", docked("o right right right right")),
        scene("event", |door| {
            let mut h = Harness::new(1, 11);
            let mut p = rich_captain(&mut h, false);
            h.open_event(&mut p, "dead_freighter");
            open(door, Some(&p), "", 5)
        }),
        scene("combat", |door| {
            let mut h = Harness::new(1, 11);
            let mut p = rich_captain(&mut h, false);
            h.fight(&mut p, "skiff");
            open(door, Some(&p), "left", 5)
        }),
        scene("combat_won", |door| {
            let mut h = Harness::new(1, 11);
            let mut p = rich_captain(&mut h, false);
            h.fight(&mut p, "skiff");
            if let Some(russh_bbs_door_long_haul::scene::Scene::Combat(c)) = &mut p.scene {
                c.phase = russh_bbs_door_long_haul::combat::Phase::Won;
            }
            open(door, Some(&p), "", 5)
        }),
        scene("boarding", |door| {
            let mut h = Harness::new(1, 11);
            let mut p = rich_captain(&mut h, false);
            h.board(&mut p, "pirate_wreck");
            open(door, Some(&p), "", 5)
        }),
    ]
}

pub fn dump_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("LH_SHOTS").map(Into::into)
}
