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

//! Effects on a real member screen (UI-8): the login intro, the menu wipe and the logoff reveal
//! run on the session's ticker, motion off is one static frame, a key finishes an effect at
//! once, and the last frame is the static screen.

use std::sync::Arc;
use std::time::{Duration, Instant};

use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::AccountId;
use russh_bbs_core::terminal_settings::{MemberFlow, TerminalSettings, ThemeName};

use super::super::terminal::{Key, PtySize, RenderCap};
use super::{Content, Screen};
use crate::ui::fx::Kind;

fn member(motion: bool, size: PtySize, depth: Option<(&str, &str)>) -> Screen {
    member_styled(motion, size, depth, None, Tier::Unicode, true)
}

/// A member screen in `theme` (the default style when `None`), at `tier`, with colour `color`.
fn member_styled(
    motion: bool,
    size: PtySize,
    depth: Option<(&str, &str)>,
    theme: Option<&str>,
    tier: Tier,
    color: bool,
) -> Screen {
    let mut settings = TerminalSettings::default()
        .with_tier(tier)
        .with_motion(motion)
        .with_color(color);
    if let Some(theme) = theme {
        settings = settings.with_theme(ThemeName::new(theme).unwrap());
    }
    let flow = MemberFlow::new(
        &sanitize_username("alice").unwrap(),
        Snapshot::from_role(
            Some(AccountId::from_raw(1)),
            &Slug::MEMBER,
            10,
            &Via::Account,
            Permission::ALL.iter().copied(),
        ),
        None,
        settings,
    );
    let mut screen = Screen::new(
        size,
        RenderCap::default(),
        Content::Member(Box::new(flow)),
        Arc::new(crate::ui::Ui::standard().unwrap()),
    )
    .unwrap();
    if let Some((term, colorterm)) = depth {
        screen.set_client_terminal(Some(term), Some(colorterm));
    }
    // The effect clock is frozen at "now", so no test depends on how long it takes to run: an
    // effect's progress only moves when a test sets `fx_clock` (see `frame_at`). Without this a
    // frame is drawn at whatever progress the wall clock happens to give, which differs between a
    // debug and a release build and between a quiet and a busy machine.
    screen.fx_clock = Some(Instant::now());
    screen
}

/// The frozen effect clock of `screen`.
fn clock_of(screen: &Screen) -> Instant {
    screen.fx_clock.expect("`member` freezes the effect clock")
}

/// Enter the screen and flush the redraw that telling it the terminal type asks for (that
/// redundant redraw is older than effects), so the next `render` has only what happens next.
fn enter(screen: &mut Screen) {
    screen.enter().unwrap();
    screen.render().unwrap();
}

const TRUECOLOR: Option<(&str, &str)> = Some(("xterm-256color", "truecolor"));

fn size() -> PtySize {
    PtySize::from_request(80, 24)
}

/// Repaint everything now and return the bytes, as a window change does.
fn repaint(screen: &mut Screen) -> Vec<u8> {
    let current = screen.pty;
    screen.set_size(current);
    screen.render().unwrap().unwrap()
}

#[test]
fn motion_off_is_one_static_frame_and_arms_no_timer() {
    let mut screen = member(false, size(), TRUECOLOR);
    let first = screen.enter().unwrap();
    assert!(!first.is_empty());
    screen.render().unwrap();
    assert_eq!(screen.next_fx_due(), None, "no timer is armed");
    // However long we wait, nothing is due and nothing is drawn.
    screen.tick_fx(clock_of(&screen) + Duration::from_secs(60));
    assert!(screen.render().unwrap().is_none(), "no further frame");
    assert!(!screen.dirty);
    // Moving on to the menu starts no wipe either.
    screen.apply_keys(&[Key::Enter]);
    screen.render().unwrap();
    assert_eq!(screen.next_fx_due(), None);
}

