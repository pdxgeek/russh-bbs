//! Drawing the mail screens. Every line is fixed chrome text or member text that core opened and
//! sanitized and that is cleaned again here; it goes into the content box only, so it cannot
//! reach the reserved status row (D39, D45), and no style is applied to it except the plain
//! ones below. There is no privacy notice on any of these screens (D78).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use russh_bbs_core::icons::Tier;
use russh_bbs_core::mail::InboxPage;
use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::terminal_settings::{TerminalSettings, format_utc};
use russh_bbs_core::waiting_room::NoticeKind;

use super::flow::{Input, InputKind, MailScreen, State, confirm_text, reader_title};
use super::strings;
use crate::settings::{self, Look};
use crate::ui::art::Slot as ArtSlot;
use crate::ui::classic::{self, MENU_MARKERS};
use crate::ui::editor::EditorView;
use crate::ui::frame::{ArtRequest, ChromeText, FrameSpec, Hint};
use crate::ui::popup::{self, Popup, PopupKind, PopupLine};
use crate::ui::screens::list::ListView;
use crate::ui::screens::reader as reader_ui;
use crate::ui::strings::StringTable;
use crate::ui::table::{self, Cell, CellStyle, Column, TableRow, TableSpec, Title, Width};
use crate::ui::{Ui, pager};

/// What drawing needs besides the screen.
pub(crate) struct DrawCtx<'a> {
    pub(crate) ui: &'a Ui,
    pub(crate) strings: &'a StringTable,
    pub(crate) settings: &'a TerminalSettings,
    pub(crate) look: Look,
    pub(crate) tier: Tier,
    /// The markers the status bar's marker cell shows (FIX-M3-6).
    pub(crate) markers: &'a [russh_bbs_core::icons::SecurityMarker],
    /// The header bar's clock text.
    pub(crate) clock: Option<&'a str>,
    /// The board's art, for the area header above the inbox (UI-3's `area` slot).
    pub(crate) art: Option<&'a crate::ui::art::draw::ArtCtx<'a>>,
}

/// Member text, cleaned once more on its way to the terminal.
fn clean(text: &str, max: usize) -> String {
    SingleLine::new(max).sanitize(text).into_string()
}

/// Draw whatever the mail screen is showing into `frame`.
pub(crate) fn render(frame: &mut Frame<'_>, screen: &MailScreen, ctx: &DrawCtx<'_>) {
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
    let hints = state_hints(&screen.state);
    let body = FrameSpec::new(ctx.strings, ctx.look, ctx.tier)
        .markers(ctx.markers)
        .place(ChromeText::fixed(strings::TITLE_INBOX))
        .clock(ctx.clock)
        .hints(&hints)
        .art(ctx.art.map(|art| ArtRequest {
            ctx: art,
            slot: ArtSlot::Area,
            keep: 14,
        }))
        .draw(frame);
    match &screen.state {
        State::Inbox {
            page,
            cursor,
            confirm,
        } => {
            inbox_table(frame, body, screen, ctx, page, *cursor);
            if confirm.is_some() {
                let keys = [
                    Hint::new(ChromeText::fixed("y"), ChromeText::fixed("delete")),
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
                        lines: vec![PopupLine::plain(strings::CONFIRM_DELETE)],
                        keys: &keys,
                    },
                );
            }
            // A problem is a modal over the list; the key that dismisses it also acts.
            if let Some((NoticeKind::Error, text)) = &screen.notice {
                let keys = [Hint::new(
                    ChromeText::fixed("any key"),
                    ChromeText::fixed("close"),
                )];
                popup::render(
                    frame,
                    body,
                    ctx.look,
                    &Popup {
                        kind: PopupKind::Error,
                        title: ChromeText::table(ctx.strings, crate::ui::keys::NOTICE_ERROR),
                        lines: vec![PopupLine::content(text)],
                        keys: &keys,
                    },
                );
            }
        }
        State::Start | State::Reader(_) | State::Editor(_) | State::Input(_) => {
            settings::boxed(
                frame,
                body,
                ctx.strings.text(crate::ui::keys::TITLE),
                lines(screen, ctx),
                ctx.look,
            );
        }
    }
}

