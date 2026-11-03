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
use ratatui::style::{Modifier, Style};
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
    /// Mail is withheld on this transport (telnet): the help says so instead of giving the D78
    /// sentence about mail at rest.
    pub(crate) mail_over_ssh_only: bool,
}

/// The markers a menu screen shows: the whole signed-in session is server-readable (MVP).
pub(crate) const MENU_MARKERS: [SecurityMarker; 1] = [SecurityMarker::ServerReadable];

/// Draw the screen: content box above, status row below.
pub(crate) fn render(frame: &mut Frame<'_>, screen: &MenuScreen<'_>) {
    let (content, status) = chrome::split_status(frame.area());
    let lines = if screen.page.help {
        help_lines(screen, None)
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

/// Everything the layout needs to draw a content screen (a list, a profile, a form): lines the
/// caller built from sanitized content, in the same box as a menu, with the same reserved
/// status row underneath. The lines are given a rectangle that excludes the status row.
pub(crate) struct ContentScreen<'a> {
    pub(crate) strings: &'a StringTable,
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) look: Look,
    pub(crate) tier: Tier,
    pub(crate) markers: &'a [SecurityMarker],
}

/// Draw a content screen: content box above, status row below.
pub(crate) fn render_content(frame: &mut Frame<'_>, screen: ContentScreen<'_>) {
    let (content, status) = chrome::split_status(frame.area());
    settings::boxed(
        frame,
        content,
        screen.strings.text(keys::TITLE),
        screen.lines,
        screen.look,
    );
    if let Some(row) = status {
        chrome::render_status(frame, row, screen.markers, screen.tier, screen.strings);
    }
}

/// The lines of a [`View`] (the invite screens, M2-D): heading, body, note, notice and footer,
/// ready for [`render_content`]. Everything in the view is chrome or already sanitized.
pub(crate) fn view_lines(strings: &StringTable, view: &View, look: Look) -> Vec<Line<'static>> {
    let (bold, dim, heading, _) = styles(look);
    let mut out = vec![
        Line::styled(strings.text(view.heading).to_owned(), heading),
        Line::raw(""),
    ];
    out.extend(view.body.iter().map(|text| Line::raw(text.clone())));
    if let Some(note) = view.note {
        out.push(Line::raw(""));
        out.push(Line::styled(strings.text(note).to_owned(), dim));
    }
    if let Some(notice) = view.notice {
        out.push(Line::raw(""));
        out.push(notice_line(strings, notice.kind, notice.text, look, bold));
    }
    out.push(Line::raw(""));
    out.push(Line::styled(strings.text(view.footer).to_owned(), dim));
    out
}

/// The styles screens built on this layout share: bold, dim, heading and hotkey.
pub(crate) fn styles(look: Look) -> (Style, Style, Style, Style) {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let dim = Style::new().add_modifier(Modifier::DIM);
    (
        bold,
        dim,
        look.accent(bold, look.palette().heading),
        look.accent(bold, look.palette().key),
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

pub(crate) fn item_line(
    strings: &StringTable,
    item: &PageItem,
    key: Style,
    dim: Style,
) -> Line<'static> {
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
pub(crate) fn notice_line(
    strings: &StringTable,
    kind: NoticeKind,
    text: &str,
    look: Look,
    bold: Style,
) -> Line<'static> {
    let (prefix, color) = match kind {
        NoticeKind::Info => (keys::NOTICE_INFO, look.palette().info),
        NoticeKind::Error => (keys::NOTICE_ERROR, look.palette().error),
    };
    // A notice from the member flow is a string-table key; any other text is shown as it is.
    Line::styled(
        format!("{}: {}", strings.text(prefix), strings.text(text)),
        look.accent(bold, color),
    )
}

/// The help screen. `extra` is a string-table key of one more line a layout adds under the
/// universal keys (the lightbar explains its cursor keys there).
pub(crate) fn help_lines(screen: &MenuScreen<'_>, extra: Option<&str>) -> Vec<Line<'static>> {
    let (_, dim, heading, key) = styles(screen.look);
    let strings = screen.strings;
    // No blank lines at all: the D78 mail explanation (two lines at 80 columns) goes right after the
    // keys, and with the sysop's ten items the lightbar's help is exactly 21 rows, which is what an
    // 80x24 screen has inside its box. A test draws it in every layout and checks the footer is
    // there; the next menu item will have to page this screen (D65) or that test fails.
    let mut out = vec![
        Line::styled(strings.text(keys::HELP_TITLE).to_owned(), heading),
        Line::raw(strings.text(keys::HELP_INTRO).to_owned()),
    ];
    for line in [
        keys::HELP_KEY_HELP,
        keys::HELP_KEY_BACK,
        keys::HELP_KEY_QUIT,
    ] {
        out.push(Line::styled(format!("  {}", strings.text(line)), key));
    }
    let mail = if screen.mail_over_ssh_only {
        keys::NOTICE_MAIL_SSH_ONLY
    } else {
        keys::HELP_MAIL
    };
    out.push(Line::raw(strings.text(mail).to_owned()));
    out.push(Line::raw(strings.text(keys::HELP_MENU).to_owned()));
    if let Some(extra) = extra {
        out.push(Line::raw(strings.text(extra).to_owned()));
    }
    for item in &screen.page.items {
        out.push(item_line(strings, item, key, dim));
    }
    out.push(Line::styled(
        strings.text(keys::HELP_FOOTER).to_owned(),
        dim,
    ));
    out
}
