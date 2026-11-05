//! The message reader (UI-4): a **header block**, **tabs** for long threads, **quote blocks**
//! and a **scrollbar**, drawn in the frame between the header and status bars. The forum
//! reader and the mail reader both use it.
//!
//! ```text
//!  russh-bbs | Forums                                           UTC 14:05
//!  < 2 3 [4] 5 6 >                                              4 of 12      <- tabs: posts
//!  Welcome to the board                                                       <- header block
//!  From ann  2026-11-04 09:12 UTC
//! +- General -----------------------------------------------------------+
//! | Thanks for the invite.                                               #
//! | | I was not sure this would work.                                    #   <- quote block
//! |                                                                      |
//! |  Space more   b back                                                 |
//! +----------------------------------------------------------------------+
//!  [marker cell]  n/p post  r reply  c quote  Esc back                       <- status bar
//! ```
//!
//! **One geometry.** [`text_rows`] and [`text_width`] are what the screens page with, and
//! [`render`] draws with the same numbers, so a page of text is never cut. The tab strip row is
//! always present (it carries the breadcrumb when there is only one post), the header block is
//! two rows, the box has a border and two rows of prompt.
//!
//! **Content.** The subject, the author and the body are content: they are sanitized again
//! with the shared sanitizer when drawn, the author is coloured by [`super::nick`], and nothing
//! uses the reserved style. A quote mark is plain text anyone can type (D80); the styled block
//! is only a typographic hint, never a security cue.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::terminal_settings::Pager;

use super::nick;
use crate::settings::Look;
use crate::ui::frame::{ChromeText, FrameSpec, HEADER_MIN_COLS, HEADER_MIN_ROWS, Hint};
use crate::ui::pager::prompt_text;
use crate::ui::strings::StringTable;
use crate::ui::table::clip;
use crate::ui::theme::Element;

/// Widest the reading box is drawn, so lines stay readable on a wide terminal.
pub(crate) const MAX_WIDTH: u16 = 100;
/// Rows the tab strip takes.
const STRIP_ROWS: u16 = 1;
/// Rows the header block takes.
const HEAD_ROWS: u16 = 2;
/// Rows the box's border takes.
const BORDER_ROWS: u16 = 2;
/// Rows under the text inside the box: a blank row and the prompt.
const PROMPT_ROWS: u16 = 2;

/// Rows between the header bar and the status bar of a frame of `area` (the header bar is
/// assumed on when it fits; the compact layout has none, and then has a row to spare).
fn body_rows(area: Rect) -> u16 {
    let header = u16::from(area.height >= HEADER_MIN_ROWS && area.width >= HEADER_MIN_COLS);
    area.height.saturating_sub(1 + header)
}

/// Rows of text one page holds in a frame of `area` (always at least one).
#[must_use]
pub(crate) fn text_rows(area: Rect) -> usize {
    usize::from(
        body_rows(area)
            .saturating_sub(STRIP_ROWS + HEAD_ROWS + BORDER_ROWS + PROMPT_ROWS)
            .max(1),
    )
}

/// Columns of text one row holds in a frame of `area` (always at least one): the box's width
/// less its border and a column of padding each side.
#[must_use]
pub(crate) fn text_width(area: Rect) -> usize {
    usize::from(area.width.min(MAX_WIDTH).saturating_sub(4).max(1))
}

/// The three parts of the reader inside the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Parts {
    pub(crate) strip: Rect,
    pub(crate) head: Rect,
    pub(crate) text: Rect,
}

/// Where the strip, the header block and the box go in `body`: centred, at most
/// [`MAX_WIDTH`] wide, and never outside it.
#[must_use]
pub(crate) fn parts(body: Rect) -> Parts {
    let width = body.width.min(MAX_WIDTH);
    let area = Rect {
        x: body.x + (body.width - width) / 2,
        width,
        ..body
    };
    let [strip, head, text] = Layout::vertical([
        Constraint::Length(STRIP_ROWS.min(area.height)),
        Constraint::Length(HEAD_ROWS.min(area.height.saturating_sub(STRIP_ROWS))),
        Constraint::Fill(1),
    ])
    .areas(area);
    Parts { strip, head, text }
}