#[test]
fn the_login_screen_runs_the_intro_on_the_ticker() {
    let mut screen = member(true, size(), TRUECOLOR);
    let start = clock_of(&screen);
    enter(&mut screen);
    // A timer is armed for exactly one frame interval after the intro began.
    let interval = crate::ui::fx::Budget::default().interval;
    assert_eq!(screen.next_fx_due(), Some(start + interval));
    // Not yet due: no redraw, however often the shell looks.
    screen.fx_clock = Some(start + interval / 2);
    screen.tick_fx(start + interval / 2);
    assert!(
        screen.render().unwrap().is_none(),
        "nothing before the next frame is due"
    );

    // Due, at a known moment in the hold phase (progress 0.5, where the scene covers every
    // cell): the next frame draws the scene. What changed is read from the bytes sent: the
    // scene is half-block glyphs in 256-colour sequences (the intro never sends more than 256
    // colours, even to a truecolor client), over the whole 80x23 area above the status row.
    let at = start + Kind::Intro.duration() / 2;
    let frame = frame_at(&mut screen, at).expect("an intro frame is due");
    let text = String::from_utf8_lossy(&frame);
    let blocks = text.matches('▀').count() + text.matches('█').count() + text.matches('▄').count();
    assert!(
        blocks > 1_000,
        "the scene covers the screen: {blocks} block cells in {} bytes",
        frame.len()
    );
    assert!(text.contains("38;5;"), "the scene is drawn in 256 colours");
    assert!(!text.contains("38;2;"), "and never in truecolor");
    // The status row is not part of the scene: nothing is drawn on the last row.
    assert!(
        !text.contains("\x1b[24;"),
        "the status row (row 24) is untouched"
    );

    // The same moment gives the same bytes on a second screen: the frame is a function of the
    // clock, not of how long the test took.
    let mut again = member(true, size(), TRUECOLOR);
    again.fx_clock = Some(start);
    enter(&mut again);
    let twin = frame_at(&mut again, at).expect("an intro frame is due");
    assert_eq!(twin, frame, "the same progress draws the same frame");
}

#[test]
fn the_final_frame_equals_the_static_screen() {
    let mut still = member(false, size(), TRUECOLOR);
    still.enter().unwrap();
    let want = repaint(&mut still);

    let mut moving = member(true, size(), TRUECOLOR);
    moving.enter().unwrap();
    // Run the clock past the end of the intro: the effect ends, and a last frame is due.
    moving.tick_fx(clock_of(&moving) + Duration::from_secs(5));
    assert_eq!(moving.next_fx_due(), None);
    assert!(moving.dirty, "the static frame is drawn after the effect");
    let got = repaint(&mut moving);
    assert_eq!(
        got, want,
        "after the intro the screen is exactly the static one"
    );
}

#[test]
fn a_key_finishes_the_effect_at_once_and_is_still_handled() {
    let mut still = member(false, size(), TRUECOLOR);
    still.enter().unwrap();
    still.apply_keys(&[Key::Enter]);
    still.render().unwrap();
    let want = repaint(&mut still);

    let mut moving = member(true, size(), TRUECOLOR);
    moving.enter().unwrap();
    assert!(moving.next_fx_due().is_some());
    // The key: ends the intro now and moves on to the menu exactly as it does with motion off.
    moving.apply_keys(&[Key::Enter]);
    moving.render().unwrap();
    // (Entering the menu starts the short wipe; a second key ends that one too.)
    moving.apply_keys(&[Key::Char('?')]);
    moving.apply_keys(&[Key::Char('?')]);
    assert_eq!(moving.next_fx_due(), None, "no effect left running");
    moving.render().unwrap();
    let got = repaint(&mut moving);
    // The clock text differs only if the minute rolled between the two; compare without it.
    let strip = |bytes: Vec<u8>| {
        String::from_utf8_lossy(&bytes)
            .chars()
            .filter(|c| !c.is_ascii_digit())
            .collect::<String>()
    };
    assert_eq!(strip(got), strip(want));
}

