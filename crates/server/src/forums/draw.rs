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

use russh_bbs_core::forums::{SearchPage, ThreadPage};

use super::exec::BoardList;
use super::flow::{ForumsScreen, Input, InputKind, ListView, State, confirm_text, reader_title};
use super::strings;
use crate::settings::{self, Look};
use crate::ui::classic::{self, ContentScreen, MENU_MARKERS};
use crate::ui::editor::EditorView;
use crate::ui::strings::StringTable;
use crate::ui::{Ui, chrome, pager};

/// What drawing needs besides the screen.
pub(crate) struct DrawCtx<'a> {
    pub(crate) ui: &'a Ui,
    pub(crate) strings: &'a StringTable,
    pub(crate) settings: &'a TerminalSettings,
    pub(crate) look: Look,
    pub(crate) tier: Tier,
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

fn row(selected: bool, number: usize, text: &str, width: usize, bold: Style) -> Line<'static> {
    let lead = if selected { "> " } else { "  " };
    let n = if number < 9 {
        format!("{}. ", number + 1)
    } else {
        "   ".to_owned()
    };
    let text = fit(text, width.saturating_sub(lead.len() + n.len()));
    let style = if selected {
        bold.add_modifier(Modifier::UNDERLINED)
    } else {
        Style::new()
    };
    Line::styled(format!("{lead}{n}{text}"), style)
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
    let (content, _) = chrome::split_status(area, &MENU_MARKERS);
    let (width, rows) = settings::inner_size(content);
    classic::render_content(
        frame,
        ContentScreen {
            strings: ctx.strings,
            lines: lines(screen, Space { width, rows }, ctx),
            look: ctx.look,
            tier: ctx.tier,
            markers: &MENU_MARKERS,
        },
    );
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
    // The remove confirmation takes the place of the prompt row.
    if let Some(text) = confirm_text(reader) {
        let (content, status) = chrome::split_status(area, &MENU_MARKERS);
        let (bold, _, _, _) = classic::styles(ctx.look);
        let lines = vec![
            Line::styled(clean(&reader_title(reader), 120), bold),
            Line::raw(""),
            Line::raw(text.to_owned()),
        ];
        settings::boxed(frame, content, strings::TITLE_BOARDS, lines, ctx.look);
        if let Some(row) = status {
            chrome::render_status(frame, row, &MENU_MARKERS, ctx.tier, ctx.strings);
        }
        return;
    }
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
}

