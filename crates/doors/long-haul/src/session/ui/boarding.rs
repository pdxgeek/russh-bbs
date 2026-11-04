//! The boarding map: the derelict's rooms as a graph laid out by distance from the airlock,
//! with the party, hostiles, sealed doors, loot and caches marked, beside the party's health
//! and what has been taken.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "room grid coordinates are tiny"
)]

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::Paragraph;

use super::kit::{Pal, Strings, key_lines, pad, panel, trunc};
use crate::boarding::Boarding;
use crate::content::Content;
use crate::scene::Scene;
use crate::session::Session;

/// A room's place in the layout: column by distance from the airlock, row within the column.
fn grid(content: &Content, b: &Boarding) -> Vec<(u8, u8)> {
    let def = b.def(content);
    let n = def.rooms.len();
    let mut depth = vec![u8::MAX; n];
    depth[0] = 0;
    let mut queue = std::collections::VecDeque::from([0usize]);
    while let Some(r) = queue.pop_front() {
        for &e in &def.rooms[r].exits {
            let e = usize::from(e);
            if e < n && depth[e] == u8::MAX {
                depth[e] = depth[r] + 1;
                queue.push_back(e);
            }
        }
    }
    let mut next_row =
        vec![0u8; usize::from(*depth.iter().filter(|&&d| d != u8::MAX).max().unwrap_or(&0)) + 2];
    depth
        .iter()
        .map(|&d| {
            let col = if d == u8::MAX { 0 } else { d };
            let row = next_row[usize::from(col)];
            next_row[usize::from(col)] += 1;
            (col, row)
        })
        .collect()
}