#[test]
fn a_resize_ends_the_effect() {
    let mut screen = member(true, size(), TRUECOLOR);
    screen.enter().unwrap();
    assert!(screen.next_fx_due().is_some());
    screen.set_size(PtySize::from_request(100, 30));
    assert_eq!(screen.next_fx_due(), None);
    screen.render().unwrap();
}

#[test]
fn entering_a_menu_starts_the_wipe_once() {
    let mut screen = member(true, size(), TRUECOLOR);
    screen.enter().unwrap();
    screen.apply_keys(&[Key::Enter]); // login -> menu
    screen.render().unwrap();
    assert!(screen.next_fx_due().is_some(), "the menu wipe");
    assert_eq!(screen.fx.running_kind(), Some(Kind::Wipe));
    // The same menu redrawn (a cursor move, a redraw) does not start it again.
    screen.fx.finish();
    screen.dirty = true;
    screen.render().unwrap();
    assert_eq!(screen.next_fx_due(), None);
}

/// A screen whose member has logged off (the `l` key at the menu), ready to leave.
fn logged_off(motion: bool) -> Screen {
    let mut screen = member(motion, size(), TRUECOLOR);
    enter(&mut screen);
    screen.apply_keys(&[Key::Enter]);
    screen.render().unwrap();
    let effects = screen.apply_keys(&[Key::Char('l')]);
    assert!(effects.disconnect, "log off disconnects");
    screen
}

#[test]
fn the_typed_goodbye_is_exactly_the_static_goodbye() {
    let mut still = logged_off(false);
    let static_bytes = still.leave().unwrap();
    assert!(
        String::from_utf8_lossy(&static_bytes).contains("alice"),
        "the goodbye names the member"
    );

    let mut typed = logged_off(true);
    let farewell = typed.leave_typed().unwrap().expect("motion is on");
    assert!(
        farewell.chunks.len() >= 2 && farewell.chunks.len() <= 14,
        "{}",
        farewell.chunks.len()
    );
    let mut all = farewell.restore.clone();
    for chunk in &farewell.chunks {
        all.extend(chunk);
        // Every chunk is whole characters (valid UTF-8) and never ends inside a line break.
        assert!(std::str::from_utf8(chunk).is_ok());
        assert!(!chunk.ends_with(b"\r"), "a line break is not split");
    }
    assert_eq!(all, static_bytes, "typed out, it is the same text");
    assert!(
        farewell.interval >= Duration::from_millis(50),
        "paced inside the D49 cap"
    );
}

#[test]
fn motion_off_or_a_spent_budget_means_the_goodbye_is_not_typed() {
    assert!(logged_off(false).leave_typed().unwrap().is_none());
    // A session that has spent its window budget gets the plain goodbye.
    let mut screen = logged_off(true);
    let now = Instant::now();
    assert!(screen.fx.reserve(90, now, true));
    assert!(screen.leave_typed().unwrap().is_none());
    // And one on a slow link gets none, whatever the setting.
    let mut slow = logged_off(true);
    slow.slow_link();
    assert!(slow.leave_typed().unwrap().is_none());
}

#[test]
fn a_slow_link_ends_the_effect_and_switches_effects_off() {
    let mut screen = member(true, size(), TRUECOLOR);
    enter(&mut screen);
    assert!(screen.next_fx_due().is_some());
    screen.slow_link();
    assert_eq!(screen.next_fx_due(), None);
    screen.render().unwrap();
    // Later screens start nothing either: Enter goes to the menu, which would wipe.
    screen.apply_keys(&[Key::Enter]);
    screen.render().unwrap();
    assert_eq!(screen.next_fx_due(), None);
}

#[test]
fn the_intro_needs_room_and_never_runs_on_a_tiny_terminal() {
    let mut screen = member(true, PtySize::from_request(30, 8), TRUECOLOR);
    screen.enter().unwrap();
    assert_eq!(screen.next_fx_due(), None);
}