/// The posts to show as tabs: a window of at most `room` numbers around `index` (0-based) in
/// `total`. Returns the first and one past the last.
#[must_use]
pub(crate) fn tab_window(index: u64, total: u64, room: u64) -> (u64, u64) {
    let room = room.max(1).min(total.max(1));
    let start = index
        .saturating_sub(room / 2)
        .min(total.saturating_sub(room));
    (start, start + room)
}

/// What the strip says.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Strip<'a> {
    /// Posts of a thread, as tabs; `index` is 0-based.
    Posts { index: u64, total: u64 },
    /// One breadcrumb of chrome words (mail).
    Label(ChromeText<'a>),
}

/// The header block's facts. All content (`subject`, `from`, `context`) is sanitized when drawn.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Head<'a> {
    pub(crate) subject: &'a str,
    pub(crate) from: &'a str,
    /// The time, formatted by the caller from a number.
    pub(crate) at: &'a str,
    /// The board the post is in, shown in the box title (content), or a chrome word.
    pub(crate) context: &'a str,
}

/// What [`render`] needs besides the reader's facts.
pub(crate) struct ReaderCtx<'a> {
    pub(crate) strings: &'a StringTable,
    pub(crate) look: Look,
    pub(crate) tier: Tier,
    pub(crate) markers: &'a [SecurityMarker],
    pub(crate) clock: Option<&'a str>,
}

/// One reader to draw.
pub(crate) struct ReaderView<'a> {
    pub(crate) place: ChromeText<'a>,
    pub(crate) head: Head<'a>,
    pub(crate) strip: Strip<'a>,
    pub(crate) pager: &'a Pager,
    pub(crate) hints: &'a [Hint<'a>],
}

fn clean(text: &str, max: usize) -> String {
    SingleLine::new(max).sanitize(text).into_string()
}

/// Draw the reader and return the body rectangle (for a popup over it).
pub(crate) fn render(frame: &mut Frame<'_>, view: &ReaderView<'_>, ctx: &ReaderCtx<'_>) -> Rect {
    let body = FrameSpec::new(ctx.strings, ctx.look, ctx.tier)
        .markers(ctx.markers)
        .place(view.place)
        .clock(ctx.clock)
        .hints(view.hints)
        .draw(frame);
    let parts = parts(body);
    draw_strip(frame, parts.strip, view, ctx.look);
    draw_head(frame, parts.head, &view.head, ctx.look);
    draw_text(frame, parts.text, view, ctx);
    body
}

