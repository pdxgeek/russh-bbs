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
use russh_bbs_core::terminal_settings::LayoutId;
use russh_bbs_core::waiting_room::{Notice, NoticeKind, View};

use super::art::Slot;
use super::art::draw::ArtCtx;
use super::board::Board;
use super::frame::{ArtRequest, ChromeText, FrameSpec, Hint};
use super::keys;
use super::strings::StringTable;
use super::theme::Element;
use super::widgets::{self, WidgetView};
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
    /// The board's identity (UI-2): what the identity slot shows.
    pub(crate) board: &'a Board,
    /// The layout the member chose, when the terminal is too small for it and a smaller one is
    /// drawn instead (the screen says so); the layout in force is `look.layout()`.
    pub(crate) wanted: LayoutId,
    /// The focus layout's widget pop-over is open.
    pub(crate) popover: bool,
    /// What to ask for the main-menu header art with (UI-3); `None` draws none.
    pub(crate) art: Option<&'a ArtCtx<'a>>,
    /// The widgets this layout shows the member, and what they show (UI-10).
    pub(crate) widgets: WidgetView<'a>,
}

impl MenuScreen<'_> {
    /// The context the widgets draw with.
    pub(crate) fn widget_cx(&self) -> widgets::Cx<'_> {
        widgets::Cx::new(
            self.look,
            self.tier,
            self.strings,
            self.widgets.data,
            self.board,
            self.widgets.settings,
            self.widgets.now,
        )
    }
}

/// The sentence saying a smaller layout is shown, when one is (UI-2). When the member chose a
/// dashboard layout that does not fit, the dashboard collapses into this line: it carries the
/// figures the member may see (who is online, unread mail, new posts) after the sentence
/// (UI-4). Only figures the widget data holds are shown, so nothing is made up and nothing
/// is shown to a member who may not reach it.
pub(crate) fn fallback_note(screen: &MenuScreen<'_>) -> Option<String> {
    let shown = screen.look.layout();
    (shown != screen.wanted).then(|| {
        let name = |layout: LayoutId| {
            screen
                .strings
                .text(keys::LAYOUT_NAMES[layout_index(layout)])
                .to_owned()
        };
        let note = screen.strings.filled(
            keys::LAYOUT_FALLBACK,
            &[("shown", &name(shown)), ("wanted", &name(screen.wanted))],
        );
        match dashboard_line(screen) {
            Some(figures) => format!("{note}  {figures}"),
            None => note,
        }
    })
}

/// The dashboard's figures on one line: `5 online | 3 unread | 7 new`.
fn dashboard_line(screen: &MenuScreen<'_>) -> Option<String> {
    let data = screen.widgets.data;
    let mut parts: Vec<String> = Vec::new();
    if let Some(online) = &data.online {
        parts.push(format!("{} online", online.len()));
    }
    if let Some(mail) = &data.mail {
        parts.push(format!("{} unread", mail.unread));
    }
    if let Some(posts) = &data.new_posts {
        parts.push(format!("{} new", posts.threads));
    }
    (!parts.is_empty()).then(|| {
        parts.join(if screen.look.ascii() {
            " | "
        } else {
            " \u{b7} "
        })
    })
}

/// A layout's place in [`keys::LAYOUT_NAMES`].
pub(crate) const fn layout_index(layout: LayoutId) -> usize {
    match layout {
        LayoutId::Classic => 0,
        LayoutId::Lightbar => 1,
        LayoutId::Compact => 2,
        LayoutId::Dashboard => 3,
        LayoutId::Wide => 4,
        LayoutId::Focus => 5,
        LayoutId::Split => 6,
    }
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
        // The compact layout is a bare list: no header, so no header art.
        .art(
            screen
                .art
                .filter(|_| !screen.look.is_borderless())
                .map(|ctx| ArtRequest {
                    ctx,
                    slot: Slot::Menu,
                    keep: menu_keep(screen),
                }),
        )
}

/// Rows of the body the menu keeps for itself: its box (the signed-in lines, the fallback note
/// and a blank, the items and the border) or the layout's own minimum, whichever is taller.
/// Art gets only the rows beyond this, so it never squeezes the menu.
fn menu_keep(screen: &MenuScreen<'_>) -> u16 {
    let rows = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
    let own = rows(screen.page.items.len() + screen.body.len() + 4);
    own.max(super::layout::min_body_rows(screen.look.layout()))
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
    draw_menu_in(frame, menu_box(body), screen, cursor, Element::Frame, true);
}

/// The menu rows answer the mouse (UI-9): a click presses the row's own hotkey, the same key
/// the keyboard uses, in every layout.
fn record_rows(screen: &MenuScreen<'_>, area: Rect, offset: usize, cursor: Option<usize>) {
    super::input::list(
        area,
        1,
        offset,
        screen.page.items.len(),
        cursor,
        super::input::Click::Hotkeys(screen.page.items.iter().map(|item| item.hotkey).collect()),
    );
}

/// The menu in exactly `outer`, drawn as `element` (a pane of a pane layout, or the main box).
/// `with_body` puts the signed-in lines and the fallback note above the items; the wide layout
/// shows them in its centre pane instead.
pub(crate) fn draw_menu_in(
    frame: &mut Frame<'_>,
    outer: Rect,
    screen: &MenuScreen<'_>,
    cursor: Option<usize>,
    element: Element,
    with_body: bool,
) {
    let look = screen.look;
    let strings = screen.strings;
    let (bold, dim, _, key) = styles(look);
    let inner = look.panel(
        frame,
        outer,
        Some(strings.text(screen.page.title)),
        element,
        false,
    );
    let mut head: Vec<Line<'static>> = Vec::new();
    if with_body {
        head.extend(
            screen
                .body
                .iter()
                .map(|text| Line::styled(text.clone(), dim)),
        );
        if let Some(note) = fallback_note(screen) {
            head.push(Line::styled(note, dim));
        }
    }
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
    let _ = surfaces;
    let list = List::new(list_items)
        .highlight_style(selected_style)
        .highlight_symbol(format!("{pointer} "))
        .highlight_spacing(HighlightSpacing::Always);
    let mut state = ListState::default();
    state.select(cursor);
    frame.render_stateful_widget(list, list_area, &mut state);
    if !screen.page.help {
        record_rows(screen, list_area, state.offset(), cursor);
    }
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
    let inner = look
        .panel(
            frame,
            outer,
            Some(strings.text(keys::HELP_TITLE)),
            Element::Frame,
            false,
        )
        .inner(Margin {
            horizontal: 1,
            vertical: 0,
        });
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
