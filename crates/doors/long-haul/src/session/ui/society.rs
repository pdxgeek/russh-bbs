//! The society screen: the lines from `Session::society_lines` in a panel.
use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::Rect;
use russh_bbs_door::text::Line;
use russh_bbs_door::widgets::{Paragraph, Wrap};

use super::kit::{Pal, panel};
use crate::session::{Mode, Session};

impl Session {
    pub(super) fn draw_society(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let Mode::Society(tab, cursor, input) = &self.mode else {
            return;
        };
        let block = panel(pal, "SOCIETY: CONVOYS, PIRACY, OUTPOSTS", true);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let lines: Vec<Line<'static>> = self
            .society_lines(*tab, *cursor, input.as_deref(), usize::from(inner.height))
            .into_iter()
            .map(|(t, s)| Line::styled(t, pal.role(s)))
            .collect();
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }
}
