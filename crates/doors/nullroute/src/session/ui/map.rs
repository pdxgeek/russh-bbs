//! The node map: the corporate network as a graph, drawn into a buffer.
//!
//! Layers run left to right (the entry on the left, the objective floor on the right). Only what
//! the run knows is drawn: a node whose type is known, and a link that is visible or was found.
//! Where a node sits does not depend on what is known, so the map does not jump as it fills in.
//!
//! Edges are braille lines (two columns by four rows of dots a cell). The edges next to the node
//! you stand on are cyan, and a packet of green dots runs along them, one step each time the log
//! moves. In the ASCII tier the same edges are `-`, `/` and `\`.

use russh_bbs_door::buffer::Buffer;
use russh_bbs_door::layout::Rect;
use russh_bbs_door::style::{Color, Modifier, Style};
use russh_bbs_door::widgets::Widget;

use super::{AMBER, CYAN, GLOW, GREEN, RED, STEEL, VIOLET};
use crate::content::Content;
use crate::net::{NodeId, Patrol};
use crate::run::Run;

/// The map widget.
pub(super) struct MapView<'a> {
    pub run: &'a Run,
    pub content: &'a Content,
    pub ascii: bool,
    /// Moves along the packets on the hot edges.
    pub pulse: u32,
    /// Draw short tokens (`glyph code digit`) rather than names.
    pub compact: bool,
}

/// What a node looks like.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Here,
    Honeypot,
    Wall,
    Ice,
    Objective,
    Locked,
    Open,
}

impl Look {
    const fn glyph(self, ascii: bool) -> char {
        match (self, ascii) {
            (Self::Here, false) => '\u{25C9}',
            (Self::Here, true) => '@',
            (Self::Honeypot, false) => '\u{00D7}',
            (Self::Honeypot, true) => '!',
            (Self::Wall, false) => '\u{25A3}',
            (Self::Wall, true) => 'X',
            (Self::Ice, false) => '\u{25B2}',
            (Self::Ice, true) => '^',
            (Self::Objective, false) => '\u{25CE}',
            (Self::Objective, true) => '$',
            (Self::Locked, false) => '\u{25C6}',
            (Self::Locked, true) => '#',
            (Self::Open, false) => '\u{25CB}',
            (Self::Open, true) => 'o',
        }
    }

    fn style(self) -> Style {
        match self {
            Self::Here => Style::new().fg(CYAN).bg(GLOW).add_modifier(Modifier::BOLD),
            Self::Honeypot | Self::Wall | Self::Ice => Style::new().fg(RED),
            Self::Objective => Style::new().fg(AMBER).add_modifier(Modifier::BOLD),
            Self::Locked => Style::new().fg(AMBER),
            Self::Open => Style::new().fg(GREEN),
        }
    }
}

/// How a node looks to this run.
fn look_of(run: &Run, content: &Content, id: NodeId, target: &str) -> Look {
    if id == run.at {
        return Look::Here;
    }
    let Some(v) = run.view(content, id) else {
        return Look::Open;
    };
    if v.honeypot {
        Look::Honeypot
    } else if v.patrol == Patrol::BlackWall && !v.wall_breached {
        Look::Wall
    } else if matches!(v.patrol, Patrol::Sentry | Patrol::Tracer) && !v.patrol_off {
        Look::Ice
    } else if v.code == target {
        Look::Objective
    } else if v.open {
        Look::Open
    } else {
        Look::Locked
    }
}

/// Braille dot bits for the sub-pixel at column `dx` (0 or 1), row `dy` (0 to 3).
const fn dot(dx: u16, dy: u16) -> u8 {
    match (dx, dy) {
        (0, 0) => 0x01,
        (0, 1) => 0x02,
        (0, 2) => 0x04,
        (0, 3) => 0x40,
        (1, 0) => 0x08,
        (1, 1) => 0x10,
        (1, 2) => 0x20,
        _ => 0x80,
    }
}