/// The status-bar hints for a mail screen: fixed words.
fn state_hints(state: &State) -> Vec<Hint<'static>> {
    let hint = |key: &'static str, label: &'static str| {
        Hint::new(ChromeText::fixed(key), ChromeText::fixed(label))
    };
    match state {
        State::Inbox { page, .. } => {
            let mut hints = vec![hint("Enter", "read"), hint("1-9", "jump")];
            if page.can_send {
                hints.push(hint("c", "write"));
            }
            hints.push(hint("d", "delete"));
            hints.push(hint("n/b", "page"));
            hints.push(hint("o", "sort"));
            hints.push(hint("Esc", "back"));
            hints
        }
        State::Input(_) => vec![hint("Enter", "continue"), hint("Esc", "cancel")],
        State::Start | State::Reader(_) | State::Editor(_) => vec![hint("Esc", "back")],
    }
}

/// The inbox as a table: an unread mark, who from, the subject and when, with the sort marked
/// in the header and the window remembered for the paging and jump keys.
fn inbox_table(
    frame: &mut Frame<'_>,
    body: Rect,
    screen: &MailScreen,
    ctx: &DrawCtx<'_>,
    page: &InboxPage,
    cursor: usize,
) {
    let columns = vec![
        Column::new(ChromeText::fixed("From"), Width::Fixed(14)),
        Column::new(ChromeText::fixed("Subject"), Width::Fill(1)),
        Column::new(ChromeText::fixed("Sent (UTC)"), Width::Fixed(16)).hide_below(70),
    ];
    let rows: Vec<TableRow> = page
        .entries
        .iter()
        .map(|entry| {
            let from = entry.from.as_deref().unwrap_or(strings::GONE_MEMBER);
            let subject = entry
                .subject
                .as_deref()
                .map_or_else(|| strings::CANNOT_OPEN.to_owned(), |s| clean(s, 120));
            // A plain asterisk marks an unread message. It is ordinary text, never a marker.
            let mark = if entry.unread { "* " } else { "  " };
            let style = if entry.unread {
                CellStyle::Bold
            } else {
                CellStyle::Plain
            };
            TableRow::new(vec![
                Cell::content(&format!("{mark}{}", clean(from, 64))).styled(CellStyle::Key),
                Cell::content(&subject).styled(style),
                Cell::plain(format_utc(entry.at).chars().take(16).collect::<String>())
                    .styled(CellStyle::Dim),
            ])
        })
        .collect();
    let info = match &screen.notice {
        Some((NoticeKind::Info, text)) => Some(format!(
            "{}: {text}",
            ctx.strings.text(crate::ui::keys::NOTICE_INFO)
        )),
        _ => None,
    };
    let counts = format!(
        "{} {}, {} unread  -  Page {}",
        page.total,
        if page.total == 1 {
            "message"
        } else {
            "messages"
        },
        page.unread,
        page.page + 1
    );
    let previous = screen.view.get();
    let drawn = table::render(
        frame,
        body,
        ctx.look,
        &TableSpec {
            title: Title::Chrome(ChromeText::fixed(strings::TITLE_INBOX)),
            columns,
            rows,
            selected: Some(cursor),
            bordered: true,
            top: previous.top,
            empty: strings::NO_MAIL,
            numbered: true,
            footer: Some(match info {
                Some(info) => format!("{info}  -  {counts}"),
                None => counts,
            }),
            sorted: screen.sort.mark(),
        },
    );
    screen.view.set(ListView {
        top: drawn.top,
        shown: drawn.shown,
    });
}

