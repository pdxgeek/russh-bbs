//! Before there is a captain: the choice of who you were, over a field of stars, and the
//! notice for a save this build cannot read.
use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Wrap};

use super::kit::{Pal, Raster, credits, mix, pad, panel};
use crate::session::{Mode, Session};

impl Session {
    pub(super) fn draw_intro(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        if matches!(self.mode, Mode::Unreadable) {
            let block = panel(pal, "LONG HAUL", true);
            let inner = block.inner(area);
            frame.render_widget(block, area);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(
                        "Your saved captain was written by a version this board cannot read.",
                        pal.bad,
                    ),
                    Line::styled(
                        "Nothing has been changed. Ask the sysop; press any key to leave.",
                        pal.plain,
                    ),
                ])
                .wrap(Wrap { trim: true }),
                inner,
            );
            return;
        }
        let Mode::Pick(cursor) = &self.mode else {
            return;
        };
        let [banner, body] =
            Layout::vertical([Constraint::Length(5), Constraint::Min(6)]).areas(area);
        // A strip of stars with the title across it.
        if pal.ascii {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled("  .   *    .      +    .   *     .", pal.dim),
                    Line::styled("LONG HAUL", pal.amber),
                    Line::styled(
                        "The gates failed long ago, and you owe for your ship.",
                        pal.plain,
                    ),
                ]),
                banner,
            );
        } else {
            let mut r = Raster::new(banner.width, banner.height);
            for k in 0..(banner.width as i32 * 6) {
                let x = (mix(7, k as u64) % (u64::from(banner.width) * 2)) as i32;
                let y = (mix(11, k as u64) % (u64::from(banner.height) * 4)) as i32;
                r.plot(x, y, if k % 5 == 0 { 2 } else { 1 });
            }
            // A freighter crossing, and a long lane behind it.
            let (cx, cy) = (
                i32::from(banner.width) * 2 - 30,
                i32::from(banner.height) * 2,
            );
            r.line((cx - 26, cy), (cx - 4, cy), 3, Some((1, 3)));
            r.line((cx - 4, cy - 4), (cx + 6, cy), 3, None);
            r.line((cx - 4, cy + 4), (cx + 6, cy), 3, None);
            r.line((cx - 4, cy - 4), (cx - 4, cy + 4), 3, None);
            frame.render_widget(
                r.widget(&[Default::default(), pal.deep, pal.star, pal.amber]),
                banner,
            );
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(" L O N G   H A U L", pal.amber),
                    Line::styled(
                        " The gates failed long ago, and you owe for your ship.",
                        pal.plain,
                    ),
                ]),
                Rect {
                    height: 2,
                    ..banner
                },
            );
        }
        let [list, detail] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Min(20)]).areas(body);
        let block = panel(pal, "WHO WERE YOU BEFORE THE LOAN?", true);
        let inner = block.inner(list);
        frame.render_widget(block, list);
        let lines: Vec<Line<'static>> = self
            .content
            .backgrounds
            .iter()
            .enumerate()
            .map(|(i, b)| {
                Line::styled(
                    format!(
                        "{} {}",
                        if i == *cursor { '>' } else { ' ' },
                        pad(&b.name, usize::from(inner.width).saturating_sub(3))
                    ),
                    if i == *cursor { pal.sel } else { pal.plain },
                )
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
        let block = panel(pal, "", false);
        let inner = block.inner(detail);
        frame.render_widget(block, detail);
        if let Some(b) = self.content.backgrounds.get(*cursor) {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(vec![
                        Span::styled("owes ", pal.dim),
                        Span::styled(format!("{} cr", credits(i64::from(b.loan))), pal.bad),
                        Span::styled("  holds ", pal.dim),
                        Span::styled(format!("{} cr", credits(i64::from(b.cash))), pal.good),
                    ]),
                    Line::styled(b.blurb.clone(), pal.dim),
                    Line::from(vec![
                        Span::styled("Perk: ", pal.amber),
                        Span::styled(b.perk_text.clone(), pal.good),
                    ]),
                    Line::styled("Up/Down choose - Enter sign - Esc leave", pal.dim),
                ])
                .wrap(Wrap { trim: true }),
                inner,
            );
        }
    }
}
