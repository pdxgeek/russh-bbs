//! Drawing the forum screens. Every line is fixed chrome text or member text that core
//! sanitized and that is cleaned again here; it goes into the content box only, so it cannot
//! reach the reserved status row (D39, D45), and no style is applied to it except the plain
//! ones below.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::terminal_settings::{TerminalSettings, format_utc};
use russh_bbs_core::waiting_room::NoticeKind;

use russh_bbs_core::forums::{SearchPage, ThreadPage};

use super::exec::BoardList;
use super::flow::{ForumsScreen, Input, InputKind, ListView, State, confirm_text, reader_title};
use super::strings;
use crate::settings::{self, Look};
use crate::ui::classic::{self, MENU_MARKERS};
use crate::ui::editor::EditorView;
use crate::ui::frame::{ChromeText, FrameSpec, Hint};
use crate::ui::popup::{self, Popup, PopupKind, PopupLine};
use crate::ui::strings::StringTable;
use crate::ui::table::{self, Cell, CellStyle, Column, TableRow, TableSpec, Title, Width};
use crate::ui::{Ui, pager};

/// What drawing needs besides the screen.
pub(crate) struct DrawCtx<'a> {
    pub(crate) ui: &'a Ui,
    pub(crate) strings: &'a StringTable,
    pub(crate) settings: &'a TerminalSettings,
    pub(crate) look: Look,
    pub(crate) tier: Tier,
    /// The markers the status bar's marker cell shows (FIX-M3-6).
    pub(crate) markers: &'a [russh_bbs_core::icons::SecurityMarker],
    /// The header bar's clock text.
    pub(crate) clock: Option<&'a str>,
}

/// Member text, cleaned once more on its way to the terminal.
fn clean(text: &str, max: usize) -> String {
    SingleLine::new(max).sanitize(text).into_string()
}

/// Cut `text` to at most `width` terminal columns so a row never wraps.
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

/// Draw whatever the forum screen is showing into `frame`.
pub(crate) fn render(frame: &mut Frame<'_>, screen: &ForumsScreen, ctx: &DrawCtx<'_>) {
    let area = frame.area();
    if let Some(editor) = screen.editor() {
        editor.with(|editor| {
            editor.render(
                frame,
                &EditorView {
                    ui: ctx.ui,
                    settings: ctx.settings,
                    markers: &MENU_MARKERS,
                },
            );
        });
        return;
    }
    if let Some((reader, stored, width)) = screen.reader_pager() {
        render_reader(frame, area, screen, reader, stored, width, ctx);
        return;
    }
    let hints = state_hints(&screen.state);
    let body = FrameSpec::new(ctx.strings, ctx.look, ctx.tier)
        .markers(ctx.markers)
        .place(ChromeText::fixed(strings::TITLE_BOARDS))
        .clock(ctx.clock)
        .hints(&hints)
        .draw(frame);
    match &screen.state {
        State::Boards { list, cursor } => board_table(frame, body, screen, ctx, list, *cursor),
        State::Threads { page, cursor } => thread_table(frame, body, screen, ctx, page, *cursor),
        State::Results {
            page,
            query,
            cursor,
        } => result_table(frame, body, screen, ctx, page, query, *cursor),
        State::Start | State::Reader(_) | State::Editor(_) | State::Input(_) => {
            let (width, rows) = settings::inner_size(body);
            let _ = (width, rows);
            settings::boxed(
                frame,
                body,
                ctx.strings.text(crate::ui::keys::TITLE),
                lines(screen, ctx),
                ctx.look,
            );
        }
    }
    // A problem is a modal over the screen; the key that dismisses it also acts, as before.
    if let Some((NoticeKind::Error, text)) = &screen.notice {
        let keys = [Hint::new(
            ChromeText::fixed("any key"),
            ChromeText::fixed("close"),
        )];
        popup::render(
            frame,
            body,
            ctx.look,
            &Popup {
                kind: PopupKind::Error,
                title: ChromeText::table(ctx.strings, crate::ui::keys::NOTICE_ERROR),
                lines: vec![PopupLine::content(text)],
                keys: &keys,
            },
        );
    }
}

