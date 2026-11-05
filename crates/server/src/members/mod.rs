//! The members screens' drawing and request running (M2-F).
//!
//! The rules and the state machine are in `russh_bbs_core::profiles`; this module only turns a
//! [`ScreenView`] into ratatui lines for the classic hotkey layout (`crate::ui::classic`) and
//! runs the screen's request against the store with a time limit.
//!
//! **Where content goes.** Every line here is drawn into the content rectangle `ui::classic`
//! hands out. The status row (the last row, where the server-readable marker lives) is outside
//! that rectangle, so a profile or a handle list cannot reach it however long it is. Content
//! text is plain: it is never given the reserved marker style, and it is sanitized once more
//! here, on render, whatever core already did.

use std::time::Duration;

use ratatui::text::{Line as TextLine, Span};
use russh_bbs_core::access::Snapshot;
use russh_bbs_core::profiles::{
    ABOUT_MAX, Line, ProfileStore, Request, Response, ScreenView, execute, keys,
};
use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::waiting_room::NoticeKind;
use tokio::time::timeout;
use tracing::warn;

use crate::settings::Look;
use crate::ssh::terminal::Key;
use crate::ui::classic;
use crate::ui::screens::list::{ListView, Nav, NavKey, Sort, text_key};
use crate::ui::strings::StringTable;

/// How long a members request may take. A slow store must not hang the screen; the screen then
/// says it did not work and changes nothing.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// Run `request` for a session holding `access`. A timeout is [`Response::Failed`]: nothing is
/// claimed about whether a write happened beyond what the store reported.
pub(crate) async fn run<S: ProfileStore>(
    store: &S,
    access: &Snapshot,
    request: Request,
) -> Response {
    if let Ok(response) = timeout(REQUEST_TIMEOUT, execute(store, access, request)).await {
        response
    } else {
        warn!("members request timed out");
        Response::Failed
    }
}

/// Content text, sanitized once more on its way to the terminal.
fn clean(text: &str) -> String {
    SingleLine::new(ABOUT_MAX).sanitize(text).into_string()
}

/// Cut `text` to at most `width` terminal columns, so a list row never wraps onto a second
/// line and pushes the page off the screen.
fn fit(text: &str, width: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        out.push(c);
        if Span::raw(out.as_str()).width() > width {
            out.pop();
            break;
        }
    }
    out
}

/// One core line as a styled terminal line. Content is sanitized again here; `width` is the room
/// inside the box, used only to keep list rows on one line.
fn line_of(line: &Line, strings: &StringTable, look: Look, width: usize) -> TextLine<'static> {
    let (bold, dim, heading, key) = classic::styles(look);
    match line {
        Line::Blank => TextLine::raw(""),
        Line::Heading(text) => TextLine::styled(strings.text(text).to_owned(), heading),
        Line::Text(text) => TextLine::raw(strings.text(text).to_owned()),
        Line::Filled { key: text, values } => {
            let values: Vec<(&str, &str)> = values.iter().map(|(k, v)| (*k, v.as_str())).collect();
            TextLine::raw(strings.filled(text, &values))
        }
        Line::Pair { label, value } => {
            let mut spans = vec![Span::styled(format!("{}: ", strings.text(label)), bold)];
            spans.push(match value {
                Some(text) => Span::raw(clean(text)),
                None => Span::styled(strings.text(keys::VALUE_NOT_SET).to_owned(), dim),
            });
            TextLine::from(spans)
        }
        Line::Row {
            key: hotkey,
            handle,
            name,
        } => {
            let lead = format!("[{hotkey}] ");
            let handle = clean(handle);
            let used = lead.chars().count() + handle.chars().count() + 2;
            let name = fit(&clean(name), width.saturating_sub(used));
            TextLine::from(vec![
                Span::styled(lead, key),
                Span::raw(handle),
                Span::raw(format!("  {name}")),
            ])
        }
        Line::Field {
            label,
            value,
            active,
        } => {
            // The cursor is `>` and `_`, words and marks, so it shows without color.
            let (lead, tail) = if *active {
                ("> ", look.cursor())
            } else {
                ("  ", "")
            };
            let label_style = if *active { heading } else { bold };
            TextLine::from(vec![
                Span::styled(format!("{lead}{}: ", strings.text(label)), label_style),
                Span::raw(format!("{}{tail}", clean(value))),
            ])
        }
        Line::Notice { error, text } => classic::notice_line(
            strings,
            if *error {
                NoticeKind::Error
            } else {
                NoticeKind::Info
            },
            strings.text(text),
            look,
            bold,
        ),
    }
}

