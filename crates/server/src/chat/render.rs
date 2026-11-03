//! Drawing the chat screen: [`ChatScreen`]'s page turned into lines for the classic content
//! box (`crate::ui::classic::render_content`), so the reserved status row, the theme's colors
//! and the member's layout all apply (M3-A's note to screens).
//!
//! **Where other people's words go.** Handles and lines were sanitized before they entered the
//! hub, and are cleaned again here. They are drawn in the content rectangle only (the status
//! row, where the security markers live, is outside it), in plain or palette styles, never in
//! the reserved marker style, and never as a label or key. A whisper is distinguished by words
//! ("whispers"), not only by color.

use ratatui::style::Style;
use ratatui::text::{Line as TextLine, Span};
use russh_bbs_core::chat::{
    ChatPage, ChatScreen, ListPage, MAX_LINE_CHARS, RoomPage, Row, strings,
};
use russh_bbs_core::sanitize::{SingleLine, Username};

use crate::settings::Look;
use crate::ui::classic;
use crate::ui::strings::StringTable;

/// Text from another person, sanitized once more on its way to the terminal.
fn clean(text: &str) -> String {
    SingleLine::new(MAX_LINE_CHARS).sanitize(text).into_string()
}

fn names(who: &[Username]) -> String {
    who.iter()
        .map(|n| clean(n.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Split `text` into pieces at most `width` columns wide. Always makes progress.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        let mut probe = current.clone();
        probe.push(c);
        if !current.is_empty() && Span::raw(probe.as_str()).width() > width {
            out.push(std::mem::take(&mut current));
        }
        current.push(c);
    }
    out.push(current);
    out
}

/// One transcript row as styled text, before wrapping.
fn row_text(row: &Row) -> (String, Option<RowKind>) {
    match row {
        Row::Say(line) => (
            format!(
                "<{}> {}",
                clean(line.from.as_str()),
                clean(line.text.as_str())
            ),
            None,
        ),
        Row::Joined(who) => (
            format!("* {} joined", clean(who.as_str())),
            Some(RowKind::Dim),
        ),
        Row::Left(who) => (
            format!("* {} left", clean(who.as_str())),
            Some(RowKind::Dim),
        ),
        Row::Whisper { from, text } => (
            format!(
                "* {} {}: {}",
                clean(from.as_str()),
                strings::WHISPER_FROM,
                clean(text.as_str())
            ),
            Some(RowKind::Whisper),
        ),
        Row::WhisperSent { to, text } => (
            format!(
                "* {} {}: {}",
                strings::WHISPER_TO,
                clean(to.as_str()),
                clean(text.as_str())
            ),
            Some(RowKind::Whisper),
        ),
        Row::Missed(n) => (
            format!("* {n} lines were skipped because this connection was slow"),
            Some(RowKind::Dim),
        ),
        Row::Here(who) => (
            format!("{}: {}", strings::HERE, names(who)),
            Some(RowKind::Dim),
        ),
        Row::Info(text) => ((*text).to_owned(), Some(RowKind::Dim)),
    }
}

#[derive(Clone, Copy)]
enum RowKind {
    Dim,
    Whisper,
}

/// The lines of the chat screen for a box `width` columns wide and `height` rows tall (the
/// room inside the box, status row and borders excluded).
pub(crate) fn lines(
    screen: &ChatScreen,
    strings_table: &StringTable,
    look: Look,
    width: usize,
    height: usize,
) -> Vec<TextLine<'static>> {
    let (bold, dim, heading, key) = classic::styles(look);
    let notice = screen.notice().map(|n| {
        let (kind, text) = n.text();
        classic::notice_line(strings_table, kind, text, look, bold)
    });
    let drawn = match screen.page() {
        ChatPage::List(list) => list_lines(
            list,
            screen.can_manage(),
            notice,
            (heading, dim, key),
            height,
        ),
        ChatPage::Room(room) => room_lines(room, notice, look, (heading, dim), width, height),
    };
    // Every line is cut to the box, so nothing wraps and pushes the input line out of view.
    drawn
        .into_iter()
        .map(|line| fit_line(line, width))
        .collect()
}