/// The status-bar hints for a forum screen: fixed words, never member text.
fn state_hints(state: &State) -> Vec<Hint<'static>> {
    let hint = |key: &'static str, label: &'static str| {
        Hint::new(ChromeText::fixed(key), ChromeText::fixed(label))
    };
    match state {
        State::Boards { list, .. } => {
            let mut hints = vec![
                hint("Enter", "open"),
                hint("1-9", "jump"),
                hint("s", "search"),
            ];
            if list.can_manage {
                hints.push(hint("n", "new board"));
            }
            if list.can_feeds {
                hints.push(hint("f", "add a feed"));
            }
            hints.push(hint("Esc", "back"));
            hints
        }
        State::Threads { page, .. } => {
            let mut hints = vec![hint("Enter", "read"), hint("1-9", "jump")];
            if page.board.feed {
                hints.push(hint("feed", "read only"));
            } else {
                hints.push(hint("w", "write"));
            }
            hints.push(hint("n/b", "page"));
            hints.push(hint("s", "search"));
            if page.can_feeds && (!page.board.restricted || page.board.feed) {
                hints.push(hint("p", "public RSS"));
            }
            if page.board.feed && page.can_feeds {
                hints.push(hint("x", "remove feed"));
            }
            if page.can_manage && page.board.restricted {
                hints.push(hint("a", "let in"));
            }
            hints.push(hint("Esc", "back"));
            hints
        }
        State::Results { .. } => vec![
            hint("Enter", "read"),
            hint("1-9", "jump"),
            hint("n/b", "page"),
            hint("Esc", "back"),
        ],
        State::Input(_) => vec![hint("Enter", "continue"), hint("Esc", "cancel")],
        State::Start | State::Reader(_) | State::Editor(_) => vec![hint("Esc", "back")],
    }
}

fn render_reader(
    frame: &mut Frame<'_>,
    area: Rect,
    screen: &ForumsScreen,
    reader: &super::flow::Reader,
    stored: &russh_bbs_core::terminal_settings::Pager,
    stored_width: usize,
    ctx: &DrawCtx<'_>,
) {
    let width = pager::text_width(area);
    let rebuilt;
    let pager_ref = if width == stored_width {
        stored
    } else {
        rebuilt = screen.pager_for(reader, width);
        &rebuilt
    };
    let title = fit(&clean(&reader_title(reader), 120), width.saturating_sub(4));
    pager::render(
        frame,
        pager_ref,
        &title,
        ctx.strings,
        ctx.look,
        ctx.tier,
        &MENU_MARKERS,
    );
    // The remove confirmation is a modal over the page.
    if let Some(text) = confirm_text(reader) {
        let keys = [
            Hint::new(ChromeText::fixed("y"), ChromeText::fixed("remove")),
            Hint::new(
                ChromeText::fixed("any other key"),
                ChromeText::fixed("keep"),
            ),
        ];
        popup::render(
            frame,
            area,
            ctx.look,
            &Popup {
                kind: PopupKind::Confirm,
                title: ChromeText::fixed("Confirm"),
                lines: vec![
                    PopupLine::content(&clean(&reader_title(reader), 120)),
                    PopupLine::plain(text),
                ],
                keys: &keys,
            },
        );
    }
}

/// An info notice as a line in a prompt screen. A problem is not drawn here: it is a modal
/// popup over the screen (see [`render`]).
fn notice(screen: &ForumsScreen, ctx: &DrawCtx<'_>, out: &mut Vec<Line<'static>>) {
    if let Some((kind @ NoticeKind::Info, text)) = &screen.notice {
        let (bold, _, _, _) = classic::styles(ctx.look);
        out.push(classic::notice_line(
            ctx.strings,
            *kind,
            text,
            ctx.look,
            bold,
        ));
    }
}

/// Remember which rows the table drew, so paging and jump keys know (FIX-M3-4).
fn remember(screen: &ForumsScreen, drawn: table::TableView) {
    screen.view.set(ListView {
        top: drawn.top,
        shown: drawn.shown,
    });
}

/// "3-9 of 40" under a table that scrolls; nothing when every row is shown.
fn position(drawn: table::TableView, total: usize) -> Option<String> {
    (drawn.shown < total)
        .then(|| format!("{}-{} of {total}", drawn.top + 1, drawn.top + drawn.shown))
}

/// A notice that is not a problem stays a line under the table (problems are modals).
fn info_footer(screen: &ForumsScreen, ctx: &DrawCtx<'_>) -> Option<String> {
    match &screen.notice {
        Some((NoticeKind::Info, text)) => Some(format!(
            "{}: {text}",
            ctx.strings.text(crate::ui::keys::NOTICE_INFO)
        )),
        _ => None,
    }
}