fn notice(screen: &ForumsScreen, ctx: &DrawCtx<'_>, out: &mut Vec<Line<'static>>) {
    if let Some((kind, text)) = &screen.notice {
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

/// The styles every list screen uses, bundled to keep the argument lists short.
#[derive(Clone, Copy)]
struct Styles {
    bold: Style,
    dim: Style,
    heading: Style,
}

/// The room inside the box: columns and rows.
#[derive(Clone, Copy)]
struct Space {
    width: usize,
    rows: usize,
}

/// How many terminal rows `text` takes when the box wraps it at `width` columns (words wrap,
/// a word longer than a row is cut). Counts a little high rather than low.
fn wrapped_rows(text: &str, width: usize) -> usize {
    let width = width.max(1);
    let mut rows = 1;
    let mut used = 0;
    for word in text.split(' ') {
        let w = Span::raw(word).width();
        if used > 0 && used + 1 + w > width {
            rows += 1;
            used = 0;
        }
        if used > 0 {
            used += 1;
        }
        let mut w = w;
        while w > width {
            rows += 1;
            w -= width;
        }
        used += w;
    }
    rows
}

fn rows_of(lines: &[Line<'_>], width: usize) -> usize {
    lines
        .iter()
        .map(|line| wrapped_rows(&line.to_string(), width))
        .sum()
}

/// One list screen: what goes above the items, the items, what goes below them.
struct ListScreen<'a> {
    head: Vec<Line<'static>>,
    /// Shown instead of items when there are none.
    empty: Option<&'a str>,
    total: usize,
    cursor: usize,
    /// Terminal rows one item takes.
    item_rows: usize,
    /// Lines under the items and above the help (a page number). Dropped first when short.
    extra: Vec<Line<'static>>,
    help: &'a str,
}

/// Lay out a list so the selected item is always drawn and the notice line is never pushed
/// off (FIX-M3-4). The window follows the cursor and is remembered in `screen.view`. When
/// the items do not all fit, a position line says which ones are shown; when even that does
/// not leave room for one item, the help and the page line go first.
fn list_lines(
    screen: &ForumsScreen,
    space: Space,
    ctx: &DrawCtx<'_>,
    styles: Styles,
    list: ListScreen<'_>,
    item: impl Fn(usize, usize) -> Vec<Line<'static>>,
) -> Vec<Line<'static>> {
    let mut notice_lines = Vec::new();
    notice(screen, ctx, &mut notice_lines);
    let mut tail = list.extra;
    tail.push(Line::raw(""));
    tail.push(Line::styled(list.help.to_owned(), styles.dim));
    let used_notice = rows_of(&notice_lines, space.width);
    let item_rows = list.item_rows.max(1);
    // On a very small screen the selected row and the notice outrank the heading's extra
    // lines (a description, a blank): drop those until one row fits.
    let mut head = list.head;
    while head.len() > 1 && rows_of(&head, space.width) + used_notice + item_rows > space.rows {
        head.remove(1);
    }
    let used_head = rows_of(&head, space.width);
    let used_tail = rows_of(&tail, space.width);

    let room = |tail_rows: usize, position: usize| {
        space
            .rows
            .saturating_sub(used_head + used_notice + tail_rows + position)
            / item_rows
    };
    // Prefer everything; then drop the help and page lines; the position line goes last
    // because it only helps, while the selected row and the notice must stay.
    let total = list.total;
    let (show_tail, position, fits) = [(true, false), (true, true), (false, false), (false, true)]
        .into_iter()
        .map(|(tail, pos)| {
            let fits = room(if tail { used_tail } else { 0 }, usize::from(pos));
            (tail, pos, fits)
        })
        .find(|&(_, pos, fits)| fits >= 1 && (pos || total <= fits))
        .unwrap_or((false, false, 1));
    let position = usize::from(position);
    let visible = fits.max(1).min(list.total.max(1));

    let previous = screen.view.get();
    let mut top = previous.top.min(list.total.saturating_sub(visible));
    let cursor = list.cursor.min(list.total.saturating_sub(1));
    if cursor < top {
        top = cursor;
    } else if cursor >= top + visible {
        top = cursor + 1 - visible;
    }
    let shown = visible.min(list.total.saturating_sub(top));
    screen.view.set(ListView { top, shown });

    let mut out = head;
    match list.empty {
        Some(text) if list.total == 0 => out.push(Line::raw(text.to_owned())),
        _ => {}
    }
    for at in top..top + shown {
        out.extend(item(at, at - top));
    }
    if position == 1 && list.total > 0 {
        out.push(Line::styled(
            format!("{}-{} of {}", top + 1, top + shown, list.total),
            styles.dim,
        ));
    }
    out.extend(notice_lines);
    if show_tail {
        out.extend(tail);
    }
    out
}

fn lines(screen: &ForumsScreen, space: Space, ctx: &DrawCtx<'_>) -> Vec<Line<'static>> {
    let (bold, dim, heading, _) = classic::styles(ctx.look);
    let styles = Styles { bold, dim, heading };
    let mut out = Vec::new();
    match &screen.state {
        State::Start | State::Reader(_) | State::Editor(_) => {
            out.push(Line::styled(strings::TITLE_BOARDS.to_owned(), heading));
            out.push(Line::raw(""));
            notice(screen, ctx, &mut out);
        }
        State::Boards { list, cursor } => {
            let help = match (list.can_manage, list.can_feeds) {
                (true, true) => strings::HELP_BOARDS_FEEDS,
                (false, true) => strings::HELP_BOARDS_FEEDS_ONLY,
                (true, false) => strings::HELP_BOARDS_MANAGE,
                (false, false) => strings::HELP_BOARDS,
            };
            return board_screen(screen, space, ctx, styles, list, *cursor, help);
        }
        State::Threads { page, cursor } => {
            let help = if page.board.feed {
                if page.can_feeds {
                    strings::HELP_THREADS_FEED_STAFF
                } else {
                    strings::HELP_THREADS_FEED
                }
            } else if page.can_manage && page.board.restricted {
                strings::HELP_THREADS_MANAGE
            } else if page.can_feeds && !page.board.restricted {
                strings::HELP_THREADS_STAFF
            } else {
                strings::HELP_THREADS
            };
            return thread_screen(screen, space, ctx, styles, page, *cursor, help);
        }
        State::Results {
            page,
            query,
            cursor,
        } => return result_screen(screen, space, ctx, styles, page, query, *cursor),
        State::Input(input) => input_lines(screen, input, heading, dim, ctx, &mut out),
    }
    out
}

fn board_screen(
    screen: &ForumsScreen,
    space: Space,
    ctx: &DrawCtx<'_>,
    styles: Styles,
    list: &BoardList,
    cursor: usize,
    help: &str,
) -> Vec<Line<'static>> {
    let head = vec![
        Line::styled(strings::TITLE_BOARDS.to_owned(), styles.heading),
        Line::raw(""),
    ];
    let plan = ListScreen {
        head,
        empty: Some(strings::NO_BOARDS),
        total: list.boards.len(),
        cursor,
        item_rows: 1,
        extra: Vec::new(),
        help,
    };
    list_lines(screen, space, ctx, styles, plan, |at, number| {
        let board = &list.boards[at];
        let threads = if board.threads == 1 {
            "thread"
        } else {
            "threads"
        };
        let about = if board.description.is_empty() {
            String::new()
        } else {
            format!(" - {}", clean(&board.description, 160))
        };
        let tag = if board.feed {
            format!(" [{}]", strings::FEED_BOARD_TAG)
        } else {
            String::new()
        };
        let text = format!(
            "{}{tag} ({} {threads}){about}",
            clean(&board.name, 40),
            board.threads
        );
        vec![row(at == cursor, number, &text, space.width, styles.bold)]
    })
}

fn thread_screen(
    screen: &ForumsScreen,
    space: Space,
    ctx: &DrawCtx<'_>,
    styles: Styles,
    page: &ThreadPage,
    cursor: usize,
    help: &str,
) -> Vec<Line<'static>> {
    let mut head = vec![Line::styled(clean(&page.board.name, 40), styles.heading)];
    if !page.board.description.is_empty() {
        head.push(Line::styled(
            clean(&page.board.description, 160),
            styles.dim,
        ));
    }
    head.push(Line::raw(""));
    let plan = ListScreen {
        head,
        empty: Some(strings::NO_THREADS),
        total: page.threads.len(),
        cursor,
        item_rows: 1,
        extra: vec![
            Line::raw(""),
            Line::styled(format!("Page {}", page.page + 1), styles.dim),
        ],
        help,
    };
    list_lines(screen, space, ctx, styles, plan, |at, number| {
        let thread = &page.threads[at];
        let who = thread.author.as_deref().unwrap_or(strings::GONE_MEMBER);
        let text = format!(
            "{}  [{}, {} {}, {}]",
            clean(&thread.subject, 120),
            clean(who, 64),
            thread.posts,
            if thread.posts == 1 { "post" } else { "posts" },
            format_utc(thread.last_post_at)
        );
        vec![row(at == cursor, number, &text, space.width, styles.bold)]
    })
}

