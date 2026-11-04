//! The example door's ratatui screen at the two sizes the owner named, in every icon tier and
//! every colour depth, through the same compositing the board does.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

use std::sync::Arc;

use russh_bbs_core::limits::{Day, ManualClock};
use russh_bbs_door::host::Driver;
use russh_bbs_door::style::{Color, Modifier};
use russh_bbs_door::{
    ColorDepth, Composite, DoorEnv, DoorKey, DoorPalette, DoorSurface, Save, Start, Tier,
};
use russh_bbs_door_example::{CoinToss, PANELS_MIN_COLS, PANELS_MIN_ROWS};

const DAY: i64 = 20_000;
const TIERS: [Tier; 3] = [Tier::Ascii, Tier::Unicode, Tier::NerdFont];
const DEPTHS: [ColorDepth; 4] = [
    ColorDepth::TrueColor,
    ColorDepth::Ansi256,
    ColorDepth::Ansi16,
    ColorDepth::Mono,
];

/// A visit that has made `tosses` calls, alternating heads and tails.
fn visit(env: &DoorEnv, tosses: usize) -> Driver {
    let door = CoinToss::new().unwrap();
    let mut driver = Driver::open(
        &door,
        Start {
            save: Save::Fresh,
            world: None,
            env,
            today: Day::from_raw(DAY),
            season: 1,
        },
        7,
        Arc::new(ManualClock::new(Day::from_raw(DAY).start())),
    );
    for n in 0..tosses {
        driver.key(DoorKey::Char(if n % 2 == 0 { 'h' } else { 't' }));
    }
    driver
}

fn screen(driver: &Driver, env: &DoorEnv, cols: u16, rows: u16, depth: ColorDepth) -> Vec<String> {
    let mut surface = DoorSurface::new(cols, rows, DoorPalette::mono());
    surface.draw(|canvas| driver.render(canvas, env));
    surface
        .composite(Composite::new(depth, env.tier == Tier::Ascii))
        .rows_text()
}

#[test]
fn the_screen_is_legible_at_both_sizes_in_every_tier_and_depth() {
    for (cols, rows) in [(80, 24), (132, 43)] {
        for tier in TIERS {
            let mut env = DoorEnv::for_tests("ann");
            env.tier = tier;
            // Before any toss (empty table and chart) and after the daily five.
            for tosses in [0, 5] {
                let driver = visit(&env, tosses);
                for depth in DEPTHS {
                    let lines = screen(&driver, &env, cols, rows, depth);
                    let text = lines.join("\n");
                    let context =
                        format!("{cols}x{rows} {tier:?} {depth:?} after {tosses}\n{text}");
                    assert_eq!(lines.len(), usize::from(rows), "{context}");
                    assert!(
                        lines.iter().all(|l| l.chars().count() <= usize::from(cols)),
                        "{context}"
                    );
                    // The facts are words, in every tier.
                    assert!(text.contains("Tosses left today:"), "{context}");
                    assert!(text.contains("tosses used today"), "{context}");
                    assert!(
                        text.contains("This visit") && text.contains("Streak"),
                        "{context}"
                    );
                    assert!(text.contains("[h] call heads"), "{context}");
                    if tosses > 0 {
                        assert!(text.contains("Result"), "{context}");
                        assert!(text.contains("won") || text.contains("lost"), "{context}");
                        // Something is plotted in the Purse panel in every tier.
                        assert!(
                            text.chars()
                                .any(|c| matches!(c, '.' | ':' | '#' | '\u{2801}'..='\u{28FF}')),
                            "{context}"
                        );
                    }
                    if tier == Tier::Ascii {
                        assert!(text.is_ascii(), "the ASCII tier is ASCII only\n{context}");
                        assert!(lines[0].starts_with('+'), "{context}");
                    } else {
                        assert!(lines[0].starts_with('┌'), "{context}");
                        if tosses > 0 {
                            assert!(
                                text.chars().any(|c| ('\u{2801}'..='\u{28FF}').contains(&c)),
                                "a braille chart\n{context}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn colors_are_the_doors_own_and_reach_the_screen_only_as_the_member_can_show_them() {
    let env = DoorEnv::for_tests("ann");
    let driver = visit(&env, 3);
    let mut surface = DoorSurface::new(80, 24, DoorPalette::mono());
    surface.draw(|canvas| driver.render(canvas, &env));
    let colors = |depth| {
        let clean = surface.composite(Composite::new(depth, false));
        clean
            .buffer()
            .content
            .iter()
            .flat_map(|cell| [cell.fg, cell.bg])
            .filter(|c| *c != Color::Reset)
            .collect::<Vec<_>>()
    };
    assert!(
        colors(ColorDepth::TrueColor)
            .iter()
            .any(|c| matches!(c, Color::Rgb(..)))
    );
    assert!(
        colors(ColorDepth::Ansi256)
            .iter()
            .any(|c| matches!(c, Color::Indexed(_)))
    );
    assert!(
        colors(ColorDepth::Ansi16)
            .iter()
            .all(|c| !matches!(c, Color::Rgb(..) | Color::Indexed(_)))
    );
    let sixteen = colors(ColorDepth::Ansi16);
    assert!(sixteen.len() > 10, "colour reaches the screen");
    let mono = colors(ColorDepth::Mono);
    assert_eq!(mono.len(), 0, "no colour at all in mono");
    // No cell carries the reserved marker style, whatever the depth.
    for depth in DEPTHS {
        let clean = surface.composite(Composite::new(depth, false));
        assert!(
            clean
                .buffer()
                .content
                .iter()
                .all(|cell| !cell.modifier.contains(Modifier::BOLD | Modifier::REVERSED))
        );
    }
}

#[test]
fn a_small_window_gets_the_text_screen_and_a_reader_gets_the_tosses_in_words() {
    let env = DoorEnv::for_tests("ann");
    let driver = visit(&env, 2);
    let (cols, rows) = (PANELS_MIN_COLS - 1, 30);
    let narrow = screen(&driver, &env, cols, rows, ColorDepth::Mono);
    assert!(narrow[0].contains("ann - day 20000"), "{narrow:?}");
    assert!(!narrow.join("").contains('┌'), "no box in the text screen");
    let short = screen(&driver, &env, 100, PANELS_MIN_ROWS - 1, ColorDepth::Mono);
    assert!(short[0].contains("ann - day 20000"), "{short:?}");
    // The look-around view is plain lines, with the tosses.
    let look = driver.look_around(&env);
    assert!(look.iter().any(|l| l.starts_with("Coins: ")));
    assert!(
        look.iter().any(|l| l.contains("1. called heads")),
        "{look:?}"
    );
    assert!(
        look.iter().any(|l| l.contains("2. called tails")),
        "{look:?}"
    );
}

#[test]
fn a_tiny_window_does_not_break_the_panels() {
    let env = DoorEnv::for_tests("ann");
    let driver = visit(&env, 6);
    for (cols, rows) in [
        (1, 1),
        (PANELS_MIN_COLS, PANELS_MIN_ROWS),
        (57, 15),
        (200, 14),
        (56, 60),
    ] {
        let lines = screen(&driver, &env, cols, rows, ColorDepth::Ansi16);
        assert_eq!(lines.len(), usize::from(rows));
        assert!(!lines.join("").contains("had a problem"), "{cols}x{rows}");
    }
}
