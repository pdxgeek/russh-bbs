//! Drawing a [`Pager`]: long text a page at a time, honoring the member's pause setting
//! (FIX-M1-L8, D65). The paging rules are `russh_bbs_core::terminal_settings::Pager`; this
//! turns its view into the classic box with the prompt on the row under the text.
//!
//! The text is clipped, never wrapped, inside the box, so the caller wraps it to
//! [`text_width`] first ([`wrap`]) and a page is exactly [`text_rows`] rows. Prompts come from
//! the string table; a prompt always says what a key does in words.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::terminal_settings::{Pager, PagerPrompt};

use super::chrome;
use super::classic::MENU_MARKERS;
use super::keys;
use super::strings::StringTable;
use crate::settings::{self, Look};

/// Rows the prompt takes under the text: a blank row and the prompt itself.
const PROMPT_ROWS: usize = 2;

/// Rows of text one page holds in a frame of `area` (always at least one).
pub(crate) fn text_rows(area: Rect) -> usize {
    let (content, _) = chrome::split_status(area, &MENU_MARKERS);
    settings::inner_size(content)
        .1
        .saturating_sub(PROMPT_ROWS)
        .max(1)
}

/// Columns of text one row holds in a frame of `area` (always at least one).
pub(crate) fn text_width(area: Rect) -> usize {
    let (content, _) = chrome::split_status(area, &MENU_MARKERS);
    settings::inner_size(content).0.max(1)
}

/// Break `text` into rows of at most `width` characters, one input line at a time. A blank
/// input line stays a blank row. Sanitized text only: it has no control characters to split.
pub(crate) fn wrap(text: &[String], width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for line in text {
        let mut chars = line.chars().peekable();
        if chars.peek().is_none() {
            out.push(String::new());
        }
        while chars.peek().is_some() {
            out.push(chars.by_ref().take(width).collect());
        }
    }
    out
}

/// Draw the current page of `pager` with its prompt, and the status row beneath.
pub(crate) fn render(
    frame: &mut Frame<'_>,
    pager: &Pager,
    title: &str,
    strings: &StringTable,
    look: Look,
    tier: Tier,
    markers: &[SecurityMarker],
) {
    let (content, status) = chrome::split_status(frame.area(), markers);
    let view = pager.view(text_rows(frame.area()));
    let dim = Style::new().add_modifier(Modifier::DIM);
    let mut lines: Vec<Line<'static>> = view
        .lines
        .iter()
        .map(|text| Line::raw(text.clone()))
        .collect();
    if let Some(prompt) = prompt_text(view.prompt, strings) {
        lines.push(Line::raw(""));
        lines.push(Line::styled(prompt, dim));
    }
    settings::boxed(frame, content, title, lines, look);
    if let Some(row) = status {
        chrome::render_status(frame, row, markers, tier, strings);
    }
}

fn prompt_text(prompt: PagerPrompt, strings: &StringTable) -> Option<String> {
    match prompt {
        PagerPrompt::None => None,
        PagerPrompt::More { page, pages } => Some(strings.filled(
            keys::PAGER_MORE,
            &[("page", &page.to_string()), ("pages", &pages.to_string())],
        )),
        PagerPrompt::End { page, pages } => Some(strings.filled(
            keys::PAGER_END,
            &[("page", &page.to_string()), ("pages", &pages.to_string())],
        )),
        PagerPrompt::Clipped { hidden } => {
            Some(strings.filled(keys::PAGER_CLIPPED, &[("hidden", &hidden.to_string())]))
        }
    }
}