/// Points of a line between two integer points (Bresenham).
fn line_points(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
    let (mut x, mut y) = a;
    let dx = (b.0 - a.0).abs();
    let dy = -(b.1 - a.1).abs();
    let sx = if a.0 < b.0 { 1 } else { -1 };
    let sy = if a.1 < b.1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut out = Vec::new();
    loop {
        out.push((x, y));
        if (x, y) == b {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
        if out.len() > 4096 {
            break;
        }
    }
    out
}

/// How hot an edge is: 0 faint, 1 known, 2 next to you, 3 a packet.
type Heat = u8;

struct Edge {
    from: (u16, u16),
    to: (u16, u16),
    hot: bool,
}

impl Widget for MapView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 12 || area.height < 3 {
            return;
        }
        let run = self.run;
        let net = run.net();
        let layers = net
            .nodes
            .iter()
            .map(|n| usize::from(n.layer))
            .max()
            .unwrap_or(0)
            + 1;
        let mut by_layer: Vec<Vec<NodeId>> = vec![Vec::new(); layers];
        for n in &net.nodes {
            by_layer[usize::from(n.layer)].push(n.id);
        }
        let token_w: u16 = if self.compact { 4 } else { 7 };
        let usable = area.width.saturating_sub(token_w);
        let body_h = area.height.saturating_sub(1).max(1);
        let layer_x = |k: usize| -> u16 {
            if layers <= 1 {
                area.x + usable / 2
            } else {
                area.x + u16::try_from(usize::from(usable) * k / (layers - 1)).unwrap_or(0)
            }
        };
        let mut pos: Vec<(u16, u16)> = vec![(area.x, area.y); net.nodes.len()];
        for (k, ids) in by_layer.iter().enumerate() {
            let n = ids.len().max(1);
            for (i, id) in ids.iter().enumerate() {
                let y = area.y
                    + u16::try_from((2 * i + 1) * usize::from(body_h) / (2 * n))
                        .unwrap_or(0)
                        .min(body_h - 1);
                pos[usize::from(*id)] = (layer_x(k), y);
            }
        }

        // Edges between nodes the run knows, over a known link.
        let exits = run.exits();
        let mut edges = Vec::new();
        for l in &net.links {
            if !(run.is_known(l.a) && run.is_known(l.b) && run.link_known(l.a, l.b)) {
                continue;
            }
            let (a, b) = if net.nodes[usize::from(l.a)].layer <= net.nodes[usize::from(l.b)].layer {
                (l.a, l.b)
            } else {
                (l.b, l.a)
            };
            let (pa, pb) = (pos[usize::from(a)], pos[usize::from(b)]);
            edges.push(Edge {
                from: (pa.0 + token_w, pa.1),
                to: (pb.0.saturating_sub(1), pb.1),
                hot: a == run.at || b == run.at,
            });
        }

        if !self.ascii {
            // A faint grid under everything, so unknown ground reads as unmapped, not as empty.
            let grid = Style::new().fg(super::GRID);
            for y in (area.y..area.y + area.height).step_by(2) {
                for x in (area.x..area.x + area.width).step_by(6) {
                    if let Some(cell) = buf.cell_mut((x + (y - area.y) % 4 / 2 * 3, y)) {
                        cell.set_char('\u{2802}');
                        cell.set_style(grid);
                    }
                }
            }
        }
        if self.ascii {
            self.draw_ascii_edges(&edges, buf);
        } else {
            self.draw_braille_edges(&edges, area, buf);
        }

        // Nodes over the edges.
        let target = run.objective_target().0.to_owned();
        for node in &net.nodes {
            if !run.is_known(node.id) {
                continue;
            }
            let look = look_of(run, self.content, node.id, &target);
            let glyph = look.glyph(self.ascii);
            let exit_no = exits.iter().position(|e| *e == node.id).map(|i| i + 1);
            let code = self
                .content
                .node(node.kind)
                .map_or("??", |d| d.code.as_str());
            let (x, y) = pos[usize::from(node.id)];
            let number = exit_no.map_or(String::new(), |n| n.to_string());
            let (text, num_len) = if self.compact {
                (
                    format!(
                        "{glyph}{code}{}",
                        if number.is_empty() { " " } else { &number }
                    ),
                    0,
                )
            } else {
                (
                    format!("{number}{glyph}{}", node.name),
                    number.chars().count(),
                )
            };
            let style = look.style();
            let num_len = u16::try_from(num_len).unwrap_or(0);
            if self.compact {
                buf.set_stringn(x, y, &text, usize::from(token_w), style);
                if exit_no.is_some() {
                    buf.set_stringn(
                        x + 3,
                        y,
                        &number,
                        1,
                        Style::new().fg(AMBER).add_modifier(Modifier::BOLD),
                    );
                }
            } else {
                buf.set_stringn(x, y, &text, usize::from(token_w), style);
                if num_len > 0 {
                    buf.set_stringn(
                        x,
                        y,
                        &number,
                        usize::from(num_len),
                        Style::new().fg(AMBER).add_modifier(Modifier::BOLD),
                    );
                }
            }
            // A scanned patrol is named under the node when there is room.
            if !self.compact
                && y + 1 < area.y + body_h
                && let Some(v) = run.view(self.content, node.id)
                && let Some(cm) = v.cm.as_deref()
                && !v.patrol_off
            {
                buf.set_stringn(
                    x + num_len,
                    y + 1,
                    cm,
                    usize::from(token_w + 2),
                    Style::new().fg(VIOLET),
                );
            }
        }

        // The depth ruler on the bottom row.
        let ruler = Style::new().fg(STEEL);
        for k in 0..layers {
            let label = format!("d{k}");
            buf.set_stringn(layer_x(k), area.y + area.height - 1, &label, 3, ruler);
        }
    }
}