/// The lines of `view`, ready for [`classic::render_content`]. `width` is the room inside the
/// box, used only to keep list rows on one line.
pub(crate) fn lines(
    view: &ScreenView,
    strings: &StringTable,
    look: Look,
    width: usize,
) -> Vec<TextLine<'static>> {
    let (_, dim, _, _) = classic::styles(look);
    let mut out = Vec::with_capacity(view.lines.len() + 2);
    out.extend(
        view.lines
            .iter()
            .map(|line| line_of(line, strings, look, width)),
    );
    out.push(TextLine::raw(""));
    out.push(TextLine::styled(strings.text(view.footer).to_owned(), dim));
    out
}

/// Whether `view` is the members list (it has rows to show as a table).
pub(crate) fn is_list(view: &ScreenView) -> bool {
    view.lines
        .iter()
        .any(|line| matches!(line, Line::Row { .. }))
}

/// One member row as drawn: the key core opens it with, the handle and the display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShownRow<'a> {
    pub(crate) key: char,
    pub(crate) handle: &'a str,
    pub(crate) name: &'a str,
}

/// The rows of `view` in the order they are drawn: core's order, or sorted by `sort`. Each row
/// keeps its own key, so a sorted list still opens the member it shows.
pub(crate) fn display_rows(view: &ScreenView, sort: Sort) -> Vec<ShownRow<'_>> {
    let mut rows: Vec<ShownRow<'_>> = view
        .lines
        .iter()
        .filter_map(|line| match line {
            Line::Row { key, handle, name } => Some(ShownRow {
                key: *key,
                handle,
                name,
            }),
            _ => None,
        })
        .collect();
    sort.apply(&mut rows, |row, column| match column {
        0 => text_key(row.handle),
        _ => text_key(row.name),
    });
    rows
}

/// A cheap fingerprint of the rows on screen, so the cursor starts again on another page.
fn signature(rows: &[ShownRow<'_>]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for row in rows {
        row.handle.hash(&mut hasher);
    }
    hasher.finish()
}

/// What to deliver to core for `key` while the members list is up: the key itself, the digit of
/// the highlighted row for Enter, or nothing when the key only moved the highlight or the sort.
/// Digits and every other key go to core unchanged, so core's own keys (`/` search, `n`/`p`
/// pages, `c` clear) and its rules are exactly as before.
pub(crate) fn nav_key(nav: &mut Nav, view: &ScreenView, key: Key) -> Option<Key> {
    if !is_list(view) || view.footer == keys::LIST_FOOTER_SEARCH {
        return Some(key);
    }
    let rows = display_rows(view, nav.sort);
    nav.rows_are(signature(&rows));
    match nav.key(key, rows.len(), 2) {
        NavKey::Pass => Some(key),
        NavKey::Consumed => None,
        NavKey::Open(at) => rows.get(at).map(|row| Key::Char(row.key)),
    }
}

/// Everything the members list needs besides its view.
pub(crate) struct ListCtx<'a> {
    pub(crate) strings: &'a StringTable,
    pub(crate) look: Look,
    pub(crate) tier: russh_bbs_core::icons::Tier,
    pub(crate) markers: &'a [russh_bbs_core::icons::SecurityMarker],
    pub(crate) clock: Option<&'a str>,
    /// The highlight, the sort and the window of the list (kept by the session).
    pub(crate) nav: &'a Nav,
}

