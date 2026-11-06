//! The framework's FW-1 seams as Hallpass uses them: the board's clock at drawing time, the
//! world a session reads, and the look-around key.
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
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::print_stderr,
    clippy::panic
)]
mod common;

use std::sync::Arc;

use common::Sandbox;
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_door::host::Driver;
use russh_bbs_door::replay::draw_to_text;
use russh_bbs_door::{DoorEnv, DoorKey, Save, Start, TransportKind};
use russh_bbs_door_hallpass::save::encode_player;
use russh_bbs_door_hallpass::{Hallpass, SAVE_VERSION};

const BELL: &str = "The morning bell has rung";

fn text(driver: &Driver, env: &DoorEnv, cols: u16, rows: u16) -> String {
    draw_to_text(driver, env, cols, rows).join("\n")
}

#[test]
fn a_visit_left_open_across_midnight_says_the_bell_has_rung_and_the_next_key_starts_the_day() {
    let door = Hallpass::shared().unwrap();
    let sb = Sandbox::new("smooth_talker", 1);
    let day = sb.game.player.day;
    let bytes = encode_player(&sb.game.player);
    let env = DoorEnv::for_tests("ann");
    let clock = Arc::new(ManualClock::new(Day::from_raw(day).start()));
    let as_clock: Arc<dyn Clock> = clock.clone();
    let mut driver = Driver::open(
        door.as_ref(),
        Start {
            save: Save::Loaded {
                bytes: &bytes,
                season: 1,
            },
            world: None,
            env: &env,
            today: Day::from_raw(day),
            season: 1,
        },
        1,
        as_clock,
    );
    assert!(
        !text(&driver, &env, 80, 24).contains(BELL),
        "same day, no bell"
    );
    // Midnight passes with no key: the next draw says so, at every size, because the draw reads
    // the board's clock and not the day the visit opened.
    clock.set(Day::from_raw(day + 1).start());
    for (cols, rows) in [(80, 24), (100, 30), (132, 43), (60, 16), (50, 12)] {
        let screen = text(&driver, &env, cols, rows);
        assert!(screen.contains(BELL), "{cols}x{rows}:\n{screen}");
    }
    // The next key is the morning: the bell is gone and the day moved on.
    driver.key(DoorKey::Char('x'));
    let after = text(&driver, &env, 80, 24);
    assert!(!after.contains(BELL), "{after}");
    assert!(
        after.contains("day 2") || after.contains("Day 2") || after.contains('2'),
        "{after}"
    );
}

#[test]
fn hallpass_never_takes_typed_text_so_the_look_around_key_is_always_the_boards() {
    let door = Hallpass::shared().unwrap();
    let sb = Sandbox::new("smooth_talker", 1);
    let bytes = encode_player(&sb.game.player);
    let env = DoorEnv::for_tests("ann");
    let day = sb.game.player.day;
    let clock = Arc::new(ManualClock::new(Day::from_raw(day).start()));
    let as_clock: Arc<dyn Clock> = clock;
    let mut driver = Driver::open(
        door.as_ref(),
        Start {
            save: Save::Loaded {
                bytes: &bytes,
                season: 1,
            },
            world: None,
            env: &env,
            today: Day::from_raw(day),
            season: 1,
        },
        1,
        as_clock,
    );
    // Walk through the screens a day offers (the map, study, the file, clubs, the journal, the
    // social hub and its pages): on every one the door takes no typed text, and no order is bound
    // to the board's look-around key.
    let mut orders = 0;
    for keys in [
        "", "g", "b", "f", "b", "c", "b", "j", "b", "o", "c", "b", "b", "p", "b",
    ] {
        for k in keys.chars() {
            driver.key(DoorKey::Char(k));
        }
        // A `?` is the board's, on every screen: it opens the look-around view, and any key
        // closes it and goes no further.
        driver.key(DoorKey::Char('?'));
        let looking = text(&driver, &env, 80, 24);
        assert!(
            looking.contains("Look around"),
            "after `{keys}`:\n{looking}"
        );
        driver.key(DoorKey::Char('?'));
        assert!(
            !text(&driver, &env, 80, 24).contains("Look around (any key returns)"),
            "after `{keys}`"
        );
        for line in driver.look_around(&env) {
            if let Some(rest) = line.trim_start().strip_prefix('[') {
                let key = rest.chars().next().unwrap();
                assert_ne!(key, '?', "{line}");
                orders += 1;
            }
        }
    }
    assert!(orders > 20, "{orders} orders read");
    // The door's own source binds no `?` either: every key is a letter or a digit.
    let source = include_str!("../src/session.rs");
    assert!(!source.contains("'?' =>"));
    let _ = (SAVE_VERSION, TransportKind::Ssh);
}