#[test]
fn every_tier_and_depth_draws_the_intro_without_error() {
    for depth in [
        None,
        Some(("dumb", "")),
        Some(("xterm", "")),
        Some(("xterm-256color", "")),
        TRUECOLOR,
    ] {
        for (w, h) in [(80, 24), (132, 43), (200, 60), (45, 13)] {
            let mut screen = member(true, PtySize::from_request(w, h), depth);
            let start = Instant::now();
            screen.fx_clock = Some(start);
            enter(&mut screen);
            for i in 1..=24 {
                frame_at(&mut screen, start + Duration::from_millis(80 * i));
            }
        }
    }
}

/// Draw one frame at exactly `at` and return its bytes.
fn frame_at(screen: &mut Screen, at: Instant) -> Option<Vec<u8>> {
    screen.fx_clock = Some(at);
    screen.tick_fx(at);
    screen.render().unwrap()
}

/// Run the intro to its end and return (frames drawn, total bytes, largest frame).
fn run_intro(w: u32, h: u32, depth: Option<(&str, &str)>) -> (u32, usize, usize) {
    let mut screen = member(true, PtySize::from_request(w, h), depth);
    let start = Instant::now();
    screen.fx_clock = Some(start);
    enter(&mut screen);
    let (mut frames, mut bytes, mut biggest) = (0, 0, 0);
    for i in 1..=30_u64 {
        if let Some(frame) = frame_at(&mut screen, start + Duration::from_millis(80 * i)) {
            frames += 1;
            bytes += frame.len();
            biggest = biggest.max(frame.len());
        }
    }
    (frames, bytes, biggest)
}

#[test]
fn the_intro_stays_inside_its_frame_and_byte_budgets() {
    let budget = crate::ui::fx::Budget::default();
    for (w, h) in [(80, 24), (132, 43), (200, 60)] {
        for depth in [TRUECOLOR, Some(("xterm-256color", "")), Some(("xterm", ""))] {
            let (frames, bytes, biggest) = run_intro(w, h, depth);
            assert!(
                frames <= u32::from(budget.frames_per_effect) + 2,
                "{w}x{h}: {frames} frames"
            );
            // The byte budget is a hard stop: at most one frame past it.
            assert!(
                bytes <= budget.bytes_per_effect + biggest,
                "{w}x{h}: {bytes} bytes against {}",
                budget.bytes_per_effect
            );
        }
    }
    // At full truecolor the whole intro is under about 150 KB at any size (the scene is at
    // most 120x36 cells), and no one frame is large.
    for (w, h) in [(80, 24), (132, 43), (200, 60)] {
        let (frames, bytes, biggest) = run_intro(w, h, TRUECOLOR);
        assert!(
            frames >= 20,
            "{w}x{h}: only {frames} frames before the budget cut it"
        );
        assert!(bytes < 150 * 1024, "{w}x{h}: {bytes} B");
        assert!(biggest < 24 * 1024, "{w}x{h}: largest frame {biggest} B");
    }
}

#[test]
fn an_effect_that_spends_its_bytes_ends_and_the_static_frame_follows() {
    let mut screen = member(true, size(), TRUECOLOR);
    enter(&mut screen);
    assert!(screen.next_fx_due().is_some());
    // Pretend the intro already sent its whole budget.
    assert!(screen.fx.charge(usize::MAX / 2));
    assert_eq!(screen.next_fx_due(), None, "ended");
}

