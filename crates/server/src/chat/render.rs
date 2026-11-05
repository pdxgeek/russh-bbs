//! Drawing the chat screen as three panes (UI-4, D-TBD): the **room list** on the left, the
//! **conversation** in the middle with the input line under it, and **who is online** on the
//! right. The frame's header and status bars sit around them; the marker cell is the status
//! bar's left cell, which no pane can reach.
//!
//! ```text
//! +- Rooms ------+ +- #lobby ----------------------------+ +- Online (3) --+
//! | > #lobby     | | * You are in #lobby.                | |  ann          |
//! |   #retro     | | <ann> morning                       | |  bob          |
//! |              | | <bob> hi ann                        | |  carol        |
//! |              | +- Say something ----------------------+ |               |
//! |              | | > hello_                              | |               |
//! +--------------+ +---------------------------------------+ +---------------+
//! ```
//!
//! **Where other people's words go.** Handles and lines were sanitized before they entered the
//! hub and are cleaned again here. They are drawn only inside the panes (never in a bar or the
//! marker cell), in the theme's content colours, and **never in the reserved marker style**.
//! A nick is coloured by a hash of its name ([`crate::ui::screens::nick`]), which uses no
//! modifier at all. A whisper is told apart by words ("whispers"), not only by colour.
//!
//! **Narrow terminals.** Below 70 columns the room list goes, below 56 the who's-online pane
//! goes too, and the borderless compact layout shows the conversation alone: the same screen,
//! never a different one.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TextLine, Span};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use russh_bbs_core::chat::{
    ChatPage, ChatScreen, ListPage, MAX_LINE_CHARS, RoomPage, Row, strings,
};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::sanitize::{SingleLine, Username};
use russh_bbs_core::waiting_room::NoticeKind;

use crate::settings::Look;
use crate::ui::art::draw::ArtCtx;
use crate::ui::frame::{ChromeText, FrameSpec, Hint};
use crate::ui::popup::{self, Popup, PopupKind, PopupLine};
use crate::ui::screens::nick;
use crate::ui::strings::StringTable;
use crate::ui::table::{self, Cell, Column, TableRow, TableSpec, Title, Width};
use crate::ui::theme::Element;

/// Everything chat needs to draw besides the screen.
pub(crate) struct ChatCtx<'a> {
    pub(crate) strings: &'a StringTable,
    pub(crate) look: Look,
    pub(crate) tier: Tier,
    pub(crate) markers: &'a [SecurityMarker],
    pub(crate) clock: Option<&'a str>,
    pub(crate) art: Option<&'a ArtCtx<'a>>,
}

/// Text from another person, sanitized once more on its way to the terminal.
fn clean(text: &str) -> String {
    SingleLine::new(MAX_LINE_CHARS).sanitize(text).into_string()
}

/// Split `text` into pieces at most `width` columns wide, at spaces where it can; a word longer
/// than a row is cut. Always makes progress.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut used = 0_usize;
    for word in text.split_inclusive(' ') {
        let w = Span::raw(word).width();
        let trimmed = Span::raw(word.trim_end()).width();
        if used > 0 && used + trimmed > width {
            out.push(std::mem::take(&mut current).trim_end().to_owned());
            used = 0;
        }
        if w <= width || used > 0 {
            current.push_str(word);
            used += w;
            continue;
        }
        // A word wider than a row: cut it where the columns run out.
        for c in word.chars() {
            let cw = Span::raw(c.to_string()).width();
            if used > 0 && used + cw > width {
                out.push(std::mem::take(&mut current));
                used = 0;
            }
            current.push(c);
            used += cw;
        }
    }
    out.push(current.trim_end().to_owned());
    out
}

/// Where the three panes go. A pure function of the body, so it is tested without drawing: the
/// panes never overlap and never leave the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Panes {
    pub(crate) rooms: Option<Rect>,
    pub(crate) talk: Rect,
    pub(crate) who: Option<Rect>,
}

