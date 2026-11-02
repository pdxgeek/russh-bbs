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
pub(crate) fn render(frame: &mut Frame<'_>, view: &View) {
    let area = frame.area();
    let [_, middle, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(MAX_WIDTH.min(area.width)),
        Constraint::Fill(1),
    ])
    .areas(area);
    let paragraph = Paragraph::new(lines(view))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", view.title))
                .title_alignment(Alignment::Center),
        );
    frame.render_widget(paragraph, fit(middle));
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

fn lines(view: &View) -> Vec<Line<'static>> {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let mut out = vec![Line::styled(view.heading, bold), Line::raw("")];

    for text in &view.banner {
        out.push(Line::raw(text.clone()));
    }
    if !view.banner.is_empty() {
        out.push(Line::raw(""));
    }
    for text in &view.body {
        out.push(Line::raw(text.clone()));
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
    out
}

pub(crate) use throttle::Throttle;
pub use throttle::{DEFAULT_IP_RATE, DEFAULT_KEY_RATE, DEFAULT_MAX_TRACKED};

#[cfg(test)]
mod tests;
