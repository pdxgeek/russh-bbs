//! The **classic hotkey** layout: a bordered box with the menu as a list of `[k] Label` lines,
//! a footer of universal keys, and the reserved status row underneath (M2-A).
//!
//! It draws any menu tree: the layout reads a [`MenuPage`] (items, depth, notice, help) and
//! never knows which menus exist. It also owns no strings; every label comes from the
//! [`StringTable`] by key. Content (the sanitized signed-in line) goes in the box; security
//! markers go only in the status row that [`chrome::split_status`] takes out of the area first.
//!
//! Color is never the only signal: notices carry the word `Notice` / `Problem`, and an item
//! whose area does not exist is tagged in words.

use ratatui::Frame;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::menu::{MenuPage, PageItem};
use russh_bbs_core::waiting_room::{Notice, NoticeKind, View};

use super::chrome;
use super::keys;
use super::strings::StringTable;
use crate::settings::{self, Look};

/// Everything the layout needs to draw one menu screen.
pub(crate) struct MenuScreen<'a> {
    pub(crate) strings: &'a StringTable,
    pub(crate) page: &'a MenuPage,
    /// Content lines shown above the menu (sanitized by the caller).
    pub(crate) body: &'a [String],
    /// A notice from the session (saved, icon choice), shown after the menu.
    pub(crate) notice: Option<Notice>,
    pub(crate) look: Look,
    /// Icon tier the markers are drawn in.
    pub(crate) tier: Tier,
    /// Markers for the status row.
    pub(crate) markers: &'a [SecurityMarker],
}

/// Everything the layout needs to draw one content screen that is not a menu (the invite
/// screens, M2-D): a [`View`] of chrome and sanitized lines, in the same frame and with the same
/// reserved status row as the menu.
pub(crate) struct ContentScreen<'a> {
    pub(crate) strings: &'a StringTable,
    pub(crate) view: &'a View,
    pub(crate) look: Look,
    /// Icon tier the markers are drawn in.
    pub(crate) tier: Tier,
    /// Markers for the status row.
    pub(crate) markers: &'a [SecurityMarker],
}

/// The markers a menu screen shows: the whole signed-in session is server-readable (MVP).
pub(crate) const MENU_MARKERS: [SecurityMarker; 1] = [SecurityMarker::ServerReadable];

/// Draw the screen: content box above, status row below.
pub(crate) fn render(frame: &mut Frame<'_>, screen: &MenuScreen<'_>) {
    let (content, status) = chrome::split_status(frame.area());
    let lines = if screen.page.help {
        help_lines(screen)
    } else {
        menu_lines(screen)
    };
    settings::boxed(
        frame,
        content,
        screen.strings.text(keys::TITLE),
        lines,
        screen.look,
    );
    if let Some(row) = status {
        chrome::render_status(frame, row, screen.markers, screen.tier, screen.strings);
    }
}

/// Draw a content screen: the box above, the status row below. The view's lines go in the box
/// only; the status row is split off first and nothing in the view can reach it.
pub(crate) fn render_content(frame: &mut Frame<'_>, screen: &ContentScreen<'_>) {
    let (content, status) = chrome::split_status(frame.area());
    settings::boxed(
        frame,
        content,
        screen.strings.text(keys::TITLE),
        content_lines(screen),
        screen.look,
    );
    if let Some(row) = status {
        chrome::render_status(frame, row, screen.markers, screen.tier, screen.strings);
    }
}

