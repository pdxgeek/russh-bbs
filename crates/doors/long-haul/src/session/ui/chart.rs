//! The star chart: the frontier as a braille map with known lanes, fuel range rings, charted
//! and unknown systems, outposts (stations) and the ship; a cursor that walks the stars; and
//! the keys that go with it. In the ASCII tier the same map is drawn in plain characters with
//! a letter for each station type.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    reason = "map geometry: spans are at most 1000 and cells at most a few hundred"
)]

use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Wrap};
use russh_bbs_door::{Ctx, DoorFrame, DoorKey};

use super::kit::{Pal, Raster, Strings, credits, key_lines, mix, panel, trunc};
use crate::frontier::{MAP_H, MAP_W};
use crate::session::{Mode, Session};

/// How much of the frontier the chart shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::session) enum Zoom {
    /// The whole map.
    Full,
    /// A region around the cursor.
    Near,
    /// A few systems.
    Close,
}

impl Zoom {
    fn next(self) -> Self {
        match self {
            Self::Full => Self::Near,
            Self::Near => Self::Close,
            Self::Close => Self::Full,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Full => Self::Close,
            Self::Near => Self::Full,
            Self::Close => Self::Near,
        }
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Full => "frontier",
            Self::Near => "region",
            Self::Close => "close",
        }
    }
}

/// What colours the stations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::session) enum Overlay {
    /// By the faction that runs the station.
    Faction,
    /// By how fresh the captain's price intel is.
    Intel,
}

/// The chart's own state.
#[derive(Debug, Clone, Copy)]
pub(in crate::session) struct ChartState {
    /// The system under the cursor.
    pub cursor: u16,
    pub zoom: Zoom,
    pub overlay: Overlay,
}

impl ChartState {
    pub(in crate::session) const fn new(at: u16) -> Self {
        Self {
            cursor: at,
            zoom: Zoom::Near,
            overlay: Overlay::Faction,
        }
    }
}

/// How a system looks to this captain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::session) enum Vis {
    /// On the chart.
    Charted,
    /// An uncharted signal one lane from a charted system.
    Signal,
    /// Not shown.
    Hidden,
}