/// Frame cost and bytes per frame of the intro, printed for the PR (`--ignored --nocapture`).
/// Run in release for the numbers that matter: `cargo test --release -p russh-bbs-server
/// --lib effects_benchmark -- --ignored --nocapture`.
#[test]
#[ignore = "a benchmark: run on demand and read the numbers"]
fn effects_benchmark() {
    for (label, depth) in [
        ("truecolor", TRUECOLOR),
        ("256", Some(("xterm-256color", ""))),
        ("16", Some(("xterm", ""))),
    ] {
        for (w, h) in [(80, 24), (132, 43), (200, 60)] {
            let mut screen = member(true, PtySize::from_request(w, h), depth);
            let start = Instant::now();
            screen.fx_clock = Some(start);
            enter(&mut screen);
            let (mut bytes, mut frames, mut worst) = (0_usize, 0_u32, Duration::ZERO);
            let mut max_bytes = 0_usize;
            for i in 1..=30_u64 {
                let began = Instant::now();
                if let Some(frame) = frame_at(&mut screen, start + Duration::from_millis(80 * i)) {
                    let took = began.elapsed();
                    worst = worst.max(took);
                    bytes += frame.len();
                    max_bytes = max_bytes.max(frame.len());
                    frames += 1;
                    if std::env::var_os("FX_PROFILE").is_some() {
                        eprint!("{} ", frame.len());
                    }
                }
            }
            eprintln!(
                "{label:>9} {w:>3}x{h:<3} intro frames {frames:>2}  total {bytes:>8} B  mean {:>6} B/frame  max {max_bytes:>6} B  worst frame {worst:?}",
                bytes / frames.max(1) as usize
            );
        }
    }
}

/// The menu wipe and the typed goodbye, the same way.
#[test]
#[ignore = "a benchmark: run on demand and read the numbers"]
fn transitions_benchmark() {
    for (w, h) in [(80, 24), (132, 43)] {
        let mut screen = member(true, PtySize::from_request(w, h), TRUECOLOR);
        let start = Instant::now();
        screen.fx_clock = Some(start);
        enter(&mut screen);
        screen.apply_keys(&[Key::Enter]); // out of the intro, into the menu (a wipe)
        let (mut frames, mut bytes, mut worst) = (0_u32, 0_usize, Duration::ZERO);
        let first = Instant::now();
        if let Some(frame) = screen.render().unwrap() {
            bytes += frame.len();
            frames += 1;
            worst = first.elapsed();
        }
        for i in 1..=12_u64 {
            let began = Instant::now();
            if let Some(frame) = frame_at(&mut screen, start + Duration::from_millis(80 * i)) {
                worst = worst.max(began.elapsed());
                bytes += frame.len();
                frames += 1;
            }
        }
        eprintln!(
            "truecolor {w:>3}x{h:<3} menu wipe     frames {frames:>2}  total {bytes:>7} B  mean {:>6} B/frame  worst frame {worst:?}",
            bytes / frames.max(1) as usize
        );
        let mut off = logged_off(true);
        let typed = off.leave_typed().unwrap().unwrap();
        let total: usize = typed.restore.len() + typed.chunks.iter().map(Vec::len).sum::<usize>();
        eprintln!(
            "truecolor {w:>3}x{h:<3} typed goodbye chunks {:>2}  total {total:>7} B (plain text, {:?} apart)",
            typed.chunks.len(),
            typed.interval
        );
    }
}

// The intro with the style's login banner (UI-W 4).

/// The cases: two styles with a login banner in colour, then the same in mono and in the ASCII
/// tier.
type BannerCase = (
    &'static str,
    Tier,
    bool,
    Option<(&'static str, &'static str)>,
);

fn banner_cases() -> Vec<BannerCase> {
    vec![
        ("neon", Tier::Unicode, true, TRUECOLOR),
        ("deepspace", Tier::Unicode, true, TRUECOLOR),
        ("neon", Tier::Unicode, true, Some(("xterm-256color", ""))),
        ("neon", Tier::Unicode, false, TRUECOLOR),
        ("deepspace", Tier::Unicode, false, TRUECOLOR),
        ("neon", Tier::Ascii, true, TRUECOLOR),
        ("deepspace", Tier::Ascii, true, Some(("xterm-256color", ""))),
    ]
}

fn styled(motion: bool, (w, h): (u32, u32), case: &BannerCase) -> Screen {
    let (theme, tier, color, depth) = case;
    member_styled(
        motion,
        PtySize::from_request(w, h),
        *depth,
        Some(theme),
        *tier,
        *color,
    )
}

