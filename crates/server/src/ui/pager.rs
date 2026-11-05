//! Drawing a [`Pager`]: long text a page at a time, honoring the member's pause setting
//! (FIX-M1-L8, D65). The paging rules are `russh_bbs_core::terminal_settings::Pager`; this
//! turns its view into the classic box with the prompt on the row under the text.
//!
//! The text is clipped, never wrapped, inside the box, so the caller wraps it to
//! [`text_width`] first ([`wrap`]) and a page is exactly [`text_rows`] rows. Prompts come from
//! the string table; a prompt always says what a key does in words.

use ratatui::layout::Rect;
use russh_bbs_core::terminal_settings::PagerPrompt;

use super::keys;
use super::strings::StringTable;

/// Rows of text one page holds in a frame of `area` (always at least one). The readers draw
/// in [`crate::ui::screens::reader`], which owns the geometry.
pub(crate) fn text_rows(area: Rect) -> usize {
    crate::ui::screens::reader::text_rows(area)
}

/// Columns of text one row holds in a frame of `area` (always at least one).
pub(crate) fn text_width(area: Rect) -> usize {
    crate::ui::screens::reader::text_width(area)
}

/// Break `text` into rows of at most `width` characters, one input line at a time, at spaces
/// where it can (a word longer than a row is cut). A blank input line stays a blank row.
/// Sanitized text only: it has no control characters to split.
pub(crate) fn wrap(text: &[String], width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for line in text {
        if line.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut row = String::new();
        let mut used = 0_usize;
        for word in line.split_inclusive(' ') {
            let len = word.chars().count();
            let trimmed = word.trim_end_matches(' ').chars().count();
            if used > 0 && used + trimmed > width {
                out.push(std::mem::take(&mut row));
                used = 0;
            }
            if len <= width || used > 0 {
                row.push_str(word);
                used += len;
                continue;
            }
            // A word wider than a row: cut it where the columns run out.
            for c in word.chars() {
                if used == width {
                    out.push(std::mem::take(&mut row));
                    used = 0;
                }
                row.push(c);
                used += 1;
            }
        }
        out.push(row);
    }
    out
}

/// [`wrap`] for a message body: a quoted line (`> text`, any depth) keeps its quote marks on
/// every row it wraps to, so the reader can draw the whole block as a quote.
pub(crate) fn wrap_quoted(text: &[String], width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in text {
        let marks = line.chars().take_while(|c| *c == '>').count();
        if marks == 0 {
            out.extend(wrap(std::slice::from_ref(line), width));
            continue;
        }
        let prefix = format!("{} ", ">".repeat(marks));
        let rest = line.trim_start_matches('>').trim_start().to_owned();
        let room = width.saturating_sub(prefix.chars().count()).max(1);
        for piece in wrap(&[rest], room) {
            out.push(format!("{prefix}{piece}"));
        }
    }
    out
}

/// The prompt row's words for what the pager says, from the string table.
pub(crate) fn prompt_text(prompt: PagerPrompt, strings: &StringTable) -> Option<String> {
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
