//! Drawing the waiting room. The rules (which pages exist, what a form does) live in
//! `russh_bbs_core::waiting_room`; this module only turns a [`View`] into ratatui widgets.
//!
//! Every string in a [`View`] is fixed chrome or already sanitized by the core, so nothing here
//! sanitizes again. Nothing here can name a member or a board: the view does not contain one.

mod throttle;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use russh_bbs_core::waiting_room::{NoticeKind, View};

/// Widest the box is drawn, so long lines stay readable on a wide terminal.
const MAX_WIDTH: u16 = 76;

/// Draw `view` centered in the frame.
///
/// The sysop's banner or rules text is the only part that can be long, so it gets the rows left
/// over after the menu, the notice and the footer have theirs (D64). A text that still does not
/// fit is clipped at the bottom of its own region; the menu is never pushed off screen.
pub(crate) fn render(frame: &mut Frame<'_>, view: &View) {
    let area = frame.area();
    let [_, middle, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(MAX_WIDTH.min(area.width)),
        Constraint::Fill(1),
    ])
    .areas(area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", view.title))
        .title_alignment(Alignment::Center);
    let outer = fit(middle);
    let inner = block.inner(outer);
    frame.render_widget(block, outer);

    let parts = parts(view);
    let wrap = |lines: Vec<Line<'static>>| Paragraph::new(lines).wrap(Wrap { trim: false });
    let head_rows = rows_of(&parts.head, inner.width);
    let tail_rows = rows_of(&parts.tail, inner.width);
    // The sysop text takes what it needs, but never the rows reserved for the head and tail.
    let room_left = inner
        .height
        .saturating_sub(head_rows)
        .saturating_sub(tail_rows);
    let flex_rows = rows_of(&parts.flex, inner.width).min(room_left);
    let [head, flex, tail, _] = Layout::vertical([
        Constraint::Length(head_rows),
        Constraint::Length(flex_rows),
        Constraint::Length(tail_rows),
        Constraint::Fill(1),
    ])
    .areas(inner);
    frame.render_widget(wrap(parts.head), head);
    frame.render_widget(wrap(parts.flex), flex);
    frame.render_widget(wrap(parts.tail), tail);
}

/// How many rows `lines` take when wrapped at `width`. Counts one extra row for every line
/// that wraps, which is the worst case for word wrapping, so the reserved space is never short.
fn rows_of(lines: &[Line<'_>], width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let rows: usize = lines
        .iter()
        .map(|line| {
            let w = line.width();
            if w <= width { 1 } else { w.div_ceil(width) + 1 }
        })
        .sum();
    u16::try_from(rows).unwrap_or(u16::MAX)
}

/// Keep the box off the very edge of a roomy terminal; use all of a small one.
fn fit(area: Rect) -> Rect {
    if area.height > 6 {
        area.inner(ratatui::layout::Margin {
            horizontal: 0,
            vertical: 1,
        })
    } else {
        area
    }
}

/// The page in three bands: a fixed heading, the sysop's flexible text, and everything after.
struct Parts {
    head: Vec<Line<'static>>,
    flex: Vec<Line<'static>>,
    tail: Vec<Line<'static>>,
}

fn parts(view: &View) -> Parts {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let head = vec![Line::styled(view.heading, bold), Line::raw("")];
    let flex: Vec<Line<'static>> = view
        .banner
        .iter()
        .chain(&view.body)
        .map(|text| Line::raw(text.clone()))
        .collect();
    let mut out = Vec::new();
    if !view.banner.is_empty() {
        out.push(Line::raw(""));
    }
    if let Some(note) = view.note {
        out.push(Line::raw(""));
        out.push(Line::styled(note, dim));
    }
    for item in &view.menu {
        out.push(Line::from(vec![
            Span::styled(format!("[{}]", item.key), bold),
            Span::raw(format!(" {}", item.label)),
        ]));
    }
    if !view.menu.is_empty() {
        let quit = View::quit_key();
        out.push(Line::from(vec![
            Span::styled(format!("[{}]", quit.key), bold),
            Span::raw(format!(" {}", quit.label)),
        ]));
    }
    for field in &view.fields {
        let cursor = if field.active { "_" } else { "" };
        out.push(Line::styled(format!("{}:", field.label), bold));
        out.push(Line::raw(format!("> {}{cursor}", field.value)));
    }
    if let Some(notice) = view.notice {
        // The word, not just a color: the message must read correctly on a mono terminal.
        let prefix = match notice.kind {
            NoticeKind::Info => "Notice",
            NoticeKind::Error => "Problem",
        };
        out.push(Line::raw(""));
        out.push(Line::styled(format!("{prefix}: {}", notice.text), bold));
    }
    out.push(Line::raw(""));
    out.push(Line::styled(view.footer, dim));
    Parts {
        head,
        flex,
        tail: out,
    }
}

pub(crate) use throttle::Throttle;
pub use throttle::{DEFAULT_IP_RATE, DEFAULT_KEY_RATE, DEFAULT_MAX_TRACKED};

#[cfg(test)]
mod tests;
