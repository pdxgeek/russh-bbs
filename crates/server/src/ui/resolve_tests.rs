#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::type_complexity,
    clippy::panic,
    clippy::assert_is_empty
)]

//! How the board's identity and limits meet a member's own choices (UI-2).

use ratatui::layout::Rect;
use russh_bbs_core::terminal_settings::{
    BorderChoice, DepthCap, LayoutId, TerminalSettings, ThemeName,
};

use super::depth::{ColorDepth, Tint};
use super::{Board, Ui};
use crate::config::BoardConfig;

fn with(edit: impl FnOnce(&mut BoardConfig)) -> Ui {
    let mut config = BoardConfig::default();
    edit(&mut config);
    Ui::standard()
        .unwrap()
        .with_board(Board::from_config(&config).unwrap())
}

fn name(text: &str) -> ThemeName {
    ThemeName::new(text).unwrap()
}

fn member() -> TerminalSettings {
    TerminalSettings::default().with_tier(russh_bbs_core::icons::Tier::Unicode)
}

#[test]
fn a_plain_board_gives_a_new_member_the_old_defaults() {
    let ui = Ui::standard().unwrap();
    assert_eq!(ui.style_for(&member()), ThemeName::default_theme());
    assert_eq!(ui.layout_for(&member()), LayoutId::Classic);
    assert_eq!(ui.allowed_styles(), ui.theme_names());
    assert_eq!(ui.allowed_layouts(), LayoutId::ALL);
}

#[test]
fn the_boards_defaults_reach_members_who_never_chose() {
    let ui = with(|c| {
        c.style = Some("neon".into());
        c.layout = Some("dashboard".into());
    });
    assert_eq!(ui.style_for(&member()), name("neon"));
    assert_eq!(ui.layout_for(&member()), LayoutId::Dashboard);
    // A member's own choice wins over the board's default.
    let chosen = member()
        .with_theme(name("paper"))
        .with_layout(LayoutId::Wide);
    assert_eq!(ui.style_for(&chosen), name("paper"));
    assert_eq!(ui.layout_for(&chosen), LayoutId::Wide);
    // Choosing the built-in default style is a choice too, not "follow the board".
    let explicit = member().with_theme(ThemeName::default_theme());
    assert_eq!(ui.style_for(&explicit), ThemeName::default_theme());
}

#[test]
fn a_member_cannot_stay_on_what_the_board_no_longer_allows() {
    let ui = with(|c| {
        c.style = Some("paper".into());
        c.styles = "paper,ansi".into();
        c.layouts = "classic,focus".into();
    });
    let stale = member()
        .with_theme(name("phosphor"))
        .with_layout(LayoutId::Wide);
    assert_eq!(
        ui.style_for(&stale),
        name("paper"),
        "falls to the board default"
    );
    assert_eq!(ui.layout_for(&stale), LayoutId::Classic);
    let fine = member()
        .with_theme(name("ansi"))
        .with_layout(LayoutId::Focus);
    assert_eq!(ui.style_for(&fine), name("ansi"));
    assert_eq!(ui.layout_for(&fine), LayoutId::Focus);
    assert_eq!(ui.allowed_styles(), [name("paper"), name("ansi")]);
}

#[test]
fn one_entry_is_a_lock() {
    let ui = with(|c| {
        c.styles = "amber".into();
        c.layouts = "split".into();
    });
    for settings in [
        member(),
        member()
            .with_theme(name("neon"))
            .with_layout(LayoutId::Compact),
    ] {
        assert_eq!(ui.style_for(&settings), name("amber"));
        assert_eq!(ui.layout_for(&settings), LayoutId::Split);
    }
    assert_eq!(ui.allowed_styles().len(), 1);
    assert_eq!(ui.allowed_layouts().len(), 1);
}

#[test]
fn a_style_the_board_names_but_is_not_loaded_is_ignored_not_fatal() {
    let ui = with(|c| {
        c.styles = "ghost".into();
        c.style = Some("ghost".into());
    });
    // The list is empty after dropping the unknown name, which means every style.
    assert_eq!(ui.allowed_styles(), ui.theme_names());
    assert_eq!(ui.style_for(&member()), ThemeName::default_theme());
}

#[test]
fn the_board_name_is_the_title_in_every_style() {
    let ui = with(|c| c.name = "Night Owl BBS".into());
    for style in ui.theme_names() {
        assert_eq!(ui.theme(style).strings().text("ui.title"), "Night Owl BBS");
    }
    let plain = Ui::standard().unwrap();
    assert_eq!(plain.strings().text("ui.title"), "russh-bbs");
}

#[test]
fn the_board_accent_replaces_the_styles_and_motion_can_be_forbidden() {
    let ui = with(|c| {
        c.accent = Some("#112233".into());
        c.motion = false;
    });
    let look = ui.look(&member(), Rect::new(0, 0, 100, 30), ColorDepth::TrueColor);
    assert_eq!(look.palette().accent.rgb(), Some((0x11, 0x22, 0x33)));
    assert!(!look.motion(), "the board turned motion off");
    let other = Ui::standard().unwrap();
    let look = other.look(&member(), Rect::new(0, 0, 100, 30), ColorDepth::TrueColor);
    assert!(look.motion());
}

#[test]
fn a_style_can_cap_motion_for_everyone_who_uses_it() {
    let ui = Ui::standard().unwrap();
    let area = Rect::new(0, 0, 100, 30);
    let paper = member().with_theme(name("paper"));
    assert!(!ui.look(&paper, area, ColorDepth::TrueColor).motion());
    let neon = member().with_theme(name("neon"));
    assert!(ui.look(&neon, area, ColorDepth::TrueColor).motion());
    // And the member's own switch still turns it off.
    assert!(
        !ui.look(&neon.with_motion(false), area, ColorDepth::TrueColor)
            .motion()
    );
}

#[test]
fn a_members_depth_cap_and_border_choice_apply_within_what_the_client_can_show() {
    let ui = Ui::standard().unwrap();
    let area = Rect::new(0, 0, 100, 30);
    let at = |settings: &TerminalSettings, client| ui.look(settings, area, client).depth();
    let truecolor = ColorDepth::TrueColor;
    assert_eq!(at(&member(), truecolor), ColorDepth::TrueColor);
    assert_eq!(
        at(&member().with_depth_cap(DepthCap::Ansi256), truecolor),
        ColorDepth::Ansi256
    );
    assert_eq!(
        at(&member().with_depth_cap(DepthCap::Ansi16), truecolor),
        ColorDepth::Ansi16
    );
    // A cap never raises what a plain terminal can show.
    assert_eq!(
        at(
            &member().with_depth_cap(DepthCap::TrueColor),
            ColorDepth::Ansi16
        ),
        ColorDepth::Ansi16
    );
    // Colour off wins over everything.
    assert_eq!(at(&member().with_color(false), truecolor), ColorDepth::Mono);
    let ascii = ui.look(&member().with_borders(BorderChoice::Ascii), area, truecolor);
    assert!(ascii.ascii());
    assert!(!ui.look(&member(), area, truecolor).ascii());
}

#[test]
fn the_accent_tint_survives_every_depth() {
    let tint = Tint::Rgb(0x11, 0x22, 0x33, ratatui::style::Color::Blue);
    for depth in ColorDepth::ALL {
        assert_eq!(tint.resolve(depth).is_some(), depth.has_color());
    }
}
