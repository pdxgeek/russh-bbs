//! The members, profile and community screens drawn in the frame (UI-4): the header and status
//! bars, then one pane in the middle that suits the screen. Core hands over a
//! [`ScreenView`] (lines of string-table keys and sanitized content); this module only decides
//! how those lines look.
//!
//! - **Last callers** is a table (who, when).
//! - **One-liners and the wall** are a pane of entries, each handle coloured by a hash of its
//!   name ([`crate::ui::screens::nick`]) and drawn through the string table's own template.
//! - **Polls** is a table; an **open poll** draws its counts as bars (when core says the member
//!   may see counts) and says in words what it hides.
//! - Everything else (a profile, the profile form, the poll form) is its lines in a pane.
//!
//! Content is sanitized again here with the shared sanitizer; nothing is given the reserved
//! marker style, and the pane never reaches the status bar (the frame hands out the body only).

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::Paragraph;
use russh_bbs_core::community::keys as ck;
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::profiles::{Line, ScreenView, keys};

use super::{clean, line_of};
use crate::settings::Look;
use crate::ui::classic;
use crate::ui::frame::{ChromeText, FrameSpec};
use crate::ui::screens::nick;
use crate::ui::strings::StringTable;
use crate::ui::table::{self, Cell, CellStyle, Column, TableRow, TableSpec, Title, Width, clip};
use crate::ui::theme::Element;
use crate::ui::widgets::Pen;
use crate::ui::widgets::gfx::bar;

/// Widest the pane is drawn.
const MAX_WIDTH: u16 = 100;

/// What the screens need besides the view.
pub(crate) struct ViewCtx<'a> {
    pub(crate) strings: &'a StringTable,
    pub(crate) look: Look,
    pub(crate) tier: Tier,
    pub(crate) markers: &'a [SecurityMarker],
    pub(crate) clock: Option<&'a str>,
}

