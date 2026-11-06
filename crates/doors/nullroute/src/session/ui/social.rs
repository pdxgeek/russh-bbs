//! The social screens: rivals, crew, arcs, the street word. Each is a bordered panel of the
//! look-around text with the key hints under it, coloured by kind, so the screens a screen reader
//! gets and the ones a sighted player gets say the same words.

use russh_bbs_door::DoorStyle;
use russh_bbs_door::layout::{Constraint, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::Line;

use super::{AMBER, CYAN, GREEN, Pen, RED, STEEL, bold, clip, fg, rows};
use crate::session::{Session, View};

fn tone(view: &View) -> russh_bbs_door::style::Color {
    match view {
        View::Rivals => RED,
        View::Crew => GREEN,
        View::Arcs | View::Scene(_) => AMBER,
        _ => CYAN,
    }
}

fn title(view: &View) -> &'static str {
    match view {
        View::Rivals => "RIVALS",
        View::Crew => "CREW",
        View::Arcs => "ARCS",
        View::Scene(_) => "A CHOICE",
        View::Street(_) => "THE STREET",
        _ => "TYPE",
    }
}

/// Draw a social screen.
pub(super) fn draw(pen: &mut Pen<'_, '_>, s: &Session, area: Rect) {
    let lines = s.lines(usize::from(area.height).saturating_sub(2));
    let width = usize::from(area.width).saturating_sub(2);
    let [main, _] = rows(area, [Constraint::Min(4), Constraint::Length(0)]);
    let inner = pen.panel(main, title(&s.view), tone(&s.view));
    let height = usize::from(inner.height);
    // The look-around ends with the key hints; keep them in view and cut the body above them.
    let out: Vec<Line<'static>> = lines
        .iter()
        .take(height.max(1))
        .enumerate()
        .map(|(i, (text, style))| {
            let hint = i + 1 >= lines.len().min(height);
            let st: Style = match style {
                DoorStyle::Good => bold(GREEN),
                DoorStyle::Bad => bold(RED),
                DoorStyle::Accent => bold(CYAN),
                DoorStyle::Strong if hint => fg(STEEL),
                DoorStyle::Strong => pen.ink.strong,
                DoorStyle::Dim => pen.ink.dim,
                _ => pen.ink.plain,
            };
            Line::styled(clip(text, width), st)
        })
        .collect();
    pen.text(inner, out);
}