fn footer_of(parts: &[Option<String>]) -> Option<String> {
    let text: Vec<&str> = parts.iter().flatten().map(String::as_str).collect();
    (!text.is_empty()).then(|| text.join("  -  "))
}

fn board_table(
    frame: &mut Frame<'_>,
    body: Rect,
    screen: &ForumsScreen,
    ctx: &DrawCtx<'_>,
    list: &BoardList,
    cursor: usize,
) {
    let columns = vec![
        Column::new(ChromeText::fixed("Board"), Width::Fill(2)),
        Column::new(ChromeText::fixed("Threads"), Width::Fixed(7)).right(),
        Column::new(ChromeText::fixed("About"), Width::Fill(3)).hide_below(60),
    ];
    let rows: Vec<TableRow> = list
        .boards
        .iter()
        .map(|board| {
            let tag = if board.feed {
                format!(" [{}]", strings::FEED_BOARD_TAG)
            } else {
                String::new()
            };
            TableRow::new(vec![
                Cell::content(&format!("{}{tag}", clean(&board.name, 40))).styled(CellStyle::Bold),
                Cell::plain(board.threads.to_string()),
                Cell::content(&board.description).styled(CellStyle::Dim),
            ])
        })
        .collect();
    let total = rows.len();
    let previous = screen.view.get();
    let drawn = table::render(
        frame,
        body,
        ctx.look,
        &TableSpec {
            title: Title::Chrome(ChromeText::fixed(strings::TITLE_BOARDS)),
            columns,
            rows,
            selected: Some(cursor),
            bordered: true,
            top: previous.top,
            empty: strings::NO_BOARDS,
            numbered: true,
            footer: None,
        },
    );
    remember(screen, drawn);
    draw_info(frame, body, screen, ctx, position(drawn, total));
}

fn thread_table(
    frame: &mut Frame<'_>,
    body: Rect,
    screen: &ForumsScreen,
    ctx: &DrawCtx<'_>,
    page: &ThreadPage,
    cursor: usize,
) {
    let columns = vec![
        Column::new(ChromeText::fixed("Subject"), Width::Fill(3)),
        Column::new(ChromeText::fixed("From"), Width::Fixed(14)).hide_below(60),
        Column::new(ChromeText::fixed("Posts"), Width::Fixed(5)).right(),
        Column::new(ChromeText::fixed("Last post"), Width::Fixed(20)).hide_below(78),
    ];
    let rows: Vec<TableRow> = page
        .threads
        .iter()
        .map(|thread| {
            let who = thread.author.as_deref().unwrap_or(strings::GONE_MEMBER);
            TableRow::new(vec![
                Cell::content(&thread.subject),
                Cell::content(who).styled(CellStyle::Key),
                Cell::plain(thread.posts.to_string()),
                Cell::plain(format_utc(thread.last_post_at)).styled(CellStyle::Dim),
            ])
        })
        .collect();
    let total = rows.len();
    let previous = screen.view.get();
    let board_name = clean(&page.board.name, 40);
    let drawn = table::render(
        frame,
        body,
        ctx.look,
        &TableSpec {
            title: Title::Content(&board_name),
            columns,
            rows,
            selected: Some(cursor),
            bordered: true,
            top: previous.top,
            empty: strings::NO_THREADS,
            numbered: true,
            footer: None,
        },
    );
    remember(screen, drawn);
    let about = (!page.board.description.is_empty()).then(|| clean(&page.board.description, 160));
    draw_info(
        frame,
        body,
        screen,
        ctx,
        footer_of(&[
            Some(format!("Page {}", page.page + 1)),
            position(drawn, total),
            about,
        ]),
    );
}

fn result_table(
    frame: &mut Frame<'_>,
    body: Rect,
    screen: &ForumsScreen,
    ctx: &DrawCtx<'_>,
    page: &SearchPage,
    query: &str,
    cursor: usize,
) {
    let columns = vec![
        Column::new(ChromeText::fixed("Board"), Width::Fixed(16)).hide_below(60),
        Column::new(ChromeText::fixed("Subject"), Width::Fill(1)),
    ];
    let rows: Vec<TableRow> = page
        .hits
        .iter()
        .map(|hit| {
            TableRow::new(vec![
                Cell::content(&hit.board_name).styled(CellStyle::Key),
                Cell::content(&hit.subject).styled(CellStyle::Bold),
            ])
            .with_detail(Cell::content(&hit.snippet))
        })
        .collect();
    let total = rows.len();
    let previous = screen.view.get();
    let title = format!("{} for: {}", strings::TITLE_SEARCH, clean(query, 200));
    let empty = if page.searched {
        strings::NO_RESULTS
    } else {
        strings::NOT_SEARCHED
    };
    let drawn = table::render(
        frame,
        body,
        ctx.look,
        &TableSpec {
            title: Title::Content(&title),
            columns,
            rows,
            selected: Some(cursor),
            bordered: true,
            top: previous.top,
            empty,
            numbered: true,
            footer: None,
        },
    );
    remember(screen, drawn);
    draw_info(frame, body, screen, ctx, position(drawn, total));
}

