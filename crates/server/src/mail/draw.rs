//! Drawing the mail screens. Every line is fixed chrome text or member text that core opened and
//! sanitized and that is cleaned again here; it goes into the content box only, so it cannot
//! reach the reserved status row (D39, D45), and no style is applied to it except the plain
//! ones below. There is no privacy notice on any of these screens (D78).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::mail::InboxPage;
use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::terminal_settings::{TerminalSettings, format_utc};

use super::flow::{Input, InputKind, MailScreen, State, confirm_text, reader_title};
use super::strings;
use crate::settings::{self, Look};
use crate::ui::classic::{self, ContentScreen, MENU_MARKERS};
use crate::ui::editor::EditorView;
use crate::ui::frame::{ChromeText, Hint};
use crate::ui::popup::{self, Popup, PopupKind, PopupLine};
use crate::ui::screens::reader as reader_ui;
use crate::ui::strings::StringTable;
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
}

/// Member text, cleaned once more on its way to the terminal.
fn clean(text: &str, max: usize) -> String {
    SingleLine::new(max).sanitize(text).into_string()
}

/// Cut `text` to at most `width` terminal columns so a row never wraps.
fn fit(text: &str, width: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        out.push(c);
        if Span::raw(out.as_str()).width() > width {
            out.pop();
            break;
        }
    }
    out
}

fn row(
    selected: bool,
    number: usize,
    unread: bool,
    text: &str,
    width: usize,
    bold: Style,
) -> Line<'static> {
    let lead = if selected { "> " } else { "  " };
    let n = if number < 9 {
        format!("{}. ", number + 1)
    } else {
        "   ".to_owned()
    };
    // A plain asterisk marks an unread message. It is ordinary text, never a security marker.
    let mark = if unread { "* " } else { "  " };
    let text = fit(
        text,
        width.saturating_sub(lead.len() + n.len() + mark.len()),
    );
    let style = if selected {
        bold.add_modifier(Modifier::UNDERLINED)
    } else {
        Style::new()
    };
    Line::styled(format!("{lead}{n}{mark}{text}"), style)
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
    let width = usize::from(area.width.min(settings::MAX_WIDTH)).saturating_sub(2);
    classic::render_content_bare(
        frame,
        ContentScreen {
            strings: ctx.strings,
            lines: lines(screen, width, ctx),
            look: ctx.look,
            tier: ctx.tier,
            markers: &MENU_MARKERS,
            clock: None,
        },
    );
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

fn lines(screen: &MailScreen, width: usize, ctx: &DrawCtx<'_>) -> Vec<Line<'static>> {
    let (bold, dim, heading, _) = classic::styles(ctx.look);
    let mut out = Vec::new();
    match &screen.state {
        State::Start | State::Reader(_) | State::Editor(_) => {
            out.push(Line::styled(strings::TITLE_INBOX.to_owned(), heading));
            out.push(Line::raw(""));
            notice(screen, ctx, &mut out);
        }
        State::Inbox {
            page,
            cursor,
            confirm,
        } => {
            inbox_lines(page, *cursor, width, bold, heading, dim, &mut out);
            if confirm.is_some() {
                out.push(Line::raw(""));
                out.push(Line::raw(strings::CONFIRM_DELETE.to_owned()));
            }
            notice(screen, ctx, &mut out);
            out.push(Line::raw(""));
            out.push(Line::styled(strings::HELP_INBOX.to_owned(), dim));
        }
        State::Input(input) => input_lines(screen, input, heading, dim, ctx, &mut out),
    }
    out
}

fn inbox_lines(
    page: &InboxPage,
    cursor: usize,
    width: usize,
    bold: Style,
    heading: Style,
    dim: Style,
    out: &mut Vec<Line<'static>>,
) {
    out.push(Line::styled(strings::TITLE_INBOX.to_owned(), heading));
    out.push(Line::styled(
        format!(
            "{} {}, {} unread",
            page.total,
            if page.total == 1 {
                "message"
            } else {
                "messages"
            },
            page.unread
        ),
        dim,
    ));
    out.push(Line::raw(""));
    if page.entries.is_empty() {
        out.push(Line::raw(strings::NO_MAIL.to_owned()));
    }
    for (at, entry) in page.entries.iter().enumerate() {
        let from = entry.from.as_deref().unwrap_or(strings::GONE_MEMBER);
        let subject = entry
            .subject
            .as_deref()
            .map_or_else(|| strings::CANNOT_OPEN.to_owned(), |s| clean(s, 120));
        let text = format!(
            "{} - {}  [{}]",
            clean(from, 64),
            subject,
            format_utc(entry.at)
        );
        out.push(row(at == cursor, at, entry.unread, &text, width, bold));
    }
    out.push(Line::raw(""));
    out.push(Line::styled(format!("Page {}", page.page + 1), dim));
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
