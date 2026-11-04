//! The **compact** layout: one line per item, no box, no blank spacer lines, no icons (M3-A).
//! For narrow or short terminals and screen readers, and the fallback when a richer layout does
//! not fit. It draws the same menu page as every other layout and keeps the reserved status row.
//!
//! Everything is in words: `Notice:` / `Problem:` prefixes, and an item whose area does not
//! exist says so after its label. No color is needed to read it.

use ratatui::Frame;
use ratatui::text::{Line, Span};
use russh_bbs_core::waiting_room::NoticeKind;

use super::chrome;
use super::classic::{self, MenuScreen};
use super::keys;
use crate::settings;

/// Draw the screen: text above, status row below. The look is borderless (see
/// `Look::themed`), so `boxed` draws only the paragraph.
pub(crate) fn render(frame: &mut Frame<'_>, screen: &MenuScreen<'_>) {
    let (content, status) = chrome::split_status(frame.area(), screen.markers);
    let lines = if screen.page.help {
        classic::help_lines(screen, None)
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
    out.push(Line::styled(
        strings.text(keys::FOOTER_MENU).to_owned(),
        dim,
    ));
    out
}