/// Narrowest body that shows the who's-online pane, and the room list as well.
const WHO_FROM: u16 = 56;
const ROOMS_FROM: u16 = 70;

/// The arrangement of `body`.
#[must_use]
pub(crate) fn panes(body: Rect, borderless: bool) -> Panes {
    if borderless || body.width < WHO_FROM || body.height < 6 {
        return Panes {
            rooms: None,
            talk: body,
            who: None,
        };
    }
    let wide = body.width >= 100;
    let who_width = if wide { 22 } else { 16 };
    if body.width < ROOMS_FROM {
        let [talk, who] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(who_width)]).areas(body);
        return Panes {
            rooms: None,
            talk,
            who: Some(who),
        };
    }
    let rooms_width = if wide { 22 } else { 18 };
    let [rooms, talk, who] = Layout::horizontal([
        Constraint::Length(rooms_width),
        Constraint::Fill(1),
        Constraint::Length(who_width),
    ])
    .areas(body);
    Panes {
        rooms: Some(rooms),
        talk,
        who: Some(who),
    }
}

/// The hints the status bar shows for the page on screen.
fn hints(screen: &ChatScreen) -> Vec<Hint<'static>> {
    let hint = |key: &'static str, label: &'static str| {
        Hint::new(ChromeText::fixed(key), ChromeText::fixed(label))
    };
    match screen.page() {
        ChatPage::List(list) if list.naming.is_some() => {
            vec![hint("Enter", "open room"), hint("Esc", "cancel")]
        }
        ChatPage::List(_) => {
            let mut hints = vec![hint("Up/Down", "choose"), hint("Enter", "join")];
            if screen.can_manage() {
                hints.push(hint("n", "new room"));
                hints.push(hint("x", "close room"));
            }
            hints.push(hint("Esc", "back"));
            hints
        }
        ChatPage::Room(_) => vec![
            hint("Enter", "send"),
            hint("/w", "whisper"),
            hint("/who", "who"),
            hint("Esc", "leave"),
        ],
    }
}

/// Draw the chat screen into `frame`.
pub(crate) fn render(frame: &mut Frame<'_>, screen: &ChatScreen, ctx: &ChatCtx<'_>) {
    let hints = hints(screen);
    let body = FrameSpec::new(ctx.strings, ctx.look, ctx.tier)
        .markers(ctx.markers)
        .place(ChromeText::fixed("Chat"))
        .clock(ctx.clock)
        .hints(&hints)
        .art(ctx.art.map(|art| crate::ui::frame::ArtRequest {
            ctx: art,
            slot: crate::ui::art::Slot::Area,
            keep: 14,
        }))
        .draw(frame);
    match screen.page() {
        ChatPage::List(list) => render_list(frame, body, screen, list, ctx),
        ChatPage::Room(room) => render_room(frame, body, screen, room, ctx),
    }
}

fn pen(look: Look) -> crate::ui::widgets::Pen {
    crate::ui::widgets::Pen(look)
}

