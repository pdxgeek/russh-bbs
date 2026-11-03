//! The reserved status line: the one place security markers are drawn.
//!
//! **Why a row and not a decoration.** Content (posts, handles, door output) is sanitized, but
//! the sanitizer cannot stop lookalikes, and in the ASCII tier content can type the same
//! letters as a marker (D39, D45). What content can never do is (a) emit the reserved style,
//! because every escape sequence is stripped, or (b) draw into a screen position it was not
//! given. So a marker is drawn only here, with the reserved style, in a row the layout takes
//! out of the area before any content is laid out. The content widgets are handed a rectangle
//! that does not include this row, and ratatui clips a widget to its rectangle, so no content
//! can reach it however long or tall it is.
//!
//! Markers are styled through [`Rendered::chrome_style`] (never by writing SGR text into a
//! cell) and the style is never applied to anything else. When the row cannot show a marker
//! the line says "security markers unavailable" instead of drawing nothing.

use ratatui::Frame;
use ratatui::crossterm::cursor::MoveTo;
use ratatui::crossterm::queue;
use ratatui::crossterm::style::Print;
use ratatui::crossterm::terminal::{Clear as ClearLine, ClearType};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use russh_bbs_core::icons::{ChromeStyle, Icon, Rendered, SecurityMarker, Tier};

use super::keys;
use super::strings::StringTable;

/// Split `area` into the content area and the reserved status row (the last row). A one-row
/// area has no room for a status row; markers are then not drawn at all.
pub(crate) fn split_status(area: Rect) -> (Rect, Option<Rect>) {
    if area.height < 2 {
        return (area, None);
    }
    let content = Rect {
        height: area.height - 1,
        ..area
    };
    let status = Rect {
        y: area.y + area.height - 1,
        height: 1,
        ..area
    };
    (content, Some(status))
}

/// The ratatui style for a rendered icon: the reserved style for security chrome, nothing for
/// anything else.
pub(crate) fn style_of(rendered: &Rendered) -> Style {
    match rendered.chrome_style() {
        Some(_) => {
            let mut modifiers = Modifier::empty();
            if ChromeStyle::BOLD {
                modifiers |= Modifier::BOLD;
            }
            if ChromeStyle::REVERSE {
                modifiers |= Modifier::REVERSED;
            }
            Style::new().add_modifier(modifiers)
        }
        None => Style::new(),
    }
}

/// The text that explains a marker, from the string table where one is defined.
fn caption(marker: SecurityMarker, strings: &StringTable) -> &str {
    match marker {
        SecurityMarker::ServerReadable => strings.text(keys::STATUS_SERVER_READABLE),
        // Not drawn by any screen yet; their honest ASCII label is the caption until they are.
        SecurityMarker::E2eeLock | SecurityMarker::Signature | SecurityMarker::KeyChanged => {
            marker.label()
        }
        // Fixed words for the transport markers, like the footer's (a theme cannot reword them).
        SecurityMarker::Unencrypted | SecurityMarker::LowSecurity => footer_note(marker),
    }
}

/// Draw `markers` in the status row `area`, clearing it first. With no markers the row stays
/// blank. If a marker does not fit the row's width, the row says markers are unavailable.
pub(crate) fn render_status(
    frame: &mut Frame<'_>,
    area: Rect,
    markers: &[SecurityMarker],
    tier: Tier,
    strings: &StringTable,
) {
    frame.render_widget(Clear, area);
    if markers.is_empty() {
        return;
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    for marker in markers {
        let rendered = Icon::from(*marker).render(tier);
        if !spans.is_empty() {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            rendered.plain().trim_end().to_owned(),
            style_of(&rendered),
        ));
        spans.push(Span::raw(format!(" {}", caption(*marker, strings))));
    }
    let line = Line::from(spans);
    // Every marker itself must fit; only a caption may be clipped. A marker that would be cut
    // off is worse than none, so then the row says so instead.
    let markers_width: usize = markers
        .iter()
        .map(|marker| Icon::from(*marker).render(tier).width())
        .sum::<usize>()
        + 3 * (markers.len() - 1);
    if markers_width > usize::from(area.width) {
        let plain = Line::raw(strings.text(keys::STATUS_UNAVAILABLE).to_owned());
        frame.render_widget(Paragraph::new(plain), area);
        return;
    }
    frame.render_widget(Paragraph::new(line), area);
}

/// What the footer says beside its marker. Fixed words, never themeable (a theme that could
/// reword it could make the marker say the opposite).
fn footer_note(marker: SecurityMarker) -> &'static str {
    match marker {
        SecurityMarker::Unencrypted => "anyone on the network can read this session",
        SecurityMarker::LowSecurity => "telnet is on",
        SecurityMarker::ServerReadable
        | SecurityMarker::E2eeLock
        | SecurityMarker::Signature
        | SecurityMarker::KeyChanged => "",
    }
}

/// The bytes that paint `marker` on the last row of a `cols` x `rows` window, outside every
/// layout: the transport reserved that row (the layouts were given one row less), so no content
/// can be drawn into it. The marker is written with [`Rendered::to_terminal`], so it carries the
/// reserved style, and the note beside it is cut to fit the width. Used for telnet's
/// `UNENCRYPTED` row (M3-H, D27).
pub(crate) fn footer(marker: SecurityMarker, tier: Tier, cols: u16, rows: u16) -> Vec<u8> {
    let rendered = Icon::from(marker).render(tier);
    let mut out = Vec::new();
    // Writing into a `Vec` cannot fail; the results are only there because `queue!` is generic.
    let _ = queue!(
        out,
        MoveTo(0, rows.saturating_sub(1)),
        ClearLine(ClearType::CurrentLine),
        Print(rendered.to_terminal())
    );
    let room = usize::from(cols).saturating_sub(rendered.width() + 1);
    let note: String = footer_note(marker).chars().take(room).collect();
    if !note.is_empty() {
        let _ = queue!(out, Print(format!(" {note}")));
    }
    out
}