impl Session {
    pub(super) fn boarding_keys(&self, pal: &Pal, width: u16) -> Vec<Line<'static>> {
        key_lines(pal, &[("1-9", " order"), ("q", " leave door")], width, 1)
    }

    #[allow(clippy::too_many_lines, reason = "one panel: map, party, room")]
    pub(super) fn draw_boarding(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let Some(p) = &self.player else { return };
        let Some(Scene::Boarding(b)) = &p.scene else {
            return;
        };
        let content = &self.content;
        let def = b.def(content);
        let menu = self.scene_menu(p);
        let menu_h = (u16::try_from(menu.len()).unwrap_or(6).min(7)) + 2;
        let [top, bottom] = Layout::vertical([
            Constraint::Min(6),
            Constraint::Length(menu_h.min(area.height.saturating_sub(7)).max(3)),
        ])
        .areas(area);
        let [map_area, party_area] =
            Layout::horizontal([Constraint::Min(30), Constraint::Length(26)]).areas(top);

        let block = panel(pal, &format!("BOARDING - {}", def.name), true);
        let inner = block.inner(map_area);
        frame.render_widget(block, map_area);
        let places = grid(content, b);
        let cols = places
            .iter()
            .map(|&(c, _)| usize::from(c))
            .max()
            .unwrap_or(0)
            + 1;
        let rows_n = places
            .iter()
            .map(|&(_, r)| usize::from(r))
            .max()
            .unwrap_or(0)
            + 1;
        let col_w = (usize::from(inner.width) / cols).max(8);
        let row_gap = if rows_n * 2 <= usize::from(inner.height) {
            2
        } else {
            1
        };
        let label_w = col_w.saturating_sub(3);
        let pos = |ix: usize| -> (u16, u16) {
            let (c, r) = places[ix];
            (
                (usize::from(c) * col_w) as u16,
                (usize::from(r) * row_gap
                    + usize::from(inner.height.saturating_sub((rows_n * row_gap) as u16)) / 2)
                    as u16,
            )
        };
        let mut strings: Vec<(u16, u16, String, Style)> = Vec::new();
        let (h, v) = if pal.ascii {
            ('-', '|')
        } else {
            ('─', '│')
        };
        // Links first, so labels draw over them.
        for (a, room) in def.rooms.iter().enumerate() {
            for &e in &room.exits {
                let e = usize::from(e);
                if e <= a || e >= def.rooms.len() || places[a].0 == places[e].0 {
                    continue;
                }
                let (ax, ay) = pos(a);
                let (bx, by) = pos(e);
                let (x0, x1) = (ax + label_w as u16, bx);
                let mid = x0 + (x1.saturating_sub(x0)) / 2;
                let open = def.rooms[e].lock == 0 || b.opened & (1u16 << u32::from(e as u8)) != 0;
                let st = if open { pal.hull } else { pal.bad };
                for x in x0..mid {
                    strings.push((x, ay, h.to_string(), st));
                }
                for x in mid..x1 {
                    strings.push((x, by, h.to_string(), st));
                }
                if ay != by {
                    let (lo, hi) = (ay.min(by), ay.max(by));
                    for y in lo..=hi {
                        strings.push((mid, y, v.to_string(), st));
                    }
                }
            }
        }
        for (ix, room) in def.rooms.iter().enumerate() {
            let (x, y) = pos(ix);
            let bit = 1u16 << u32::from(ix as u8);
            let here = usize::from(b.at) == ix;
            let mut status = String::new();
            let foes_alive = b.cleared & bit == 0 && room.foes > 0;
            if foes_alive {
                status.push('!');
            }
            if room.lock > 0 && b.opened & bit == 0 {
                status.push_str(&format!("L{}", room.lock));
            }
            if b.looted & bit == 0 && (room.credits[1] > 0 || room.tons > 0 || room.parts > 0) {
                status.push('$');
            }
            if room.cache > 0 && b.read & bit == 0 {
                status.push('#');
            }
            let name_w = label_w.saturating_sub(status.chars().count() + 2);
            let text = format!(
                "{}{} {}",
                if here {
                    if pal.ascii { '>' } else { '▶' }
                } else {
                    ' '
                },
                trunc(&room.name, name_w.max(3)),
                status
            );
            let style = if here {
                pal.sel
            } else if foes_alive {
                pal.bad
            } else if b.cleared & bit != 0 || ix == 0 {
                pal.plain
            } else {
                pal.dim
            };
            strings.push((x, y, pad(&text, label_w), style));
        }
        frame.render_widget(Strings(strings), inner);

        // The party and what it carries.
        let block = panel(pal, "PARTY", false);
        let inner = block.inner(party_area);
        frame.render_widget(block, party_area);
        let mut lines: Vec<Line<'static>> = Vec::new();
        for m in &b.party {
            let name = m
                .crew
                .and_then(|i| p.crew.get(usize::from(i)))
                .map_or("Captain", |c| c.name.split(' ').next().unwrap_or("?"));
            let pct = (i32::from(m.hp.max(0)) * 100 / i32::from(m.max.max(1))).max(0) as u32;
            lines.push(Line::from(vec![
                Span::styled(pad(name, 9), if m.up() { pal.plain } else { pal.bad }),
                Span::styled(
                    super::kit::pips(pct.div_ceil(20), 5, pal.ascii),
                    pal.level(pct),
                ),
                Span::styled(format!(" {}/{}", m.hp.max(0), m.max), pal.dim),
            ]));
        }
        lines.push(Line::styled(
            format!("taken: {} cr, {} parts", b.haul.credits, b.haul.parts),
            pal.dim,
        ));
        frame.render_widget(Paragraph::new(lines), inner);

        // The room, and the orders.
        let room = b.room(content);
        let block = panel(pal, &room.name, false);
        let inner = block.inner(bottom);
        frame.render_widget(block, bottom);
        let foes = b.foes_here(content);
        let mut lines: Vec<Line<'static>> = Vec::new();
        let w = usize::from(inner.width);
        let mut text = trunc(&room.text, w * 2);
        if foes > 0 {
            text = format!("{foes} {} here. {text}", def.foe_name);
        }
        lines.push(Line::styled(
            trunc(&text, w),
            if foes > 0 { pal.bad } else { pal.dim },
        ));
        for (i, (label, _, closed)) in menu.iter().enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("[{}] ", i + 1), pal.amber),
                Span::styled(
                    trunc(label, w.saturating_sub(5)),
                    if closed.is_some() {
                        pal.dim
                    } else {
                        pal.strong
                    },
                ),
            ]));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}