fn render_list(
    frame: &mut Frame<'_>,
    body: Rect,
    screen: &ChatScreen,
    list: &ListPage,
    ctx: &ChatCtx<'_>,
) {
    let side = body.width >= 80;
    let (rooms_area, help_area) = if side {
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
                .areas(body);
        (left, Some(right))
    } else {
        (body, None)
    };
    let rows: Vec<TableRow> = list
        .rooms
        .iter()
        .map(|room| TableRow::new(vec![Cell::content(&format!("#{}", room.name.as_str()))]))
        .collect();
    let _ = table::render(
        frame,
        rooms_area,
        ctx.look,
        &TableSpec {
            title: Title::Chrome(ChromeText::fixed(strings::HEADING_ROOMS)),
            columns: vec![Column::new(ChromeText::fixed("Room"), Width::Fill(1))],
            rows,
            selected: Some(list.cursor),
            bordered: true,
            top: 0,
            empty: strings::NO_ROOMS,
            numbered: false,
            footer: None,
        },
    );
    if let Some(area) = help_area {
        let inner = ctx.look.panel(
            frame,
            area,
            Some("Commands"),
            Element::Pane,
            false,
        );
        let pen = pen(ctx.look);
        let mut lines = vec![TextLine::styled(
            "Inside a room, type to talk.".to_owned(),
            pen.text(),
        )];
        lines.push(TextLine::raw(""));
        for help in strings::HELP {
            lines.push(TextLine::styled((*help).to_owned(), pen.muted()));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
    if let Some(name) = &list.naming {
        let keys = [Hint::new(
            ChromeText::fixed("Enter"),
            ChromeText::fixed("open"),
        )];
        popup::render(
            frame,
            body,
            ctx.look,
            &Popup {
                kind: PopupKind::Info,
                title: ChromeText::fixed("New room"),
                lines: vec![
                    PopupLine::plain(strings::PROMPT_NEW),
                    PopupLine::plain(format!("> {}{}", clean(name), ctx.look.cursor())),
                ],
                keys: &keys,
            },
        );
    } else if list.closing.is_some() {
        let keys = [
            Hint::new(ChromeText::fixed("y"), ChromeText::fixed("close")),
            Hint::new(
                ChromeText::fixed("any other key"),
                ChromeText::fixed("keep"),
            ),
        ];
        popup::render(
            frame,
            body,
            ctx.look,
            &Popup {
                kind: PopupKind::Confirm,
                title: ChromeText::fixed("Confirm"),
                lines: vec![PopupLine::plain(strings::CONFIRM_CLOSE)],
                keys: &keys,
            },
        );
    } else if let Some(notice) = screen.notice() {
        let (kind, text) = notice.text();
        let keys = [Hint::new(
            ChromeText::fixed("any key"),
            ChromeText::fixed("close"),
        )];
        popup::render(
            frame,
            body,
            ctx.look,
            &Popup {
                kind: if kind == NoticeKind::Error {
                    PopupKind::Error
                } else {
                    PopupKind::Info
                },
                title: ChromeText::table(
                    ctx.strings,
                    if kind == NoticeKind::Error {
                        crate::ui::keys::NOTICE_ERROR
                    } else {
                        crate::ui::keys::NOTICE_INFO
                    },
                ),
                lines: vec![PopupLine::plain(text)],
                keys: &keys,
            },
        );
    }
}

fn render_room(
    frame: &mut Frame<'_>,
    body: Rect,
    screen: &ChatScreen,
    room: &RoomPage,
    ctx: &ChatCtx<'_>,
) {
    let layout = panes(body, ctx.look.is_borderless());
    if let Some(area) = layout.rooms {
        draw_rooms(frame, area, room, ctx);
    }
    if let Some(area) = layout.who {
        draw_who(frame, area, room, ctx);
    }
    draw_talk(frame, layout.talk, screen, room, ctx);
}

fn draw_rooms(frame: &mut Frame<'_>, area: Rect, room: &RoomPage, ctx: &ChatCtx<'_>) {
    let inner = ctx
        .look
        .panel(frame, area, Some(strings::HEADING_ROOMS), Element::Pane, false);
    let pen = pen(ctx.look);
    let width = usize::from(inner.width);
    let lines: Vec<TextLine<'static>> = room
        .rooms
        .iter()
        .take(usize::from(inner.height))
        .map(|other| {
            let here = other.id == room.room.id;
            let lead = if here { "> " } else { "  " };
            let name = crate::ui::table::clip(
                &format!("#{}", other.name.as_str()),
                width.saturating_sub(2),
                ctx.look.ascii(),
            );
            let style = if here {
                pen.accent().add_modifier(Modifier::UNDERLINED)
            } else {
                pen.text()
            };
            TextLine::from(vec![
                Span::styled(lead.to_owned(), pen.key()),
                Span::styled(name, style),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_who(frame: &mut Frame<'_>, area: Rect, room: &RoomPage, ctx: &ChatCtx<'_>) {
    let title = format!("Online ({})", room.roster.len());
    let inner = ctx
        .look
        .panel(frame, area, Some(&title), Element::Pane, false);
    let width = usize::from(inner.width).saturating_sub(1);
    let rows = usize::from(inner.height);
    let pen = pen(ctx.look);
    let mut lines: Vec<TextLine<'static>> = Vec::new();
    let overflow = room.roster.len() > rows;
    let shown = if overflow { rows.saturating_sub(1) } else { rows };
    for who in room.roster.iter().take(shown) {
        let name = clean(who.as_str());
        let dot = if ctx.look.ascii() { "*" } else { "\u{25cf}" };
        lines.push(TextLine::from(vec![
            Span::styled(format!("{dot} "), nick::style(ctx.look, &name)),
            Span::styled(
                crate::ui::table::clip(&name, width.saturating_sub(2), ctx.look.ascii()),
                nick::style(ctx.look, &name),
            ),
        ]));
    }
    if overflow {
        lines.push(TextLine::styled(
            format!("+{} more", room.roster.len() - shown),
            pen.muted(),
        ));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// One transcript row as styled text pieces, before wrapping: the nick (if any) and the rest.
struct Piece {
    nick: Option<String>,
    text: String,
    kind: Kind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Say,
    Dim,
    Whisper,
}

fn names(who: &[Username]) -> String {
    who.iter()
        .map(|n| clean(n.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn piece(row: &Row) -> Piece {
    let plain = |text: String, kind| Piece {
        nick: None,
        text,
        kind,
    };
    match row {
        Row::Say(line) => Piece {
            nick: Some(clean(line.from.as_str())),
            text: clean(line.text.as_str()),
            kind: Kind::Say,
        },
        Row::Joined(who) => plain(format!("* {} joined", clean(who.as_str())), Kind::Dim),
        Row::Left(who) => plain(format!("* {} left", clean(who.as_str())), Kind::Dim),
        Row::Whisper { from, text } => plain(
            format!(
                "* {} {}: {}",
                clean(from.as_str()),
                strings::WHISPER_FROM,
                clean(text.as_str())
            ),
            Kind::Whisper,
        ),
        Row::WhisperSent { to, text } => plain(
            format!(
                "* {} {}: {}",
                strings::WHISPER_TO,
                clean(to.as_str()),
                clean(text.as_str())
            ),
            Kind::Whisper,
        ),
        Row::Missed(n) => plain(
            format!("* {n} lines were skipped because this connection was slow"),
            Kind::Dim,
        ),
        Row::Here(who) => plain(format!("{}: {}", strings::HERE, names(who)), Kind::Dim),
        Row::Entered(name) => plain(
            format!("{} #{}.", strings::YOU_ARE_IN, clean(name.as_str())),
            Kind::Dim,
        ),
        Row::Info(text) => plain((*text).to_owned(), Kind::Dim),
    }
}

/// The transcript as styled lines for `width` columns, newest last, at most `rows` of them.
pub(crate) fn transcript(
    room: &RoomPage,
    look: Look,
    width: usize,
    rows: usize,
) -> Vec<TextLine<'static>> {
    let pen = pen(look);
    let mut lines: Vec<TextLine<'static>> = Vec::new();
    for row in room.rows.iter().rev() {
        let piece = piece(row);
        let lead = piece
            .nick
            .as_ref()
            .map(|nick| format!("<{nick}> "))
            .unwrap_or_default();
        let lead_chars = lead.chars().count();
        let full = format!("{lead}{}", piece.text);
        let style = match piece.kind {
            Kind::Say => pen.text(),
            Kind::Dim => pen.muted(),
            Kind::Whisper => look.accent(
                Style::new().add_modifier(Modifier::ITALIC),
                look.palette().info,
            ),
        };
        let mut block: Vec<TextLine<'static>> = Vec::new();
        for (index, part) in wrap(&full, width).into_iter().enumerate() {
            let continuation = if index == 0 { "" } else { "  " };
            let part = format!("{continuation}{part}");
            if index == 0
                && let Some(nick) = &piece.nick
            {
                let take = lead_chars.min(part.chars().count());
                let head: String = part.chars().take(take).collect();
                let tail: String = part.chars().skip(take).collect();
                block.push(TextLine::from(vec![
                    Span::styled(head, nick::style(look, nick)),
                    Span::styled(tail, style),
                ]));
            } else {
                block.push(TextLine::styled(part, style));
            }
        }
        block.reverse();
        lines.extend(block);
        if lines.len() >= rows {
            break;
        }
    }
    lines.reverse();
    let skip = lines.len().saturating_sub(rows);
    lines.split_off(skip)
}

fn draw_talk(
    frame: &mut Frame<'_>,
    area: Rect,
    screen: &ChatScreen,
    room: &RoomPage,
    ctx: &ChatCtx<'_>,
) {
    let look = ctx.look;
    let pen = pen(look);
    // The input sits in its own box under the conversation when there is room for the box.
    let boxed_input = area.height >= 8 && !look.is_borderless();
    let (log_area, input_area) = if boxed_input {
        let [log, input] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(3)]).areas(area);
        (log, input)
    } else {
        let [log, input] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
        (log, input)
    };
    let title = format!("#{}", clean(room.room.name.as_str()));
    let inner = look.panel(frame, log_area, Some(&title), Element::Pane, false);
    let notice = screen.notice().map(|notice| {
        let (kind, text) = notice.text();
        let color = if kind == NoticeKind::Error {
            look.palette().error
        } else {
            look.palette().info
        };
        TextLine::styled(
            format!(
                "{}: {text}",
                ctx.strings.text(if kind == NoticeKind::Error {
                    crate::ui::keys::NOTICE_ERROR
                } else {
                    crate::ui::keys::NOTICE_INFO
                })
            ),
            look.accent(Style::new().add_modifier(Modifier::BOLD), color),
        )
    });
    let log_rows = usize::from(inner.height).saturating_sub(usize::from(notice.is_some()));
    let text_width = usize::from(inner.width).saturating_sub(1).max(1);
    let mut lines = transcript(room, look, text_width, log_rows);
    if let Some(notice) = notice {
        while lines.len() < log_rows {
            lines.insert(0, TextLine::raw(""));
        }
        lines.push(notice);
    }
    frame.render_widget(Paragraph::new(lines), inner);
    // A scrollbar shows where the newest lines sit in the 200 kept.
    if inner.width > 2 && inner.height > 2 {
        let total = room.rows.len().max(1);
        let mut state = ScrollbarState::new(total).position(total.saturating_sub(1));
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(look.muted())
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some(if look.ascii() { "|" } else { "\u{2502}" }))
                .thumb_symbol(if look.ascii() { "#" } else { "\u{2503}" }),
            log_area.inner(Margin {
                horizontal: 0,
                vertical: 1,
            }),
            &mut state,
        );
    }
    let typed = clean(&room.input);
    let input_inner = if boxed_input {
        look.panel(
            frame,
            input_area,
            Some("Say something"),
            Element::Pane,
            false,
        )
    } else {
        input_area
    };
    let room_for_input = usize::from(input_inner.width).saturating_sub(3).max(1);
    let shown: String = {
        let chars: Vec<char> = typed.chars().collect();
        let from = chars.len().saturating_sub(room_for_input);
        chars.get(from..).unwrap_or_default().iter().collect()
    };
    frame.render_widget(
        Paragraph::new(TextLine::from(vec![
            Span::styled("> ".to_owned(), pen.key()),
            Span::styled(format!("{shown}{}", look.cursor()), pen.text()),
        ])),
        input_inner,
    );
}
