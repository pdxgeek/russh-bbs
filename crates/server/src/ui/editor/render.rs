//! Drawing the editor: a bordered box across the content area, with the text, the counters on
//! the bottom border, a notice row and a key row, and the reserved status row underneath.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::terminal_settings::strings::fill;

use super::{Editor, EditorView, Mode, strings};
use crate::settings::Look;
use crate::ui::{chrome, classic};

/// Narrowest and shortest box interior the editor draws in. Smaller and it says so in words
/// (the text and the cursor are kept).
const MIN_COLS: u16 = 24;
const MIN_ROWS: u16 = 6;

/// Printable-ASCII box for the ASCII tier (the same one the other screens use).
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

pub(super) fn render(editor: &Editor, frame: &mut Frame<'_>, view: &EditorView<'_>) {
    let look = Look::of(view.settings);
    let tier = view.settings.icon_tier();
    let (content, status) = chrome::split_status(frame.area());
    frame.render_widget(Clear, content);

    let (chars, lines) = editor.measure();
    let counts = fill(
        strings::COUNTS,
        &[
            ("lines", &lines.to_string()),
            ("max_lines", &editor.limits.max_lines().to_string()),
            ("chars", &chars.to_string()),
            ("max_chars", &editor.limits.max_chars().to_string()),
        ],
    );
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(look.accent(Style::new(), Color::Cyan))
        .title(format!(" {} ", editor.title.as_str()))
        .title_alignment(Alignment::Center)
        .title_bottom(format!(" {counts} "));
    if tier == Tier::Ascii {
        block = block.border_set(ASCII_BORDER);
    }
    let inner = block.inner(content);
    frame.render_widget(block, content);

    if inner.width < MIN_COLS || inner.height < MIN_ROWS {
        let text = fill(
            strings::TOO_SMALL,
            &[
                ("cols", &MIN_COLS.to_string()),
                ("rows", &MIN_ROWS.to_string()),
            ],
        );
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
    } else {
        let [text, notice, keys] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);
        draw_text(editor, frame, text, look);
        draw_notice(editor, frame, notice, view, look);
        draw_keys(editor, frame, keys);
    }
    if let Some(row) = status {
        chrome::render_status(frame, row, view.markers, tier, view.ui.strings());
    }
}

fn draw_text(editor: &Editor, frame: &mut Frame<'_>, area: Rect, look: Look) {
    if editor.mode == Mode::Help {
        let (_, dim, heading, key) = classic::styles(look);
        let mut lines = vec![
            Line::styled(strings::HELP_TITLE, heading),
            Line::raw(strings::HELP_INTRO),
            Line::raw(""),
        ];
        for (chord, what) in strings::HELP_KEYS {
            lines.push(Line::from(vec![
                ratatui::text::Span::styled(format!("{chord:<16}"), key),
                ratatui::text::Span::raw((*what).to_owned()),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(strings::HELP_FOOTER, dim));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
    } else {
        frame.render_widget(&editor.area, area);
    }
}

fn draw_notice(
    editor: &Editor,
    frame: &mut Frame<'_>,
    area: Rect,
    view: &EditorView<'_>,
    look: Look,
) {
    let Some(message) = &editor.message else {
        return;
    };
    let (bold, _, _, _) = classic::styles(look);
    let line = classic::notice_line(view.ui.strings(), message.kind, &message.text, look, bold);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_keys(editor: &Editor, frame: &mut Frame<'_>, area: Rect) {
    let dim = Style::new().add_modifier(Modifier::DIM);
    let line = match editor.mode {
        Mode::Leaving if editor.context.is_some() => Line::styled(
            strings::LEAVE_WITH_DRAFT,
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Mode::Leaving => Line::styled(
            strings::LEAVE_WITHOUT_DRAFT,
            Style::new().add_modifier(Modifier::BOLD),
        ),
        Mode::Editing | Mode::Help => Line::styled(strings::HINT, dim),
    };
    frame.render_widget(Paragraph::new(line), area);
}