fn content_lines(screen: &ContentScreen<'_>) -> Vec<Line<'static>> {
    let (bold, dim, heading, _) = styles(screen.look);
    let view = screen.view;
    let mut out = vec![
        Line::styled(view.heading.to_owned(), heading),
        Line::raw(""),
    ];
    out.extend(view.body.iter().map(|text| Line::raw(text.clone())));
    if let Some(note) = view.note {
        out.push(Line::raw(""));
        out.push(Line::styled(note.to_owned(), dim));
    }
    if let Some(notice) = view.notice {
        out.push(Line::raw(""));
        out.push(notice_line(
            screen.strings,
            notice.kind,
            notice.text,
            screen.look,
            bold,
        ));
    }
    out.push(Line::raw(""));
    out.push(Line::styled(view.footer.to_owned(), dim));
    out
}

fn styles(look: Look) -> (Style, Style, Style, Style) {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let dim = Style::new().add_modifier(Modifier::DIM);
    (
        bold,
        dim,
        look.accent(bold, Color::Cyan),
        look.accent(bold, Color::Yellow),
    )
}

fn menu_lines(screen: &MenuScreen<'_>) -> Vec<Line<'static>> {
    let (bold, dim, heading, key) = styles(screen.look);
    let strings = screen.strings;
    let mut out = vec![
        Line::styled(strings.text(screen.page.title).to_owned(), heading),
        Line::raw(""),
    ];
    out.extend(screen.body.iter().map(|text| Line::raw(text.clone())));
    if !screen.body.is_empty() {
        out.push(Line::raw(""));
    }
    for item in &screen.page.items {
        out.push(item_line(strings, item, key, dim));
    }
    if let Some(note) = screen.page.notice {
        out.push(Line::raw(""));
        out.push(notice_line(
            strings,
            NoticeKind::Info,
            strings.text(note),
            screen.look,
            bold,
        ));
    }
    if let Some(notice) = screen.notice {
        out.push(Line::raw(""));
        out.push(notice_line(
            strings,
            notice.kind,
            notice.text,
            screen.look,
            bold,
        ));
    }
    out.push(Line::raw(""));
    out.push(Line::styled(
        strings.text(keys::FOOTER_MENU).to_owned(),
        dim,
    ));
    out
}

fn item_line(strings: &StringTable, item: &PageItem, key: Style, dim: Style) -> Line<'static> {
    let mut spans = vec![
        Span::styled(format!("[{}]", item.hotkey), key),
        Span::raw(format!(" {}", strings.text(item.label))),
    ];
    if item.placeholder {
        spans.push(Span::styled(
            format!("  ({})", strings.text(keys::TAG_NOT_BUILT)),
            dim,
        ));
    }
    Line::from(spans)
}

/// The word, not only a color: the message must read correctly on a mono terminal.
fn notice_line(
    strings: &StringTable,
    kind: NoticeKind,
    text: &str,
    look: Look,
    bold: Style,
) -> Line<'static> {
    let (prefix, color) = match kind {
        NoticeKind::Info => (keys::NOTICE_INFO, Color::Green),
        NoticeKind::Error => (keys::NOTICE_ERROR, Color::Red),
    };
    Line::styled(
        format!("{}: {text}", strings.text(prefix)),
        look.accent(bold, color),
    )
}

fn help_lines(screen: &MenuScreen<'_>) -> Vec<Line<'static>> {
    let (_, dim, heading, key) = styles(screen.look);
    let strings = screen.strings;
    let mut out = vec![
        Line::styled(strings.text(keys::HELP_TITLE).to_owned(), heading),
        Line::raw(""),
        Line::raw(strings.text(keys::HELP_INTRO).to_owned()),
    ];
    for line in [
        keys::HELP_KEY_HELP,
        keys::HELP_KEY_BACK,
        keys::HELP_KEY_QUIT,
    ] {
        out.push(Line::styled(format!("  {}", strings.text(line)), key));
    }
    out.push(Line::raw(""));
    out.push(Line::raw(strings.text(keys::HELP_MENU).to_owned()));
    for item in &screen.page.items {
        out.push(item_line(strings, item, key, dim));
    }
    out.push(Line::raw(""));
    out.push(Line::styled(
        strings.text(keys::HELP_FOOTER).to_owned(),
        dim,
    ));
    out
}
