//! Event scenes: a framed vignette drawn in braille for the kind of event, the story, and the
//! choices. A choice the crew or standing closes stays on the list and says why.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "vignette geometry"
)]

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Wrap};

use super::kit::{Pal, Raster, key_lines, mix, panel, trunc};
use crate::scene::Scene;
use crate::session::Session;

/// Draw a picture for an event family into a raster: stars, then the thing itself.
fn vignette(family: &str, seed: u64, r: &mut Raster) {
    let (w, h) = (r.dots_w(), r.dots_h());
    for k in 0..(w * h / 40).max(8) {
        let x = (mix(seed, k as u64) % w.max(1) as u64) as i32;
        let y = (mix(seed ^ 0x55, k as u64) % h.max(1) as u64) as i32;
        r.plot(x, y, 1);
    }
    let (cx, cy) = (w / 2, h / 2);
    let ship = |r: &mut Raster, x: i32, y: i32, s: i32, layer: u8| {
        let s = s * 2;
        // A little hull: a wedge with a tail flame.
        r.line((x - s, y), (x + s, y - s / 2), layer, None);
        r.line((x - s, y), (x + s, y + s / 2), layer, None);
        r.line((x + s, y - s / 2), (x + s, y + s / 2), layer, None);
        r.line((x - s - s / 2, y), (x - s, y), 3, Some((1, 1)));
    };
    match family {
        "distress" => {
            ship(r, cx, cy, h / 5, 2);
            for k in 1..=3 {
                r.circle((cx, cy), h / 4 + k * 4, 4, Some((2, 3)));
            }
        }
        "pirate" => {
            ship(r, cx - w / 4, cy, h / 7, 2);
            ship(r, cx + w / 4, cy - h / 6, h / 6, 4);
            r.line((cx - w / 4, cy), (cx + w / 4, cy - h / 6), 4, Some((1, 4)));
        }
        "inspection" => {
            ship(r, cx + w / 5, cy, h / 4, 2);
            ship(r, cx - w / 4, cy + h / 8, h / 8, 3);
            r.line(
                (cx + w / 5 - h / 4, cy),
                (cx - w / 4, cy + h / 8),
                3,
                Some((2, 2)),
            );
        }
        "drive" => {
            ship(r, cx, cy, h / 5, 2);
            for k in 0..6 {
                r.line(
                    (cx - w / 3 - k * 3, cy - 2 + k),
                    (cx - h / 4, cy),
                    3,
                    Some((1, 2)),
                );
            }
        }
        "signal" => {
            let mut prev = (0, cy);
            for x in 0..w {
                let y = cy
                    + ((f64::from(x) / 5.0).sin()
                        * f64::from(h)
                        * 0.28
                        * (1.0 - f64::from((x - cx).abs()) / f64::from(w)))
                        as i32;
                r.line(prev, (x, y), 3, None);
                prev = (x, y);
            }
        }
        "hazard" => {
            for k in 0..(w * h / 6) {
                let x = (mix(seed ^ 9, k as u64) % w.max(1) as u64) as i32;
                let y = (mix(seed ^ 7, k as u64) % h.max(1) as u64) as i32;
                if (x - cx).pow(2) / 3 + (y - cy).pow(2) < (h / 2).pow(2) {
                    r.plot(x, y, 3);
                }
            }
        }
        "derelict" => {
            let s = h / 4;
            r.line((cx - 2 * s, cy - s), (cx + s, cy - s / 2), 2, None);
            r.line((cx - 2 * s, cy + s), (cx + s, cy + s / 2), 2, None);
            r.line((cx - 2 * s, cy - s), (cx - 2 * s, cy + s), 2, Some((2, 2)));
            r.line((cx + s, cy - s / 2), (cx + 2 * s, cy + s), 2, Some((2, 3)));
        }
        "omen" => {
            r.circle((cx, cy), h / 2 - 1, 4, None);
            r.circle((cx, cy), h / 3, 3, Some((2, 2)));
            for k in 0..8 {
                let a = f64::from(k) * std::f64::consts::TAU / 8.0;
                r.line(
                    (cx, cy),
                    (
                        cx + (a.cos() * f64::from(h) / 2.0) as i32,
                        cy + (a.sin() * f64::from(h) / 2.0) as i32,
                    ),
                    3,
                    Some((1, 3)),
                );
            }
        }
        _ => {
            ship(r, cx - w / 5, cy, h / 6, 2);
            ship(r, cx + w / 5, cy, h / 6, 4);
            r.line(
                (cx - w / 5 + h / 6, cy),
                (cx + w / 5 - h / 6, cy),
                3,
                Some((1, 2)),
            );
        }
    }
}

impl Session {
    pub(super) fn event_keys(&self, pal: &Pal, width: u16) -> Vec<Line<'static>> {
        key_lines(
            pal,
            &[("1-9", " choose"), ("q", " leave door (scene is kept)")],
            width,
            1,
        )
    }

    pub(super) fn draw_event(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let Some(p) = &self.player else { return };
        let Some(Scene::Event(ix)) = &p.scene else {
            return;
        };
        let def = &self.content.transit.event[usize::from(*ix)];
        let menu = self.scene_menu(p);
        let art_w = if area.width >= 64 && area.height >= 16 {
            26
        } else {
            0
        };
        let block = panel(pal, &def.title, true);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let opts_h = (u16::try_from(menu.len()).unwrap_or(5) * 2)
            .min(inner.height.saturating_sub(5))
            .max(3);
        let [top, opts] =
            Layout::vertical([Constraint::Min(4), Constraint::Length(opts_h)]).areas(inner);
        let [art, story] =
            Layout::horizontal([Constraint::Length(art_w), Constraint::Min(20)]).areas(top);
        if art_w > 0 && art.height >= 3 {
            let mut r = Raster::new(art.width.saturating_sub(1), art.height);
            vignette(&def.family, mix(u64::from(*ix), 17), &mut r);
            let styles = [Default::default(), pal.deep, pal.hull, pal.amber, pal.star];
            if pal.ascii {
                frame.render_widget(
                    Paragraph::new(format!("[ {} ]", def.family)).style(pal.dim),
                    art,
                );
            } else {
                frame.render_widget(r.widget(&styles), art);
            }
        }
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(def.family.to_uppercase(), pal.dim),
                Line::styled(def.text.clone(), pal.plain),
            ])
            .wrap(Wrap { trim: true }),
            story,
        );
        let w = usize::from(opts.width);
        let mut lines: Vec<Line<'static>> = Vec::new();
        for (i, (label, _, closed)) in menu.iter().enumerate() {
            match closed {
                None => lines.push(Line::from(vec![
                    Span::styled(format!("[{}] ", i + 1), pal.amber),
                    Span::styled(trunc(label, w.saturating_sub(5)), pal.strong),
                ])),
                Some(why) => {
                    lines.push(Line::from(vec![
                        Span::styled(format!("[{}] ", i + 1), pal.dim),
                        Span::styled(trunc(label, w.saturating_sub(5)), pal.dim),
                    ]));
                    lines.push(Line::styled(
                        trunc(&format!("      closed: {why}"), w),
                        pal.bad,
                    ));
                }
            }
        }
        frame.render_widget(Paragraph::new(lines), opts);
    }
}
