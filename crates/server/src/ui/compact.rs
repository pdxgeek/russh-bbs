//! The **compact** layout: one line per item, no box, no blank spacer lines, no icons (M3-A).
//! For narrow or short terminals and screen readers, and the fallback when a richer layout does
//! not fit. It draws the same menu page as every other layout and keeps the reserved status row.
//!
//! Everything is in words: `Notice:` / `Problem:` prefixes, and an item whose area does not
//! exist says so after its label. No color is needed to read it.

use ratatui::Frame;
use ratatui::text::{Line, Span};
use russh_bbs_core::waiting_room::NoticeKind;

use super::classic::{self, MenuScreen};
use super::keys;
use crate::settings;

/// Draw the screen: text above, the status bar below. The look is borderless (see
/// `Look::themed`), so there is no header bar and `boxed` draws only the paragraph.
pub(crate) fn render(frame: &mut Frame<'_>, screen: &MenuScreen<'_>) {
    let hints = classic::menu_hints(screen.strings, false);
    let body = classic::frame_spec(screen, &hints).draw(frame);
    let lines = if screen.page.help {
        help_lines(screen)
    } else {
        menu_lines(screen)
    };
    settings::boxed(
        frame,
        body,
        screen.strings.text(keys::TITLE),
        lines,
        screen.look,
    );
}

/// The compact help: one line each, no panes.
fn help_lines(screen: &MenuScreen<'_>) -> Vec<Line<'static>> {
    let (_, dim, heading, key) = classic::styles(screen.look);
    let strings = screen.strings;
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
    if screen.mail_over_ssh_only {
        out.push(Line::raw(
            strings.text(keys::NOTICE_MAIL_SSH_ONLY).to_owned(),
        ));
    }
    out.push(Line::raw(strings.text(keys::HELP_MENU).to_owned()));
    for item in &screen.page.items {
        out.push(classic::item_line(strings, item, key, dim));
    }
    out.push(Line::styled(
        strings.text(keys::HELP_FOOTER).to_owned(),
        dim,
    ));
    out
}

fn menu_lines(screen: &MenuScreen<'_>) -> Vec<Line<'static>> {
    let (bold, dim, heading, key) = classic::styles(screen.look);
    let strings = screen.strings;
    let mut out = vec![Line::styled(
        strings.text(screen.page.title).to_owned(),
        heading,
    )];
    out.extend(screen.body.iter().map(|text| Line::raw(text.clone())));
    for item in &screen.page.items {
        let mut spans = vec![
            Span::styled(item.hotkey.to_string(), key),
            Span::raw(format!(" {}", strings.text(item.label))),
        ];
        if item.placeholder {
            spans.push(Span::styled(
                format!(" ({})", strings.text(keys::TAG_NOT_BUILT)),
                dim,
            ));
        }
        out.push(Line::from(spans));
    }
    if let Some(note) = screen.page.notice {
        out.push(classic::notice_line(
            strings,
            NoticeKind::Info,
            strings.text(note),
            screen.look,
            bold,
        ));
    }
    if let Some(notice) = screen.notice {
        out.push(classic::notice_line(
            strings,
            notice.kind,
            notice.text,
            screen.look,
            bold,
        ));
    }
    out
}