fn draw_strip(frame: &mut Frame<'_>, area: Rect, view: &ReaderView<'_>, look: Look) {
    if area.height == 0 {
        return;
    }
    let muted = look.muted();
    let key = look.accent(
        Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        look.palette().key,
    );
    let line = match view.strip {
        Strip::Label(text) => Line::styled(format!(" {}", text.as_str()), muted),
        Strip::Posts { index, total } if total <= 1 => {
            Line::styled(format!(" Post {} of {}", index + 1, total.max(1)), muted)
        }
        Strip::Posts { index, total } => {
            // " < 2 3 [4] 5 6 >  4 of 12": the tabs take what is left of the row.
            let tail = format!("  {} of {total}", index + 1);
            let used = u64::try_from(tail.chars().count()).unwrap_or(0) + 6;
            let room = u64::from(area.width).saturating_sub(used) / 4;
            let (start, end) = tab_window(index, total, room);
            let (less, more) = if look.ascii() {
                ("<", ">")
            } else {
                ("\u{2039}", "\u{203a}")
            };
            let mut spans = vec![Span::styled(
                format!(" {} ", if start > 0 { less } else { " " }),
                muted,
            )];
            for post in start..end {
                if post == index {
                    spans.push(Span::styled(format!("[{}]", post + 1), key));
                } else {
                    spans.push(Span::styled(format!(" {} ", post + 1), muted));
                }
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                (if end < total { more } else { " " }).to_owned(),
                muted,
            ));
            spans.push(Span::styled(tail, muted));
            Line::from(spans)
        }
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_head(frame: &mut Frame<'_>, area: Rect, head: &Head<'_>, look: Look) {
    if area.height == 0 {
        return;
    }
    let bar = look.bg(look.palette().head);
    frame.render_widget(Block::default().style(bar), area);
    let width = usize::from(area.width).saturating_sub(2);
    let subject = clip(&clean(head.subject, 200), width, look.ascii());
    let heading = look.accent(
        Style::new().add_modifier(Modifier::BOLD),
        look.palette().heading,
    );
    let from = clean(head.from, 64);
    let first = Line::from(vec![
        Span::raw(" "),
        Span::styled(subject, bar.patch(heading)),
    ]);
    let second = Line::from(vec![
        Span::styled(" From ".to_owned(), bar.patch(look.muted())),
        Span::styled(from.clone(), bar.patch(nick::style(look, &from))),
        Span::styled(format!("   {}", head.at), bar.patch(look.muted())),
    ]);
    frame.render_widget(Paragraph::new(vec![first, second]), area);
}

/// How deep the quote is and the text after the marks, for a line that starts with `>`.
fn quote_of(line: &str) -> Option<(usize, &str)> {
    let mut depth = 0;
    let mut rest = line;
    while let Some(after) = rest.strip_prefix('>') {
        depth += 1;
        rest = after.strip_prefix(' ').unwrap_or(after);
    }
    (depth > 0).then_some((depth, rest))
}

fn text_line(text: &str, look: Look, width: usize) -> Line<'static> {
    let Some((depth, rest)) = quote_of(text) else {
        return Line::styled(
            clip(text, width, look.ascii()),
            look.fg(look.palette().text),
        );
    };
    let bar = if look.ascii() { "| " } else { "\u{2502} " };
    let style = look.accent(
        Style::new().add_modifier(Modifier::ITALIC),
        look.palette().quote,
    );
    let marks = bar.repeat(depth.min(3));
    let room = width.saturating_sub(marks.chars().count());
    Line::from(vec![
        Span::styled(marks, look.accent(Style::new(), look.palette().quote)),
        Span::styled(clip(rest, room, look.ascii()), style),
    ])
}

fn draw_text(frame: &mut Frame<'_>, area: Rect, view: &ReaderView<'_>, ctx: &ReaderCtx<'_>) {
    let look = ctx.look;
    let title = clip(
        &clean(view.head.context, 80),
        usize::from(area.width).saturating_sub(6),
        look.ascii(),
    );
    let inner = look.panel(frame, area, Some(&title), Element::Pane, false);
    let rows = usize::from(inner.height).saturating_sub(usize::from(PROMPT_ROWS));
    let rows = rows.max(1);
    let page = view.pager.view(rows);
    let width = usize::from(inner.width).saturating_sub(2);
    let mut lines: Vec<Line<'static>> = page
        .lines
        .iter()
        .map(|text| text_line(&clean(text, 400), look, width))
        .collect();
    while lines.len() < rows {
        lines.push(Line::raw(""));
    }
    lines.truncate(rows);
    if let Some(prompt) = prompt_text(page.prompt, ctx.strings) {
        lines.push(Line::raw(""));
        lines.push(Line::styled(prompt, look.muted()));
    }
    frame.render_widget(
        Paragraph::new(lines),
        Rect {
            x: inner.x + 1,
            width: inner.width.saturating_sub(1),
            ..inner
        },
    );
    let (top, total) = view.pager.position(rows);
    if total > rows && area.height > 3 {
        let mut state = ScrollbarState::new(total.saturating_sub(rows))
            .position(top)
            .viewport_content_length(rows);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(look.muted())
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some(if look.ascii() { "|" } else { "\u{2502}" }))
                .thumb_symbol(if look.ascii() { "#" } else { "\u{2503}" }),
            area.inner(Margin {
                horizontal: 0,
                vertical: 1,
            }),
            &mut state,
        );
    }
}