/// The one line under a list's border: an info notice, or the position. Drawn on the table's
/// bottom border row so the list keeps every row.
fn draw_info(
    frame: &mut Frame<'_>,
    body: Rect,
    screen: &ForumsScreen,
    ctx: &DrawCtx<'_>,
    other: Option<String>,
) {
    let text = footer_of(&[info_footer(screen, ctx), other]);
    let Some(text) = text else { return };
    if body.height < 3 || body.width < 6 {
        return;
    }
    let row = Rect {
        x: body.x + 2,
        y: body.y + body.height - 1,
        width: body.width - 4,
        height: 1,
    };
    let dim = Style::new().add_modifier(Modifier::DIM);
    let clipped = table::clip(
        &text,
        usize::from(row.width.saturating_sub(2)),
        ctx.look.ascii(),
    );
    frame.render_widget(Line::styled(format!(" {clipped} "), dim), row);
}

/// The lines of the screens that are not lists: the start screen and the one-line prompts.
fn lines(screen: &ForumsScreen, ctx: &DrawCtx<'_>) -> Vec<Line<'static>> {
    let (_, dim, heading, _) = classic::styles(ctx.look);
    let mut out = Vec::new();
    if let State::Input(input) = &screen.state {
        input_lines(screen, input, heading, dim, ctx, &mut out);
    } else {
        out.push(Line::styled(strings::TITLE_BOARDS.to_owned(), heading));
        out.push(Line::raw(""));
        notice(screen, ctx, &mut out);
    }
    out
}

fn input_lines(
    screen: &ForumsScreen,
    input: &Input,
    heading: Style,
    dim: Style,
    ctx: &DrawCtx<'_>,
    out: &mut Vec<Line<'static>>,
) {
    let (title, prompt) = match &input.kind {
        InputKind::Search => (strings::TITLE_SEARCH, strings::PROMPT_SEARCH),
        InputKind::Subject { .. } => (strings::TITLE_SUBJECT, strings::PROMPT_SUBJECT),
        InputKind::BoardName => (strings::TITLE_NEW_BOARD, strings::PROMPT_BOARD_NAME),
        InputKind::BoardDescription { .. } => {
            (strings::TITLE_NEW_BOARD, strings::PROMPT_BOARD_DESCRIPTION)
        }
        InputKind::BoardRestricted { .. } => {
            (strings::TITLE_NEW_BOARD, strings::PROMPT_BOARD_RESTRICTED)
        }
        InputKind::Allow { .. } => (strings::TITLE_ALLOW, strings::PROMPT_ALLOW),
        InputKind::FeedUrl => (strings::TITLE_FEED, strings::PROMPT_FEED_URL),
        InputKind::FeedName { .. } => (strings::TITLE_FEED, strings::PROMPT_FEED_NAME),
        InputKind::FeedHttp { .. } => (strings::TITLE_FEED, strings::PROMPT_FEED_HTTP),
        InputKind::FeedRemove { .. } => (strings::TITLE_FEED_REMOVE, strings::PROMPT_FEED_REMOVE),
    };
    out.push(Line::styled(title.to_owned(), heading));
    out.push(Line::raw(""));
    out.push(Line::raw(prompt.to_owned()));
    if !matches!(
        input.kind,
        InputKind::BoardRestricted { .. }
            | InputKind::FeedHttp { .. }
            | InputKind::FeedRemove { .. }
    ) {
        out.push(Line::raw(format!(
            "> {}{}",
            clean(&input.text, 200),
            ctx.look.cursor()
        )));
    }
    out.push(Line::raw(""));
    notice(screen, ctx, out);
    out.push(Line::styled(strings::HELP_INPUT.to_owned(), dim));
}
