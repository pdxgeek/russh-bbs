#![allow(clippy::unwrap_used)]
#![allow(
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::unchecked_time_subtraction,
    clippy::assert_is_empty,
    clippy::manual_range_contains,
    reason = "tests: small bounded numbers, and a failed expectation should panic"
)]
#![allow(
    clippy::many_single_char_names,
    reason = "pixel and colour maths use x, y, w, h, r, g, b by convention"
)]

//! The effect engine: budget, gate, timing, and what each effect may touch.

use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use russh_bbs_core::terminal_settings::TerminalSettings;

use super::effects::{self, Stage};
use super::{Budget, Fx, Kind, Overlay, allowed};
use crate::settings::Look;
use crate::ui::Ui;
use crate::ui::depth::ColorDepth;

/// Settings that have answered the glyph test (the default is the plain ASCII, no-colour layout).
fn settings(motion: bool, ascii: bool) -> TerminalSettings {
    TerminalSettings::default()
        .with_tier(if ascii {
            russh_bbs_core::icons::Tier::Ascii
        } else {
            russh_bbs_core::icons::Tier::Unicode
        })
        .with_motion(motion)
}

fn look(motion: bool, depth: ColorDepth) -> Look {
    let ui = Ui::standard().unwrap();
    ui.look(&settings(motion, false), Rect::new(0, 0, 80, 24), depth)
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

// The gate.

#[test]
fn motion_is_allowed_only_when_the_look_says_so_and_never_in_the_plain_path() {
    assert!(allowed(look(true, ColorDepth::TrueColor), false));
    assert!(
        !allowed(look(false, ColorDepth::TrueColor), false),
        "motion off"
    );
    assert!(
        !allowed(look(true, ColorDepth::TrueColor), true),
        "accessibility mode"
    );
    assert!(!allowed(look(false, ColorDepth::TrueColor), true));
}

#[test]
fn the_board_and_the_style_can_turn_motion_off() {
    // A board that turns motion off turns it off for everyone.
    let ui = Ui::standard().unwrap();
    let settings = settings(true, false);
    let on = ui.look(&settings, Rect::new(0, 0, 80, 24), ColorDepth::TrueColor);
    assert!(allowed(on, false));
    let off = on.with_board(None, false);
    assert!(!allowed(off, false));
    // The Paper style ships with no motion: a member who wants motion still gets none.
    let paper = ui.theme(russh_bbs_core::terminal_settings::ThemeName::new("paper").unwrap());
    let look = Look::for_theme(
        &settings,
        russh_bbs_core::terminal_settings::LayoutId::Classic,
        paper,
    );
    assert!(!allowed(look, false), "Paper has motion off");
}

#[test]
fn motion_off_starts_nothing_arms_no_timer_and_draws_no_frame() {
    let mut fx = Fx::default();
    let t0 = Instant::now();
    for kind in [Kind::Intro, Kind::Wipe, Kind::Reveal] {
        assert!(!fx.start(kind, t0, false));
        assert_eq!(fx.next_due(), None, "no timer");
        assert!(
            !fx.tick(t0 + Duration::from_secs(60)),
            "no redraw is ever due"
        );
        assert_eq!(fx.frame(t0), None, "the static frame");
        assert!(!fx.finish());
    }
}

// Timing and the budget.

#[test]
fn an_effect_runs_to_its_end_and_then_the_static_frame_returns() {
    let mut fx = Fx::default();
    let t0 = Instant::now();
    assert!(fx.start(Kind::Wipe, t0, true));
    assert_eq!(fx.next_due(), Some(t0 + Budget::default().interval));
    let (kind, t) = fx.frame(t0).unwrap();
    assert_eq!(kind, Kind::Wipe);
    assert!(t < 0.01);
    // Not yet due: no redraw.
    assert!(!fx.tick(t0 + ms(10)));
    // Due: a frame, and the progress has moved on.
    assert!(fx.tick(t0 + ms(90)));
    let (_, t) = fx.frame(t0 + ms(90)).unwrap();
    assert!(t > 0.3 && t < 0.5, "{t}");
    // Past the end: one last redraw (the static frame), then nothing is due.
    assert!(fx.tick(t0 + ms(300)));
    assert_eq!(fx.frame(t0 + ms(300)), None);
    assert_eq!(fx.next_due(), None);
    assert!(!fx.tick(t0 + ms(400)));
}

#[test]
fn a_closed_gate_drops_the_due_frame_but_never_the_static_one() {
    let mut fx = Fx::default();
    let t0 = Instant::now();
    fx.start(Kind::Intro, t0, true);
    // Due, but the link is behind: no redraw, and the clock moves on so the effect is not
    // stretched.
    assert!(!fx.tick_gated(t0 + ms(90), false));
    assert_eq!(
        fx.next_due(),
        Some(t0 + ms(90) + Budget::default().interval)
    );
    assert!(fx.is_running());
    // Open again: the next due frame is drawn.
    assert!(fx.tick_gated(t0 + ms(180), true));
    // The end of the effect asks for the static frame even with the gate closed.
    assert!(fx.tick_gated(t0 + ms(2000), false));
    assert!(!fx.is_running());
    assert_eq!(fx.frame(t0 + ms(2000)), None);
}

#[test]
fn a_late_tick_skips_ahead_and_never_stretches_the_effect() {
    let mut fx = Fx::default();
    let t0 = Instant::now();
    fx.start(Kind::Intro, t0, true);
    // A tick two seconds late (a stalled session): the effect is over.
    assert!(fx.tick(t0 + Duration::from_secs(2)));
    assert_eq!(fx.frame(t0 + Duration::from_secs(2)), None);
}

#[test]
fn a_key_finishes_the_effect_at_once() {
    let mut fx = Fx::default();
    let t0 = Instant::now();
    fx.start(Kind::Intro, t0, true);
    assert!(fx.finish(), "something was running");
    assert_eq!(fx.next_due(), None);
    assert_eq!(fx.frame(t0 + ms(10)), None, "the next frame is static");
    assert!(!fx.finish(), "nothing left to finish");
}

#[test]
fn one_effect_never_draws_more_than_its_frame_cap() {
    let budget = Budget {
        frames_per_effect: 5,
        ..Budget::default()
    };
    let mut fx = Fx::new(budget);
    let t0 = Instant::now();
    fx.start(Kind::Intro, t0, true);
    let mut frames = 0;
    let mut at = t0;
    while fx.frame(at).is_some() {
        frames += 1;
        at += ms(1);
        assert!(frames < 100, "the cap must stop it");
    }
    assert_eq!(frames, 5);
    assert_eq!(fx.next_due(), None, "and it is over");
}

#[test]
fn a_session_cannot_spend_more_than_its_window_budget() {
    let budget = Budget {
        frames_per_window: 40,
        ..Budget::default()
    };
    let mut fx = Fx::new(budget);
    let t0 = Instant::now();
    let mut started = 0;
    // Retrigger an intro again and again within one minute.
    for i in 0..10 {
        let at = t0 + Duration::from_secs(i);
        if fx.start(Kind::Intro, at, true) {
            started += 1;
            let mut t = at;
            while fx.frame(t).is_some() {
                t += budget.interval;
            }
        }
    }
    assert!(started >= 1 && started < 10, "started {started}");
    // Whatever was refused was refused whole: no half effect. And the spent frames fit.
    let spent = fx.spent.len();
    assert!(spent <= 40, "spent {spent}");
    // After the window passes, effects run again.
    assert!(fx.start(Kind::Intro, t0 + Duration::from_secs(300), true));
}

#[test]
fn the_frame_interval_is_slower_than_the_redraw_cap() {
    // D49: the shell redraws at most every 50 ms. An effect frame is one redraw cycle, so the
    // effect's interval must not ask for more than that cap allows.
    assert!(Budget::default().interval >= Duration::from_millis(50));
    // And the longest effect fits its frame cap.
    let budget = Budget::default();
    let frames = Kind::Intro.duration().as_millis() / budget.interval.as_millis() + 1;
    assert!(frames <= u128::from(budget.frames_per_effect));
    assert!(
        Kind::Intro.duration() < Duration::from_secs(2),
        "the intro is under two seconds"
    );
}

// What each effect may touch.

/// A screen-like buffer: text on a panel, bars at top and bottom, and a marker in reserved style
/// at the left of the last row.
fn screen(w: u16, h: u16) -> Buffer {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    let body = Style::new()
        .fg(Color::Rgb(200, 220, 255))
        .bg(Color::Rgb(10, 12, 30));
    for y in 0..h {
        let line = format!(
            "{:<width$}",
            format!("row {y} some text for the screen"),
            width = w as usize
        );
        buf.set_string(0, y, &line, body);
    }
    let marker = Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED);
    buf.set_string(0, h - 1, "UNENCRYPTED", marker);
    buf.set_string(12, h - 1, "status bar hints", Style::new());
    buf
}