/// Which way the cursor is asked to go.
#[derive(Clone, Copy)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Session {
    pub(in crate::session) fn vis(&self, id: u16) -> Vis {
        if self.world.is_charted(&self.frontier, id) {
            return Vis::Charted;
        }
        let near_charted = self
            .frontier
            .neighbours(id)
            .iter()
            .any(|&(n, _)| self.world.is_charted(&self.frontier, n));
        if near_charted {
            Vis::Signal
        } else {
            Vis::Hidden
        }
    }

    /// Move the cursor to the nearest shown system in `dir`.
    fn chart_step(&mut self, dir: Dir) {
        let Some(here) = self.frontier.system(self.chart.cursor) else {
            return;
        };
        let (hx, hy) = (f64::from(here.x), f64::from(here.y));
        let (dx, dy) = match dir {
            Dir::Left => (-1.0, 0.0),
            Dir::Right => (1.0, 0.0),
            Dir::Up => (0.0, -1.0),
            Dir::Down => (0.0, 1.0),
        };
        let mut best: Option<(f64, u16)> = None;
        for relax in [0.6, 0.15] {
            for s in &self.frontier.systems {
                if s.id == self.chart.cursor || self.vis(s.id) == Vis::Hidden {
                    continue;
                }
                let (vx, vy) = (f64::from(s.x) - hx, f64::from(s.y) - hy);
                let len = vx.hypot(vy);
                let along = vx * dx + vy * dy;
                if len < 0.5 || along <= 0.0 || along / len < relax {
                    continue;
                }
                // Prefer near systems that lie straight ahead.
                let score = len * len / along;
                if best.is_none_or(|(b, _)| score < b) {
                    best = Some((score, s.id));
                }
            }
            if best.is_some() {
                break;
            }
        }
        if let Some((_, id)) = best {
            self.chart.cursor = id;
        }
    }

    pub(in crate::session) fn chart_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        match key {
            DoorKey::Up => self.chart_step(Dir::Up),
            DoorKey::Down => self.chart_step(Dir::Down),
            DoorKey::Left => self.chart_step(Dir::Left),
            DoorKey::Right => self.chart_step(Dir::Right),
            DoorKey::Escape | DoorKey::Char('q') => self.mode = Mode::Bridge,
            DoorKey::Char('+' | '=' | 'z') => self.chart.zoom = self.chart.zoom.next(),
            DoorKey::Char('-' | '_') => self.chart.zoom = self.chart.zoom.prev(),
            DoorKey::Char('o') => {
                self.chart.overlay = match self.chart.overlay {
                    Overlay::Faction => Overlay::Intel,
                    Overlay::Intel => Overlay::Faction,
                };
            }
            DoorKey::Char('.' | '0') => {
                if let Some(p) = &self.player {
                    self.chart.cursor = p.at;
                }
            }
            DoorKey::Enter | DoorKey::Char('j') => self.open_route(ctx),
            _ => {}
        }
    }

    /// Open the planner for the system under the cursor (or continue the route in hand).
    fn open_route(&mut self, _ctx: &mut Ctx<'_>) {
        let Some(player) = &self.player else { return };
        if self.chart.cursor == player.at {
            self.push(
                "The cursor is on your own system. Move it to a destination first.".to_owned(),
                russh_bbs_door::DoorStyle::Dim,
            );
            return;
        }
        if self.vis(self.chart.cursor) == Vis::Hidden {
            return;
        }
        let goal = self
            .plan
            .as_ref()
            .map_or(super::route::Goal::Fastest, |p| p.goal);
        self.plan = Some(super::route::Plan {
            target: self.chart.cursor,
            goal,
        });
        self.mode = Mode::Route;
    }

    // ---------------------------------------------------------------- drawing

    pub(super) fn draw_chart(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
        let Some(player) = &self.player else { return };
        let wide = area.width >= 96;
        let (map_area, info_area) = if wide {
            let [m, i] =
                Layout::horizontal([Constraint::Min(40), Constraint::Length(34)]).areas(area);
            (m, i)
        } else {
            let [m, i] = Layout::vertical([Constraint::Min(6), Constraint::Length(4)]).areas(area);
            (m, i)
        };
        let title = format!(
            "STAR CHART  {}  {}",
            self.chart.zoom.name(),
            match self.chart.overlay {
                Overlay::Faction => "by faction",
                Overlay::Intel => "by intel age",
            }
        );
        let block = panel(pal, &title, false);
        let inner = block.inner(map_area);
        frame.render_widget(block, map_area);
        self.draw_map(
            frame,
            inner,
            pal,
            player.at,
            self.chart.cursor,
            None,
            self.chart.zoom,
        );
        self.draw_chart_info(frame, info_area, pal, wide);
    }

    /// The map itself, drawn into `inner`. `route` highlights a path.
    #[allow(
        clippy::too_many_lines,
        reason = "one pass over lanes, rings and glyphs"
    )]
    pub(super) fn draw_map(
        &self,
        frame: &mut DoorFrame<'_>,
        inner: Rect,
        pal: &Pal,
        ship_at: u16,
        cursor: u16,
        route: Option<&[u16]>,
        zoom: Zoom,
    ) {
        if inner.width < 4 || inner.height < 3 {
            return;
        }
        let Some(player) = &self.player else { return };
        let (cols, rows) = (inner.width, inner.height);
        let (dots_w, dots_h) = (f64::from(cols) * 2.0, f64::from(rows) * 4.0);
        // Spans per dot. Full fits the whole map; the others zoom in on the cursor, or on the
        // middle of the route being planned.
        let fit = (f64::from(MAP_W) / dots_w).max(f64::from(MAP_H) / dots_h) * 1.04;
        let scale = match zoom {
            Zoom::Full => fit,
            Zoom::Near => fit / 2.4,
            Zoom::Close => fit / 5.0,
        };
        let focus = if zoom == Zoom::Full {
            None
        } else {
            self.frontier.system(cursor)
        };
        let (cx, cy) = focus.map_or((f64::from(MAP_W) / 2.0, f64::from(MAP_H) / 2.0), |s| {
            (f64::from(s.x), f64::from(s.y))
        });
        let project = |x: u16, y: u16| -> (i32, i32) {
            (
                ((f64::from(x) - cx) / scale + dots_w / 2.0).round() as i32,
                ((f64::from(y) - cy) / scale + dots_h / 2.0).round() as i32,
            )
        };
        let cell_of = |x: u16, y: u16| -> (i32, i32) {
            let (dx, dy) = project(x, y);
            (dx.div_euclid(2), dy.div_euclid(4))
        };
        let on_route = |a: u16, b: u16| -> bool {
            route.is_some_and(|r| {
                r.windows(2)
                    .any(|w| (w[0] == a && w[1] == b) || (w[0] == b && w[1] == a))
            })
        };
        let stats = player.stats(&self.content);
        let range_spans = if stats.burn == 0 {
            0.0
        } else {
            f64::from(player.ship.fuel) * 100.0 / f64::from(stats.burn)
        };
        let ship_pos = self.frontier.system(ship_at).map(|s| (s.x, s.y));

        let mut strings: Vec<(u16, u16, String, Style)> = Vec::new();
        if pal.ascii {
            // Plain ASCII: lanes as dots on the cell grid, rings as colons.
            let mut grid: Vec<(i32, i32, char, Style)> = Vec::new();
            let mut ring_ok =
                |cx_: i32, cy_: i32, ch: char, st: Style| grid.push((cx_, cy_, ch, st));
            if let Some((sx, sy)) = ship_pos
                && range_spans > 0.0
            {
                let (mx, my) = cell_of(sx, sy);
                let (rx, ry) = (range_spans / (scale * 2.0), range_spans / (scale * 4.0));
                for k in 0..120 {
                    let a = f64::from(k) * std::f64::consts::TAU / 120.0;
                    ring_ok(
                        mx + (rx * a.cos()).round() as i32,
                        my + (ry * a.sin()).round() as i32,
                        ':',
                        pal.dim,
                    );
                }
            }
            for lane in &self.frontier.lanes {
                let (va, vb) = (self.vis(lane.a), self.vis(lane.b));
                if va == Vis::Hidden || vb == Vis::Hidden {
                    continue;
                }
                let known = va == Vis::Charted && vb == Vis::Charted;
                let (Some(a), Some(b)) =
                    (self.frontier.system(lane.a), self.frontier.system(lane.b))
                else {
                    continue;
                };
                let (ca, cb) = (cell_of(a.x, a.y), cell_of(b.x, b.y));
                let route_lane = on_route(lane.a, lane.b);
                let steps = (ca.0 - cb.0).abs().max((ca.1 - cb.1).abs()).max(1);
                for k in 1..steps {
                    if !known && k % 2 == 0 {
                        continue;
                    }
                    let x = ca.0 + (cb.0 - ca.0) * k / steps;
                    let y = ca.1 + (cb.1 - ca.1) * k / steps;
                    grid.push((
                        x,
                        y,
                        if route_lane { '=' } else { '.' },
                        if route_lane { pal.amber } else { pal.hull },
                    ));
                }
            }
            for (x, y, ch, st) in grid {
                if x >= 0 && y >= 0 && x < i32::from(cols) && y < i32::from(rows) {
                    strings.push((x as u16, y as u16, ch.to_string(), st));
                }
            }
        } else {
            let mut raster = Raster::new(cols, rows);
            // Layer 0 is empty; 1 stars, 2 ring, 3 unknown lane, 4 lane, 5 route.
            for dy in 0..dots_h as i32 {
                for dx in 0..dots_w as i32 {
                    let h = mix(
                        u64::from(self.frontier.season),
                        (u64::from(dy as u32) << 20) | u64::from(dx as u32),
                    );
                    if h.is_multiple_of(97) {
                        raster.plot(dx, dy, 1);
                    }
                }
            }
            if let Some((sx, sy)) = ship_pos
                && range_spans > 0.0
            {
                let c = project(sx, sy);
                raster.circle(c, (range_spans / scale).round() as i32, 2, Some((3, 3)));
                raster.circle(
                    c,
                    (range_spans / 2.0 / scale).round() as i32,
                    2,
                    Some((1, 5)),
                );
            }
            for lane in &self.frontier.lanes {
                let (va, vb) = (self.vis(lane.a), self.vis(lane.b));
                if va == Vis::Hidden || vb == Vis::Hidden {
                    continue;
                }
                let known = va == Vis::Charted && vb == Vis::Charted;
                let (Some(a), Some(b)) =
                    (self.frontier.system(lane.a), self.frontier.system(lane.b))
                else {
                    continue;
                };
                let route_lane = on_route(lane.a, lane.b);
                let layer = if route_lane {
                    5
                } else if known {
                    4
                } else {
                    3
                };
                raster.line(
                    project(a.x, a.y),
                    project(b.x, b.y),
                    layer,
                    if known { None } else { Some((1, 3)) },
                );
            }
            let styles = [
                Style::default(),
                pal.deep,
                pal.dim,
                pal.dim,
                pal.hull,
                pal.amber,
            ];
            frame.render_widget(raster.widget(&styles), inner);
        }

        // Systems, then labels (cursor, ship, route and neighbours first).
        struct Mark {
            x: u16,
            y: u16,
            glyph: String,
            style: Style,
            name: String,
            prio: u8,
        }
        let mut marks: Vec<Mark> = Vec::new();
        let neighbours: Vec<u16> = self
            .frontier
            .neighbours(ship_at)
            .iter()
            .map(|&(n, _)| n)
            .collect();
        for s in &self.frontier.systems {
            let vis = self.vis(s.id);
            if vis == Vis::Hidden {
                continue;
            }
            let (px, py) = cell_of(s.x, s.y);
            if px < 0 || py < 0 || px >= i32::from(cols) || py >= i32::from(rows) {
                continue;
            }
            let station = s.stations.first().and_then(|&id| self.frontier.station(id));
            let (glyph, style) = match (vis, station) {
                (Vis::Signal, _) => ("?".to_owned(), pal.dim),
                (_, Some(st)) => {
                    let eco = &self.content.economies[usize::from(st.economy)];
                    let style = match self.chart.overlay {
                        Overlay::Faction => pal.faction(eco.faction.index()),
                        Overlay::Intel => self.intel_style(pal, st.id),
                    };
                    if pal.ascii {
                        (eco.letter.clone(), style)
                    } else {
                        ("●".to_owned(), style)
                    }
                }
                (_, None) => (if pal.ascii { "o" } else { "○" }.to_owned(), pal.hull),
            };
            let mut prio = u8::from(s.id == ship_at || s.id == cursor) * 3;
            if route.is_some_and(|r| r.contains(&s.id)) {
                prio = prio.max(2);
            }
            if neighbours.contains(&s.id) {
                prio = prio.max(1);
            }
            marks.push(Mark {
                x: px as u16,
                y: py as u16,
                glyph,
                style,
                name: if vis == Vis::Signal {
                    "signal".to_owned()
                } else {
                    s.name.clone()
                },
                prio,
            });
            if s.id == ship_at {
                let m = marks.last_mut();
                if let Some(m) = m {
                    if pal.ascii { "@" } else { "▲" }.clone_into(&mut m.glyph);
                    m.style = pal.amber;
                }
            }
            if s.id == cursor
                && let Some(m) = marks.last_mut()
            {
                m.style = pal.sel;
            }
        }
        marks.sort_by_key(|m| std::cmp::Reverse(m.prio));
        let mut taken: Vec<(u16, u16, u16)> = marks.iter().map(|m| (m.x, m.y, 1)).collect();
        let overlaps = |taken: &[(u16, u16, u16)], x: u16, y: u16, w: u16| -> bool {
            taken
                .iter()
                .any(|&(tx, ty, tw)| ty == y && x < tx + tw + 1 && tx < x + w + 1)
        };
        let mut labels: Vec<(u16, u16, String, Style)> = Vec::new();
        for m in &marks {
            strings.push((m.x, m.y, m.glyph.clone(), m.style));
            let want_label = m.prio > 0 || zoom == Zoom::Close;
            if !want_label {
                continue;
            }
            let text = trunc(&m.name, 14);
            let w = text.chars().count() as u16;
            // Right of the glyph, else left, else above.
            let right = (m.x + 2, m.y);
            let left = (m.x.saturating_sub(w + 1), m.y);
            let spot = [right, left]
                .into_iter()
                .find(|&(x, y)| x + w <= cols && (x != m.x) && !overlaps(&taken, x, y, w));
            if let Some((x, y)) = spot {
                taken.push((x, y, w));
                let style = if m.prio >= 3 {
                    pal.amber
                } else if m.prio > 0 {
                    pal.plain
                } else {
                    pal.dim
                };
                labels.push((x, y, text, style));
            }
        }
        strings.extend(labels);
        frame.render_widget(Strings(strings), inner);
    }

    fn intel_style(&self, pal: &Pal, station: u16) -> Style {
        let Some(player) = &self.player else {
            return pal.dim;
        };
        match player.intel_at(station) {
            None => pal.dim,
            Some(i) => match player.last_day - i.day {
                ..=1 => pal.good,
                2..=4 => pal.amber,
                _ => pal.bad,
            },
        }
    }

    fn draw_chart_info(&self, frame: &mut DoorFrame<'_>, area: Rect, pal: &Pal, wide: bool) {
        let Some(player) = &self.player else { return };
        let block = panel(pal, "SYSTEM", false);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let lines = self.system_card(pal, self.chart.cursor, wide);
        let mut lines = lines;
        if let Some(route) = self.route_to(self.chart.cursor, super::route::Goal::Fastest)
            && self.chart.cursor != player.at
        {
            let short = route.short_at.is_some();
            lines.push(Line::from(vec![
                Span::styled("Route ", pal.dim),
                Span::styled(
                    format!(
                        "{} jump{}, {} fuel, {} days{}",
                        route.legs.len(),
                        if route.legs.len() == 1 { "" } else { "s" },
                        route.fuel,
                        route.days,
                        if short { "  NOT ENOUGH FUEL" } else { "" }
                    ),
                    if short { pal.bad } else { pal.plain },
                ),
            ]));
        }
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
    }

    /// A few lines about one system: what it is and who is there.
    pub(super) fn system_card(&self, pal: &Pal, id: u16, wide: bool) -> Vec<Line<'static>> {
        let Some(sys) = self.frontier.system(id) else {
            return Vec::new();
        };
        if self.vis(id) != Vis::Charted {
            return vec![
                Line::styled("Uncharted signal", pal.amber),
                Line::styled(
                    "Nobody has been there. Survey data is paid for the first look.",
                    pal.dim,
                ),
            ];
        }
        let mut out = vec![Line::from(vec![
            Span::styled(sys.name.clone(), pal.accent),
            Span::styled(
                format!("  {} - {}", sys.star.name(), sys.hazard.name()),
                pal.dim,
            ),
        ])];
        let mut bits = Vec::new();
        if sys.planets > 0 {
            bits.push(format!("{} planets", sys.planets));
        }
        if sys.belts > 0 {
            bits.push(format!("{} belts", sys.belts));
        }
        if sys.gas_giant {
            bits.push("gas giant".to_owned());
        }
        if wide && !bits.is_empty() {
            out.push(Line::styled(bits.join(", "), pal.dim));
        }
        for &sid in &sys.stations {
            if let Some(st) = self.frontier.station(sid) {
                let eco = &self.content.economies[usize::from(st.economy)];
                let mut tags = vec![eco.name.clone(), format!("size {}", st.size)];
                if st.shipyard {
                    tags.push("yard".to_owned());
                }
                if st.repair {
                    tags.push("repair".to_owned());
                }
                out.push(Line::from(vec![
                    Span::styled(
                        if pal.ascii {
                            eco.letter.clone()
                        } else {
                            "●".to_owned()
                        },
                        pal.faction(eco.faction.index()),
                    ),
                    Span::styled(format!(" {} ", st.name), pal.plain),
                    Span::styled(tags.join(" - "), pal.dim),
                ]));
            }
        }
        if sys.stations.is_empty() {
            out.push(Line::styled("No station.", pal.dim));
        }
        if let Some(player) = &self.player
            && let Some(lane) = self.frontier.lane(player.at, id)
        {
            let stats = player.stats(&self.content);
            out.push(Line::styled(
                format!(
                    "Lane from here: {} spans, {} fuel, {} days",
                    lane,
                    crate::ship::Ship::fuel_for(&stats, lane),
                    crate::ship::Ship::days_for(&stats, lane)
                ),
                pal.plain,
            ));
        }
        let _ = credits;
        out
    }

    pub(super) fn chart_keys(&self, pal: &Pal, width: u16) -> Vec<Line<'static>> {
        key_lines(
            pal,
            &[
                ("arrows", " move"),
                ("Enter", " plan route"),
                ("+/-", " zoom"),
                ("o", " overlay"),
                (".", " to ship"),
                ("Esc", " back"),
            ],
            width,
            2,
        )
    }
}
