#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::type_complexity,
    clippy::panic,
    clippy::assert_is_empty
)]

//! The styles and layouts as a product (UI-2): six complete styles, readable at every depth, in
//! mono and in the ASCII tier, with the security marker untouched by any of them.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::terminal_settings::{LayoutId, TerminalSettings, ThemeName};

use super::classic::MenuScreen;
use super::depth::{ColorDepth, Tint, named_rgb};
use super::layout::{self, render_menu};
use super::layout_tests::{DRAWN, hostile_body, marker_cell_width, page, text, ui};
use super::theme::{Backdrop, Theme, ThemeError, contrast_ratio};
use super::{Board, Ui};
use crate::config::BoardConfig;
use crate::settings::Look;

/// The six styles of the first slice, in the order the docs list them.
pub(super) const STYLES: [&str; 6] = ["phosphor", "neon", "paper", "amber", "deepspace", "ansi"];

pub(super) fn style<'a>(ui: &'a Ui, name: &str) -> &'a Theme {
    ui.theme(ThemeName::new(name).unwrap())
}

/// A board with every optional line set, so the identity panel has something to show.
pub(super) fn board() -> Board {
    Board::from_config(&BoardConfig {
        location: "Portland, OR".into(),
        since: "2026".into(),
        ..BoardConfig::default()
    })
    .unwrap()
}

/// Draw the main menu in `chosen` (after the size fallback) at `depth`.
#[allow(clippy::too_many_arguments)]
pub(super) fn shot(
    markers: &[SecurityMarker],
    chosen: LayoutId,
    theme: &Theme,
    body: &[String],
    tier: Tier,
    color: bool,
    depth: ColorDepth,
    size: (u16, u16),
) -> Buffer {
    let settings = TerminalSettings::default()
        .with_tier(tier)
        .with_color(color)
        .with_layout(chosen);
    let area = Rect::new(0, 0, size.0, size.1);
    let layout = layout::effective(chosen, area);
    let look = Look::for_theme(&settings, layout, theme).at_depth(depth);
    let page = page(&[]);
    let board = board();
    let widget_data = crate::ui::widgets::fixture::data();
    let widget_settings = crate::ui::widgets::fixture::settings();
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
                    body,
                    notice: None,
                    look,
                    tier,
                    markers,
                    clock: Some("UTC 12:00"),
                    mail_over_ssh_only: false,
                    board: &board,
                    wanted: chosen,
                    popover: false,
                    art: None,
                    widgets: crate::ui::widgets::WidgetView {
                        shown: &shown,
                        data: &widget_data,
                        settings: &widget_settings,
                        now: crate::ui::widgets::fixture::NOW,
                    },
                },
            );
            super::backdrop::finish(frame, look);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn plain_body() -> Vec<String> {
    vec!["Signed in as alice.".to_owned()]
}

#[test]
fn six_complete_styles_ship_and_each_is_its_own_look() {
    let ui = ui();
    for name in STYLES {
        assert!(
            ui.theme_names().iter().any(|n| n.as_str() == name),
            "{name}"
        );
    }
    // Pairwise different palettes AND different drawings: not a recolour of one look.
    for (i, a) in STYLES.iter().enumerate() {
        for b in &STYLES[i + 1..] {
            let (ta, tb) = (style(&ui, a), style(&ui, b));
            assert_ne!(ta.palette(), tb.palette(), "{a} vs {b}");
            let (sa, sb) = (ta.skin(), tb.skin());
            let differing = [
                sa.frame != sb.frame,
                sa.popup != sb.popup,
                sa.title != sb.title,
                sa.deco != sb.deco,
                sa.backdrop != sb.backdrop,
                sa.gradient != sb.gradient,
                sa.align != sb.align,
            ]
            .into_iter()
            .filter(|d| *d)
            .count();
            assert!(
                differing >= 2,
                "{a} and {b} differ in only {differing} drawing choice(s)"
            );
            let shot = |t: &Theme| {
                text(&shot(
                    &[],
                    LayoutId::Lightbar,
                    t,
                    &plain_body(),
                    Tier::Unicode,
                    true,
                    ColorDepth::TrueColor,
                    (80, 24),
                ))
            };
            assert_ne!(shot(ta), shot(tb), "{a} and {b} draw the same characters");
        }
    }
}

