//! The **lightbar** layout: the classic box with a highlighted row that Up and Down move and
//! Enter chooses (M3-A). It draws the same [`MenuPage`](russh_bbs_core::menu::MenuPage) as the
//! classic layout; hotkeys still work, so nothing is lost by choosing it.
//!
//! **The highlight is not the security style.** Markers use bold + reverse video, and nothing
//! else may (D39), so the cursor row is drawn with a pointer character, bold and underline (and
//! the theme's `selected` color when color is on). It never uses reverse video, and it is drawn
//! only inside the content box: the status row is taken out of the area first, exactly as in
//! every other layout.

use ratatui::Frame;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use russh_bbs_core::waiting_room::NoticeKind;

use super::chrome;
use super::classic::{self, MenuScreen};
use super::keys;
use crate::settings;

/// Draw the screen: content box above, status row below.
pub(crate) fn render(frame: &mut Frame<'_>, screen: &MenuScreen<'_>) {
    let (content, status) = chrome::split_status(frame.area(), screen.markers);
    let lines = if screen.page.help {
        classic::help_lines(screen, Some(keys::HELP_LIGHTBAR))
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
    let mut out = vec![
        Line::styled(strings.text(screen.page.title).to_owned(), heading),
        Line::raw(""),
    ];
    out.extend(screen.body.iter().map(|text| Line::raw(text.clone())));
    if !screen.body.is_empty() {
        out.push(Line::raw(""));
    }
    let selected = screen.look.accent(
        Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        screen.look.palette().selected,
    );
    let pointer = strings.text(keys::POINTER);
    let blank = " ".repeat(pointer.chars().count());
    for (index, item) in screen.page.items.iter().enumerate() {
        let label = strings.text(item.label);
        let tag = if item.placeholder {
            format!("  ({})", strings.text(keys::TAG_NOT_BUILT))
        } else {
            String::new()
        };
        if index == screen.page.cursor {
            out.push(Line::from(vec![Span::styled(
                format!("{pointer} [{}] {label}{tag}", item.hotkey),
                selected,
            )]));
        } else {
            let mut spans = vec![
                Span::raw(format!("{blank} ")),
                Span::styled(format!("[{}]", item.hotkey), key),
                Span::raw(format!(" {label}")),
            ];
            if item.placeholder {
                spans.push(Span::styled(tag, dim));
            }
            out.push(Line::from(spans));
        }
    }
    if let Some(note) = screen.page.notice {
        out.push(Line::raw(""));
        out.push(classic::notice_line(
            strings,
            NoticeKind::Info,
            strings.text(note),
            screen.look,
            bold,
        ));
    }
    if let Some(notice) = screen.notice {
        out.push(Line::raw(""));
        out.push(classic::notice_line(
            strings,
            notice.kind,
            notice.text,
            screen.look,
            bold,
        ));
    }
    out.push(Line::raw(""));
    out.push(Line::styled(
        strings.text(keys::FOOTER_LIGHTBAR).to_owned(),
        dim,
    ));
    out
}
