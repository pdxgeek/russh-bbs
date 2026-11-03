#![allow(clippy::unwrap_used)]

//! Drawing the signed-in screens under different settings: color off means no color cells,
//! the ASCII tier means an ASCII border, and the glyph test draws the registry glyphs.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
use russh_bbs_core::icons::{ContentIcon, Icon, Tier};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::AccountId;
use russh_bbs_core::terminal_settings::{LayoutId, MemberFlow, MemberInput, TerminalSettings};

use super::{Look, render};
use crate::ui::Ui;

fn flow(settings: TerminalSettings) -> MemberFlow {
    MemberFlow::new(
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
    )
}

fn draw(flow: &MemberFlow, cols: u16, rows: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
    let ui = Ui::standard().unwrap();
    let view = flow.view(ui.strings());
    let look = Look::of(flow.settings());
    terminal
        .draw(|frame| render(frame, &view, look, ui.strings()))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn text(buffer: &Buffer) -> String {
    let width = usize::from(buffer.area.width);
    buffer
        .content()
        .chunks(width)
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn has_color(buffer: &Buffer) -> bool {
    buffer
        .content()
        .iter()
        .any(|cell| cell.fg != Color::Reset || cell.bg != Color::Reset)
}

#[test]
fn before_the_glyph_test_is_answered_the_layout_is_plain() {
    let buffer = draw(&flow(TerminalSettings::default()), 80, 24);
    assert!(!has_color(&buffer), "no color before a tier is chosen");
    let out = text(&buffer);
    assert!(out.contains("Signed in as alice."), "{out}");
    assert!(
        out.contains('+') && !out.contains('\u{250c}'),
        "ASCII border: {out}"
    );
}

#[test]
fn color_on_with_a_glyph_tier_draws_color_and_line_drawing() {
    let settings = TerminalSettings::default().with_tier(Tier::Unicode);
    let buffer = draw(&flow(settings), 80, 24);
    assert!(has_color(&buffer));
    assert!(text(&buffer).contains('\u{250c}'), "line-drawing border");
}

#[test]
fn color_off_draws_no_color_even_with_a_glyph_tier() {
    let settings = TerminalSettings::default()
        .with_tier(Tier::Unicode)
        .with_color(false);
    assert!(!has_color(&draw(&flow(settings), 80, 24)));
}

#[test]
fn the_ascii_tier_draws_an_ascii_border_even_with_color() {
    let settings = TerminalSettings::default().with_tier(Tier::Ascii);
    let buffer = draw(&flow(settings), 80, 24);
    assert!(has_color(&buffer));
    assert!(text(&buffer).contains("+--"), "ASCII border");
}

#[test]
fn the_glyph_test_draws_registry_glyphs_for_each_step() {
    let mut f = flow(TerminalSettings::default());
    f.input(MemberInput::Char('x'));
    let nerd = text(&draw(&f, 100, 30));
    let nerd_row = Icon::from(ContentIcon::Board).render(Tier::NerdFont);
    assert!(
        nerd.contains(nerd_row.plain().trim_end()),
        "nerd glyph on screen"
    );
    assert!(nerd.contains("Do these look like icons?"));
    f.input(MemberInput::Char('n'));
    let unicode = text(&draw(&f, 100, 30));
    let unicode_glyph = Icon::from(ContentIcon::Mail).glyph(Tier::Unicode);
    assert!(unicode.contains(unicode_glyph), "unicode glyph on screen");
    assert!(unicode.contains("Style: Unicode"));
}

#[test]
fn tiny_windows_do_not_panic() {
    let f = flow(TerminalSettings::default());
    for (cols, rows) in [(1, 1), (2, 2), (10, 3), (500, 200)] {
        let _ = draw(&f, cols, rows);
    }
}

#[test]
fn notices_are_words_not_only_color() {
    let mut f = flow(
        TerminalSettings::default()
            .with_tier(Tier::Ascii)
            .with_color(false),
    );
    for key in ['x', 's', 'w', '9', '9', '9', '9'] {
        f.input(MemberInput::Char(key));
    }
    f.input(MemberInput::Enter);
    let out = text(&draw(&f, 80, 30));
    assert!(
        out.contains("Problem: Width must be from 20 to 500."),
        "{out}"
    );
}

#[test]
fn the_settings_screen_with_a_notice_fits_80x24_in_every_layout_and_tier() {
    // Every row of the screen, the footer and the last menu entry included, must be on screen
    // at 80x24; a new entry that pushes them off fails here instead of being clipped (M3-A).
    for layout in LayoutId::ALL {
        for tier in Tier::ALL {
            let settings = TerminalSettings::default()
                .with_tier(tier)
                .with_layout(layout);
            let mut f = flow(settings);
            for key in ['x', 's', 'c'] {
                f.input(MemberInput::Char(key)); // the last one leaves a "Saved." notice
            }
            let out = text(&draw(&f, 80, 24));
            for expected in [
                "Terminal settings",
                "Width: automatic",
                "Layout: ",
                "[l] Next layout",
                "[t] Next theme",
                "[b] Back",
                "Notice: Saved.",
                "Esc: back",
            ] {
                assert!(
                    out.contains(expected),
                    "{layout:?} {tier:?}: {expected:?} is off screen:\n{out}"
                );
            }
        }
    }
}