impl MapView<'_> {
    /// Braille edges: sub-pixels two across and four down a cell. The colour of a cell is the
    /// hottest edge that touches it.
    fn draw_braille_edges(&self, edges: &[Edge], area: Rect, buf: &mut Buffer) {
        let (w, h) = (usize::from(area.width), usize::from(area.height));
        let mut bits = vec![0u8; w * h];
        let mut heat: Vec<Heat> = vec![0; w * h];
        for e in edges {
            let a = (
                i32::from(e.from.0 - area.x) * 2,
                i32::from(e.from.1 - area.y) * 4 + 1,
            );
            let b = (
                i32::from(e.to.0 - area.x) * 2 + 1,
                i32::from(e.to.1 - area.y) * 4 + 2,
            );
            if b.0 < a.0 {
                continue;
            }
            for (i, (px, py)) in line_points(a, b).into_iter().enumerate() {
                let (cx, cy) = (px / 2, py / 4);
                let Ok(idx) = usize::try_from(cy * i32::try_from(w).unwrap_or(0) + cx) else {
                    continue;
                };
                if cx < 0 || cy < 0 || idx >= bits.len() || usize::try_from(cx).unwrap_or(w) >= w {
                    continue;
                }
                bits[idx] |= dot(
                    u16::try_from(px % 2).unwrap_or(0),
                    u16::try_from(py % 4).unwrap_or(0),
                );
                let packet = e.hot && (i + self.pulse as usize) % 7 < 2;
                let level = if packet {
                    3
                } else if e.hot {
                    2
                } else {
                    1
                };
                heat[idx] = heat[idx].max(level);
            }
        }
        for y in 0..h {
            for x in 0..w {
                let b = bits[y * w + x];
                if b == 0 {
                    continue;
                }
                let color = match heat[y * w + x] {
                    3 => GREEN,
                    2 => CYAN,
                    _ => STEEL,
                };
                if let Some(cell) = buf.cell_mut((
                    area.x + u16::try_from(x).unwrap_or(0),
                    area.y + u16::try_from(y).unwrap_or(0),
                )) {
                    let ch = char::from_u32(0x2800 + u32::from(b)).unwrap_or(' ');
                    cell.set_char(ch);
                    cell.set_style(Style::new().fg(color));
                }
            }
        }
    }

    /// ASCII edges: a column step of the line is `-`, a row step is `/` or `\`.
    fn draw_ascii_edges(&self, edges: &[Edge], buf: &mut Buffer) {
        for e in edges {
            let (x0, y0) = (i32::from(e.from.0), i32::from(e.from.1));
            let (x1, y1) = (i32::from(e.to.0), i32::from(e.to.1));
            if x1 < x0 {
                continue;
            }
            let dx = (x1 - x0).max(1);
            let y_at = |x: i32| y0 + ((y1 - y0) * (x - x0) + dx / 2) / dx;
            for x in x0..=x1 {
                let (y, next) = (y_at(x), y_at(x + 1));
                let ch = if x == x1 || y == next {
                    '-'
                } else if next > y {
                    '\\'
                } else {
                    '/'
                };
                let packet =
                    e.hot && (usize::try_from(x - x0).unwrap_or(0) + self.pulse as usize) % 5 < 2;
                let color: Color = if packet {
                    GREEN
                } else if e.hot {
                    CYAN
                } else {
                    STEEL
                };
                if let (Ok(cx), Ok(cy)) = (u16::try_from(x), u16::try_from(y))
                    && let Some(cell) = buf.cell_mut((cx, cy))
                {
                    cell.set_char(if packet && ch == '-' { '=' } else { ch });
                    cell.set_style(Style::new().fg(color));
                }
            }
        }
    }
}