/// Cut a line to at most `width` columns, keeping each span's style.
fn fit_line(line: TextLine<'static>, width: usize) -> TextLine<'static> {
    let mut left = width;
    let mut spans = Vec::with_capacity(line.spans.len());
    for span in line.spans {
        if left == 0 {
            break;
        }
        let mut kept = String::new();
        for c in span.content.chars() {
            let w = Span::raw(c.to_string()).width();
            if w > left {
                left = 0;
                break;
            }
            kept.push(c);
            left -= w;
        }
        spans.push(Span::styled(kept, span.style));
    }
    TextLine::from(spans).style(line.style)
}

fn list_lines(
    list: &ListPage,
    manage: bool,
    notice: Option<TextLine<'static>>,
    (heading, dim, key): (Style, Style, Style),
    height: usize,
) -> Vec<TextLine<'static>> {
    let mut out = vec![
        TextLine::styled(strings::HEADING_ROOMS.to_owned(), heading),
        TextLine::raw(""),
    ];
    if list.rooms.is_empty() {
        out.push(TextLine::raw(strings::NO_ROOMS.to_owned()));
    }
    // Rows that fit between the heading and the prompt/notice/footer; keep the cursor in view.
    let room_rows = height.saturating_sub(6).max(1);
    let start = list.cursor.saturating_sub(room_rows - 1);
    for (index, room) in list.rooms.iter().enumerate().skip(start).take(room_rows) {
        let marker = if index == list.cursor { ">" } else { " " };
        out.push(TextLine::from(vec![
            Span::styled(format!("{marker} "), key),
            Span::raw(clean(room.name.as_str())),
        ]));
    }
    if let Some(name) = &list.naming {
        out.push(TextLine::raw(""));
        out.push(TextLine::styled(strings::PROMPT_NEW.to_owned(), dim));
        out.push(TextLine::raw(format!("> {}_", clean(name))));
    } else if list.closing.is_some() {
        out.push(TextLine::raw(""));
        out.push(TextLine::styled(strings::CONFIRM_CLOSE.to_owned(), dim));
    }
    if let Some(notice) = notice {
        out.push(TextLine::raw(""));
        out.push(notice);
    }
    out.push(TextLine::raw(""));
    let footer = if manage {
        format!("{}; {}", strings::FOOTER_ROOMS, strings::FOOTER_STAFF)
    } else {
        strings::FOOTER_ROOMS.to_owned()
    };
    out.push(TextLine::styled(footer, dim));
    out
}

fn room_lines(
    room: &RoomPage,
    notice: Option<TextLine<'static>>,
    look: Look,
    (heading, dim): (Style, Style),
    width: usize,
    height: usize,
) -> Vec<TextLine<'static>> {
    // Heading, then the transcript, then a notice or the hint, then the input line.
    let transcript_rows = height.saturating_sub(3).max(1);
    let mut wrapped: Vec<TextLine<'static>> = Vec::new();
    for row in room.rows.iter().rev() {
        let (text, kind) = row_text(row);
        let style = match kind {
            None => Style::new(),
            Some(RowKind::Dim) => dim,
            Some(RowKind::Whisper) => look.accent(Style::new(), look.palette().info),
        };
        for piece in wrap(&text, width).into_iter().rev() {
            wrapped.push(TextLine::styled(piece, style));
        }
        if wrapped.len() >= transcript_rows {
            break;
        }
    }
    wrapped.reverse();
    let skip = wrapped.len().saturating_sub(transcript_rows);
    let mut out = vec![TextLine::styled(
        format!("#{}", clean(room.room.name.as_str())),
        heading,
    )];
    out.extend(wrapped.into_iter().skip(skip));
    // Pad so the notice and input stay at the bottom of the box.
    while out.len() < transcript_rows + 1 {
        out.push(TextLine::raw(""));
    }
    out.push(notice.unwrap_or_else(|| TextLine::styled(strings::FOOTER_ROOM.to_owned(), dim)));
    // The input line shows its tail when it is longer than the box.
    let typed = clean(&room.input);
    let room_for_input = width.saturating_sub(3).max(1);
    let shown: String = {
        let chars: Vec<char> = typed.chars().collect();
        let from = chars.len().saturating_sub(room_for_input);
        chars.get(from..).unwrap_or_default().iter().collect()
    };
    out.push(TextLine::raw(format!("> {shown}_")));
    // A very short window drops the heading and the oldest rows first; the hint and the input
    // line stay at the bottom.
    let excess = out.len().saturating_sub(height.max(1));
    out.drain(..excess);
    out
}