/// A value of a `Filled` line by placeholder name.
fn value<'a>(values: &'a [(&'static str, String)], name: &str) -> &'a str {
    values
        .iter()
        .find(|(key, _)| *key == name)
        .map_or("", |(_, v)| v.as_str())
}

/// What kind of screen a view is.
fn heading_of(view: &ScreenView) -> Option<&'static str> {
    view.lines.iter().find_map(|line| match line {
        Line::Heading(key) => Some(*key),
        _ => None,
    })
}

fn is_poll_detail(view: &ScreenView) -> bool {
    matches!(
        view.lines.first(),
        Some(Line::Pair {
            label: ck::POLL_TITLE,
            ..
        })
    )
}

/// The pane's rectangle: centred, at most [`MAX_WIDTH`] wide.
fn pane_of(body: Rect) -> Rect {
    let width = body.width.min(MAX_WIDTH);
    Rect {
        x: body.x + (body.width - width) / 2,
        width,
        ..body
    }
}

/// Draw `view` in the frame.
pub(crate) fn render(frame: &mut Frame<'_>, view: &ScreenView, ctx: &ViewCtx<'_>) {
    let hints = classic::content_hints(ctx.strings);
    let heading = heading_of(view);
    let place = if is_poll_detail(view) {
        ck::POLLS_TITLE
    } else {
        heading.unwrap_or(keys::VIEW_TITLE)
    };
    let body = FrameSpec::new(ctx.strings, ctx.look, ctx.tier)
        .markers(ctx.markers)
        .place(ChromeText::table(ctx.strings, place))
        .clock(ctx.clock)
        .hints(&hints)
        .draw(frame);
    let area = pane_of(body);
    match heading {
        Some(ck::CALLERS_TITLE) => callers(frame, area, view, ctx),
        Some(ck::POLLS_TITLE) => polls(frame, area, view, ctx),
        Some(ck::ONELINERS_TITLE | ck::GRAFFITI_TITLE) => entries(frame, area, view, ctx),
        _ if is_poll_detail(view) => poll(frame, area, view, ctx),
        _ => generic(frame, area, view, ctx),
    }
}

/// The footer words (the keys on offer) and any notice, as the pane's last lines.
fn tail(view: &ScreenView, ctx: &ViewCtx<'_>, lines: &mut Vec<TextLine<'static>>, keep: bool) {
    let pen = Pen(ctx.look);
    for line in &view.lines {
        if let Line::Notice { .. } = line {
            lines.push(line_of(line, ctx.strings, ctx.look, usize::MAX));
        }
    }
    if keep {
        lines.push(TextLine::raw(""));
    }
    lines.push(TextLine::styled(
        ctx.strings.text(view.footer).to_owned(),
        pen.muted(),
    ));
}

fn title_of(view: &ScreenView, ctx: &ViewCtx<'_>) -> String {
    heading_of(view).map_or_else(
        || ctx.strings.text(keys::VIEW_TITLE).to_owned(),
        |key| ctx.strings.text(key).to_owned(),
    )
}

fn generic(frame: &mut Frame<'_>, area: Rect, view: &ScreenView, ctx: &ViewCtx<'_>) {
    let inner = ctx
        .look
        .panel(frame, area, Some(&title_of(view, ctx)), Element::Pane, false);
    let width = usize::from(inner.width).saturating_sub(2);
    let mut lines: Vec<TextLine<'static>> = view
        .lines
        .iter()
        .filter(|line| !matches!(line, Line::Heading(_) | Line::Notice { .. }))
        .map(|line| line_of(line, ctx.strings, ctx.look, width))
        .collect();
    tail(view, ctx, &mut lines, true);
    frame.render_widget(Paragraph::new(lines), inner);
}

fn callers(frame: &mut Frame<'_>, area: Rect, view: &ScreenView, ctx: &ViewCtx<'_>) {
    let rows: Vec<TableRow> = view
        .lines
        .iter()
        .filter_map(|line| match line {
            Line::Filled { key, values } if *key == ck::CALLER => Some(TableRow::new(vec![
                Cell::content(value(values, "handle")).styled(CellStyle::Key),
                Cell::plain(value(values, "when")).styled(CellStyle::Dim),
            ])),
            _ => None,
        })
        .collect();
    let [table_area, tail_area] = split_tail(area);
    let _ = table::render(
        frame,
        table_area,
        ctx.look,
        &TableSpec {
            title: Title::Chrome(ChromeText::table(ctx.strings, ck::CALLERS_TITLE)),
            columns: vec![
                Column::new(ChromeText::fixed("Caller"), Width::Fill(1)),
                Column::new(ChromeText::fixed("When (UTC)"), Width::Fixed(22)),
            ],
            rows,
            selected: None,
            bordered: true,
            top: 0,
            empty: ctx.strings.text(ck::CALLERS_EMPTY),
            numbered: false,
            footer: None,
            sorted: None,
        },
    );
    let mut lines = Vec::new();
    tail(view, ctx, &mut lines, false);
    frame.render_widget(Paragraph::new(lines), tail_area);
}

/// Split off the bottom rows for the footer and notices.
fn split_tail(area: Rect) -> [Rect; 2] {
    let rows = 2.min(area.height.saturating_sub(4));
    Layout::vertical([Constraint::Fill(1), Constraint::Length(rows)]).areas(area)
}

fn polls(frame: &mut Frame<'_>, area: Rect, view: &ScreenView, ctx: &ViewCtx<'_>) {
    let rows: Vec<TableRow> = view
        .lines
        .iter()
        .filter_map(|line| match line {
            Line::Filled { key, values }
                if *key == ck::POLL_ROW_OPEN || *key == ck::POLL_ROW_CLOSED =>
            {
                let closed = *key == ck::POLL_ROW_CLOSED;
                Some(TableRow::new(vec![
                    Cell::plain(value(values, "key")).styled(CellStyle::Key),
                    Cell::content(value(values, "question")).styled(CellStyle::Bold),
                    Cell::plain(value(values, "votes")),
                    Cell::plain(if closed { "closed" } else { "open" }).styled(CellStyle::Dim),
                ]))
            }
            _ => None,
        })
        .collect();
    let [table_area, tail_area] = split_tail(area);
    let _ = table::render(
        frame,
        table_area,
        ctx.look,
        &TableSpec {
            title: Title::Chrome(ChromeText::table(ctx.strings, ck::POLLS_TITLE)),
            columns: vec![
                Column::new(ChromeText::fixed("Key"), Width::Fixed(3)),
                Column::new(ChromeText::fixed("Question"), Width::Fill(1)),
                Column::new(ChromeText::fixed("Votes"), Width::Fixed(6)).right(),
                Column::new(ChromeText::fixed("State"), Width::Fixed(7)),
            ],
            rows,
            selected: None,
            bordered: true,
            top: 0,
            empty: ctx.strings.text(ck::POLLS_EMPTY),
            numbered: false,
            footer: None,
            sorted: None,
        },
    );
    let mut lines: Vec<TextLine<'static>> = Vec::new();
    for line in &view.lines {
        if let Line::Text(key) = line {
            lines.push(TextLine::styled(
                ctx.strings.text(key).to_owned(),
                Pen(ctx.look).muted(),
            ));
        }
        if let Line::Filled { key, .. } = line
            && *key == ck::POLLS_PAGE
        {
            lines.push(line_of(line, ctx.strings, ctx.look, usize::MAX));
        }
    }
    tail(view, ctx, &mut lines, false);
    let keep = usize::from(tail_area.height);
    let skip = lines.len().saturating_sub(keep);
    frame.render_widget(Paragraph::new(lines.split_off(skip)), tail_area);
}

/// One-liners and the wall: each entry with its handle coloured by hash, through the string
/// table's template for an entry.
fn entries(frame: &mut Frame<'_>, area: Rect, view: &ScreenView, ctx: &ViewCtx<'_>) {
    let look = ctx.look;
    let pen = Pen(look);
    let inner = look.panel(frame, area, Some(&title_of(view, ctx)), Element::Pane, false);
    let width = usize::from(inner.width).saturating_sub(2);
    let template = ctx.strings.text(ck::ENTRY).to_owned();
    let mut lines: Vec<TextLine<'static>> = Vec::new();
    for line in &view.lines {
        match line {
            Line::Heading(_) | Line::Notice { .. } => {}
            Line::Filled { key, values } if *key == ck::ENTRY => {
                let handle = clean(value(values, "handle"));
                let n = value(values, "n");
                let text = clean(value(values, "text"));
                let fill = |piece: &str| piece.replace("{n}", n).replace("{text}", &text);
                if let Some((before, after)) = template.split_once("{handle}") {
                    lines.push(TextLine::from(vec![
                        Span::styled(fill(before), pen.muted()),
                        Span::styled(handle.clone(), nick::style(look, &handle)),
                        Span::styled(clip(&fill(after), width, look.ascii()), pen.text()),
                    ]));
                } else {
                    lines.push(line_of(line, ctx.strings, look, width));
                }
            }
            Line::Filled { .. } | Line::Text(_) => {
                lines.push(TextLine::styled(
                    clip(
                        &match line {
                            Line::Text(key) => ctx.strings.text(key).to_owned(),
                            Line::Filled { key, values } => {
                                let values: Vec<(&str, &str)> =
                                    values.iter().map(|(k, v)| (*k, v.as_str())).collect();
                                ctx.strings.filled(key, &values)
                            }
                            _ => String::new(),
                        },
                        width,
                        look.ascii(),
                    ),
                    pen.muted(),
                ));
            }
            other => lines.push(line_of(other, ctx.strings, look, width)),
        }
    }
    tail(view, ctx, &mut lines, true);
    frame.render_widget(Paragraph::new(lines), inner);
}

/// An open poll: the question, who asked, the options with their counts as bars.
fn poll(frame: &mut Frame<'_>, area: Rect, view: &ScreenView, ctx: &ViewCtx<'_>) {
    let look = ctx.look;
    let pen = Pen(look);
    let inner = look.panel(
        frame,
        area,
        Some(ctx.strings.text(ck::POLL_TITLE)),
        Element::Pane,
        false,
    );
    let width = usize::from(inner.width).saturating_sub(2);
    let bar_width = width.saturating_sub(24).clamp(8, 40);
    let mut lines: Vec<TextLine<'static>> = Vec::new();
    for line in &view.lines {
        match line {
            Line::Pair {
                value: Some(question),
                ..
            } => lines.push(TextLine::styled(
                clip(&clean(question), width, look.ascii()),
                pen.heading(),
            )),
            Line::Filled { key, values }
                if *key == ck::POLL_OPTION_COUNT || *key == ck::POLL_OPTION_COUNT_MINE =>
            {
                let count: u64 = value(values, "count").parse().unwrap_or(0);
                let percent: u64 = value(values, "percent").parse().unwrap_or(0);
                let mine = *key == ck::POLL_OPTION_COUNT_MINE;
                let (filled, rest) = bar(percent, 100, bar_width, look.ascii());
                lines.push(TextLine::from(vec![
                    Span::styled(format!("[{}] ", value(values, "key")), pen.key()),
                    Span::styled(
                        clip(&clean(value(values, "text")), width.saturating_sub(6), look.ascii()),
                        pen.text(),
                    ),
                    Span::styled(
                        if mine { "  (your vote)" } else { "" }.to_owned(),
                        pen.muted(),
                    ),
                ]));
                lines.push(TextLine::from(vec![
                    Span::raw("    "),
                    Span::styled(filled, pen.accent()),
                    Span::styled(rest, pen.muted()),
                    Span::styled(format!(" {count} ({percent}%)"), pen.text()),
                ]));
            }
            Line::Filled { key, values } if *key == ck::POLL_BY => {
                lines.push(TextLine::from(vec![
                    Span::styled("Asked by ".to_owned(), pen.muted()),
                    Span::styled(
                        clean(value(values, "handle")),
                        nick::style(look, value(values, "handle")),
                    ),
                ]));
            }
            Line::Notice { .. } => {}
            other => lines.push(line_of(other, ctx.strings, look, width)),
        }
    }
    tail(view, ctx, &mut lines, true);
    frame.render_widget(
        Paragraph::new(lines).style(Style::new().remove_modifier(Modifier::REVERSED)),
        inner,
    );
}
