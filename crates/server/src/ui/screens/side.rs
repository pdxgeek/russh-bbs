//! The split layout's second pane (UI-4): a real **chat pane** (the rooms the member may join and
//! who is in each) above a real **mail pane** (the newest messages), beside the menu.
//!
//! The data is the live widgets' data, read through the same access checks: `chat` is filled
//! only for a member who may join a room, `recent_mail` only for one who holds `mail.read`
//! (the server opens subjects with the board's mail key and nothing else of a message). A pane
//! whose data is `None` is not drawn, and when neither is available the layout falls back to its
//! widget cells.
//!
//! **Hooks for UI-9 (interaction).** Both functions draw into any [`Rect`] they are given, so a
//! pane drag only changes the rectangles ([`crate::ui::panes::Plan::divider`] is the grip to
//! drag, [`crate::ui::panes::Plan::chat`] and `mail` the two panes), and "chat beside any
//! screen" can call [`chat_pane`] for the second pane of any other screen. Neither function draws
//! outside its rectangle, uses reverse video, or applies a style to member text beyond a colour.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::widgets::WidgetData;

use super::nick;
use crate::settings::Look;
use crate::ui::table::clip;
use crate::ui::theme::Element;
use crate::ui::widgets::Pen;

fn clean(text: &str, max: usize) -> String {
    SingleLine::new(max).sanitize(text).into_string()
}

/// Whether the data has something for the chat pane.
#[must_use]
pub(crate) fn has_chat(data: &WidgetData) -> bool {
    data.chat.as_ref().is_some_and(|rooms| !rooms.is_empty())
}

/// Whether the data has something for the mail pane.
#[must_use]
pub(crate) fn has_mail(data: &WidgetData) -> bool {
    data.mail.is_some() || data.recent_mail.is_some()
}

/// The chat pane: each room with a count, and the handles in it, coloured by hash.
pub(crate) fn chat_pane(frame: &mut Frame<'_>, area: Rect, look: Look, data: &WidgetData) {
    let inner = look.panel(frame, area, Some("Chat"), Element::Pane, false);
    let pen = Pen(look);
    let width = usize::from(inner.width).saturating_sub(1);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for room in data.chat.iter().flatten() {
        lines.push(Line::from(vec![
            Span::styled(
                clip(&format!("#{}", clean(&room.name, 40)), width, look.ascii()),
                pen.heading(),
            ),
            Span::styled(format!("  {}", room.members), pen.muted()),
        ]));
        for who in &room.here {
            let name = clean(who, 40);
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    clip(&name, width.saturating_sub(2), look.ascii()),
                    nick::style(look, &name),
                ),
            ]));
        }
        if room.here.is_empty() {
            lines.push(Line::styled("  nobody here".to_owned(), pen.muted()));
        }
    }
    lines.truncate(usize::from(inner.height));
    frame.render_widget(Paragraph::new(lines), inner);
}

/// How long ago, in a few characters: `5m`, `3h`, `2d`.
fn ago(now: i64, at: i64) -> String {
    let secs = now.saturating_sub(at).max(0);
    match secs {
        0..=3_599 => format!("{}m", secs / 60),
        3_600..=86_399 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// The mail pane: how many are unread, then the newest messages with who from, the subject and
/// how long ago. An unread message has a plain asterisk and a bold subject.
pub(crate) fn mail_pane(
    frame: &mut Frame<'_>,
    area: Rect,
    look: Look,
    data: &WidgetData,
    now: i64,
) {
    let inner = look.panel(frame, area, Some("Mail"), Element::Pane, false);
    let pen = Pen(look);
    let width = usize::from(inner.width).saturating_sub(1);
    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Some(count) = &data.mail {
        lines.push(Line::from(vec![
            Span::styled(format!("{} unread", count.unread), pen.key()),
            Span::styled(format!(" of {}", count.total), pen.muted()),
        ]));
    }
    for mail in data.recent_mail.iter().flatten() {
        let from = clean(mail.from.as_deref().unwrap_or("(gone)"), 40);
        let subject = clean(mail.subject.as_deref().unwrap_or("(cannot be opened)"), 80);
        let mark = if mail.unread { "* " } else { "  " };
        let when = ago(now, mail.at.as_raw());
        let from_width = 9.min(width / 3).max(3);
        let used = 2 + from_width + 1 + when.chars().count() + 1;
        let subject = clip(&subject, width.saturating_sub(used), look.ascii());
        let subject_style = if mail.unread {
            pen.text().add_modifier(Modifier::BOLD)
        } else {
            pen.text()
        };
        lines.push(Line::from(vec![
            Span::styled(mark.to_owned(), pen.key()),
            Span::styled(
                format!("{:<from_width$}", clip(&from, from_width, look.ascii())),
                nick::style(look, &from),
            ),
            Span::raw(" "),
            Span::styled(subject, subject_style),
            Span::styled(format!(" {when}"), pen.muted()),
        ]));
    }
    if data.recent_mail.as_ref().is_some_and(Vec::is_empty) {
        lines.push(Line::styled(
            "Your mailbox is empty.".to_owned(),
            pen.muted(),
        ));
    }
    lines.truncate(usize::from(inner.height));
    frame.render_widget(Paragraph::new(lines), inner);
}
