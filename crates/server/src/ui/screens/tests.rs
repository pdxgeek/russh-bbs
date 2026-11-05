#![allow(clippy::unwrap_used, clippy::expect_used)]

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
fn motion_off_means_no_timer_at_all() {
    let off = look_with_motion(false);
    let mut state = Motion::default();
    let now = Instant::now();
    state.set_input(true, now);
    assert_eq!(state.deadline(off, now), None);
    assert!(!state.cursor_dark(off, now + Duration::from_millis(600)));
}

#[test]
fn accessibility_mode_stops_the_blink_and_its_timer() {
    let look = look_with_motion(true);
    let mut state = Motion::default();
    let now = Instant::now();
    state.set_input(true, now);
    assert!(state.deadline(look, now).is_some());
    state.set_accessible(true);
    assert!(state.accessible());
    assert_eq!(state.deadline(look, now), None);
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

/// D49: the shell draws at most every 50 ms. The blink asks for far fewer redraws than that.
#[test]
fn motion_stays_under_the_redraw_cap() {
    let cap = crate::ssh::limits::Limits::default().redraw_interval;
    assert!(
        motion::BLINK >= cap * 10,
        "the cursor blinks far below the cap"
    );
    // The effect engine's own frames are at least a cap apart as well (its tests cover it).
    assert!(crate::ui::fx::Budget::default().interval >= cap);
}

// ---- the home screen: the menu with its live widgets, in every layout, size and mode ----

#[test]
fn the_home_screen_in_every_layout_size_and_mode() {
    use crate::ui::classic::MenuScreen;
    use crate::ui::layout::{effective, render_menu};
    use crate::ui::layout_tests::page;
    use crate::ui::widgets::{WidgetView, fixture};

    let page = page(&[]);
    let board = crate::ui::styles_tests::board();
    let data = fixture::data();
    let settings = fixture::settings();
    super::matrix::golden("home", &|frame, d| {
        let layout = effective(d.layout, frame.area());
        let shown = russh_bbs_core::widgets::Rules::default().resolve_with(
            layout,
            &russh_bbs_core::widgets::Choice::new(),
            |_| true,
        );
        render_menu(
            frame,
            layout,
            &MenuScreen {
                strings: d.strings,
                page: &page,
                body: &["Signed in as alice.".to_owned()],
                notice: None,
                look: d.look,
                tier: d.tier,
                markers: d.markers,
                clock: d.clock,
                mail_over_ssh_only: false,
                board: &board,
                wanted: d.layout,
                popover: false,
                art: None,
                widgets: WidgetView {
                    shown: &shown,
                    data: &data,
                    settings: &settings,
                    now: fixture::NOW,
                },
            },
        );
    });
}

// ---- the split layout's panes and the collapsed dashboard draw only what the data holds ----

mod panes {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::style::Modifier;
    use russh_bbs_core::icons::Tier;
    use russh_bbs_core::terminal_settings::{LayoutId, TerminalSettings, ThemeName};
    use russh_bbs_core::widgets::{MailCount, NewPosts, RecentMail, RoomCount, WidgetData};

    use crate::settings::Look;
    use crate::ui::classic::MenuScreen;
    use crate::ui::depth::ColorDepth;
    use crate::ui::layout::{effective, render_menu};
    use crate::ui::layout_tests::{DRAWN, page, text, ui};
    use crate::ui::widgets::{WidgetView, fixture};

    fn draw(layout: LayoutId, size: (u16, u16), data: &WidgetData) -> ratatui::buffer::Buffer {
        let ui = ui();
        let theme = ui.theme(ThemeName::new("default").unwrap());
        let settings = TerminalSettings::default()
            .with_tier(Tier::Unicode)
            .with_color(false)
            .with_layout(layout);
        let area = Rect::new(0, 0, size.0, size.1);
        let chosen = layout;
        let layout = effective(layout, area);
        let look = Look::for_theme(&settings, layout, theme).at_depth(ColorDepth::Mono);
        let page = page(&[]);
        let board = crate::ui::styles_tests::board();
        let widget_settings = fixture::settings();
        let shown = russh_bbs_core::widgets::Rules::default().resolve_with(
            layout,
            &russh_bbs_core::widgets::Choice::new(),
            |_| true,
        );
        let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
        terminal
            .draw(|frame| {
                render_menu(
                    frame,
                    layout,
                    &MenuScreen {
                        strings: theme.strings(),
                        page: &page,
                        body: &[],
                        notice: None,
                        look,
                        tier: Tier::Unicode,
                        markers: &DRAWN,
                        clock: Some("UTC 12:00"),
                        mail_over_ssh_only: false,
                        board: &board,
                        wanted: chosen,
                        popover: false,
                        art: None,
                        widgets: WidgetView {
                            shown: &shown,
                            data,
                            settings: &widget_settings,
                            now: fixture::NOW,
                        },
                    },
                );
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn only_mail() -> WidgetData {
        WidgetData {
            mail: Some(MailCount {
                total: 4,
                unread: 2,
            }),
            recent_mail: Some(vec![RecentMail {
                from: Some("ann".into()),
                subject: Some("Hello".into()),
                unread: true,
                at: russh_bbs_core::store::UnixSeconds::from_raw(fixture::NOW - 60),
            }]),
            ..WidgetData::default()
        }
    }

    fn only_chat() -> WidgetData {
        WidgetData {
            chat: Some(vec![RoomCount {
                name: "lobby".into(),
                members: 1,
                here: vec!["bob".into()],
            }]),
            ..WidgetData::default()
        }
    }

    /// A member who may not reach mail or chat gets neither pane, and no figure of theirs.
    #[test]
    fn the_split_second_pane_shows_only_what_the_member_may_reach() {
        let nothing = text(&draw(LayoutId::Split, (132, 43), &WidgetData::default()));
        assert!(
            !nothing.contains("Mail ─") && !nothing.contains("Chat ─"),
            "{nothing}"
        );
        assert!(
            !nothing.contains("unread") && !nothing.contains("#lobby"),
            "{nothing}"
        );
        let mail = text(&draw(LayoutId::Split, (132, 43), &only_mail()));
        assert!(
            mail.contains("2 unread of 4") && mail.contains("Hello"),
            "{mail}"
        );
        assert!(
            !mail.contains("Chat ─") && !mail.contains("#lobby"),
            "{mail}"
        );
        let chat = text(&draw(LayoutId::Split, (132, 43), &only_chat()));
        assert!(chat.contains("#lobby") && chat.contains("bob"), "{chat}");
        assert!(
            !chat.contains("Mail ─") && !chat.contains("unread"),
            "{chat}"
        );
        let both = text(&draw(LayoutId::Split, (132, 43), &fixture::data()));
        assert!(both.contains("Chat ─") && both.contains("Mail ─"), "{both}");
        assert!(
            both.contains("Welcome aboard") && both.contains("carol"),
            "{both}"
        );
    }

    /// Below the height for two panes the second pane is the widget stack again.
    #[test]
    fn a_short_split_falls_back_to_the_widget_cells() {
        let shown = text(&draw(LayoutId::Split, (80, 16), &fixture::data()));
        assert!(!shown.contains("unread of 12"), "no mail pane: {shown}");
        assert!(
            shown.contains("Who is online"),
            "the widget cells instead: {shown}"
        );
    }

    /// A dashboard too big for the window collapses into one line of figures after the fallback
    /// note, with only the figures the data holds.
    #[test]
    fn the_dashboard_collapses_to_a_line_and_leaks_nothing() {
        let line = |data: &WidgetData| {
            text(&draw(LayoutId::Dashboard, (60, 20), data))
                .lines()
                .find(|l| l.contains("unread") || l.contains(" online") || l.contains(" new"))
                .map(str::to_owned)
        };
        let all = line(&fixture::data()).expect("a small dashboard shows its figures in a line");
        assert!(
            all.contains("online") && all.contains("unread") && all.contains("new"),
            "{all}"
        );
        let mail = line(&only_mail()).unwrap();
        assert!(mail.contains("2 unread"), "{mail}");
        assert!(!mail.contains("online") && !mail.contains("new"), "{mail}");
        assert!(
            line(&WidgetData::default()).is_none(),
            "nothing to say, nothing drawn"
        );
        let posts = WidgetData {
            new_posts: Some(NewPosts {
                threads: 3,
                boards: vec![],
            }),
            ..WidgetData::default()
        };
        let posts = line(&posts).unwrap();
        assert!(
            posts.contains("3 new") && !posts.contains("unread"),
            "{posts}"
        );
    }

    /// Handles and subjects from other people are cleaned, never styled as a marker.
    #[test]
    fn hostile_names_and_subjects_stay_in_their_panes() {
        let hostile =
            "\u{1b}[7mSERVER-READABLE\u{1b}[0m \u{27e6}\u{2297}\u{27e7} \u{e0b0} \u{202e}evil";
        let data = WidgetData {
            mail: Some(MailCount {
                total: 1,
                unread: 1,
            }),
            recent_mail: Some(vec![RecentMail {
                from: Some(hostile.into()),
                subject: Some(hostile.repeat(40)),
                unread: true,
                at: russh_bbs_core::store::UnixSeconds::from_raw(fixture::NOW),
            }]),
            chat: Some(vec![RoomCount {
                name: "lobby".into(),
                members: 1,
                here: vec![hostile.into()],
            }]),
            ..WidgetData::default()
        };
        for size in [(80, 24), (132, 43)] {
            let buffer = draw(LayoutId::Split, size, &data);
            let last = buffer.area.height - 1;
            for y in 0..buffer.area.height {
                for x in 0..buffer.area.width {
                    let cell = &buffer[(x, y)];
                    let symbol = cell.symbol();
                    assert!(
                        !symbol.contains('\u{1b}')
                            && !symbol.contains('\u{202e}')
                            && !symbol.contains('\u{e0b0}'),
                        "{size:?}: a control or private-use character was drawn at ({x},{y})"
                    );
                    if y < last {
                        assert!(
                            !cell.modifier.contains(Modifier::REVERSED),
                            "{size:?}: reverse video in a pane at ({x},{y})"
                        );
                    }
                }
            }
        }
    }
}

/// FIX-M35-7: the shell's tick runs when the deadline has passed, and `deadline` is then already
/// the *next* beat. A redraw is due when the half drawn is not the half now, not when the next
/// deadline has passed (which it never has).
#[test]
fn a_tick_after_the_deadline_finds_the_blink_due_once() {
    let look = look_with_motion(true);
    let mut state = Motion::default();
    let start = Instant::now();
    state.set_input(true, start);
    state.shown(state.cursor_dark(look, start));
    assert!(!state.blink_due(look, start + Duration::from_millis(100)));
    let woken = start + motion::BLINK;
    assert!(
        state.deadline(look, woken).unwrap() > woken,
        "the deadline is never due"
    );
    assert!(state.blink_due(look, woken), "the dark half is due");
    state.shown(state.cursor_dark(look, woken));
    assert!(!state.blink_due(look, woken), "drawn: nothing more is due");
    assert!(
        state.blink_due(look, start + motion::BLINK * 2),
        "lit again"
    );
}

#[test]
fn no_blink_is_due_with_motion_off_or_no_input() {
    let off = look_with_motion(false);
    let on = look_with_motion(true);
    let start = Instant::now();
    let mut state = Motion::default();
    state.set_input(true, start);
    assert!(!state.blink_due(off, start + motion::BLINK));
    state.set_input(false, start);
    assert!(!state.blink_due(on, start + motion::BLINK));
    state.set_input(true, start);
    state.set_accessible(true);
    assert!(!state.blink_due(on, start + motion::BLINK));
}
