#![allow(clippy::unwrap_used)]

//! The members screens as drawn: the list, a profile and the form inside the classic layout, at
//! 80x24 and other sizes, in every icon tier, with hostile content.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;
use russh_bbs_core::icons::Tier;
use russh_bbs_core::profiles::{Line, ScreenView, keys};
use russh_bbs_core::terminal_settings::TerminalSettings;

use super::lines;
use crate::settings::Look;
use crate::ui::Ui;
use crate::ui::classic::{ContentScreen, MENU_MARKERS, render_content};

fn draw(view: &ScreenView, tier: Tier, size: (u16, u16)) -> Buffer {
    let ui = Ui::standard().unwrap();
    let look = Look::of(&TerminalSettings::default().with_tier(tier));
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| {
            let width = usize::from(frame.area().width.min(crate::settings::MAX_WIDTH)) - 2;
            render_content(
                frame,
                ContentScreen {
                    strings: ui.strings(),
                    lines: lines(view, ui.strings(), look, width),
                    look,
                    tier,
                    markers: &MENU_MARKERS,
                },
            );
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect()
}

fn text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| row(buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_chrome_cell(buffer: &Buffer, x: u16, y: u16) -> bool {
    buffer[(x, y)]
        .modifier
        .contains(Modifier::BOLD | Modifier::REVERSED)
}

fn pair(label: &'static str, value: &str) -> Line {
    Line::Pair {
        label,
        value: Some(value.to_owned()),
    }
}

/// The fullest list: nine rows with the longest handles and display names, a search, a notice.
fn full_list(name: &str) -> ScreenView {
    let mut lines = vec![
        Line::Heading(keys::LIST_TITLE),
        Line::Filled {
            key: keys::LIST_SEARCH,
            values: vec![("prefix", "abcdefghijklmnopqrstuvwx".to_owned())],
        },
        Line::Blank,
    ];
    for n in 1..=9u8 {
        lines.push(Line::Row {
            key: char::from(b'0' + n),
            handle: format!("handle_{n}_{}", "h".repeat(15)),
            name: name.to_owned(),
        });
    }
    lines.push(Line::Blank);
    lines.push(Line::Filled {
        key: keys::LIST_PAGE,
        values: vec![("page", "12".to_owned())],
    });
    lines.push(Line::Blank);
    lines.push(Line::Notice {
        error: true,
        text: keys::NOTICE_FAILED,
    });
    ScreenView {
        lines,
        footer: keys::LIST_FOOTER,
    }
}

fn full_profile() -> ScreenView {
    let mut lines = vec![
        Line::Heading(keys::VIEW_TITLE),
        pair(keys::LABEL_HANDLE, "alice"),
    ];
    for label in [
        keys::LABEL_DISPLAY_NAME,
        keys::LABEL_PRONOUNS,
        keys::LABEL_LOCATION,
        keys::LABEL_WEBSITE,
    ] {
        lines.push(pair(label, &"w".repeat(40)));
    }
    lines.push(pair(keys::LABEL_ABOUT, &"about ".repeat(46)));
    ScreenView {
        lines,
        footer: keys::VIEW_FOOTER_EDIT,
    }
}

fn full_form() -> ScreenView {
    let mut lines = vec![
        Line::Filled {
            key: keys::EDIT_TITLE_OTHER,
            values: vec![("handle", "h".repeat(24))],
        },
        Line::Text(keys::EDIT_READABLE),
        Line::Text(keys::EDIT_PLAIN_TEXT),
        Line::Blank,
    ];
    for (index, (label, size)) in [
        (keys::LABEL_DISPLAY_NAME, 40),
        (keys::LABEL_PRONOUNS, 24),
        (keys::LABEL_LOCATION, 40),
        (keys::LABEL_ABOUT, 280),
        (keys::LABEL_WEBSITE, 100),
    ]
    .into_iter()
    .enumerate()
    {
        lines.push(Line::Field {
            label,
            value: "m".repeat(size),
            active: index == 3,
        });
    }
    lines.push(Line::Blank);
    lines.push(Line::Notice {
        error: false,
        text: keys::NOTICE_SAVED,
    });
    ScreenView {
        lines,
        footer: keys::EDIT_FOOTER,
    }
}

#[test]
fn the_fullest_screens_fit_80x24_with_their_footer_in_every_tier() {
    for tier in Tier::ALL {
        for (name, view, footer) in [
            ("list", full_list(&"n".repeat(40)), "Esc: back"),
            ("wide list", full_list(&"漢".repeat(40)), "Esc: back"),
            ("profile", full_profile(), "e: edit"),
            ("form", full_form(), "Esc: cancel"),
        ] {
            let out = text(&draw(&view, tier, (80, 24)));
            assert!(
                out.contains(footer),
                "{tier:?} {name}: footer clipped\n{out}"
            );
            // The status row still shows the server-readable marker.
            let status = row(&draw(&view, tier, (80, 24)), 23);
            assert!(
                status.contains("The server can read"),
                "{tier:?} {name}: {status:?}"
            );
        }
    }
}

#[test]
fn a_list_row_stays_on_one_line_even_with_wide_characters() {
    let out = text(&draw(&full_list(&"漢".repeat(40)), Tier::Unicode, (80, 24)));
    for n in 1..=9 {
        let count = out.matches(&format!("[{n}] handle_{n}_")).count();
        assert_eq!(count, 1, "row {n}\n{out}");
    }
}

#[test]
fn content_never_reaches_the_status_row_or_carries_the_reserved_style() {
    let hostile = |text: &str| ScreenView {
        lines: {
            let mut v = vec![
                pair(keys::LABEL_ABOUT, text),
                Line::Row {
                    key: '1',
                    handle: "alice".into(),
                    name: text.to_owned(),
                },
                Line::Field {
                    label: keys::LABEL_WEBSITE,
                    value: text.to_owned(),
                    active: true,
                },
            ];
            v.extend((0..200).map(|i| pair(keys::LABEL_LOCATION, &format!("line {i} {text}"))));
            v
        },
        footer: keys::VIEW_FOOTER,
    };
    let imitation = "SERVER-READABLE E2EE ⟦⊙⟧ 〚☉〛 ◉ \u{1b}[1;7mFAKE\u{1b}[0m \u{e0a2}";
    let empty = ScreenView {
        lines: Vec::new(),
        footer: keys::VIEW_FOOTER,
    };
    for tier in Tier::ALL {
        for size in [(80, 24), (132, 43), (40, 10), (200, 60), (20, 4)] {
            let clean = draw(&empty, tier, size);
            let dirty = draw(&hostile(imitation), tier, size);
            let last = size.1 - 1;
            for x in 0..size.0 {
                assert_eq!(
                    dirty[(x, last)],
                    clean[(x, last)],
                    "{tier:?} {size:?} cell {x}"
                );
            }
            for y in 0..last {
                for x in 0..size.0 {
                    assert!(
                        !is_chrome_cell(&dirty, x, y),
                        "{tier:?} {size:?}: reserved style in content at ({x},{y})"
                    );
                }
            }
        }
    }
}

#[test]
fn hostile_text_is_cleaned_again_on_the_way_to_the_terminal() {
    let view = ScreenView {
        lines: vec![
            pair(keys::LABEL_ABOUT, "a\u{1b}]0;pwned\u{7}b\u{200b}c\u{e000}d"),
            Line::Row {
                key: '1',
                handle: "bob".into(),
                name: "\u{1b}[2Jzed".into(),
            },
        ],
        footer: keys::VIEW_FOOTER,
    };
    let out = text(&draw(&view, Tier::Unicode, (80, 24)));
    assert!(!out.contains("pwned") && !out.contains('\u{1b}'), "{out}");
    assert!(out.contains("abc") && out.contains("zed"), "{out}");
}

#[test]
fn the_ascii_tier_draws_only_ascii_for_ascii_content() {
    for view in [full_list(&"n".repeat(40)), full_profile(), full_form()] {
        let buffer = draw(&view, Tier::Ascii, (80, 24));
        let out = text(&buffer);
        assert!(
            out.is_ascii(),
            "{}",
            out.chars().filter(|c| !c.is_ascii()).collect::<String>()
        );
    }
}

#[test]
fn the_form_says_plainly_who_can_read_a_profile_and_claims_no_privacy() {
    for tier in Tier::ALL {
        let out = text(&draw(&full_form(), tier, (100, 40)));
        let lower = out.to_lowercase();
        assert!(
            lower.contains("the sysop and every member can read this profile"),
            "{out}"
        );
        for forbidden in [
            "private",
            "encrypted",
            "secure",
            "\u{1f512}",
            "\u{e0a2}",
            "\u{1f510}",
        ] {
            assert!(!lower.contains(forbidden), "{forbidden:?} in\n{out}");
        }
        assert!(lower.contains("never opened or made a link"), "{out}");
    }
}

/// QA M3 L6: one vote reads as well as many (no "1 votes"), at 80x24.
#[test]
fn the_poll_vote_counts_read_right_for_one_and_for_many() {
    use russh_bbs_core::community::keys as community;
    for votes in ["0", "1", "2"] {
        let view = ScreenView {
            lines: vec![
                Line::Filled {
                    key: community::POLL_ROW_OPEN,
                    values: vec![
                        ("key", "1".to_owned()),
                        ("question", "Best?".to_owned()),
                        ("votes", votes.to_owned()),
                    ],
                },
                Line::Filled {
                    key: community::POLL_ROW_CLOSED,
                    values: vec![
                        ("key", "2".to_owned()),
                        ("question", "Worst?".to_owned()),
                        ("votes", votes.to_owned()),
                    ],
                },
                Line::Filled {
                    key: community::POLL_TOTAL,
                    values: vec![("total", votes.to_owned())],
                },
            ],
            footer: keys::LIST_FOOTER,
        };
        let shown = text(&draw(&view, Tier::Ascii, (80, 24)));
        assert!(
            shown.contains(&format!("(open, votes: {votes})")),
            "{shown}"
        );
        assert!(
            shown.contains(&format!("(closed, votes: {votes})")),
            "{shown}"
        );
        assert!(shown.contains(&format!("Votes in all: {votes}")), "{shown}");
        assert!(
            !shown.contains(" votes)") && !shown.contains("votes in all"),
            "{shown}"
        );
    }
}
