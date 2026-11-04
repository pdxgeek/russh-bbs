//! The first screen: the title over a picture of the marsh, and the box where a clan is named.

use russh_bbs_door::layout::{Alignment, Constraint, Layout, Rect};
use russh_bbs_door::style::Modifier;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::Paragraph;
use russh_bbs_door::{DoorFrame, DoorStyle};

use super::art::{Ground, landscape};
use super::{Look, Pal, centred, fit};
use crate::clan::NAME_MAX;
use crate::lore::{Daypart, Water};

/// The title, the picture and the naming box.
pub(super) fn draw(
    f: &mut DoorFrame<'_>,
    area: Rect,
    look: Look,
    pal: &Pal,
    buffer: &str,
    notes: &[(String, DoorStyle)],
) {
    let pic_h = (area.height * 5 / 12).clamp(6, 16);
    let [pic, text] = Layout::vertical([Constraint::Length(pic_h), Constraint::Min(8)]).areas(area);
    f.render_widget(
        landscape(
            look,
            pal,
            (pic.width, pic.height),
            Ground::Hold,
            Daypart::Dusk,
            Water::Mid,
            0x5A17,
        ),
        pic,
    );
    let title = if look.ascii {
        "S A L T M A R C H"
    } else {
        "S  A  L  T  M  A  R  C  H"
    };
    let mut lines = vec![
        Line::from(Span::styled(title, pal.salt().add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(
            "The sea took the old coast. The marsh is what it left.",
            pal.sea(),
        )),
        Line::from(Span::styled(
            "Stake a hold on the high ground. The water is the clock.",
            pal.dim,
        )),
        Line::raw(""),
        Line::from(Span::styled("Name your clan", pal.accent)),
    ];
    let shown = fit(buffer, usize::from(area.width).saturating_sub(12));
    lines.push(Line::from(vec![
        Span::styled(format!("{} ", look.g("▸", ">")), pal.gold()),
        Span::styled(format!("Clan name: {shown}"), pal.strong),
        Span::styled(look.g("▏", "_"), pal.gold()),
    ]));
    lines.push(Line::from(Span::styled(
        format!(
            "{}/{NAME_MAX} letters   Enter stakes the hold   Esc leaves",
            buffer.chars().count()
        ),
        pal.dim,
    )));
    for (text, style) in notes.iter().take(2) {
        lines.push(Line::styled(text.clone(), pal.role(*style)));
    }
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), text);
}

/// The save or the world could not be read.
pub(super) fn unreadable(f: &mut DoorFrame<'_>, area: Rect, look: Look, pal: &Pal) {
    let r = centred(area, 64, 8);
    let lines = vec![
        Line::from(Span::styled(
            format!("{} Saltmarch cannot read your clan", look.g("▲", "!")),
            pal.rust(),
        )),
        Line::raw(""),
        Line::styled(
            "The saved clan or the season's world is not in a form this version understands.",
            pal.plain,
        ),
        Line::styled("Nothing was changed. Ask the sysop. Esc leaves.", pal.dim),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .wrap(russh_bbs_door::widgets::Wrap { trim: true })
            .block(russh_bbs_door::widgets::Block::bordered().border_style(pal.rust())),
        r,
    );
}
