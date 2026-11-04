//! Popups and modals: a centred box over the body, with a title, wrapped text and the keys that
//! answer it (UI-1, D102).
//!
//! ```text
//!        +- Confirm ---------------------------+
//!        | Remove this post for everyone?      |
//!        |                                     |
//!        |  y remove   any other key keep      |
//!        +-------------------------------------+
//! ```
//!
//! **Using it.** Build a [`Popup`] and call [`render`] with the **body** rectangle the frame
//! returned ([`super::frame::FrameSpec::draw`]). The popup is placed inside that rectangle and
//! clamped to it, so it can never cover the header bar, the status bar or the marker cell in it:
//! those are outside the body. It clears the cells under it and draws a shadow inside the body,
//! also clamped.
//!
//! **Words, not only colour.** The kind of popup is its title word (`Notice`, `Problem`,
//! `Confirm` from the string table or fixed here), with colour as an extra cue. A popup is not a
//! security marker: it never uses the reserved style (bold + reverse). Its text is chrome or
//! sanitized content ([`PopupLine`]); a popup cannot be handed raw text.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use russh_bbs_core::sanitize::SingleLine;

use super::frame::{ChromeText, Hint};
use super::table::framed;
use crate::settings::Look;

/// Narrowest and widest a popup is drawn, in columns.
const MIN_WIDTH: u16 = 30;
const MAX_WIDTH: u16 = 64;

/// What a popup is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PopupKind {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of the popup API; the info case is used by the screens UI-4 moves onto popups"
        )
    )]
    Info,
    Error,
    Confirm,
}

/// One line of popup text: chrome, or content that is sanitized when the line is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PopupLine(String);

impl PopupLine {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "part of the popup API for chrome-only popups")
    )]
    pub(crate) fn chrome(text: ChromeText<'_>) -> Self {
        Self(text.as_str().to_owned())
    }

    /// Text the code formatted from numbers and chrome.
    pub(crate) fn plain(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// Content from a member or the store, sanitized with the shared sanitizer.
    pub(crate) fn content(text: &str) -> Self {
        Self(SingleLine::new(400).sanitize(text).into_string())
    }
}

/// A popup to draw.
pub(crate) struct Popup<'a> {
    pub(crate) kind: PopupKind,
    pub(crate) title: ChromeText<'a>,
    pub(crate) lines: Vec<PopupLine>,
    /// The answering keys, drawn on the last row.
    pub(crate) keys: &'a [Hint<'a>],
}

/// How many rows `text` takes wrapped at `width`; counts a little high rather than low.
fn wrapped_rows(text: &str, width: usize) -> usize {
    let width = width.max(1);
    let mut rows = 1;
    let mut used = 0;
    for word in text.split(' ') {
        let mut w = Span::raw(word).width();
        if used > 0 && used + 1 + w > width {
            rows += 1;
            used = 0;
        }
        if used > 0 {
            used += 1;
        }
        while w > width {
            rows += 1;
            w -= width;
        }
        used += w;
    }
    rows
}

/// Where a popup of `lines` and `keys` goes inside `body`.
pub(crate) fn place(body: Rect, popup: &Popup<'_>) -> Rect {
    let longest = popup
        .lines
        .iter()
        .map(|line| Span::raw(line.0.as_str()).width())
        .max()
        .unwrap_or(0);
    let want = u16::try_from(longest + 4).unwrap_or(MAX_WIDTH);
    let width = want.clamp(MIN_WIDTH, MAX_WIDTH).min(body.width);
    let inner = usize::from(width.saturating_sub(4));
    let text_rows: usize = popup
        .lines
        .iter()
        .map(|line| wrapped_rows(&line.0, inner))
        .sum();
    let key_rows = usize::from(!popup.keys.is_empty()) * 2;
    let height = u16::try_from(text_rows + key_rows + 2)
        .unwrap_or(u16::MAX)
        .min(body.height);
    Rect {
        x: body.x + (body.width - width) / 2,
        y: body.y + (body.height - height) / 2,
        width,
        height,
    }
}

/// Draw `popup` centred in `body`.
pub(crate) fn render(frame: &mut Frame<'_>, body: Rect, look: Look, popup: &Popup<'_>) {
    let area = place(body, popup);
    if area.width < 3 || area.height < 3 {
        return;
    }
    // A shadow one cell right and below, kept inside the body.
    if look.depth().has_color() {
        let shadow = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width,
            height: area.height,
        }
        .intersection(body);
        frame.render_widget(
            Block::default().style(Style::new().add_modifier(Modifier::DIM)),
            shadow,
        );
    }
    frame.render_widget(Clear, area);
    let tone = match popup.kind {
        PopupKind::Info => look.palette().info,
        PopupKind::Error => look.palette().error,
        PopupKind::Confirm => look.palette().heading,
    };
    let title = popup.title.as_str().to_owned();
    let mut block = framed(look, Some(title)).border_style(look.accent(Style::new(), tone));
    block = block.style(look.bg(look.surfaces().popup_bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let key_rows = u16::from(!popup.keys.is_empty()) * 2;
    let text_area = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        height: inner.height.saturating_sub(key_rows),
        ..inner
    };
    let lines: Vec<Line<'static>> = popup
        .lines
        .iter()
        .map(|line| Line::raw(line.0.clone()))
        .collect();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), text_area);
    if !popup.keys.is_empty() && inner.height >= 2 {
        let key_style = look.accent(
            Style::new().add_modifier(Modifier::BOLD),
            look.palette().key,
        );
        let mut spans: Vec<Span<'static>> = Vec::new();
        for hint in popup.keys {
            spans.push(Span::styled(hint.key.as_str().to_owned(), key_style));
            spans.push(Span::raw(format!(" {}   ", hint.label.as_str())));
        }
        let row = Rect {
            y: inner.y + inner.height - 1,
            height: 1,
            x: inner.x + 1,
            width: inner.width.saturating_sub(2),
        };
        frame.render_widget(
            Paragraph::new(Line::from(spans)).alignment(Alignment::Left),
            row,
        );
    }
}

#[cfg(test)]
mod tests;
