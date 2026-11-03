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
use russh_bbs_core::terminal_settings::{LayoutId, TerminalSettings};
use russh_bbs_core::waiting_room::View;

use crate::ui::classic::notice_line;
use crate::ui::strings::StringTable;
use crate::ui::theme::Palette;

/// Widest the box is drawn, so long lines stay readable on a wide terminal.
pub(crate) const MAX_WIDTH: u16 = 76;

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
    /// The compact layout draws no box (M3-A).
    borderless: bool,
    palette: Palette,
}

impl Look {
    /// The look for `settings`: unanswered glyph test and a rejected one both give the plain
    /// layout (ASCII, no color), because [`TerminalSettings`] says so.
    pub(crate) fn of(settings: &TerminalSettings) -> Self {
        Self::themed(settings, settings.layout(), Palette::DEFAULT)
    }

    /// The look for `settings` drawn in `layout` (the layout actually in force, which may be a
    /// fallback for a small terminal) with a theme's `palette`.
    pub(crate) fn themed(settings: &TerminalSettings, layout: LayoutId, palette: Palette) -> Self {
        Self {
            color: settings.use_color(),
            ascii_borders: settings.icon_tier() == Tier::Ascii,
            borderless: layout == LayoutId::Compact,
            palette,
        }
    }

    /// The theme's colors.
    pub(crate) const fn palette(self) -> Palette {
        self.palette
    }

    pub(crate) fn accent(self, style: Style, color: Color) -> Style {
        if self.color { style.fg(color) } else { style }
    }
}

/// Draw `view` centered in the frame. The view's fixed fields are string-table keys (the member
/// screens) or text (anything else); `strings` resolves the keys and leaves other text alone.
pub(crate) fn render(frame: &mut Frame<'_>, view: &View, look: Look, strings: &StringTable) {
    boxed(
        frame,
        frame.area(),
        view.title,
        lines(view, look, strings),
        look,
    );
}

/// Draw `lines` in the bordered box, centered in `area`. Shared with the menu framework's
/// layout (`crate::ui`), which draws its content here and keeps its own reserved rows outside
/// `area`.
pub(crate) fn boxed(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    look: Look,
) {
    let [_, middle, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(MAX_WIDTH.min(area.width)),
        Constraint::Fill(1),
    ])
    .areas(area);
    let border_style = look.accent(Style::new(), look.palette.border);
    let mut block = Block::default();
    if !look.borderless {
        block = block
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(format!(" {title} "))
            .title_alignment(Alignment::Center);
    }
    if look.ascii_borders {
        block = block.border_set(ASCII_BORDER);
    }
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block);
    frame.render_widget(paragraph, fit(middle));
}

/// The columns and rows inside the box [`boxed`] draws in `area`: what a caller has to fit its
/// text into if it wants to page or wrap it itself.
pub(crate) fn inner_size(area: Rect) -> (usize, usize) {
    let [_, middle, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(MAX_WIDTH.min(area.width)),
        Constraint::Fill(1),
    ])
    .areas(area);
    let boxed = fit(middle);
    (
        usize::from(boxed.width.saturating_sub(2)),
        usize::from(boxed.height.saturating_sub(2)),
    )
}

/// Keep the box off the very edge of a roomy terminal; use all of a small one, and all of an
/// 80x24 screen (the settings screen needs those two rows, M3-A).
fn fit(area: Rect) -> Rect {
    if area.height > 24 {
        area.inner(Margin {
            horizontal: 0,
            vertical: 1,
        })
    } else {
        area
    }
}

fn lines(view: &View, look: Look, strings: &StringTable) -> Vec<Line<'static>> {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let heading = look.accent(bold, look.palette.heading);
    let key = look.accent(bold, look.palette.key);
    let mut out = Vec::new();
    if !view.heading.is_empty() {
        out.push(Line::styled(strings.text(view.heading).to_owned(), heading));
        out.push(Line::raw(""));
    }
    for text in view.banner.iter().chain(&view.body) {
        out.push(Line::raw(text.clone()));
    }
    if let Some(note) = view.note {
        out.push(Line::raw(""));
        out.push(Line::styled(strings.text(note).to_owned(), dim));
    }
    if !view.menu.is_empty() {
        out.push(Line::raw(""));
    }
    for item in &view.menu {
        out.push(Line::from(vec![
            Span::styled(format!("[{}]", item.key), key),
            Span::raw(format!(" {}", strings.text(item.label))),
        ]));
    }
    for field in &view.fields {
        let cursor = if field.active { "_" } else { "" };
        out.push(Line::styled(
            format!("{}:", strings.text(field.label)),
            bold,
        ));
        out.push(Line::raw(format!("> {}{cursor}", field.value)));
    }
    if let Some(notice) = view.notice {
        // The word, not only a color: the message must read correctly on a mono terminal.
        out.push(Line::raw(""));
        out.push(notice_line(strings, notice.kind, notice.text, look, bold));
    }
    if !view.footer.is_empty() {
        out.push(Line::raw(""));
        out.push(Line::styled(strings.text(view.footer).to_owned(), dim));
    }
    out
}

#[cfg(test)]
mod tests;
