#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::too_many_lines,
    clippy::type_complexity
)]

//! The widget system without a store: the sysop's settings, the drawing helpers, every widget in
//! every layout cell at 80x24 and 132x43 in mono and in the ASCII tier, hostile data, the refresh
//! budgets and the Widgets screen. What the widgets are *allowed* to read (a member without access
//! to a board sees nothing from it) is tested against the real stores in `russh-bbs-store`'s
//! shared suite and in `russh_bbs_core::widgets`.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::store::{AccountId, UnixSeconds};
use russh_bbs_core::terminal_settings::{LayoutId, TerminalSettings};
use russh_bbs_core::widgets::{Caller, Choice, Headline, Line, Lock, Rules, WidgetData, WidgetId};

use super::fixture::{self, NOW};
use super::screen::{self, Ctx, Outcome, State, WidgetsScreen};
use super::{
    Cx, MIN_REFRESH, WidgetSettings, assign, draw_cells, figures, refresh_interval, widget,
};
use crate::config::{BoardConfig, WidgetsConfig};
use crate::settings::Look;
use crate::ssh::terminal::Key;
use crate::ui::board::Board;
use crate::ui::depth::ColorDepth;
use crate::ui::layout_tests::ui;
use crate::ui::panes;

// ---- the sysop's settings -----------------------------------------------------------------

fn config(edit: impl FnOnce(&mut WidgetsConfig)) -> Result<WidgetSettings, String> {
    let mut c = WidgetsConfig::default();
    edit(&mut c);
    WidgetSettings::from_config(&c).map_err(|e| e.to_string())
}

#[test]
fn the_default_config_changes_nothing() {
    let s = config(|_| {}).unwrap();
    assert!(s.motd().is_empty() && !s.has_quotes());
    assert_eq!(s.rules(), &Rules::default());
}

#[test]
fn lists_and_text_are_read_and_checked() {
    let s = config(|c| {
        c.hidden = "wall".into();
        c.on = " motd , clock".into();
        c.off = "quote".into();
        c.dashboard = "clock,mail".into();
        c.motd = "Back at nine.".into();
        c.quotes = "One | Two |  ".into();
    })
    .unwrap();
    assert_eq!(s.rules().lock(WidgetId::Wall), Lock::Hidden);
    assert_eq!(s.rules().lock(WidgetId::Motd), Lock::On);
    assert_eq!(s.rules().lock(WidgetId::Quote), Lock::Off);
    assert_eq!(
        s.rules().default_for(LayoutId::Dashboard),
        vec![WidgetId::Clock, WidgetId::Mail]
    );
    assert_eq!(s.motd().as_str(), "Back at nine.");
    // One quote a day, in turn.
    assert_eq!(s.quote(0).unwrap().as_str(), "One");
    assert_eq!(s.quote(1).unwrap().as_str(), "Two");
    assert_eq!(s.quote(-1).unwrap().as_str(), "Two");
}