fn render_reader(
    frame: &mut Frame<'_>,
    area: Rect,
    screen: &MailScreen,
    reader: &super::flow::Reader,
    stored: &russh_bbs_core::terminal_settings::Pager,
    stored_width: usize,
    ctx: &DrawCtx<'_>,
) {
    // The page answers the wheel (UI-9): it turns the reader's pages, like PageUp and PageDown.
    crate::ui::input::page(area);
    let width = pager::text_width(area);
    let rebuilt;
    let pager_ref = if width == stored_width {
        stored
    } else {
        rebuilt = screen.pager_for(reader, width);
        &rebuilt
    };
    let view = &reader.view;
    let hint = |key: &'static str, label: &'static str| {
        Hint::new(ChromeText::fixed(key), ChromeText::fixed(label))
    };
    let mut hints = vec![hint("Space", "more"), hint("b", "back")];
    if view.can_reply {
        hints.push(hint("r", "reply"));
        hints.push(hint("c", "quote"));
    }
    hints.push(hint("d", "delete"));
    if view.can_report {
        hints.push(hint("x", "report"));
    }
    hints.push(hint("Esc", "back"));
    let from = view.from.as_deref().unwrap_or(strings::GONE_MEMBER);
    let at = format_utc(view.at);
    let body = reader_ui::render(
        frame,
        &reader_ui::ReaderView {
            place: ChromeText::fixed(strings::TITLE_INBOX),
            head: reader_ui::Head {
                subject: &view.subject,
                from,
                at: &at,
                context: strings::TITLE_INBOX,
            },
            strip: reader_ui::Strip::Label(ChromeText::fixed(strings::TITLE_INBOX)),
            pager: pager_ref,
            hints: &hints,
        },
        &reader_ui::ReaderCtx {
            strings: ctx.strings,
            look: ctx.look,
            tier: ctx.tier,
            markers: ctx.markers,
            clock: ctx.clock,
        },
    );
    // The confirmation is a modal over the message.
    if let Some(text) = confirm_text(reader) {
        let keys = [
            Hint::new(ChromeText::fixed("y"), ChromeText::fixed("yes")),
            Hint::new(
                ChromeText::fixed("any other key"),
                ChromeText::fixed("cancel"),
            ),
        ];
        popup::render(
            frame,
            body,
            ctx.look,
            &Popup {
                kind: PopupKind::Confirm,
                title: ChromeText::fixed("Confirm"),
                lines: vec![
                    PopupLine::content(&clean(&reader_title(reader), 120)),
                    PopupLine::plain(text),
                ],
                keys: &keys,
            },
        );
    }
}

fn notice(screen: &MailScreen, ctx: &DrawCtx<'_>, out: &mut Vec<Line<'static>>) {
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

fn lines(screen: &MailScreen, ctx: &DrawCtx<'_>) -> Vec<Line<'static>> {
    let (_, dim, heading, _) = classic::styles(ctx.look);
    let mut out = Vec::new();
    match &screen.state {
        State::Start | State::Reader(_) | State::Editor(_) => {
            out.push(Line::styled(strings::TITLE_INBOX.to_owned(), heading));
            out.push(Line::raw(""));
            notice(screen, ctx, &mut out);
        }
        State::Inbox { .. } => {}
        State::Input(input) => input_lines(screen, input, heading, dim, ctx, &mut out),
    }
    out
}

fn input_lines(
    screen: &MailScreen,
    input: &Input,
    heading: Style,
    dim: Style,
    ctx: &DrawCtx<'_>,
    out: &mut Vec<Line<'static>>,
) {
    let (title, prompt) = match &input.kind {
        InputKind::To => (strings::TITLE_TO, strings::PROMPT_TO),
        InputKind::Subject { .. } => (strings::TITLE_SUBJECT, strings::PROMPT_SUBJECT),
    };
    out.push(Line::styled(title.to_owned(), heading));
    out.push(Line::raw(""));
    out.push(Line::raw(prompt.to_owned()));
    out.push(Line::raw(format!(
        "> {}{}",
        clean(&input.text, 200),
        ctx.look.cursor()
    )));
    out.push(Line::raw(""));
    notice(screen, ctx, out);
    out.push(Line::styled(strings::HELP_INPUT.to_owned(), dim));
}
