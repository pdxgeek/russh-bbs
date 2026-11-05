#![allow(clippy::unwrap_used, clippy::panic)]

//! The render matrix for the converted screens (UI-4): every screen is drawn in all seven
//! layouts at 80x24, 100x30 and 132x43, in mono (Unicode glyphs, no colour) and in the ASCII
//! tier, and checked two ways.
//!
//! - **Golden text** in `snapshots/<screen>.txt`. A render that equals the same size and mode
//!   in the first layout is written as `= same as classic`, so the file shows which layouts
//!   really change a screen. To accept a deliberate change:
//!   `UPDATE_SNAPSHOTS=1 cargo test -p russh-bbs-server screens`, then read the diff.
//! - **Properties of every cell** in every render: the status bar's marker cell is exactly the
//!   reserved style (bold + reverse, no colour) and nothing else on screen has reverse video,
//!   mono draws no colour, and the ASCII tier draws seven-bit text only.

use std::fmt::Write as _;
use std::path::PathBuf;

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::terminal_settings::{LayoutId, TerminalSettings, ThemeName};

use crate::settings::Look;
use crate::ui::depth::ColorDepth;
use crate::ui::layout::effective;
use crate::ui::layout_tests::{DRAWN, marker_cell_width, text};
use crate::ui::strings::StringTable;
use crate::ui::{Ui, backdrop};

/// The sizes the plan names.
pub(crate) const SIZES: [(u16, u16); 3] = [(80, 24), (100, 30), (132, 43)];

/// What a screen is drawn with.
pub(crate) struct Draw<'a> {
    pub(crate) ui: &'a Ui,
    pub(crate) strings: &'a StringTable,
    pub(crate) look: Look,
    pub(crate) tier: Tier,
    pub(crate) markers: &'a [SecurityMarker],
    pub(crate) clock: Option<&'a str>,
    pub(crate) layout: LayoutId,
}

/// Draw one render of a screen at `size`.
pub(crate) fn shot(
    ui: &Ui,
    layout: LayoutId,
    tier: Tier,
    size: (u16, u16),
    draw: &dyn Fn(&mut Frame<'_>, &Draw<'_>),
) -> Buffer {
    let theme = ui.theme(ThemeName::new("default").unwrap());
    let settings = TerminalSettings::default()
        .with_tier(tier)
        .with_color(false)
        .with_layout(layout);
    let area = Rect::new(0, 0, size.0, size.1);
    let look =
        Look::for_theme(&settings, effective(layout, area), theme).at_depth(ColorDepth::Mono);
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| {
            draw(
                frame,
                &Draw {
                    ui,
                    strings: theme.strings(),
                    look,
                    tier,
                    markers: &DRAWN,
                    clock: Some("UTC 12:00"),
                    layout,
                },
            );
            backdrop::finish(frame, look);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

/// The properties that hold for every cell of every render.
fn check_cells(buffer: &Buffer, tier: Tier, label: &str) {
    let area = buffer.area;
    let last = area.height - 1;
    let marker_width = marker_cell_width(tier, area.width);
    let mut seen_marker = false;
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buffer[(x, y)];
            let in_marker = y == last && x < marker_width;
            let reversed = cell.modifier.contains(Modifier::REVERSED);
            if in_marker {
                // The glyph is bold + reverse; the caption after it is plain text in the same
                // cell. Either way the marker cell carries no colour.
                if reversed {
                    assert!(
                        cell.modifier.contains(Modifier::BOLD),
                        "{label}: the marker cell at ({x},{y}) lost its reserved style"
                    );
                    seen_marker = true;
                }
            } else {
                assert!(
                    !reversed,
                    "{label}: reverse video outside the marker cell at ({x},{y}): {:?}",
                    cell.symbol()
                );
            }
            assert!(
                cell.fg == Color::Reset && cell.bg == Color::Reset,
                "{label}: mono drew a colour at ({x},{y})"
            );
            if tier == Tier::Ascii {
                assert!(
                    cell.symbol().is_ascii(),
                    "{label}: non-ASCII {:?} at ({x},{y}) in the ASCII tier",
                    cell.symbol()
                );
            }
        }
    }
    assert!(
        seen_marker,
        "{label}: the marker cell is missing from the status bar"
    );
}

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/ui/screens/snapshots")
        .join(format!("{name}.txt"))
}

/// QA M3.5 M2: lists overflow, and scrollbars appear, in windows shorter than the golden
/// sizes. Draw the screen in every layout at heights 8 to 24 and widths 40, 80 and 132 in the
/// ASCII tier and require seven-bit text in every cell. No golden is written for these.
fn ascii_at_small_heights(ui: &Ui, name: &str, draw: &dyn Fn(&mut Frame<'_>, &Draw<'_>)) {
    for layout in LayoutId::ALL {
        for height in 8..=24u16 {
            for width in [40u16, 80, 132] {
                let buffer = shot(ui, layout, Tier::Ascii, (width, height), draw);
                for y in 0..height {
                    for x in 0..width {
                        let symbol = buffer[(x, y)].symbol();
                        assert!(
                            symbol.is_ascii(),
                            "{name} {layout:?} {width}x{height}: non-ASCII {symbol:?} at ({x},{y})"
                        );
                    }
                }
            }
        }
    }
}

/// Draw `draw` in every layout, size and mode; check every cell; compare with the golden file.
pub(crate) fn golden(name: &str, draw: &dyn Fn(&mut Frame<'_>, &Draw<'_>)) {
    let ui = Ui::standard().unwrap();
    ascii_at_small_heights(&ui, name, draw);
    let mut out = String::new();
    for size in SIZES {
        for tier in [Tier::Unicode, Tier::Ascii] {
            let mut first: Option<String> = None;
            for layout in LayoutId::ALL {
                let buffer = shot(&ui, layout, tier, size, draw);
                let label = format!(
                    "{name} {} {}x{} {tier:?}",
                    russh_bbs_core::terminal_settings::layout_to_stored(layout),
                    size.0,
                    size.1
                );
                check_cells(&buffer, tier, &label);
                let shown = text(&buffer)
                    .lines()
                    .map(str::trim_end)
                    .collect::<Vec<_>>()
                    .join("\n");
                let _ = writeln!(
                    out,
                    "=== {} / {}x{} / {}",
                    russh_bbs_core::terminal_settings::layout_to_stored(layout),
                    size.0,
                    size.1,
                    if tier == Tier::Ascii {
                        "ascii mono"
                    } else {
                        "unicode mono"
                    }
                );
                match &first {
                    Some(base) if *base == shown => {
                        let _ = writeln!(out, "= same as classic\n");
                    }
                    _ => {
                        let _ = writeln!(out, "{shown}\n");
                        if first.is_none() {
                            first = Some(shown);
                        }
                    }
                }
            }
        }
    }
    let file = path(name);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, &out).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&file).unwrap_or_else(|_| {
        panic!(
            "{} is missing; run with UPDATE_SNAPSHOTS=1 to create it",
            file.display()
        )
    });
    if out != want {
        let first = out
            .lines()
            .zip(want.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| out.lines().count().min(want.lines().count()));
        panic!(
            "snapshot {} differs at line {}:\n  got:  {:?}\n  want: {:?}\nrun with UPDATE_SNAPSHOTS=1 to accept",
            file.display(),
            first + 1,
            out.lines().nth(first),
            want.lines().nth(first)
        );
    }
}
