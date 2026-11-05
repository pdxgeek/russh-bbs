#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use russh_bbs_core::terminal_settings::{LayoutId, TerminalSettings};

use super::motion::{self, Motion};
use super::nick;
use crate::settings::Look;
use crate::ui::layout_tests::ui;

fn look_with_motion(motion_on: bool) -> Look {
    let settings = TerminalSettings::default()
        .with_motion(motion_on)
        .with_color(true);
    Look::themed(
        &settings,
        LayoutId::Classic,
        crate::ui::theme::Palette::default(),
    )
}

#[test]
fn nick_colours_are_stable_and_case_blind() {
    assert_eq!(nick::hue_of("ann"), nick::hue_of("Ann"));
    assert_eq!(nick::hue_of("ann"), nick::hue_of("ann"));
    let spread: std::collections::HashSet<usize> = (0..200)
        .map(|n| nick::hue_of(&format!("member{n}")))
        .collect();
    assert!(spread.len() >= 6, "hashes should spread over the hues");
}

#[test]
fn a_nick_never_gets_the_reserved_style() {
    let ui = ui();
    let _ = &ui;
    for handle in ["ann", "bob", "carol", "\u{1b}[7mevil", "◉ UNENCRYPTED", ""] {
        let style = nick::style(look_with_motion(true), handle);
        assert!(
            !style
                .add_modifier
                .intersects(ratatui::style::Modifier::REVERSED | ratatui::style::Modifier::BOLD),
            "a nick must not carry reverse video or bold"
        );
        assert!(style.bg.is_none(), "a nick is a foreground colour only");
    }
}

#[test]
fn motion_is_off_when_the_look_says_so_or_in_accessibility_mode() {
    assert!(motion::allowed(look_with_motion(true), false));
    assert!(!motion::allowed(look_with_motion(false), false));
    assert!(!motion::allowed(look_with_motion(true), true));
}

#[test]
fn a_screen_change_starts_a_wipe_that_ends_after_its_steps() {
    let look = look_with_motion(true);
    let mut state = Motion::default();
    let start = Instant::now();
    state.note_screen(1, look, start);
    assert!(!state.wiping(), "the first screen does not wipe");
    state.note_screen(2, look, start);
    assert!(state.wiping());
    assert_eq!(state.wipe_step(start), Some(0));
    assert_eq!(state.wipe_step(start + motion::STEP), Some(1));
    assert_eq!(state.wipe_step(start + motion::STEP * 3), Some(3));
    assert_eq!(state.wipe_step(start + motion::STEP * 4), None);
    assert!(!state.wiping());
}

#[test]
fn motion_off_means_no_timer_at_all() {
    let off = look_with_motion(false);
    let mut state = Motion::default();
    let now = Instant::now();
    state.note_screen(1, off, now);
    state.note_screen(2, off, now);
    state.set_input(true, now);
    assert!(!state.wiping());
    assert_eq!(state.deadline(off, now), None);
    assert!(!state.cursor_dark(off, now + Duration::from_millis(600)));
}

#[test]
fn accessibility_mode_cancels_and_blocks_motion() {
    let look = look_with_motion(true);
    let mut state = Motion::default();
    let now = Instant::now();
    state.note_screen(1, look, now);
    state.note_screen(2, look, now);
    state.set_input(true, now);
    assert!(state.deadline(look, now).is_some());
    state.set_accessible(true);
    assert!(state.accessible());
    assert!(!state.wiping());
    assert_eq!(state.deadline(look, now), None);
    state.note_screen(3, look, now);
    assert!(!state.wiping());
    assert!(!state.cursor_dark(look, now + Duration::from_millis(600)));
}

#[test]
fn the_cursor_blinks_only_while_an_input_is_on_screen() {
    let look = look_with_motion(true);
    let mut state = Motion::default();
    let now = Instant::now();
    assert_eq!(state.deadline(look, now), None, "no input, no timer");
    state.set_input(true, now);
    assert!(!state.cursor_dark(look, now));
    assert!(state.cursor_dark(look, now + motion::BLINK));
    assert!(!state.cursor_dark(look, now + motion::BLINK * 2));
    let next = state.deadline(look, now).unwrap();
    assert_eq!(next, now + motion::BLINK);
    state.set_input(false, now);
    assert_eq!(state.deadline(look, now), None);
    assert!(!state.cursor_dark(look, now + motion::BLINK));
}

#[test]
fn the_cursor_is_steady_when_motion_is_off() {
    let off = look_with_motion(false);
    assert_eq!(off.with_cursor_dark(true).cursor(), "_");
    let on = look_with_motion(true);
    assert_eq!(on.with_cursor_dark(true).cursor(), " ");
    assert_eq!(on.with_cursor_dark(false).cursor(), "_");
}

/// D49: the shell draws at most every 50 ms. Motion asks for fewer redraws than that, and the
/// cost of a wipe is a few frames once per screen change.
#[test]
fn motion_stays_under_the_redraw_cap() {
    let cap = crate::ssh::limits::Limits::default().redraw_interval;
    assert!(
        motion::STEP >= cap,
        "a wipe frame is no sooner than the cap"
    );
    assert!(
        motion::BLINK >= cap * 10,
        "the cursor blinks far below the cap"
    );
    assert!(motion::STEPS <= 6, "a wipe is short");
    let wipe_time = motion::STEP * u32::from(motion::STEPS);
    assert!(wipe_time <= Duration::from_millis(500), "a wipe is a flick");
}