#[test]
fn bad_settings_are_refused_naming_the_key() {
    let cases: Vec<(&str, Box<dyn FnOnce(&mut WidgetsConfig)>)> = vec![
        ("widgets.hidden", Box::new(|c| c.hidden = "nope".into())),
        ("widgets.on", Box::new(|c| c.on = "clock,clock".into())),
        (
            "widgets.on",
            Box::new(|c| {
                c.on = "clock".into();
                c.off = "clock".into();
            }),
        ),
        (
            "widgets.wide",
            Box::new(|c| {
                c.wide = WidgetId::ALL
                    .iter()
                    .map(|w| w.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
            }),
        ),
        ("widgets.motd", Box::new(|c| c.motd = "x".repeat(241))),
        (
            "widgets.motd",
            Box::new(|c| c.motd = "\u{1b}[31mred".into()),
        ),
        (
            "widgets.motd",
            Box::new(|c| c.motd = "a \u{27e6}\u{2299}\u{27e7} b".into()),
        ),
        ("widgets.motd", Box::new(|c| c.motd = "tab\tbed".into())),
        (
            "widgets.motd",
            Box::new(|c| c.motd = "A private, encrypted board".into()),
        ),
        (
            "widgets.quotes",
            Box::new(|c| c.quotes = "ok|\u{e0b0}".into()),
        ),
        (
            "widgets.quotes",
            Box::new(|c| {
                c.quotes = (0..65)
                    .map(|n| format!("q{n}"))
                    .collect::<Vec<_>>()
                    .join("|");
            }),
        ),
    ];
    for (key, edit) in cases {
        let error = config(edit).unwrap_err();
        assert!(error.starts_with(key), "{key}: {error}");
    }
}

// ---- drawing helpers ------------------------------------------------------------------------

#[test]
fn dates_are_right() {
    let d = figures::date_of(NOW);
    assert_eq!((d.year, d.month, d.day, d.weekday), (2026, 11, 5, 3));
    let epoch = figures::date_of(0);
    assert_eq!(
        (epoch.year, epoch.month, epoch.day, epoch.weekday),
        (1970, 1, 1, 3)
    );
    let leap = figures::date_of(1_709_164_800); // 2024-02-29 00:00 UTC, a Thursday
    assert_eq!(
        (leap.year, leap.month, leap.day, leap.weekday),
        (2024, 2, 29, 3)
    );
    let before = figures::date_of(-1);
    assert_eq!((before.year, before.month, before.day), (1969, 12, 31));
    assert_eq!(figures::days_in_month(2024, 2), 29);
    assert_eq!(figures::days_in_month(2100, 2), 28);
    assert_eq!(figures::days_in_month(2000, 2), 29);
    let weeks = figures::month_weeks(d);
    // November 2026 starts on a Sunday: the first week has one day, in the last column.
    assert_eq!(weeks[0], [None, None, None, None, None, None, Some(1)]);
    assert_eq!(weeks.iter().flatten().flatten().count(), 30);
    assert!(weeks.iter().any(|w| w.contains(&Some(5))));
}

#[test]
fn big_digits_are_three_rows_and_ascii_safe() {
    for ascii in [false, true] {
        let rows = figures::big_digits("12:09", ascii);
        let width = rows[0].chars().count();
        assert!(rows.iter().all(|r| r.chars().count() == width));
        assert_eq!(width, 4 + 4 + 2 + 4 + 4);
        assert_eq!(rows.iter().all(|r| r.is_ascii()), ascii);
    }
    // Eight lights every segment; one lights two uprights.
    assert_eq!(figures::big_digits("8", true), [" _  ", "|_| ", "|_| "]);
    assert_eq!(figures::big_digits("1", true), ["    ", "  | ", "  | "]);
}

#[test]
fn figures_are_short_and_exact() {
    assert_eq!(figures::ago(100, 100), None);
    assert_eq!(figures::ago(100, 41), None);
    assert_eq!(figures::ago(1000, 700), Some((figures::Unit::Minute, 5)));
    assert_eq!(figures::ago(10_000, 0), Some((figures::Unit::Hour, 2)));
    assert_eq!(figures::ago(300_000, 0), Some((figures::Unit::Day, 3)));
    assert_eq!(figures::ago(0, 100), None, "a time in the future is now");
    assert_eq!(figures::bytes(812), "812B");
    assert_eq!(figures::bytes(3_500), "3.4K");
    assert_eq!(figures::bytes(12 * 1024 * 1024), "12M");
    assert_eq!(figures::span(59), "0m");
    assert_eq!(figures::span(3 * 3600 + 5 * 60), "3h 05m");
    assert_eq!(figures::span(2 * 86_400 + 4 * 3600), "2d 4h");
}

#[test]
fn clip_and_wrap_never_exceed_the_width() {
    assert_eq!(figures::clip("hello", 5, false), "hello");
    assert_eq!(figures::clip("hello world", 6, true), "hello~");
    assert_eq!(figures::clip("hello world", 6, false), "hello\u{2026}");
    assert_eq!(figures::clip("x", 0, false), "");
    for ascii in [false, true] {
        for width in 1..30 {
            let text = "the quick brown fox jumps over the lazy dog and keeps running far away";
            for line in figures::wrap(text, width, 3, ascii) {
                assert!(line.chars().count() <= width, "{width}: {line:?}");
                assert_eq!(line.is_ascii(), ascii || line.is_ascii());
            }
            assert!(figures::wrap(text, width, 3, ascii).len() <= 3);
        }
    }
    assert_eq!(figures::wrap("a b c", 10, 5, false), vec!["a b c"]);
    assert!(figures::wrap("a b c d e f g", 3, 1, true)[0].ends_with('~'));
    assert!(figures::wrap("", 5, 2, false).is_empty());
    assert_eq!(figures::wrap("superlongword", 4, 2, false), vec!["supe"]);
}

// ---- the widgets in the layouts -------------------------------------------------------------

fn board() -> Board {
    Board::from_config(&BoardConfig {
        location: "Portland, OR".into(),
        since: "2026".into(),
        ..BoardConfig::default()
    })
    .unwrap()
}

struct Setup {
    data: WidgetData,
    settings: WidgetSettings,
    board: Board,
}

impl Setup {
    fn full() -> Self {
        Self {
            data: fixture::data(),
            settings: fixture::settings(),
            board: board(),
        }
    }

    fn with(data: WidgetData) -> Self {
        Self {
            data,
            ..Self::full()
        }
    }
}

fn look(tier: Tier, color: bool, depth: ColorDepth, layout: LayoutId) -> Look {
    let ui = ui();
    let settings = TerminalSettings::default()
        .with_tier(tier)
        .with_color(color)
        .with_layout(layout);
    Look::for_theme(&settings, layout, ui.theme(settings_theme())).at_depth(depth)
}

fn settings_theme() -> russh_bbs_core::terminal_settings::ThemeName {
    russh_bbs_core::terminal_settings::ThemeName::new("neon").unwrap()
}

/// Draw `ids` into the cells `layout` makes of a `w`x`h` screen's body (less a header row and a
/// two-row status bar, which the widgets are never given), returning the buffer and the cells.
fn draw(
    layout: LayoutId,
    size: (u16, u16),
    tier: Tier,
    color: bool,
    depth: ColorDepth,
    setup: &Setup,
    ids: &[WidgetId],
) -> (Buffer, Vec<(WidgetId, Rect)>, Rect) {
    let ui = ui();
    let theme = ui.theme(settings_theme());
    let look = look(tier, color, depth, layout);
    let body = Rect::new(0, 1, size.0, size.1 - 3);
    let plan = panes::plan(layout, body);
    let mut placed = Vec::new();
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| {
            let cx = Cx::new(
                look,
                tier,
                theme.strings(),
                &setup.data,
                &setup.board,
                &setup.settings,
                NOW,
            );
            placed = assign(&plan.cells, ids, &cx);
            draw_cells(frame, &placed, &cx);
        })
        .unwrap();
    (terminal.backend().buffer().clone(), placed, body)
}