#[test]
fn each_style_has_the_decoration_the_docs_promise() {
    let ui = ui();
    let skin = |n: &str| style(&ui, n).skin();
    assert_eq!(skin("phosphor").backdrop, Backdrop::Scanlines);
    assert!(skin("neon").gradient && skin("deepspace").gradient);
    assert_eq!(skin("deepspace").backdrop, Backdrop::Stars);
    assert!(!skin("paper").motion, "paper has no motion");
    assert_eq!(skin("ansi").frame, super::theme::BorderSet::Double);
    assert_eq!(skin("amber").frame, super::theme::BorderSet::Thick);
    for name in ["ansi", "paper"] {
        let colors = style(&ui, name).palette();
        let all = [colors.heading, colors.border, colors.panel, colors.surface];
        if name == "ansi" {
            assert!(
                all.iter().all(|t| matches!(t, Tint::Named(_))),
                "classic ANSI is sixteen colours only"
            );
        }
    }
}

/// Resolve `tint` at `depth` to the RGB the terminal would show.
fn shown(tint: Tint, depth: ColorDepth) -> Option<(u8, u8, u8)> {
    match tint.resolve(depth)? {
        Color::Reset => None,
        color => Some(named_rgb(color)),
    }
}

#[test]
fn every_style_stays_readable_at_every_depth() {
    let ui = ui();
    for name in STYLES {
        let p = style(&ui, name).palette();
        let pairs = [
            (p.text, p.panel, 3.0),
            (p.heading, p.panel, 3.0),
            (p.key, p.panel, 3.0),
            (p.bar_text, p.bar, 3.0),
            (p.selected, p.select_bg, 3.0),
            (p.error, p.panel, 2.5),
            (p.muted, p.panel, 2.0),
        ];
        for depth in [
            ColorDepth::Ansi16,
            ColorDepth::Ansi256,
            ColorDepth::TrueColor,
        ] {
            for (fg, bg, least) in pairs {
                let (Some(f), Some(b)) = (shown(fg, depth), shown(bg, depth)) else {
                    continue;
                };
                let ratio = contrast_ratio(f, b);
                assert!(
                    ratio >= least,
                    "{name} at {depth:?}: {fg:?} on {bg:?} has contrast {ratio:.2}, need {least}"
                );
            }
        }
    }
}

