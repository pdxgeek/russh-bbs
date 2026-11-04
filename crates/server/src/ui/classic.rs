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
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    HighlightSpacing, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::menu::{MenuPage, PageItem};
use russh_bbs_core::waiting_room::{Notice, NoticeKind, View};

use super::frame::{ChromeText, FrameSpec, Hint};
use super::keys;
use super::strings::StringTable;
use super::table;
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
    /// Markers for the status bar's marker cell.
    pub(crate) markers: &'a [SecurityMarker],
    /// The header clock text, if the session has one (UTC, redrawn with the screen).
    pub(crate) clock: Option<&'a str>,
    /// Mail is withheld on this transport (telnet): the help says so (D93 retired the D78
    /// sentence about mail at rest).
    pub(crate) mail_over_ssh_only: bool,
}

/// The markers a member screen shows over ssh: none (D93). The server-readable status row is
/// retired, so no row is reserved and content gets the space back. Telnet's `UNENCRYPTED` is a
/// transport footer outside every layout, and staff screens add `LOW-SECURITY` themselves.
pub(crate) const MENU_MARKERS: [SecurityMarker; 0] = [];

/// The status-bar hints every menu shows. `lightbar` adds the cursor keys.
pub(crate) fn menu_hints(strings: &StringTable, lightbar: bool) -> Vec<Hint<'_>> {
    let mut hints = Vec::with_capacity(5);
    if lightbar {
        hints.push(Hint::new(
            ChromeText::fixed("Up/Down"),
            ChromeText::table(strings, keys::HINT_MOVE),
        ));
        hints.push(Hint::new(
            ChromeText::fixed("Enter"),
            ChromeText::table(strings, keys::HINT_CHOOSE),
        ));
    }
    hints.push(Hint::new(
        ChromeText::fixed("?"),
        ChromeText::table(strings, keys::HINT_HELP),
    ));
    hints.push(Hint::new(
        ChromeText::fixed("Esc"),
        ChromeText::table(strings, keys::HINT_BACK),
    ));
    hints.push(Hint::new(
        ChromeText::fixed("Ctrl-C"),
        ChromeText::table(strings, keys::HINT_LOGOFF),
    ));
    hints
}

/// The frame (header bar and status bar) for a menu screen.
pub(crate) fn frame_spec<'a>(screen: &'a MenuScreen<'a>, hints: &'a [Hint<'a>]) -> FrameSpec<'a> {
    FrameSpec::new(screen.strings, screen.look, screen.tier)
        .markers(screen.markers)
        .place(ChromeText::table(screen.strings, screen.page.title))
        .clock(screen.clock)
        .hints(hints)
}

/// Draw the screen: the frame's bars, and the menu box in the body between them.
pub(crate) fn render(frame: &mut Frame<'_>, screen: &MenuScreen<'_>) {
    let hints = menu_hints(screen.strings, false);
    let body = frame_spec(screen, &hints).draw(frame);
    if screen.page.help {
        draw_help(frame, body, screen, None);
    } else {
        draw_menu(frame, body, screen, None);
    }
}

/// The box a menu is drawn in: centred in `body`, no wider than [`settings::MAX_WIDTH`].
fn menu_box(body: Rect) -> Rect {
    let width = body.width.min(settings::MAX_WIDTH);
    Rect {
        x: body.x + (body.width - width) / 2,
        width,
        ..body
    }
}

/// The menu: heading lines, the items as a list, notices under them. `cursor` is the lightbar's
/// highlighted row; `None` draws no highlight. The menu box is the same in every layout.
pub(crate) fn draw_menu(
    frame: &mut Frame<'_>,
    body: Rect,
    screen: &MenuScreen<'_>,
    cursor: Option<usize>,
) {
    let look = screen.look;
    let strings = screen.strings;
    let (bold, dim, _, key) = styles(look);
    let outer = menu_box(body);
    let block = table::framed(look, Some(strings.text(screen.page.title).to_owned()));
    let inner = block.inner(outer);
    frame.render_widget(block, outer);
    let mut head: Vec<Line<'static>> = screen
        .body
        .iter()
        .map(|text| Line::styled(text.clone(), dim))
        .collect();
    if !head.is_empty() {
        head.push(Line::raw(""));
    }
    let mut tail: Vec<Line<'static>> = Vec::new();
    if let Some(note) = screen.page.notice {
        tail.push(notice_line(
            strings,
            NoticeKind::Info,
            strings.text(note),
            look,
            bold,
        ));
    }
    if let Some(notice) = screen.notice {
        tail.push(notice_line(strings, notice.kind, notice.text, look, bold));
    }
    // The items outrank the heading and the notices when the box is short.
    let items = u16::try_from(screen.page.items.len()).unwrap_or(u16::MAX);
    let tail_rows = u16::try_from(tail.len()).unwrap_or(0);
    let head_rows = u16::try_from(head.len()).unwrap_or(0);
    let spare = inner.height.saturating_sub(items);
    let tail_rows = tail_rows.min(spare);
    let head_rows = head_rows.min(spare - tail_rows);
    let [head_area, list_area, tail_area] = Layout::vertical([
        Constraint::Length(head_rows),
        Constraint::Min(1),
        Constraint::Length(tail_rows),
    ])
    .areas(inner.inner(Margin {
        horizontal: 1,
        vertical: 0,
    }));
    frame.render_widget(Paragraph::new(head).wrap(Wrap { trim: false }), head_area);
    let pointer = strings.text(keys::POINTER).to_owned();
    let list_items: Vec<ListItem<'static>> = screen
        .page
        .items
        .iter()
        .map(|item| ListItem::new(item_line(strings, item, key, dim)))
        .collect();
    let surfaces = look.surfaces();
    let selected_style = look
        .accent(
            Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            look.palette().selected,
        )
        .patch(look.bg(surfaces.select_bg));
    let list = List::new(list_items)
        .highlight_style(selected_style)
        .highlight_symbol(format!("{pointer} "))
        .highlight_spacing(HighlightSpacing::Always);
    let mut state = ListState::default();
    state.select(cursor);
    frame.render_stateful_widget(list, list_area, &mut state);
    if usize::from(list_area.height) < screen.page.items.len() {
        let mut bar_state = ScrollbarState::new(
            screen
                .page
                .items
                .len()
                .saturating_sub(usize::from(list_area.height)),
        )
        .position(state.offset());
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            outer.inner(Margin {
                horizontal: 0,
                vertical: 1,
            }),
            &mut bar_state,
        );
    }
    frame.render_widget(Paragraph::new(tail).wrap(Wrap { trim: false }), tail_area);
}