const SIZES: [(u16, u16); 2] = [(80, 24), (132, 43)];
const PANE_LAYOUTS: [LayoutId; 3] = [LayoutId::Dashboard, LayoutId::Wide, LayoutId::Split];

fn symbols(buffer: &Buffer) -> String {
    buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}

#[test]
fn every_widget_in_every_layout_slot_at_both_sizes_in_every_tier() {
    let setup = Setup::full();
    for id in WidgetId::ALL {
        for layout in PANE_LAYOUTS {
            for size in SIZES {
                for (tier, color, depth) in [
                    (Tier::Unicode, true, ColorDepth::TrueColor),
                    (Tier::Unicode, false, ColorDepth::Mono),
                    (Tier::Ascii, false, ColorDepth::Mono),
                    (Tier::Ascii, true, ColorDepth::Ansi16),
                ] {
                    // One widget alone fills the first cell it fits, so each is tried in every
                    // cell the layout offers by listing it first.
                    let (buffer, placed, body) =
                        draw(layout, size, tier, color, depth, &setup, &[id]);
                    assert_eq!(
                        placed.len(),
                        1,
                        "{id:?} {layout:?} {size:?}: it has content and a cell it fits"
                    );
                    let cell = placed[0].1;
                    // The widget is inside the body: never in the header row, the status bar or
                    // the marker cell (its row is the last one).
                    assert!(
                        cell.x >= body.x
                            && cell.y >= body.y
                            && cell.x + cell.width <= body.x + body.width
                            && cell.y + cell.height <= body.y + body.height,
                        "{id:?} {layout:?} {size:?}: {cell:?} leaves {body:?}"
                    );
                    for y in 0..size.1 {
                        for x in 0..size.0 {
                            let c = &buffer[(x, y)];
                            let inside = x >= cell.x
                                && x < cell.x + cell.width
                                && y >= cell.y
                                && y < cell.y + cell.height;
                            if !inside {
                                assert_eq!(
                                    c.symbol(),
                                    " ",
                                    "{id:?} {layout:?} {size:?} wrote outside its cell at {x},{y}"
                                );
                            }
                            assert!(
                                !c.modifier.contains(Modifier::REVERSED),
                                "{id:?}: reverse video is the marker's"
                            );
                            if color && depth != ColorDepth::Mono {
                                continue;
                            }
                            assert!(
                                matches!(c.fg, Color::Reset) && matches!(c.bg, Color::Reset),
                                "{id:?} {layout:?} {tier:?} mono: colour at {x},{y}: {c:?}"
                            );
                        }
                    }
                    if tier == Tier::Ascii {
                        assert!(
                            symbols(&buffer).is_ascii(),
                            "{id:?} {layout:?} {size:?} drew non-ASCII in the ASCII tier:\n{}",
                            symbols(&buffer)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn the_widgets_show_their_data() {
    let setup = Setup::full();
    let (buffer, placed, _) = draw(
        LayoutId::Dashboard,
        (132, 43),
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        &setup,
        &[
            WidgetId::Online,
            WidgetId::Callers,
            WidgetId::Mail,
            WidgetId::Poll,
        ],
    );
    assert_eq!(placed.len(), 4);
    let text = symbols(&buffer);
    for want in [
        "5 online",
        "alice",
        "bob",
        "3 unread",
        "12 in your inbox",
        "Best terminal font?",
        "Mono",
    ] {
        assert!(text.contains(want), "missing {want:?}");
    }
    // The vote is marked, and the percentages add up.
    assert!(text.contains('\u{25b8}'));
    assert!(
        text.contains("54%") || text.contains("55%"),
        "12 of 22 votes"
    );
}

#[test]
fn a_widget_with_nothing_to_show_collapses() {
    // Only the static ones have content without data: identity, clock, and (set in the fixture)
    // the message of the day and the quote.
    let empty = Setup::with(WidgetData::default());
    let cx = |setup: &Setup| {
        let ui: &'static crate::ui::Ui = Box::leak(Box::new(ui()));
        Cx::new(
            look(
                Tier::Unicode,
                true,
                ColorDepth::TrueColor,
                LayoutId::Dashboard,
            ),
            Tier::Unicode,
            ui.theme(settings_theme()).strings(),
            // Leaked for the test's sake: the context borrows for its whole life.
            Box::leak(Box::new(setup.data.clone())),
            Box::leak(Box::new(setup.board.clone())),
            Box::leak(Box::new(setup.settings.clone())),
            NOW,
        )
    };
    let with: Vec<WidgetId> = WidgetId::ALL
        .into_iter()
        .filter(|id| widget(*id).has_content(&cx(&empty)))
        .collect();
    assert_eq!(
        with,
        vec![
            WidgetId::Identity,
            WidgetId::Clock,
            WidgetId::Motd,
            WidgetId::Quote
        ]
    );
    // Without the sysop's text those two collapse as well.
    let bare = Setup {
        settings: WidgetSettings::default(),
        ..Setup::with(WidgetData::default())
    };
    assert!(!widget(WidgetId::Motd).has_content(&cx(&bare)));
    assert!(!widget(WidgetId::Quote).has_content(&cx(&bare)));
    // A collapsed widget takes no cell: the next one moves up.
    let (_, placed, _) = draw(
        LayoutId::Dashboard,
        (80, 24),
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        &empty,
        &[WidgetId::Mail, WidgetId::Online, WidgetId::Clock],
    );
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].0, WidgetId::Clock);
    assert_eq!(
        placed[0].1,
        panes::plan(LayoutId::Dashboard, Rect::new(0, 1, 80, 21)).cells[0]
    );
}

#[test]
fn a_widget_too_big_for_a_cell_is_left_out_not_squeezed() {
    let setup = Setup::full();
    // A very short dashboard: cells of two rows cannot hold a panel with the poll's three.
    let ui = ui();
    let theme = ui.theme(settings_theme());
    let look = look(
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        LayoutId::Dashboard,
    );
    let cx = Cx::new(
        look,
        Tier::Unicode,
        theme.strings(),
        &setup.data,
        &setup.board,
        &setup.settings,
        NOW,
    );
    let tiny = [Rect::new(0, 0, 20, 3), Rect::new(0, 3, 20, 3)];
    assert!(assign(&tiny, &[WidgetId::Poll, WidgetId::Status], &cx).is_empty());
    let roomy = [Rect::new(0, 0, 30, 9)];
    assert_eq!(assign(&roomy, &[WidgetId::Poll], &cx).len(), 1);
}

#[test]
fn widgets_fill_cells_in_the_members_order() {
    let setup = Setup::full();
    let order = [
        WidgetId::Mail,
        WidgetId::Online,
        WidgetId::Clock,
        WidgetId::Callers,
    ];
    let (_, placed, _) = draw(
        LayoutId::Dashboard,
        (132, 43),
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        &setup,
        &order,
    );
    assert_eq!(placed.iter().map(|(id, _)| *id).collect::<Vec<_>>(), order);
    // Reading order: each cell comes after the one before it.
    let cells: Vec<Rect> = placed.iter().map(|(_, r)| *r).collect();
    assert!(
        cells
            .windows(2)
            .all(|w| (w[0].y, w[0].x) < (w[1].y, w[1].x))
    );
}

// ---- hostile data ---------------------------------------------------------------------------

#[test]
fn member_text_in_widgets_is_cleaned_in_every_tier() {
    let hostile = [
        "\u{1b}[31mred\u{1b}]0;title\u{7}",
        "\u{27e6}\u{2299}\u{27e7} UNENCRYPTED",
        "nerd \u{f07c}\u{e0b0} glyph",
        "bell\u{7}tab\ttab\u{0}nul\u{8}back",
        "zero\u{200b}width\u{202e}rtl",
        "wide \u{ff21}\u{ff22} and \u{1f600}",
    ];
    let mut data = WidgetData {
        online: Some(hostile.iter().map(|s| (*s).to_owned()).collect()),
        ..WidgetData::default()
    };
    data.callers = Some(
        hostile
            .iter()
            .map(|s| Caller {
                handle: (*s).to_owned(),
                at: UnixSeconds::from_raw(NOW - 60),
            })
            .collect(),
    );
    let line = |s: &str| Line {
        author: s.to_owned(),
        text: s.to_owned(),
        at: UnixSeconds::from_raw(NOW - 60),
    };
    data.oneliners = Some(hostile.iter().map(|s| line(s)).collect());
    data.wall = data.oneliners.clone();
    data.news = Some(
        hostile
            .iter()
            .map(|s| Headline {
                board: (*s).to_owned(),
                subject: (*s).to_owned(),
                at: UnixSeconds::from_raw(NOW - 60),
            })
            .collect(),
    );
    data.poll = Some(russh_bbs_core::widgets::PollView {
        question: hostile[0].to_owned(),
        options: hostile.iter().map(|s| ((*s).to_owned(), Some(1))).collect(),
        my_vote: Some(1),
    });
    data.new_posts = Some(russh_bbs_core::widgets::NewPosts {
        threads: 9,
        boards: hostile.iter().map(|s| ((*s).to_owned(), 2)).collect(),
    });
    data.chat = Some(
        hostile
            .iter()
            .map(|s| russh_bbs_core::widgets::RoomCount {
                name: (*s).to_owned(),
                members: 2,
                here: vec![(*s).to_owned()],
            })
            .collect(),
    );
    data.doors = Some(
        hostile
            .iter()
            .map(|s| russh_bbs_core::widgets::DoorLine {
                name: (*s).to_owned(),
                season: 1,
            })
            .collect(),
    );
    let setup = Setup::with(data);
    let ids = [
        WidgetId::Online,
        WidgetId::Callers,
        WidgetId::Oneliner,
        WidgetId::Wall,
        WidgetId::News,
        WidgetId::Poll,
        WidgetId::NewPosts,
        WidgetId::Chat,
        WidgetId::Doors,
    ];
    for layout in PANE_LAYOUTS {
        for size in SIZES {
            for (tier, color, depth) in [
                (Tier::Unicode, true, ColorDepth::TrueColor),
                (Tier::Ascii, false, ColorDepth::Mono),
            ] {
                for id in ids {
                    let (buffer, placed, _) = draw(layout, size, tier, color, depth, &setup, &[id]);
                    assert_eq!(placed.len(), 1, "{id:?}");
                    for cell in &buffer.content {
                        for ch in cell.symbol().chars() {
                            assert!(
                                !ch.is_control()
                                    && !('\u{e000}'..='\u{f8ff}').contains(&ch)
                                    && ![
                                        '\u{27e6}', '\u{27e7}', '\u{2299}', '\u{229b}', '\u{270e}',
                                        '\u{26a0}'
                                    ]
                                    .contains(&ch)
                                    && !['\u{200b}', '\u{202e}'].contains(&ch),
                                "{id:?} {layout:?} {tier:?} drew {ch:?}"
                            );
                        }
                        assert!(!cell.modifier.contains(Modifier::REVERSED));
                    }
                    if tier == Tier::Ascii {
                        assert!(symbols(&buffer).is_ascii(), "{id:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn enormous_data_stays_inside_its_cell() {
    let mut data = fixture::data();
    let long = "w".repeat(5_000);
    data.oneliners = Some(vec![
        Line {
            author: long.clone(),
            text: long.clone(),
            at: UnixSeconds::from_raw(NOW),
        };
        8
    ]);
    data.online = Some(vec![long.clone(); 500]);
    data.activity = Some(vec![u64::MAX; 500]);
    data.credits = Some(russh_bbs_core::widgets::Credits {
        balance: u64::MAX,
        series: vec![u64::MAX; 500],
    });
    let setup = Setup::with(data);
    for id in [
        WidgetId::Oneliner,
        WidgetId::Online,
        WidgetId::Activity,
        WidgetId::Credits,
    ] {
        for size in SIZES {
            let (buffer, placed, _) = draw(
                LayoutId::Wide,
                size,
                Tier::Unicode,
                true,
                ColorDepth::TrueColor,
                &setup,
                &[id],
            );
            assert_eq!(placed.len(), 1, "{id:?}");
            let cell = placed[0].1;
            for y in 0..size.1 {
                for x in 0..size.0 {
                    let outside = x < cell.x
                        || x >= cell.x + cell.width
                        || y < cell.y
                        || y >= cell.y + cell.height;
                    if outside {
                        assert_eq!(buffer[(x, y)].symbol(), " ", "{id:?} at {x},{y}");
                    }
                }
            }
        }
    }
}

// ---- refresh budgets ------------------------------------------------------------------------

#[test]
fn refresh_budgets_stay_far_under_the_redraw_cap() {
    // D49: at most 20 redraws a second (50 ms). Widgets refresh on a timer no faster than
    // MIN_REFRESH, so the timer adds at most one redraw per five seconds.
    assert!(MIN_REFRESH.as_secs() >= 5);
    for id in WidgetId::ALL {
        let w = widget(id);
        assert_eq!(w.id(), id);
        assert!(w.budget().as_millis() >= 5_000, "{id:?}: {:?}", w.budget());
        let (min_w, min_h) = w.min_size();
        let (pref_w, pref_h) = w.preferred();
        assert!(min_w <= pref_w && min_h <= pref_h, "{id:?}");
        assert!(min_w >= 4 && min_h >= 1, "{id:?}");
        assert_eq!(
            refresh_interval(&[id]),
            Some(w.budget().max(MIN_REFRESH)),
            "{id:?}"
        );
    }
    // The interval for a screen is the smallest budget among its widgets.
    let all = refresh_interval(&WidgetId::ALL).unwrap();
    assert_eq!(all, MIN_REFRESH, "status asks for five seconds");
    assert_eq!(
        refresh_interval(&[WidgetId::Clock, WidgetId::Motd])
            .unwrap()
            .as_secs(),
        60
    );
    // No widgets, no timer.
    assert_eq!(refresh_interval(&[]), None);
    let per_second = 1.0 / all.as_secs_f64();
    assert!(
        per_second <= 20.0 / 100.0,
        "{per_second} redraws a second from widgets"
    );
}

#[test]
fn every_widget_title_and_label_is_in_the_table() {
    let ui = ui();
    for id in WidgetId::ALL {
        let key = widget(id).title_key();
        assert!(
            ui.strings().text(key).len() > 1,
            "{key} is missing from the string table"
        );
        assert_eq!(key, format!("ui.widget.title.{}", id.as_str()));
    }
    for key in super::TEXT_KEYS.iter().chain(screen::TEXT_KEYS) {
        assert!(ui.strings().keys().any(|k| k == *key), "{key}");
    }
}

#[test]
fn the_text_form_is_ascii_and_matches_the_content() {
    let setup = Setup::full();
    let ui = ui();
    let look = look(
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        LayoutId::Dashboard,
    );
    let cx = Cx::new(
        look,
        Tier::Unicode,
        ui.strings(),
        &setup.data,
        &setup.board,
        &setup.settings,
        NOW,
    );
    for id in WidgetId::ALL {
        let text = widget(id).text(&cx, 30);
        assert!(!text.is_empty(), "{id:?} has a text form");
        for line in &text {
            assert!(line.is_ascii(), "{id:?}: {line:?}");
            assert!(line.chars().count() <= 60, "{id:?}: {line:?}");
        }
    }
}

// ---- the Widgets screen ---------------------------------------------------------------------

fn member() -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::parse("member").unwrap(),
        10,
        &Via::Account,
        [
            Permission::MembersView,
            Permission::CommunityView,
            Permission::MailRead,
            Permission::AccountEditSettings,
            Permission::ForumList,
        ],
    )
}

fn press(
    screen: &mut WidgetsScreen,
    keys: &[Key],
    rules: &Rules,
    choice: &mut Choice,
    access: &Snapshot,
) -> Vec<(LayoutId, Option<Vec<WidgetId>>)> {
    let mut saved = Vec::new();
    for key in keys {
        let ctx = Ctx {
            rules,
            choice,
            access,
        };
        match screen.press(*key, &ctx) {
            Outcome::Stay | Outcome::Back => {}
            Outcome::Edit {
                choice: next,
                layout,
                list,
            } => {
                *choice = next;
                saved.push((layout, list));
            }
        }
    }
    saved
}

#[test]
fn the_screen_toggles_moves_and_resets_within_the_sysops_locks() {
    let access = member();
    let rules = WidgetSettings::from_config(&WidgetsConfig {
        on: "motd".into(),
        off: "wall".into(),
        hidden: "quote".into(),
        ..WidgetsConfig::default()
    })
    .unwrap();
    let rules = rules.rules();
    let mut choice = Choice::new();
    let mut s = WidgetsScreen::open(LayoutId::Dashboard);
    let rows = screen::rows(
        LayoutId::Dashboard,
        &Ctx {
            rules,
            choice: &choice,
            access: &access,
        },
    );
    // The hidden widget is not listed; the locked ones show their lock; the member cannot reach
    // chat, status, doors or news.
    let ids: Vec<WidgetId> = rows.iter().map(|r| r.id).collect();
    assert!(
        !ids.contains(&WidgetId::Quote)
            && !ids.contains(&WidgetId::Status)
            && !ids.contains(&WidgetId::Chat)
    );
    assert!(
        rows.iter()
            .any(|r| r.id == WidgetId::Motd && r.state == State::LockedOn)
    );
    assert!(
        rows.iter()
            .any(|r| r.id == WidgetId::Wall && r.state == State::LockedOff)
    );
    // Shown widgets come first, in order, then the rest.
    let first_off = rows
        .iter()
        .position(|r| matches!(r.state, State::Off | State::LockedOff))
        .unwrap();
    assert!(
        rows[..first_off]
            .iter()
            .all(|r| matches!(r.state, State::On | State::LockedOn))
    );

    // Switch the first widget off with Space: the list is saved as shown, less that widget.
    let before = rules.resolve(LayoutId::Dashboard, &choice, &access);
    let saved = press(&mut s, &[Key::Char(' ')], rules, &mut choice, &access);
    assert_eq!(saved.len(), 1);
    let after = rules.resolve(LayoutId::Dashboard, &choice, &access);
    assert!(
        !after.contains(&before[0])
            && after.len() == before.len() - 1 + usize::from(!before.contains(&WidgetId::Motd))
    );
    assert_eq!(saved[0].1.as_deref(), choice.get(LayoutId::Dashboard));
    // And back on: it goes to the end of the list (the locked-on widget is in it too).
    let rows = screen::rows(
        LayoutId::Dashboard,
        &Ctx {
            rules,
            choice: &choice,
            access: &access,
        },
    );
    let at = rows.iter().position(|r| r.id == before[0]).unwrap();
    assert!(
        matches!(rows[at].state, State::Off),
        "it is listed as off now"
    );
    let mut to_it = vec![Key::Up; 40];
    to_it.extend(vec![Key::Down; at]);
    to_it.push(Key::Char(' '));
    press(&mut s, &to_it, rules, &mut choice, &access);
    assert_eq!(
        rules.resolve(LayoutId::Dashboard, &choice, &access).last(),
        Some(&before[0])
    );

    // Reorder: move the first down once and up again.
    let mut s = WidgetsScreen::open(LayoutId::Dashboard);
    let list = rules.resolve(LayoutId::Dashboard, &choice, &access);
    press(&mut s, &[Key::Char('d')], rules, &mut choice, &access);
    let moved = rules.resolve(LayoutId::Dashboard, &choice, &access);
    assert_eq!((moved[0], moved[1]), (list[1], list[0]));
    assert_eq!(s.cursor(), 1, "the cursor follows the widget");
    press(&mut s, &[Key::Char('u')], rules, &mut choice, &access);
    assert_eq!(rules.resolve(LayoutId::Dashboard, &choice, &access), list);

    // A locked widget cannot be switched, a locked-on one can be moved.
    let mut s = WidgetsScreen::open(LayoutId::Dashboard);
    let rows = screen::rows(
        LayoutId::Dashboard,
        &Ctx {
            rules,
            choice: &choice,
            access: &access,
        },
    );
    let motd_at = rows.iter().position(|r| r.id == WidgetId::Motd).unwrap();
    let wall_at = rows.iter().position(|r| r.id == WidgetId::Wall).unwrap();
    let down = |n: usize| vec![Key::Down; n];
    let mut keys = down(motd_at);
    keys.push(Key::Char(' '));
    assert!(
        press(&mut s, &keys, rules, &mut choice, &access).is_empty(),
        "locked on: no change"
    );
    assert!(
        rules
            .resolve(LayoutId::Dashboard, &choice, &access)
            .contains(&WidgetId::Motd)
    );
    let mut keys = vec![Key::Char('d')];
    assert_eq!(
        press(&mut s, &keys, rules, &mut choice, &access).len(),
        1,
        "locked on can move"
    );
    keys = down(wall_at.saturating_sub(motd_at + 1));
    keys.push(Key::Char(' '));
    let before = choice.clone();
    press(&mut s, &keys, rules, &mut choice, &access);
    assert!(
        !rules
            .resolve(LayoutId::Dashboard, &choice, &access)
            .contains(&WidgetId::Wall),
        "locked off stays off"
    );
    let _ = before;

    // Reset follows the board's default again and stores nothing.
    let saved = press(&mut s, &[Key::Char('r')], rules, &mut choice, &access);
    assert_eq!(saved, vec![(LayoutId::Dashboard, None)]);
    assert!(choice.get(LayoutId::Dashboard).is_none());
    assert!(
        press(&mut s, &[Key::Char('r')], rules, &mut choice, &access).is_empty(),
        "nothing to reset"
    );

    // Tab and the arrows cycle only the layouts that draw widgets; the classic family is never
    // offered, and Esc leaves.
    let mut s = WidgetsScreen::open(LayoutId::Classic);
    assert_eq!(s.layout(), LayoutId::Dashboard);
    let mut seen = Vec::new();
    for _ in 0..4 {
        press(&mut s, &[Key::Ctrl('i')], rules, &mut choice, &access);
        seen.push(s.layout());
    }
    assert_eq!(
        seen,
        [
            LayoutId::Wide,
            LayoutId::Focus,
            LayoutId::Split,
            LayoutId::Dashboard
        ]
    );
    press(&mut s, &[Key::Left], rules, &mut choice, &access);
    assert_eq!(s.layout(), LayoutId::Split);
    let ctx = Ctx {
        rules,
        choice: &choice,
        access: &access,
    };
    assert_eq!(s.press(Key::Escape, &ctx), Outcome::Back);
}

#[test]
fn a_lock_the_sysop_adds_later_wins_over_a_saved_list() {
    let access = member();
    let mut choice = Choice::new();
    let free = Rules::default();
    let mut s = WidgetsScreen::open(LayoutId::Dashboard);
    // The member switches the clock off while nothing is locked.
    let rows = screen::rows(
        LayoutId::Dashboard,
        &Ctx {
            rules: &free,
            choice: &choice,
            access: &access,
        },
    );
    let clock = rows.iter().position(|r| r.id == WidgetId::Clock).unwrap();
    let mut keys = vec![Key::Down; clock];
    keys.push(Key::Char(' '));
    press(&mut s, &keys, &free, &mut choice, &access);
    assert!(
        !free
            .resolve(LayoutId::Dashboard, &choice, &access)
            .contains(&WidgetId::Clock)
    );
    // The sysop then locks it on: the saved list loses.
    let locked = WidgetSettings::from_config(&WidgetsConfig {
        on: "clock".into(),
        ..WidgetsConfig::default()
    })
    .unwrap();
    assert!(
        locked
            .rules()
            .resolve(LayoutId::Dashboard, &choice, &access)
            .contains(&WidgetId::Clock)
    );
}

fn render_screen(
    size: (u16, u16),
    tier: Tier,
    color: bool,
    depth: ColorDepth,
    layout: LayoutId,
    cursor_keys: &[Key],
) -> Buffer {
    let ui = ui();
    let theme = ui.theme(settings_theme());
    let look = look(tier, color, depth, layout);
    let setup = Setup::full();
    let access = member();
    let rules = Rules::default();
    let choice = Choice::new();
    let mut s = WidgetsScreen::open(layout);
    let mut c = choice.clone();
    press(&mut s, cursor_keys, &rules, &mut c, &access);
    let marker = [russh_bbs_core::icons::SecurityMarker::ServerReadable];
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| {
            screen::render(
                frame,
                &s,
                &screen::DrawIn {
                    strings: theme.strings(),
                    look,
                    tier,
                    markers: &marker,
                    clock: Some("UTC 12:00"),
                    ctx: Ctx {
                        rules: &rules,
                        choice: &c,
                        access: &access,
                    },
                    data: &setup.data,
                    board: &setup.board,
                    settings: &setup.settings,
                    now: NOW,
                },
            );
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn the_widgets_screen_previews_the_layout_at_every_size_and_tier() {
    for layout in [
        LayoutId::Dashboard,
        LayoutId::Wide,
        LayoutId::Focus,
        LayoutId::Split,
    ] {
        for size in SIZES {
            for (tier, color, depth) in [
                (Tier::Unicode, true, ColorDepth::TrueColor),
                (Tier::Unicode, false, ColorDepth::Mono),
                (Tier::Ascii, false, ColorDepth::Mono),
            ] {
                let buffer = render_screen(size, tier, color, depth, layout, &[]);
                let text = symbols(&buffer);
                assert!(
                    text.contains("Widgets") && text.contains("Preview"),
                    "{layout:?} {size:?}"
                );
                // The list names the widgets on offer.
                assert!(
                    text.contains("Clock") && text.contains("Who is online"),
                    "{layout:?}:\n{text}"
                );
                if layout == LayoutId::Focus {
                    assert!(text.contains("draws no widgets") || text.contains("Clock"));
                }
                // Reverse video only in the marker cell: the status bar's first cell.
                let last = size.1 - 1;
                for y in 0..size.1 {
                    for x in 0..size.0 {
                        let reversed = buffer[(x, y)].modifier.contains(Modifier::REVERSED);
                        assert_eq!(
                            reversed,
                            y == last && x < marker_width(&buffer, last),
                            "{layout:?} {size:?} at {x},{y}"
                        );
                    }
                }
                if tier == Tier::Ascii {
                    assert!(text.is_ascii(), "{layout:?} {size:?} ASCII tier:\n{text}");
                }
            }
        }
    }
}

/// How many cells at the start of the last row are the marker (reverse video).
fn marker_width(buffer: &Buffer, row: u16) -> u16 {
    (0..buffer.area.width)
        .take_while(|x| buffer[(*x, row)].modifier.contains(Modifier::REVERSED))
        .count()
        .try_into()
        .unwrap()
}

#[test]
fn the_preview_changes_when_the_member_changes_the_list() {
    let before = symbols(&render_screen(
        (132, 43),
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        LayoutId::Dashboard,
        &[],
    ));
    // Switch the first widget (identity) off: its panel title leaves the preview.
    let after = symbols(&render_screen(
        (132, 43),
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        LayoutId::Dashboard,
        &[Key::Char(' ')],
    ));
    assert_ne!(before, after);
    assert!(
        before.matches("This board").count() > after.matches("This board").count(),
        "the identity panel is gone from the preview"
    );
}

#[test]
fn a_waiting_room_key_is_shown_no_widget_and_nothing_is_fetched() {
    let nobody = Snapshot::empty();
    for layout in LayoutId::ALL {
        assert!(
            Rules::default()
                .resolve(layout, &Choice::new(), &nobody)
                .is_empty()
        );
    }
    assert!(Rules::default().offered(&nobody).is_empty());
}