/// Draw the members list: the frame's bars, then a box holding the search line, a table of the
/// page's members (`1`-`9` pick a row, so the table has a number column and no cursor), the page
/// number and the key line. Handles and names are sanitized by the table's cells.
pub(crate) fn render_list(frame: &mut ratatui::Frame<'_>, view: &ScreenView, ctx: &ListCtx<'_>) {
    use ratatui::layout::{Constraint, Layout, Margin};
    use ratatui::widgets::Paragraph;

    use crate::ui::frame::{ChromeText, FrameSpec};
    use crate::ui::table::{self, Cell, CellStyle, Column, TableRow, TableSpec, Title, Width};

    let hints = classic::content_hints(ctx.strings);
    let body = FrameSpec::new(ctx.strings, ctx.look, ctx.tier)
        .markers(ctx.markers)
        .place(ChromeText::table(ctx.strings, keys::LIST_TITLE))
        .clock(ctx.clock)
        .hints(&hints)
        .draw(frame);
    let first_row = view
        .lines
        .iter()
        .position(|line| matches!(line, Line::Row { .. }))
        .unwrap_or(view.lines.len());
    let last_row = view
        .lines
        .iter()
        .rposition(|line| matches!(line, Line::Row { .. }))
        .map_or(first_row, |at| at + 1);
    let width = usize::from(body.width.min(crate::settings::MAX_WIDTH)).saturating_sub(4);
    let to_lines = |range: &[Line]| -> Vec<TextLine<'static>> {
        range
            .iter()
            .filter(|line| !matches!(line, Line::Heading(_) | Line::Blank))
            .map(|line| line_of(line, ctx.strings, ctx.look, width))
            .collect()
    };
    let before = to_lines(&view.lines[..first_row]);
    let mut after = to_lines(&view.lines[last_row..]);
    let (_, dim, _, _) = classic::styles(ctx.look);
    after.push(TextLine::styled(
        ctx.strings.text(view.footer).to_owned(),
        dim,
    ));
    let shown = display_rows(view, ctx.nav.sort);
    let rows: Vec<TableRow> = shown
        .iter()
        .map(|row| {
            TableRow::new(vec![
                Cell::plain(row.key.to_string()).styled(CellStyle::Dim),
                Cell::content(row.handle).styled(CellStyle::Key),
                Cell::content(row.name),
            ])
        })
        .collect();
    let outer = crate::ui::table::framed(ctx.look, Some(ctx.strings.text(keys::LIST_TITLE)));
    let inner = outer.inner(body).inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    frame.render_widget(outer, body);
    let [top, middle, bottom] = Layout::vertical([
        Constraint::Length(u16::try_from(before.len()).unwrap_or(0)),
        Constraint::Min(2),
        Constraint::Length(
            u16::try_from(after.len())
                .unwrap_or(0)
                .min(inner.height / 3),
        ),
    ])
    .areas(inner);
    frame.render_widget(Paragraph::new(before), top);
    let drawn = table::render(
        frame,
        middle,
        ctx.look,
        &TableSpec {
            title: Title::None,
            columns: vec![
                Column::new(ChromeText::fixed("Key"), Width::Fixed(3)),
                Column::new(ChromeText::fixed("Handle"), Width::Fixed(24)),
                Column::new(ChromeText::fixed("Name"), Width::Fill(1)),
            ],
            rows,
            selected: (!shown.is_empty()).then(|| ctx.nav.cursor.min(shown.len() - 1)),
            bordered: false,
            top: ctx.nav.view.get().top,
            empty: ctx.strings.text(keys::LIST_EMPTY),
            numbered: false,
            footer: None,
            sorted: ctx.nav.sort.mark().map(|(column, desc)| (column + 1, desc)),
        },
    );
    ctx.nav.view.set(ListView {
        top: drawn.top,
        shown: drawn.shown,
    });
    frame.render_widget(Paragraph::new(after), bottom);
}

#[cfg(test)]
mod tests;
