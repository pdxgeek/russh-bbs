//! Drawing the signed-in screens (login, glyph test, settings, home, logoff). The rules live in
//! `russh_bbs_core::terminal_settings`; this module only turns a [`View`] into ratatui widgets
//! according to the person's [`TerminalSettings`] (M1-E).
//!
//! * **Color off** draws with bold and dim only. Every state is also in words, so nothing
//!   depends on color.
//! * **ASCII tier** draws the border with `+ - |` instead of line-drawing characters, so the
//!   plain layout needs no Unicode at all.
//! * No security marker is drawn on these screens. When one is needed it must go through
//!   `Rendered::to_terminal()` / `chrome_style()` in a chrome-only position (D39), never through
//!   this renderer's content styles.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::terminal_settings::TerminalSettings;
use russh_bbs_core::waiting_room::{NoticeKind, View};

/// Widest the box is drawn, so long lines stay readable on a wide terminal.
const MAX_WIDTH: u16 = 76;

/// Printable-ASCII box: the border for the ASCII tier.
const ASCII_BORDER: border::Set = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

/// How a view is drawn, derived from the person's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Look {
    color: bool,
    ascii_borders: bool,
}

impl Look {
    /// The look for `settings`: unanswered glyph test and a rejected one both give the plain
    /// layout (ASCII, no color), because [`TerminalSettings`] says so.
    pub(crate) fn of(settings: &TerminalSettings) -> Self {
        Self {
            color: settings.use_color(),
            ascii_borders: settings.icon_tier() == Tier::Ascii,
        }
    }

    fn accent(self, style: Style, color: Color) -> Style {
        if self.color { style.fg(color) } else { style }
    }
}

/// Draw `view` centered in the frame.
pub(crate) fn render(frame: &mut Frame<'_>, view: &View, look: Look) {
    let area = frame.area();
    let [_, middle, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(MAX_WIDTH.min(area.width)),
        Constraint::Fill(1),
    ])
    .areas(area);
    let border_style = look.accent(Style::new(), Color::Cyan);
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(format!(" {} ", view.title))
        .title_alignment(Alignment::Center);
    if look.ascii_borders {
        block = block.border_set(ASCII_BORDER);
    }
    let paragraph = Paragraph::new(lines(view, look))
        .wrap(Wrap { trim: false })
        .block(block);
    frame.render_widget(paragraph, fit(middle));
}

/// Keep the box off the very edge of a roomy terminal; use all of a small one.
fn fit(area: Rect) -> Rect {
    if area.height > 6 {
        area.inner(Margin {
            horizontal: 0,
            vertical: 1,
        })
    } else {
        area
    }
}

fn lines(view: &View, look: Look) -> Vec<Line<'static>> {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let heading = look.accent(bold, Color::Cyan);
    let key = look.accent(bold, Color::Yellow);
    let mut out = Vec::new();
    if !view.heading.is_empty() {
        out.push(Line::styled(view.heading, heading));
        out.push(Line::raw(""));
    }
    for text in view.banner.iter().chain(&view.body) {
        out.push(Line::raw(text.clone()));
    }
    if let Some(note) = view.note {
        out.push(Line::raw(""));
        out.push(Line::styled(note, dim));
    }
    if !view.menu.is_empty() {
        out.push(Line::raw(""));
    }
    for item in &view.menu {
        out.push(Line::from(vec![
            Span::styled(format!("[{}]", item.key), key),
            Span::raw(format!(" {}", item.label)),
        ]));
    }
    for field in &view.fields {
        let cursor = if field.active { "_" } else { "" };
        out.push(Line::styled(format!("{}:", field.label), bold));
        out.push(Line::raw(format!("> {}{cursor}", field.value)));
    }
    if let Some(notice) = view.notice {
        // The word, not only a color: the message must read correctly on a mono terminal.
        let (prefix, color) = match notice.kind {
            NoticeKind::Info => ("Notice", Color::Green),
            NoticeKind::Error => ("Problem", Color::Red),
        };
        out.push(Line::raw(""));
        out.push(Line::styled(
            format!("{prefix}: {}", notice.text),
            look.accent(bold, color),
        ));
    }
    if !view.footer.is_empty() {
        out.push(Line::raw(""));
        out.push(Line::styled(view.footer, dim));
    }
    out
}

#[cfg(test)]
mod tests;