fn result_screen(
    screen: &ForumsScreen,
    space: Space,
    ctx: &DrawCtx<'_>,
    styles: Styles,
    page: &SearchPage,
    query: &str,
    cursor: usize,
) -> Vec<Line<'static>> {
    let head = vec![
        Line::styled(strings::TITLE_SEARCH.to_owned(), styles.heading),
        Line::styled(format!("for: {}", clean(query, 200)), styles.dim),
        Line::raw(""),
    ];
    let empty = if page.searched {
        strings::NO_RESULTS
    } else {
        strings::NOT_SEARCHED
    };
    let plan = ListScreen {
        head,
        empty: Some(empty),
        total: page.hits.len(),
        cursor,
        item_rows: 2,
        extra: Vec::new(),
        help: strings::HELP_RESULTS,
    };
    list_lines(screen, space, ctx, styles, plan, |at, number| {
        let hit = &page.hits[at];
        let text = format!(
            "{} / {}",
            clean(&hit.board_name, 40),
            clean(&hit.subject, 120)
        );
        vec![
            row(at == cursor, number, &text, space.width, styles.bold),
            Line::styled(
                format!(
                    "      {}",
                    fit(&clean(&hit.snippet, 120), space.width.saturating_sub(6))
                ),
                styles.dim,
            ),
        ]
    })
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
        out.push(Line::raw(format!("> {}_", clean(&input.text, 200))));
    }
    out.push(Line::raw(""));
    notice(screen, ctx, out);
    out.push(Line::styled(strings::HELP_INPUT.to_owned(), dim));
}