fn run(kind: Kind, t: f32, depth: ColorDepth, ascii: bool, before: &Buffer) -> Buffer {
    let (w, h) = (before.area.width, before.area.height);
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let ui = Ui::standard().unwrap();
    let look = ui.look(&settings(true, ascii), Rect::new(0, 0, w, h), depth);
    let done = terminal
        .draw(|frame| {
            *frame.buffer_mut() = before.clone();
            Overlay {
                kind,
                t,
                look,
                title: "Test Board",
            }
            .apply(frame);
        })
        .unwrap();
    done.buffer.clone()
}

#[test]
fn no_effect_ever_touches_the_status_row_or_a_reserved_cell() {
    let before = screen(80, 24);
    for kind in [Kind::Intro, Kind::Wipe, Kind::Reveal] {
        for depth in ColorDepth::ALL {
            for ascii in [false, true] {
                for step in 0..=20 {
                    let t = step as f32 / 20.0 * 0.999;
                    let after = run(kind, t, depth, ascii, &before);
                    for x in 0..80 {
                        assert_eq!(
                            after[(x, 23)],
                            before[(x, 23)],
                            "{kind:?} t={t} {depth:?} ascii={ascii}: status row cell {x} changed"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_reserved_cell_above_the_status_row_is_also_left_alone() {
    let mut before = screen(80, 24);
    let marker = Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED);
    before.set_string(30, 5, "M", marker);
    for kind in [Kind::Intro, Kind::Wipe, Kind::Reveal] {
        for step in 0..=10 {
            let after = run(
                kind,
                step as f32 / 11.0,
                ColorDepth::TrueColor,
                false,
                &before,
            );
            assert_eq!(after[(30, 5)], before[(30, 5)], "{kind:?} step {step}");
        }
    }
}

#[test]
fn effects_never_draw_the_reserved_style_or_the_marker_glyphs() {
    let before = screen(80, 24);
    for kind in [Kind::Intro, Kind::Wipe, Kind::Reveal] {
        for depth in ColorDepth::ALL {
            for step in 0..=20 {
                let after = run(kind, step as f32 / 21.0, depth, false, &before);
                for y in 0..23 {
                    for x in 0..80 {
                        let cell = &after[(x, y)];
                        assert!(
                            !cell.modifier.contains(Modifier::BOLD | Modifier::REVERSED),
                            "{kind:?} drew the reserved style at {x},{y}"
                        );
                        for ch in cell.symbol().chars() {
                            assert!(
                                !matches!(
                                    ch,
                                    '\u{27E6}'
                                        | '\u{27E7}'
                                        | '\u{2299}'
                                        | '\u{229B}'
                                        | '\u{270E}'
                                        | '\u{26A0}'
                                ),
                                "{kind:?} drew {ch:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn at_the_end_every_effect_changes_nothing() {
    let before = screen(80, 24);
    for kind in [Kind::Intro, Kind::Wipe, Kind::Reveal] {
        for depth in ColorDepth::ALL {
            assert_eq!(
                run(kind, 1.0, depth, false, &before),
                before,
                "{kind:?} at t=1 must equal the static screen"
            );
        }
    }
    let stage = Stage::new(Rect::new(0, 0, 80, 24), look(true, ColorDepth::TrueColor));
    for apply in [
        (|b: &mut Buffer, s: &Stage| effects::dissolve(b, s, 1.0)) as fn(&mut Buffer, &Stage),
        |b, s| effects::fade(b, s, 1.0, (0, 0, 0)),
        |b, s| effects::wipe(b, s, 1.0),
        |b, s| effects::typewriter(b, s, 1.0),
    ] {
        let mut buf = before.clone();
        apply(&mut buf, &stage);
        assert_eq!(buf, before);
    }
}

#[test]
fn the_wipe_never_changes_a_symbol() {
    // A client that reads the text mid-wipe still sees the whole screen.
    let before = screen(80, 24);
    for depth in ColorDepth::ALL {
        for step in 0..=20 {
            let after = run(Kind::Wipe, step as f32 / 21.0, depth, false, &before);
            for y in 0..24 {
                for x in 0..80 {
                    assert_eq!(
                        after[(x, y)].symbol(),
                        before[(x, y)].symbol(),
                        "{depth:?} {step} {x},{y}"
                    );
                }
            }
        }
    }
    // It does move colour: partway through, something differs.
    assert_ne!(
        run(Kind::Wipe, 0.4, ColorDepth::TrueColor, false, &before),
        before
    );
}

#[test]
fn the_typewriter_reveals_in_reading_order_with_a_cursor_and_hides_nothing_at_the_end() {
    let before = screen(80, 24);
    let shown = |t: f32| {
        let after = run(Kind::Reveal, t, ColorDepth::TrueColor, false, &before);
        (0..23_u16)
            .flat_map(|y| (0..80_u16).map(move |x| (x, y)))
            .filter(|&(x, y)| after[(x, y)].symbol() != " ")
            .count()
    };
    let (a, b, c) = (shown(0.2), shown(0.5), shown(0.9));
    assert!(a < b && b < c, "{a} {b} {c}");
    let after = run(Kind::Reveal, 0.5, ColorDepth::TrueColor, false, &before);
    assert!(
        (0..23_u16).any(|y| (0..80_u16).any(|x| after[(x, y)].symbol() == "▌")),
        "a cursor at the head"
    );
    let ascii = run(Kind::Reveal, 0.5, ColorDepth::Mono, true, &before);
    assert!(
        (0..23_u16).any(|y| (0..80_u16).any(|x| ascii[(x, y)].symbol() == "_")),
        "an ASCII cursor in the ASCII tier"
    );
}

#[test]
fn the_intro_draws_cells_only_and_dissolves_to_the_static_screen() {
    let before = screen(80, 24);
    for depth in ColorDepth::ALL {
        let mid = run(Kind::Intro, 0.5, depth, false, &before);
        assert_ne!(mid, before, "{depth:?}: the scene is drawn");
        // The scene is gone by the end: at 0.999 almost every cell is the static one.
        let late = run(Kind::Intro, 0.999, depth, false, &before);
        let differing = (0..23_u16)
            .flat_map(|y| (0..80_u16).map(move |x| (x, y)))
            .filter(|&(x, y)| late[(x, y)] != before[(x, y)])
            .count();
        assert!(
            differing < 5,
            "{depth:?}: {differing} cells still differ at the end"
        );
    }
    // The ASCII tier draws ASCII only.
    let ascii = run(Kind::Intro, 0.5, ColorDepth::TrueColor, true, &before);
    for y in 0..23 {
        for x in 0..80 {
            assert!(
                ascii[(x, y)].symbol().is_ascii(),
                "{:?}",
                ascii[(x, y)].symbol()
            );
        }
    }
}

#[test]
fn the_plain_tiers_get_the_board_name_as_readable_text() {
    let before = screen(80, 24);
    // ASCII tier, and a Unicode terminal with no colour: spaced capitals, not a ramp smudge.
    for (ascii, depth) in [(true, ColorDepth::TrueColor), (false, ColorDepth::Mono)] {
        let after = run(Kind::Intro, 0.55, depth, ascii, &before);
        let found = (0..23_u16).any(|y| {
            (0..80_u16)
                .map(|x| after[(x, y)].symbol().to_owned())
                .collect::<String>()
                .contains("T E S T   B O A R D".replace("   ", " ").as_str())
        });
        assert!(found, "ascii={ascii} {depth:?}: the title is readable");
    }
}

#[test]
fn the_intro_is_skipped_on_a_terminal_too_small_for_it() {
    let before = screen(30, 8);
    assert_eq!(
        run(Kind::Intro, 0.5, ColorDepth::TrueColor, false, &before),
        before
    );
}

#[test]
fn a_hostile_board_name_cannot_put_anything_but_font_pixels_on_screen() {
    let before = screen(80, 24);
    for name in [
        "\u{1b}[31mRED\u{7}",
        "\u{27E6}\u{2299}\u{27E7} UNENCRYPTED",
        "日本語のボード",
        "",
        "                       ",
        &"X".repeat(5000),
    ] {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let done = terminal
            .draw(|frame| {
                *frame.buffer_mut() = before.clone();
                Overlay {
                    kind: Kind::Intro,
                    t: 0.5,
                    look: look(true, ColorDepth::TrueColor),
                    title: name,
                }
                .apply(frame);
            })
            .unwrap();
        for y in 0..23 {
            for x in 0..80 {
                for ch in done.buffer[(x, y)].symbol().chars() {
                    assert!(
                        ch == ' ' || ch.is_ascii_graphic() || "▀▄█░▒▓".contains(ch),
                        "{name:?} produced {ch:?}"
                    );
                }
            }
        }
    }
}

// The reusable effects.

#[test]
fn fade_blends_toward_the_target_and_dissolves_below_256_colours() {
    let before = screen(40, 10);
    let stage = Stage::new(Rect::new(0, 0, 40, 10), look(true, ColorDepth::TrueColor));
    let mut half = before.clone();
    effects::fade(&mut half, &stage, 0.5, (0, 0, 0));
    assert_eq!(half[(0, 0)].fg, Color::Rgb(100, 110, 128));
    assert_eq!(half[(0, 0)].bg, Color::Rgb(5, 6, 15));
    let mut none = before.clone();
    effects::fade(&mut none, &stage, 0.0, (0, 0, 0));
    assert_eq!(none[(0, 0)].fg, Color::Rgb(0, 0, 0));
    // Sixteen colours: no blend, a dissolve (symbols vanish, colours stay).
    let stage16 = Stage::new(Rect::new(0, 0, 40, 10), look(true, ColorDepth::Ansi16));
    let mut d = before.clone();
    effects::fade(&mut d, &stage16, 0.5, (0, 0, 0));
    let blank = (0..9_u16)
        .flat_map(|y| (0..40_u16).map(move |x| (x, y)))
        .filter(|&(x, y)| d[(x, y)].symbol() == " ")
        .count();
    assert!(blank > 50 && blank < 330, "{blank} of 360 hidden");
}

#[test]
fn glow_moves_only_backgrounds_and_only_with_enough_colours() {
    let mut buf = screen(10, 3);
    let bg = buf[(2, 1)].bg;
    let fg = buf[(2, 1)].fg;
    effects::glow(
        &mut buf,
        Rect::new(0, 0, 10, 3),
        ColorDepth::TrueColor,
        (255, 200, 0),
        1.0,
    );
    assert_ne!(buf[(2, 1)].bg, bg);
    assert_eq!(buf[(2, 1)].fg, fg);
    assert_eq!(buf[(2, 1)].symbol(), "w", "glyphs are untouched");
    let mut sixteen = screen(10, 3);
    effects::glow(
        &mut sixteen,
        Rect::new(0, 0, 10, 3),
        ColorDepth::Ansi16,
        (255, 200, 0),
        1.0,
    );
    assert_eq!(sixteen, screen(10, 3), "no glow below 256 colours");
}

#[test]
fn gradient_text_runs_from_one_colour_to_the_other() {
    // (Row 1 of this buffer is the marker row, which a gradient must skip; row 0 is text.)
    let mut buf = screen(20, 2);
    let marker_cell = buf[(0, 1)].clone();
    effects::gradient_text(
        &mut buf,
        Rect::new(0, 0, 20, 2),
        ColorDepth::TrueColor,
        (255, 0, 0),
        (0, 0, 255),
    );
    assert_eq!(buf[(0, 0)].fg, Color::Rgb(255, 0, 0));
    // (Column 19 is a space; 18 is the last letter.)
    assert!(
        matches!(buf[(18, 0)].fg, Color::Rgb(r, _, b) if r < 20 && b > 230),
        "{:?}",
        buf[(18, 0)].fg
    );
    assert_eq!(
        buf[(19, 0)].fg,
        Color::Rgb(200, 220, 255),
        "blanks are not recoloured"
    );
    let mid = buf[(10, 0)].fg;
    assert!(
        matches!(mid, Color::Rgb(r, _, b) if r > 100 && b > 100),
        "{mid:?}"
    );
    assert_eq!(
        buf[(0, 1)],
        marker_cell,
        "the marker cell keeps its own style"
    );
}