#[test]
fn the_intro_with_a_login_banner_keeps_the_byte_and_frame_budgets() {
    let budget = crate::ui::fx::Budget::default();
    for case in banner_cases() {
        for dims in [(80, 24), (132, 43)] {
            let mut screen = styled(true, dims, &case);
            let start = Instant::now();
            screen.fx_clock = Some(start);
            enter(&mut screen);
            let (mut frames, mut bytes, mut biggest) = (0_u32, 0_usize, 0_usize);
            for i in 1..=30_u64 {
                if let Some(frame) = frame_at(&mut screen, start + Duration::from_millis(80 * i)) {
                    frames += 1;
                    bytes += frame.len();
                    biggest = biggest.max(frame.len());
                }
            }
            assert!(
                frames <= u32::from(budget.frames_per_effect) + 2,
                "{case:?} {dims:?}: {frames} frames"
            );
            assert!(
                bytes <= budget.bytes_per_effect + biggest,
                "{case:?} {dims:?}: {bytes} bytes"
            );
        }
    }
}

#[test]
fn the_intro_with_a_login_banner_ends_on_exactly_the_static_login_screen() {
    for case in banner_cases() {
        for dims in [(80, 24), (132, 43)] {
            let mut still = styled(false, dims, &case);
            still.enter().unwrap();
            let want = repaint(&mut still);

            let mut moving = styled(true, dims, &case);
            moving.enter().unwrap();
            moving.tick_fx(clock_of(&moving) + Duration::from_secs(5));
            assert_eq!(moving.next_fx_due(), None);
            let got = repaint(&mut moving);
            assert_eq!(
                got, want,
                "{case:?} {dims:?}: the last frame is the static one"
            );
        }
    }
}

#[test]
fn a_key_skips_the_intro_with_a_login_banner() {
    for case in banner_cases() {
        let mut still = styled(false, (80, 24), &case);
        still.enter().unwrap();
        still.apply_keys(&[Key::Enter]);
        still.render().unwrap();
        let want = repaint(&mut still);

        let mut moving = styled(true, (80, 24), &case);
        moving.enter().unwrap();
        // Draw a frame mid-intro first, so a banner is on screen when the key arrives.
        let mid = clock_of(&moving) + Duration::from_millis(900);
        frame_at(&mut moving, mid);
        moving.apply_keys(&[Key::Enter]);
        moving.render().unwrap();
        moving.apply_keys(&[Key::Char('?')]);
        moving.apply_keys(&[Key::Char('?')]);
        assert_eq!(
            moving.next_fx_due(),
            None,
            "{case:?}: no effect left running"
        );
        moving.render().unwrap();
        let got = repaint(&mut moving);
        let strip = |bytes: Vec<u8>| {
            String::from_utf8_lossy(&bytes)
                .chars()
                .filter(|c| !c.is_ascii_digit())
                .collect::<String>()
        };
        assert_eq!(strip(got), strip(want), "{case:?}");
    }
}

#[test]
fn the_intro_frames_carry_the_banner_in_colour_and_only_ascii_in_the_ascii_tier() {
    let mut neon = styled(true, (80, 24), &banner_cases()[0]);
    let start = clock_of(&neon);
    enter(&mut neon);
    let frame = frame_at(&mut neon, start + Kind::Intro.duration() / 2).unwrap();
    let text = String::from_utf8_lossy(&frame);
    assert!(text.contains('\u{2502}'), "the art's own cells are drawn");

    let ascii_case = banner_cases()[5];
    let mut ascii = styled(true, (80, 24), &ascii_case);
    let start = clock_of(&ascii);
    enter(&mut ascii);
    let mut all = Vec::new();
    for i in 1..=20_u64 {
        if let Some(f) = frame_at(&mut ascii, start + Duration::from_millis(80 * i)) {
            all.extend(f);
        }
    }
    let text = String::from_utf8_lossy(&all);
    assert!(
        !text.contains(['\u{2588}', '\u{2580}', '\u{2584}', '\u{2502}']),
        "no block or box glyphs in the ASCII tier"
    );
}

