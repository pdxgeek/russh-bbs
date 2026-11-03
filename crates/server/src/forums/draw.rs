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
use super::flow::{ForumsScreen, Input, InputKind, State, confirm_text, reader_title};
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
    let width = usize::from(area.width.min(settings::MAX_WIDTH)).saturating_sub(2);
    classic::render_content(
        frame,
        ContentScreen {
            strings: ctx.strings,
            lines: lines(screen, width, ctx),
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
        let (content, status) = chrome::split_status(area);
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

fn lines(screen: &ForumsScreen, width: usize, ctx: &DrawCtx<'_>) -> Vec<Line<'static>> {
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
            board_lines(list, *cursor, width, styles, &mut out);
            notice(screen, ctx, &mut out);
            footer(
                &mut out,
                match (list.can_manage, list.can_feeds) {
                    (true, true) => strings::HELP_BOARDS_FEEDS,
                    (false, true) => strings::HELP_BOARDS_FEEDS_ONLY,
                    (true, false) => strings::HELP_BOARDS_MANAGE,
                    (false, false) => strings::HELP_BOARDS,
                },
                dim,
            );
        }
        State::Threads { page, cursor } => {
            thread_lines(page, *cursor, width, styles, &mut out);
            notice(screen, ctx, &mut out);
            footer(
                &mut out,
                if page.board.feed {
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
                },
                dim,
            );
        }
        State::Results {
            page,
            query,
            cursor,
        } => {
            result_lines(page, query, *cursor, width, styles, &mut out);
            notice(screen, ctx, &mut out);
            footer(&mut out, strings::HELP_RESULTS, dim);
        }
        State::Input(input) => input_lines(screen, input, heading, dim, ctx, &mut out),
    }
    out
}

fn footer(out: &mut Vec<Line<'static>>, text: &str, dim: Style) {
    out.push(Line::raw(""));
    out.push(Line::styled(text.to_owned(), dim));
}

fn board_lines(
    list: &BoardList,
    cursor: usize,
    width: usize,
    styles: Styles,
    out: &mut Vec<Line<'static>>,
) {
    out.push(Line::styled(
        strings::TITLE_BOARDS.to_owned(),
        styles.heading,
    ));
    out.push(Line::raw(""));
    if list.boards.is_empty() {
        out.push(Line::raw(strings::NO_BOARDS.to_owned()));
    }
    for (at, board) in list.boards.iter().enumerate() {
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
        out.push(row(at == cursor, at, &text, width, styles.bold));
    }
}

fn thread_lines(
    page: &ThreadPage,
    cursor: usize,
    width: usize,
    styles: Styles,
    out: &mut Vec<Line<'static>>,
) {
    out.push(Line::styled(clean(&page.board.name, 40), styles.heading));
    if !page.board.description.is_empty() {
        out.push(Line::styled(
            clean(&page.board.description, 160),
            styles.dim,
        ));
    }
    out.push(Line::raw(""));
    if page.threads.is_empty() {
        out.push(Line::raw(strings::NO_THREADS.to_owned()));
    }
    for (at, thread) in page.threads.iter().enumerate() {
        let who = thread.author.as_deref().unwrap_or(strings::GONE_MEMBER);
        let text = format!(
            "{}  [{}, {} {}, {}]",
            clean(&thread.subject, 120),
            clean(who, 64),
            thread.posts,
            if thread.posts == 1 { "post" } else { "posts" },
            format_utc(thread.last_post_at)
        );
        out.push(row(at == cursor, at, &text, width, styles.bold));
    }
    out.push(Line::raw(""));
    out.push(Line::styled(format!("Page {}", page.page + 1), styles.dim));
}

fn result_lines(
    page: &SearchPage,
    query: &str,
    cursor: usize,
    width: usize,
    styles: Styles,
    out: &mut Vec<Line<'static>>,
) {
    out.push(Line::styled(
        strings::TITLE_SEARCH.to_owned(),
        styles.heading,
    ));
    out.push(Line::styled(
        format!("for: {}", clean(query, 200)),
        styles.dim,
    ));
    out.push(Line::raw(""));
    if !page.searched {
        out.push(Line::raw(strings::NOT_SEARCHED.to_owned()));
    } else if page.hits.is_empty() {
        out.push(Line::raw(strings::NO_RESULTS.to_owned()));
    }
    for (at, hit) in page.hits.iter().enumerate() {
        let text = format!(
            "{} / {}",
            clean(&hit.board_name, 40),
            clean(&hit.subject, 120)
        );
        out.push(row(at == cursor, at, &text, width, styles.bold));
        out.push(Line::styled(
            format!(
                "      {}",
                fit(&clean(&hit.snippet, 120), width.saturating_sub(6))
            ),
            styles.dim,
        ));
    }
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