#[test]
fn mono_and_ascii_are_readable_in_every_style_and_layout() {
    let ui = ui();
    for name in STYLES {
        for chosen in LayoutId::ALL {
            for size in [(80, 24), (132, 43)] {
                let theme = style(&ui, name);
                let buffer = shot(
                    &[],
                    chosen,
                    theme,
                    &plain_body(),
                    Tier::Ascii,
                    false,
                    ColorDepth::TrueColor,
                    size,
                );
                let shown = text(&buffer);
                let ctx = format!("{name} {chosen:?} {size:?}");
                assert!(
                    shown.is_ascii(),
                    "{ctx}: the ASCII tier draws only ASCII:\n{shown}"
                );
                assert!(
                    shown.contains("Signed in as alice.")
                        || chosen == LayoutId::Compact
                        || shown.contains("Signed in"),
                    "{ctx}"
                );
                assert!(shown.contains("Forums"), "{ctx}:\n{shown}");
                for y in 0..buffer.area.height {
                    for x in 0..buffer.area.width {
                        let cell = &buffer[(x, y)];
                        assert!(
                            cell.fg == Color::Reset && cell.bg == Color::Reset,
                            "{ctx}: mono has no colour at ({x},{y})"
                        );
                        assert!(
                            !cell.modifier.contains(Modifier::REVERSED),
                            "{ctx}: nothing but a marker may reverse ({x},{y})"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_sixteen_colour_terminal_gets_only_named_colours() {
    let ui = ui();
    for name in STYLES {
        for chosen in LayoutId::ALL {
            let buffer = shot(
                &[],
                chosen,
                style(&ui, name),
                &plain_body(),
                Tier::Unicode,
                true,
                ColorDepth::Ansi16,
                (100, 30),
            );
            for y in 0..buffer.area.height {
                for x in 0..buffer.area.width {
                    for color in [buffer[(x, y)].fg, buffer[(x, y)].bg] {
                        assert!(
                            !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
                            "{name} {chosen:?}: {color:?} at ({x},{y}) on a 16-colour terminal"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_256_colour_terminal_gets_no_truecolor() {
    let ui = ui();
    for name in STYLES {
        let buffer = shot(
            &[],
            LayoutId::Dashboard,
            style(&ui, name),
            &plain_body(),
            Tier::Unicode,
            true,
            ColorDepth::Ansi256,
            (100, 30),
        );
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                for color in [buffer[(x, y)].fg, buffer[(x, y)].bg] {
                    assert!(!matches!(color, Color::Rgb(..)), "{name}: {color:?}");
                }
            }
        }
    }
}

/// The marker cell is bold plus reverse with no colour, and nothing else on the screen
/// reverses, in every style, layout, depth, tier and size, with hostile content.
#[test]
fn the_marker_cell_is_untouched_by_every_style_layout_and_depth() {
    let ui = ui();
    for theme in super::layout_tests::themes(&ui) {
        for chosen in LayoutId::ALL {
            for depth in [
                ColorDepth::Mono,
                ColorDepth::Ansi16,
                ColorDepth::Ansi256,
                ColorDepth::TrueColor,
            ] {
                for tier in [Tier::Ascii, Tier::Unicode] {
                    for size in [(80, 24), (132, 43), (30, 8)] {
                        let buffer = shot(
                            &DRAWN,
                            chosen,
                            theme,
                            &hostile_body(),
                            tier,
                            depth.has_color(),
                            depth,
                            size,
                        );
                        let ctx =
                            format!("{} {chosen:?} {depth:?} {tier:?} {size:?}", theme.name());
                        let last = buffer.area.height - 1;
                        let cell_width = marker_cell_width(tier, size.0).min(size.0);
                        let mut seen_marker = false;
                        for y in 0..buffer.area.height {
                            for x in 0..buffer.area.width {
                                let cell = &buffer[(x, y)];
                                let reversed = cell.modifier.contains(Modifier::REVERSED);
                                if y == last && x < cell_width {
                                    // The marker: reserved style, and no colour at all. (The
                                    // caption after the glyph is plain text in the same cell.)
                                    if reversed {
                                        assert!(
                                            cell.modifier.contains(Modifier::BOLD),
                                            "{ctx}: marker cell ({x},{y}) lost the reserved style"
                                        );
                                        seen_marker = true;
                                    }
                                    assert!(
                                        cell.fg == Color::Reset && cell.bg == Color::Reset,
                                        "{ctx}: marker cell ({x},{y}) has a colour: {:?}/{:?}",
                                        cell.fg,
                                        cell.bg
                                    );
                                } else {
                                    assert!(
                                        !reversed,
                                        "{ctx}: reverse at ({x},{y}) outside the marker"
                                    );
                                }
                            }
                        }
                        assert!(seen_marker, "{ctx}: the marker is drawn");
                    }
                }
            }
        }
    }
}

/// The status row is the same cell for cell whatever the style would paint behind the body.
#[test]
fn the_backdrop_never_reaches_the_status_row() {
    let ui = ui();
    for name in STYLES {
        let theme = style(&ui, name);
        let with_content = shot(
            &DRAWN,
            LayoutId::Dashboard,
            theme,
            &hostile_body(),
            Tier::Unicode,
            true,
            ColorDepth::TrueColor,
            (132, 43),
        );
        let bare = shot(
            &DRAWN,
            LayoutId::Dashboard,
            theme,
            &[],
            Tier::Unicode,
            true,
            ColorDepth::TrueColor,
            (132, 43),
        );
        let last = 42;
        let cell_width = marker_cell_width(Tier::Unicode, 132);
        for x in 0..cell_width {
            assert_eq!(with_content[(x, last)], bare[(x, last)], "{name} x={x}");
            let cell = &with_content[(x, last)];
            assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset), "{name}");
        }
    }
}

// ---- hostile theme files ----

fn parse(text: &str) -> Result<Theme, ThemeError> {
    Theme::parse(text, ui().strings())
}

#[test]
fn a_theme_cannot_name_the_marker_or_its_style() {
    let attempts = [
        "name = \"x\"\n[colors]\nmarker = \"red\"\n",
        "name = \"x\"\n[colors]\nstatus = \"red\"\n",
        "name = \"x\"\n[colors]\nreverse = \"white\"\n",
        "name = \"x\"\n[colors]\nunencrypted = \"black\"\n",
        "name = \"x\"\n[style.marker]\ncolor = \"red\"\n",
        "name = \"x\"\n[style.header]\nreverse = true\n",
        "name = \"x\"\n[style.header]\nbold = true\n",
        "name = \"x\"\n[style.title]\nmodifier = \"reversed\"\n",
        "name = \"x\"\n[style.backdrop]\nkind = \"reverse\"\n",
        "name = \"x\"\n[style.status]\nposition = \"top\"\n",
        "name = \"x\"\n[style]\nmarker = \"hidden\"\n",
        "name = \"x\"\n[style.borders]\nmarker = \"none\"\n",
    ];
    for toml in attempts {
        let error = parse(toml).expect_err(toml);
        assert!(
            matches!(error, ThemeError::Colors(_) | ThemeError::Style(_)),
            "{toml}: {error}"
        );
    }
}

#[test]
fn a_theme_cannot_reword_a_marker_or_carry_marker_glyphs() {
    for toml in [
        "name = \"x\"\n[ui.status]\nunavailable = \"all clear\"\n",
        "name = \"x\"\n[ui]\ntitle = \"\u{27e6}\u{2299}\u{27e7} UNENCRYPTED\"\n",
        "name = \"x\"\n[ui]\ntitle = \"\u{1b}[7mfake\"\n",
        "name = \"x\"\n[ui]\ntitle = \"\u{f015} nerd\"\n",
        "name = \"x\"\n[ui]\ntitle = \"Private lounge\"\n",
    ] {
        assert!(parse(toml).is_err(), "{toml}");
    }
}

#[test]
fn a_theme_cannot_make_the_screen_unreadable() {
    // Text the colour of its own panel: invisible.
    let invisible = "name = \"x\"\n[colors]\npanel = \"#101010\"\ntext = \"#111111\"\n";
    assert!(matches!(parse(invisible), Err(ThemeError::Contrast { .. })));
    // Bar text the colour of the bar: the hints disappear.
    let bar = "name = \"x\"\n[colors]\nbar = \"#202020\"\nbar_text = \"#202020\"\n";
    assert!(matches!(parse(bar), Err(ThemeError::Contrast { .. })));
    // A cursor row you cannot read.
    let cursor = "name = \"x\"\n[colors]\nselect_bg = \"#ffff00\"\nselected = \"#fffff0\"\n";
    assert!(matches!(parse(cursor), Err(ThemeError::Contrast { .. })));
    // A readable one is fine.
    let good = "name = \"x\"\n[colors]\npanel = \"#101010\"\ntext = \"#eeeeee\"\n";
    assert!(parse(good).is_ok());
}

#[test]
fn bad_colour_and_style_values_are_refused() {
    for toml in [
        "name = \"x\"\n[colors]\nheading = \"#12\"\n",
        "name = \"x\"\n[colors]\nheading = 999\n",
        "name = \"x\"\n[colors]\nheading = true\n",
        "name = \"x\"\n[colors]\nheading = { rgb = \"#ffffff\", named = \"chartreuse\" }\n",
        "name = \"x\"\n[colors]\nheading = { rgb = \"#ffffff\", extra = 1 }\n",
        "name = \"x\"\n[style.borders]\nframe = \"zigzag\"\n",
        "name = \"x\"\n[style.title]\ntreatment = \"flashing\"\n",
        "name = \"x\"\n[style.backdrop]\nkind = 4\n",
        "name = \"x\"\nstyle = 3\n",
    ] {
        assert!(parse(toml).is_err(), "{toml}");
    }
}

/// A valid but hostile-looking style (bright backgrounds everywhere, stars, thick borders) still
/// leaves the marker cell alone.
#[test]
fn a_loud_valid_theme_still_cannot_touch_the_marker_cell() {
    let loud = "name = \"loud\"\n[colors]\nsurface = \"#ffffff\"\npanel = \"#ffffff\"\nbar = \"#ffffff\"\nbar_text = \"#000000\"\ntext = \"#000000\"\nheading = \"#000000\"\nkey = \"#000000\"\nborder = \"#000000\"\naccent = \"#000000\"\nselected = \"#ffffff\"\nselect_bg = \"#000000\"\nerror = \"#000000\"\nmuted = \"#000000\"\ninfo = \"#000000\"\nwarning = \"#000000\"\n[style.backdrop]\nkind = \"stars\"\n[style.header]\nencoding = 1\n";
    // The stray key is refused, so fix it and check the drawing.
    assert!(parse(loud).is_err());
    let loud = loud.replace(
        "[style.header]\nencoding = 1\n",
        "[style.header]\ndecoration = \"blocks\"\n",
    );
    let theme = parse(&loud).unwrap();
    for chosen in LayoutId::ALL {
        let buffer = shot(
            &DRAWN,
            chosen,
            &theme,
            &hostile_body(),
            Tier::Unicode,
            true,
            ColorDepth::TrueColor,
            (132, 43),
        );
        let width = marker_cell_width(Tier::Unicode, 132);
        for x in 0..width {
            let cell = &buffer[(x, 42)];
            assert_eq!(
                (cell.fg, cell.bg),
                (Color::Reset, Color::Reset),
                "{chosen:?} x={x}"
            );
        }
    }
}

// ---- layouts ----

#[test]
fn a_layout_that_does_not_fit_says_which_smaller_one_is_shown() {
    let ui = ui();
    let theme = style(&ui, "phosphor");
    let cases = [
        (
            LayoutId::Wide,
            (100, 30),
            "Showing Dashboard: this terminal is too small for Wide.",
        ),
        (
            LayoutId::Dashboard,
            (60, 24),
            "Showing Lightbar: this terminal is too small for Dashboard.",
        ),
        (
            LayoutId::Split,
            (60, 24),
            "Showing Lightbar: this terminal is too small for Split.",
        ),
    ];
    for (chosen, size, note) in cases {
        let shown = text(&shot(
            &[],
            chosen,
            theme,
            &plain_body(),
            Tier::Unicode,
            true,
            ColorDepth::TrueColor,
            size,
        ));
        // The note may wrap inside a narrow box, so look for its first words.
        let start: Vec<&str> = note.split(' ').take(2).collect();
        assert!(
            shown.contains(&start.join(" ")),
            "{chosen:?} {size:?}:\n{shown}"
        );
    }
    // A layout that fits says nothing.
    let fits = text(&shot(
        &[],
        LayoutId::Dashboard,
        theme,
        &plain_body(),
        Tier::Unicode,
        true,
        ColorDepth::TrueColor,
        (100, 30),
    ));
    assert!(!fits.contains("too small"));
}

#[test]
fn the_fallback_chain_ends_in_a_drawing_at_every_size() {
    for chosen in LayoutId::ALL {
        for (w, h) in [
            (1, 1),
            (10, 3),
            (23, 8),
            (43, 11),
            (71, 19),
            (109, 21),
            (500, 198),
        ] {
            let shown = layout::effective(chosen, Rect::new(0, 0, w, h));
            assert!(LayoutId::ALL.contains(&shown));
            let again = layout::effective(shown, Rect::new(0, 0, w, h));
            assert_eq!(shown, again, "{chosen:?} {w}x{h} must settle");
        }
    }
    // Each pane layout's first fallback is the one the docs name.
    let at = |layout, w, h| layout::effective(layout, Rect::new(0, 0, w, h));
    assert_eq!(at(LayoutId::Wide, 109, 40), LayoutId::Dashboard);
    assert_eq!(at(LayoutId::Wide, 109, 13), LayoutId::Lightbar);
    assert_eq!(at(LayoutId::Wide, 109, 10), LayoutId::Classic);
    assert_eq!(at(LayoutId::Dashboard, 71, 40), LayoutId::Lightbar);
    assert_eq!(at(LayoutId::Split, 71, 40), LayoutId::Lightbar);
    assert_eq!(at(LayoutId::Focus, 39, 40), LayoutId::Classic);
    assert_eq!(at(LayoutId::Focus, 43, 11), LayoutId::Focus);
    assert_eq!(at(LayoutId::Wide, 132, 43), LayoutId::Wide);
}

#[test]
fn the_pane_layouts_really_have_panes() {
    let ui = ui();
    let theme = style(&ui, "neon");
    let draw = |layout, size| {
        text(&shot(
            &[],
            layout,
            theme,
            &plain_body(),
            Tier::Unicode,
            true,
            ColorDepth::TrueColor,
            size,
        ))
    };
    let dashboard = draw(LayoutId::Dashboard, (100, 30));
    for title in [
        "This board",
        "Clock",
        "Who is online",
        "Last callers",
        "New posts",
        "Mail",
    ] {
        assert!(
            dashboard.contains(title),
            "dashboard lacks {title}:\n{dashboard}"
        );
    }
    assert!(dashboard.contains("Portland, OR") && dashboard.contains("since 2026"));
    assert!(
        dashboard.contains("UTC 12:00"),
        "the header clock shows the real time"
    );
    assert!(
        dashboard.contains("Thu 5 Nov 2026"),
        "the clock widget shows the date"
    );
    let wide = draw(LayoutId::Wide, (132, 43));
    assert!(wide.contains("Activity") && wide.contains("This board"));
    let split = draw(LayoutId::Split, (100, 30));
    assert!(split.contains("Chat"), "{split}");
    // The widgets show the data they were given, not made-up numbers.
    assert!(dashboard.contains("5 online") && dashboard.contains("Active threads: 7"));
}