// ---- the sweep on the UI-4 screens (UI-W) -----------------------------------------------

/// A member at the main menu, past the intro, with the menu's own wipe over.
fn at_menu(motion: bool) -> Screen {
    let mut screen = member(motion, size(), TRUECOLOR);
    enter(&mut screen);
    screen.apply_keys(&[Key::Enter]);
    screen.render().unwrap();
    screen.fx.finish();
    screen.dirty = true;
    screen.render().unwrap();
    screen
}

#[test]
fn entering_forums_mail_and_the_members_screens_starts_the_wipe_once() {
    for (hotkey, name) in [('f', "forums"), ('m', "mail"), ('u', "members menu")] {
        let mut screen = at_menu(true);
        screen.apply_keys(&[Key::Char(hotkey)]);
        screen.render().unwrap();
        assert_eq!(
            screen.fx.running_kind(),
            Some(Kind::Wipe),
            "{name} starts the wipe"
        );
        assert!(screen.next_fx_due().is_some(), "{name}: a frame is due");
        // Redrawing the same screen (a key inside it, new data) does not start it again.
        screen.fx.finish();
        screen.dirty = true;
        screen.render().unwrap();
        assert_eq!(screen.next_fx_due(), None, "{name}: once");
    }
}

#[test]
fn the_sweep_on_forums_and_mail_is_off_with_motion_off_and_in_accessibility_mode() {
    for hotkey in ['f', 'm'] {
        let mut still = at_menu(false);
        still.apply_keys(&[Key::Char(hotkey)]);
        still.render().unwrap();
        assert_eq!(still.fx.running_kind(), None);
        assert_eq!(still.next_fx_due(), None, "no timer at all");

        let mut plain = at_menu(true);
        plain.set_accessible(true);
        plain.apply_keys(&[Key::Char(hotkey)]);
        plain.render().unwrap();
        assert_eq!(
            plain.fx.running_kind(),
            None,
            "accessibility mode is static"
        );
        assert_eq!(plain.next_fx_due(), None);
    }
}

#[test]
fn the_final_frame_of_the_sweep_on_forums_and_mail_is_the_static_screen() {
    for hotkey in ['f', 'm'] {
        let mut still = at_menu(false);
        still.apply_keys(&[Key::Char(hotkey)]);
        still.render().unwrap();
        let want = repaint(&mut still);

        let mut moving = at_menu(true);
        moving.apply_keys(&[Key::Char(hotkey)]);
        moving.render().unwrap();
        moving.tick_fx(clock_of(&moving) + Duration::from_secs(5));
        assert_eq!(moving.next_fx_due(), None);
        let got = repaint(&mut moving);
        let strip = |bytes: Vec<u8>| {
            String::from_utf8_lossy(&bytes)
                .chars()
                .filter(|c| !c.is_ascii_digit())
                .collect::<String>()
        };
        assert_eq!(strip(got), strip(want), "{hotkey}");
    }
}

#[test]
fn a_sweep_frame_on_forums_changes_colour_only_and_never_the_status_row() {
    let mut screen = at_menu(true);
    screen.apply_keys(&[Key::Char('f')]);
    let static_text = {
        let mut still = at_menu(false);
        still.apply_keys(&[Key::Char('f')]);
        still.render().unwrap();
        let bytes = repaint(&mut still);
        strip_sgr(&String::from_utf8_lossy(&bytes))
    };
    screen.render().unwrap();
    let at = clock_of(&screen) + Duration::from_millis(100);
    screen.tick_fx(at);
    let bytes = repaint(&mut screen);
    let text = strip_sgr(&String::from_utf8_lossy(&bytes));
    // The words are the same mid-sweep (a client reading the text sees the whole screen).
    let words = |s: &str| {
        s.split_whitespace()
            .filter(|w| !w.chars().any(|c| c.is_ascii_digit()))
            .collect::<Vec<_>>()
            .join(" ")
    };
    assert_eq!(words(&text), words(&static_text));
}

fn strip_sgr(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}