/// The help screen: the universal keys beside the menu on a wide box, stacked on a narrow one.
/// `extra` is a string-table key of one more line a layout adds (the lightbar's cursor keys).
pub(crate) fn draw_help(
    frame: &mut Frame<'_>,
    body: Rect,
    screen: &MenuScreen<'_>,
    extra: Option<&str>,
) {
    let look = screen.look;
    let strings = screen.strings;
    let (_, dim, _, key) = styles(look);
    let outer = menu_box(body);
    let block = table::framed(look, Some(strings.text(keys::HELP_TITLE).to_owned()));
    let inner = block.inner(outer).inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    frame.render_widget(block, outer);
    if inner.height == 0 {
        return;
    }
    let [main, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    frame.render_widget(
        Paragraph::new(Line::styled(
            strings.text(keys::HELP_FOOTER).to_owned(),
            dim,
        )),
        footer,
    );
    let mut text: Vec<Line<'static>> = vec![Line::raw(strings.text(keys::HELP_INTRO).to_owned())];
    for line in [
        keys::HELP_KEY_HELP,
        keys::HELP_KEY_BACK,
        keys::HELP_KEY_QUIT,
    ] {
        text.push(Line::styled(format!("  {}", strings.text(line)), key));
    }
    if screen.mail_over_ssh_only {
        text.push(Line::raw(
            strings.text(keys::NOTICE_MAIL_SSH_ONLY).to_owned(),
        ));
    }
    text.push(Line::raw(strings.text(keys::HELP_MENU).to_owned()));
    if let Some(extra) = extra {
        text.push(Line::raw(strings.text(extra).to_owned()));
    }
    let items: Vec<Line<'static>> = screen
        .page
        .items
        .iter()
        .map(|item| item_line(strings, item, key, dim))
        .collect();
    if main.width >= 64 {
        let [left, right] = Layout::horizontal([Constraint::Fill(3), Constraint::Fill(2)])
            .spacing(2)
            .areas(main);
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), left);
        frame.render_widget(Paragraph::new(items), right);
    } else {
        text.extend(items);
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), main);
    }
}

/// Everything the layout needs to draw a content screen (a list, a profile, a form): lines the
/// caller built from sanitized content, in the same box as a menu, with the same frame
/// (header bar, status bar and its marker cell). The lines are given the body rectangle only.
pub(crate) struct ContentScreen<'a> {
    pub(crate) strings: &'a StringTable,
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) look: Look,
    pub(crate) tier: Tier,
    pub(crate) markers: &'a [SecurityMarker],
    pub(crate) clock: Option<&'a str>,
}

/// The hints a content screen shows when it has none of its own: the universal keys.
pub(crate) fn content_hints(strings: &StringTable) -> Vec<Hint<'_>> {
    menu_hints(strings, false)
}

/// Draw a content screen on the older geometry: the box fills the screen and a status row is
/// reserved only when a marker applies (the chat, mail and door screens, which size their own
/// content to the box and move onto the frame in UI-4/UI-5). The row is the same marker row.
pub(crate) fn render_content_bare(frame: &mut Frame<'_>, screen: ContentScreen<'_>) {
    let (content, status) = super::chrome::split_status(frame.area(), screen.markers);
    settings::boxed(
        frame,
        content,
        screen.strings.text(keys::TITLE),
        screen.lines,
        screen.look,
    );
    if let Some(row) = status {
        super::chrome::render_status(frame, row, screen.markers, screen.tier, screen.strings);
    }
}

/// Draw a content screen: the frame's bars, and the box with the caller's lines in the body.
pub(crate) fn render_content(frame: &mut Frame<'_>, screen: ContentScreen<'_>) {
    let hints = content_hints(screen.strings);
    let body = FrameSpec::new(screen.strings, screen.look, screen.tier)
        .markers(screen.markers)
        .clock(screen.clock)
        .hints(&hints)
        .draw(frame);
    settings::boxed_flush(
        frame,
        body,
        screen.strings.text(keys::TITLE),
        screen.lines,
        screen.look,
    );
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
