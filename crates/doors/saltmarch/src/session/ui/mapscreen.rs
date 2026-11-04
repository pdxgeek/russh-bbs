//! The marsh map: charted ground with fog of war, the tide drawn over it as a colour band (or
//! as a change of glyph when colour is off), the hold, the Tidecourt, places of interest, the
//! party and its route. The strip across the top of the screen is the tide (`tides::strip`).
//!
//! With colour and a Unicode tier the map is drawn in half blocks: every cell shows two rows
//! of tiles (the upper half block's foreground is the upper tile, its background the lower),
//! so the marsh is as square as it is on paper. Otherwise one glyph is one tile.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Color, Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Wrap};

use super::{Cells, Size, Ui, fit};
use crate::map::{PoiKind, SIZE, TileKind};
use crate::tide::Access;
use crate::trip::{self, Trip};

type Rgb = (u8, u8, u8);

/// The ground's colour: soft, earthy, a little lighter on higher ground.
const fn ground_rgb(kind: TileKind) -> Rgb {
    match kind {
        TileKind::Mudflat => (112, 100, 82),
        TileKind::Reedbed => (126, 132, 66),
        TileKind::PeatBog => (86, 64, 50),
        TileKind::SaltPan => (206, 212, 200),
        TileKind::Channel => (52, 118, 134),
        TileKind::Island => (92, 136, 88),
        TileKind::DrownedTown => (112, 118, 126),
        TileKind::Causeway => (176, 134, 86),
        TileKind::Wreck => (140, 90, 64),
        TileKind::Mere => (34, 78, 110),
        TileKind::Deep => (74, 48, 96),
    }
}

const WET_RGB: Rgb = (44, 118, 138);
const DROWNED_RGB: Rgb = (18, 52, 100);
const FOG_RGB: Rgb = (24, 32, 40);

/// Fog over unknown ground: flecked, so it reads as mist and not as nothing.
fn fog(x: i32, y: i32) -> Rgb {
    let h = (x.wrapping_mul(31) ^ y.wrapping_mul(17)).rem_euclid(11);
    if h == 0 {
        lerp(FOG_RGB, (60, 74, 84), 40)
    } else {
        FOG_RGB
    }
}

fn lerp(a: Rgb, b: Rgb, pct: u32) -> Rgb {
    let mix = |x: u8, y: u8| {
        u8::try_from((u32::from(x) * (100 - pct) + u32::from(y) * pct) / 100).unwrap_or(255)
    };
    (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

fn color((r, g, b): Rgb) -> Color {
    Color::Rgb(r, g, b)
}

/// The plain glyph of a tile kind (the ASCII tier, and colour off).
const fn glyph(kind: TileKind, ascii: bool) -> &'static str {
    if ascii {
        match kind {
            TileKind::Mudflat => ".",
            TileKind::Reedbed => "\"",
            TileKind::PeatBog => ",",
            TileKind::SaltPan => ":",
            TileKind::Channel => "~",
            TileKind::Island => "^",
            TileKind::DrownedTown => "#",
            TileKind::Causeway => "=",
            TileKind::Wreck => "x",
            TileKind::Mere => "o",
            TileKind::Deep => "%",
        }
    } else {
        match kind {
            TileKind::Mudflat => "·",
            TileKind::Reedbed => "\"",
            TileKind::PeatBog => "∙",
            TileKind::SaltPan => "░",
            TileKind::Channel => "≈",
            TileKind::Island => "▲",
            TileKind::DrownedTown => "▦",
            TileKind::Causeway => "═",
            TileKind::Wreck => "×",
            TileKind::Mere => "○",
            TileKind::Deep => "▓",
        }
    }
}

/// The marker for a point of interest: a letter, so it reads in every tier.
const fn poi_mark(kind: PoiKind) -> (&'static str, Rgb) {
    match kind {
        PoiKind::Camp => ("c", (232, 196, 120)),
        PoiKind::Shrine => ("+", (200, 170, 240)),
        PoiKind::Salvage => ("s", (150, 220, 200)),
        PoiKind::Lair => ("!", (230, 90, 70)),
        PoiKind::Hermit => ("h", (170, 210, 150)),
        PoiKind::Trader => ("$", (240, 200, 90)),
        PoiKind::Ruin => ("R", (200, 200, 210)),
    }
}

/// What the map is drawn around.
pub(super) struct View {
    /// The tile at the middle of the map.
    pub centre: (i32, i32),
    pub party: Option<(u8, u8)>,
    pub target: Option<(u8, u8)>,
    pub route: Vec<(u8, u8)>,
}

/// A marker on a tile.
struct Mark {
    glyph: &'static str,
    style: Style,
}

fn mark_at(ui: &Ui<'_>, view: &View, x: i32, y: i32) -> Option<Mark> {
    let p = &ui.pal;
    let (ux, uy) = (u8::try_from(x).ok()?, u8::try_from(y).ok()?);
    let strong = |s: Style| s.add_modifier(Modifier::BOLD);
    if view.party == Some((ux, uy)) {
        return Some(Mark {
            glyph: "@",
            style: strong(p.on(Color::Rgb(255, 232, 140), Color::Rgb(120, 60, 20))),
        });
    }
    if view.target == Some((ux, uy)) {
        return Some(Mark {
            glyph: ui.look.g("×", "X"),
            style: strong(p.gold()),
        });
    }
    if ui.s.map.sites.get(usize::from(ui.clan.site)) == Some(&(ux, uy)) {
        return Some(Mark {
            glyph: ui.look.g("⌂", "H"),
            style: strong(p.on(Color::Rgb(250, 230, 200), Color::Rgb(120, 76, 40))),
        });
    }
    if ui.s.map.tidecourt == (ux, uy) {
        return Some(Mark {
            glyph: ui.look.g("▣", "T"),
            style: strong(p.on(Color::Rgb(240, 244, 236), Color::Rgb(60, 90, 120))),
        });
    }
    if ui.clan.is_charted(usize::from(ux), usize::from(uy))
        && let Some(poi) = ui.s.map.poi_at(ux, uy)
    {
        let (g, rgb) = poi_mark(poi.kind);
        return Some(Mark {
            glyph: g,
            style: strong(p.fg(color(rgb))),
        });
    }
    if view.route.contains(&(ux, uy)) {
        return Some(Mark {
            glyph: ui.look.g("•", "."),
            style: p.gold(),
        });
    }
    None
}

/// The colour a tile is drawn in, charted or not, wet or dry.
fn tile_rgb(ui: &Ui<'_>, x: i32, y: i32) -> Option<(Rgb, TileKind, Access)> {
    let (ux, uy) = (usize::try_from(x).ok()?, usize::try_from(y).ok()?);
    let tile = ui.s.map.tile(ux, uy)?;
    if !ui.clan.is_charted(ux, uy) {
        return None;
    }
    let access = Access::of(i32::from(tile.elevation), ui.level);
    let base = ground_rgb(tile.kind);
    // Relief: higher ground is a little lighter.
    let lift = u32::from(tile.elevation) * 30 / 100;
    let lit = lerp(
        (base.0 / 2, base.1 / 2, base.2 / 2),
        (base.0, base.1, base.2),
        70 + lift,
    );
    let shaded = match access {
        Access::Dry => lit,
        Access::Wet => lerp(lit, WET_RGB, 55),
        Access::Drowned => lerp(lit, DROWNED_RGB, 88),
    };
    Some((shaded, tile.kind, access))
}

/// The cells of the map for an area `w` by `h`.
fn cells(ui: &Ui<'_>, view: &View, w: u16, h: u16) -> (Cells, bool) {
    let p = &ui.pal;
    let half = ui.look.color && !ui.look.ascii;
    let per = if half { 2 } else { 1 };
    let (wi, hi) = (i32::from(w), i32::from(h));
    // With room for the whole marsh, show all of it.
    let whole = i32::try_from(SIZE).unwrap_or(64);
    let centre = if wi >= whole && hi * per >= whole {
        (whole / 2, whole / 2)
    } else {
        view.centre
    };
    let x0 = centre.0 - wi / 2;
    let y0 = centre.1 - hi * per / 2;
    let mut rows = Vec::with_capacity(usize::from(h));
    for cy in 0..hi {
        let mut row: Vec<(&'static str, Style)> = Vec::with_capacity(usize::from(w));
        for cx in 0..wi {
            let tx = x0 + cx;
            let ty = y0 + cy * per;
            let off = |x: i32, y: i32| {
                !(0..i32::try_from(SIZE).unwrap_or(64)).contains(&x)
                    || !(0..i32::try_from(SIZE).unwrap_or(64)).contains(&y)
            };
            if half {
                let top = (!off(tx, ty)).then(|| tile_rgb(ui, tx, ty)).flatten();
                let bot = (!off(tx, ty + 1))
                    .then(|| tile_rgb(ui, tx, ty + 1))
                    .flatten();
                // Off the map is the terminal's own background; unknown ground is fog, a little
                // lighter and flecked, so the edge of what is charted shows.
                let paint = |t: Option<(Rgb, TileKind, Access)>, x: i32, y: i32| -> Color {
                    match t {
                        Some((rgb, _, _)) => color(rgb),
                        None if off(x, y) => Color::Reset,
                        None => color(fog(x, y)),
                    }
                };
                let mark = mark_at(ui, view, tx, ty).or_else(|| mark_at(ui, view, tx, ty + 1));
                let (upper, lower) = (paint(top, tx, ty), paint(bot, tx, ty + 1));
                if let Some(m) = mark {
                    row.push((m.glyph, m.style.bg(upper)));
                } else if upper == Color::Reset && lower == Color::Reset {
                    // Off the map on both halves: nothing to draw.
                    row.push((" ", Style::new()));
                } else if upper == Color::Reset {
                    row.push(("▄", Style::new().fg(lower)));
                } else {
                    row.push(("▀", Style::new().fg(upper).bg(lower)));
                }
            } else {
                if off(tx, ty) {
                    row.push((" ", Style::new()));
                    continue;
                }
                let info = tile_rgb(ui, tx, ty);
                if let Some(m) = mark_at(ui, view, tx, ty) {
                    // A marker on a tile: its own style, over the water-tinted ground.
                    let style = if let (true, Some((rgb, _, _))) = (ui.look.color, info) {
                        m.style.bg(color(lerp(rgb, (0, 0, 0), 55)))
                    } else {
                        m.style
                    };
                    row.push((m.glyph, style));
                    continue;
                }
                match info {
                    None => row.push((
                        if (tx.wrapping_mul(31) ^ ty.wrapping_mul(17)).rem_euclid(7) == 0 {
                            ui.look.g("·", ".")
                        } else {
                            " "
                        },
                        p.slate().add_modifier(Modifier::DIM),
                    )),
                    Some((rgb, kind, access)) => {
                        if ui.look.color {
                            row.push((
                                glyph(kind, ui.look.ascii),
                                Style::new()
                                    .fg(color(lerp(rgb, (255, 255, 255), 35)))
                                    .bg(color(lerp(rgb, (0, 0, 0), 55))),
                            ));
                        } else {
                            // No colour: the water is a change of glyph.
                            let g = match access {
                                Access::Dry => glyph(kind, ui.look.ascii),
                                Access::Wet => ui.look.g("∼", "-"),
                                Access::Drowned => ui.look.g("≈", "~"),
                            };
                            row.push((g, p.plain));
                        }
                    }
                }
            }
        }
        rows.push(row);
    }
    (Cells { rows }, half)
}

/// The legend: how the tide shows (a colour band, or glyphs without colour) and the markers.
fn legend(ui: &Ui<'_>) -> Vec<Line<'static>> {
    let p = &ui.pal;
    let swatch = |rgb: Rgb, glyph: &'static str, word: &'static str| -> Vec<Span<'static>> {
        let style = if ui.look.color {
            Style::new()
                .fg(color(lerp(rgb, (255, 255, 255), 30)))
                .bg(color(rgb))
        } else {
            p.plain
        };
        vec![
            Span::styled(format!("{glyph}{glyph}"), style),
            Span::styled(format!(" {word}  "), p.dim),
        ]
    };
    let mut water: Vec<Span<'static>> = vec![Span::styled("water ", p.dim)];
    water.extend(swatch((96, 110, 80), ui.look.g("·", "."), "dry"));
    water.extend(swatch(WET_RGB, ui.look.g("∼", "-"), "knee-deep"));
    water.extend(swatch(DROWNED_RGB, ui.look.g("≈", "~"), "under"));
    let marks = Line::from(vec![
        Span::styled(format!("{} ", "@"), p.gold()),
        Span::styled("party  ", p.dim),
        Span::styled(format!("{} ", ui.look.g("⌂", "H")), p.peat()),
        Span::styled("hold  ", p.dim),
        Span::styled(format!("{} ", ui.look.g("▣", "T")), p.salt()),
        Span::styled("Tidecourt  ", p.dim),
        Span::styled("c", p.reed()),
        Span::styled("amp ", p.dim),
        Span::styled("+", p.fg(color(poi_mark(PoiKind::Shrine).1))),
        Span::styled("shrine ", p.dim),
        Span::styled("s", p.fg(color(poi_mark(PoiKind::Salvage).1))),
        Span::styled("alvage ", p.dim),
        Span::styled("!", p.rust()),
        Span::styled("lair ", p.dim),
        Span::styled("$", p.gold()),
        Span::styled("trader ", p.dim),
        Span::styled("R", p.salt()),
        Span::styled("uin", p.dim),
    ]);
    vec![Line::from(water), marks]
}

/// The party and the ground it stands on.
fn party_card(ui: &Ui<'_>, t: &Trip) -> Vec<Line<'static>> {
    let p = &ui.pal;
    let clan = ui.clan;
    let mut lines: Vec<Line<'static>> = Vec::new();
    for id in &t.party {
        if let Some(c) = clan.crew_by(*id) {
            let hurt = !c.injuries.is_empty();
            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "{} ",
                        if hurt {
                            ui.look.g("✖", "x")
                        } else {
                            ui.look.g("●", "o")
                        }
                    ),
                    if hurt { p.rust() } else { p.moss() },
                ),
                Span::styled(fit(&c.name, 12), p.plain),
                Span::styled(format!(" {}", c.trade.name()), p.dim),
            ]));
        }
    }
    lines
}

fn side_trip(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, t: &Trip) {
    let p = &ui.pal;
    let clan = ui.clan;
    let roomy = ui.look.size >= Size::Roomy;
    let party = party_card(ui, t);
    let party_h = u16::try_from(party.len()).unwrap_or(5) + 2;
    let state_h: u16 = if roomy { 7 } else { 6 };
    let [a, b, c] = Layout::vertical([
        Constraint::Length(party_h),
        Constraint::Length(state_h),
        Constraint::Min(4),
    ])
    .areas(area);
    f.render_widget(Paragraph::new(party).block(ui.panel("Party")), a);

    let block = ui.panel("Underfoot");
    let inner = block.inner(b);
    f.render_widget(block, b);
    let cap = t.capacity(clan, &ui.s.cat);
    let load = t.load();
    let tile = ui.s.map.tile(usize::from(t.at.0), usize::from(t.at.1));
    let access = tile.map(|x| Access::of(i32::from(x.elevation), ui.level));
    let ground = match (tile, access) {
        (Some(x), Some(Access::Dry)) => (format!("{}, dry", x.kind.name()), p.moss()),
        (Some(x), Some(Access::Wet)) => (format!("{}, knee-deep", x.kind.name()), p.gold()),
        (Some(x), Some(Access::Drowned)) => (format!("{}, under water", x.kind.name()), p.rust()),
        _ => ("nowhere".to_owned(), p.dim),
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("{},{} ", t.at.0, t.at.1), p.salt()),
            Span::styled(ground.0, ground.1),
        ]),
        ui.gauge_with(
            "Load",
            load.min(cap.max(1)),
            cap.max(1),
            inner.width,
            if load > cap { p.rust() } else { p.sea() },
            format!(" {load}/{cap}"),
        ),
    ];
    let env = &ui.env;
    let warn = match trip::safe_level(env, t) {
        Some(safe) => {
            let soon = trip::turns_until_drowned(env, clan, t, 12);
            let text = match soon {
                Some(n) => format!(
                    "{} Goes under at water {} in ~{n} turns",
                    ui.look.g("▲", "!"),
                    safe + 1
                ),
                None => format!("Goes under at water {}", safe + 1),
            };
            (text, if soon.is_some() { p.rust() } else { p.dim })
        }
        None => ("A boat keeps the party afloat.".to_owned(), p.sea()),
    };
    lines.push(Line::styled(warn.0, warn.1));
    let home = trip::home(env, clan);
    let eta = if t.at == home {
        "At the hold.".to_owned()
    } else {
        ui.s.reach_of(clan, t)
            .cost(home)
            .map_or("No safe way home today!".to_owned(), |c| {
                format!("{c} turns home.")
            })
    };
    lines.push(Line::styled(eta, p.plain));
    if t.hungry() {
        lines.push(Line::styled("HUNGRY: the rations are gone.", p.rust()));
    }
    if t.overloaded(clan, &ui.s.cat) {
        lines.push(Line::styled("OVERLOADED: every step costs more.", p.rust()));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);

    let items: Vec<_> = ui.entries.iter().enumerate().collect();
    ui.orders(f, c, "Orders", items);
}

fn side_chart(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let clan = ui.clan;
    let total = SIZE * SIZE;
    let seen = usize::try_from(clan.charted_count()).unwrap_or(0);
    let [a, b] = Layout::vertical([Constraint::Length(5), Constraint::Min(4)]).areas(area);
    let lines = vec![
        ui.gauge_with(
            "Charted",
            u32::try_from(seen).unwrap_or(0),
            u32::try_from(total).unwrap_or(1),
            a.width.saturating_sub(2),
            p.sea(),
            format!(" {}%", seen * 100 / total),
        ),
        Line::styled(
            format!(
                "Your hold stands at {},{}.",
                ui.s.map
                    .sites
                    .get(usize::from(clan.site))
                    .map_or(0, |s| s.0),
                ui.s.map
                    .sites
                    .get(usize::from(clan.site))
                    .map_or(0, |s| s.1),
            ),
            p.plain,
        ),
        Line::styled(
            "Arrows pan. Survey from the hold to chart more ground.",
            p.dim,
        ),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(ui.panel("The chart")),
        a,
    );
    f.render_widget(
        Paragraph::new(legend(ui))
            .wrap(Wrap { trim: true })
            .block(ui.panel("Legend")),
        b,
    );
}

pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, on_trip: bool) {
    let side_w = if ui.look.size >= Size::Roomy { 34 } else { 30 };
    let [map_area, side] =
        Layout::horizontal([Constraint::Min(30), Constraint::Length(side_w)]).areas(area);
    let trip = ui.clan.trip.as_ref().filter(|_| on_trip);
    let home =
        ui.s.map
            .sites
            .get(usize::from(ui.clan.site))
            .copied()
            .unwrap_or((32, 32));
    let (centre, party, target, route) = match trip {
        Some(t) => {
            let route = t
                .target
                .and_then(|to| ui.s.reach_of(ui.clan, t).path_to(to))
                .map(|(path, _)| path)
                .unwrap_or_default();
            (
                (i32::from(t.at.0), i32::from(t.at.1)),
                Some(t.at),
                t.target,
                route,
            )
        }
        None => (
            (
                (i32::from(home.0) + i32::from(ui.s.pan.0)).clamp(0, 63),
                (i32::from(home.1) + i32::from(ui.s.pan.1)).clamp(0, 63),
            ),
            None,
            None,
            Vec::new(),
        ),
    };
    let view = View {
        centre,
        party,
        target,
        route,
    };
    let title = format!("The marsh ({},{})", centre.0, centre.1);
    let block = ui.panel(&title);
    let inner = block.inner(map_area);
    f.render_widget(block, map_area);
    // The legend sits under the map when the map is tall enough to give up two rows.
    let (grid, leg) = if on_trip && inner.height >= 12 {
        let [g, l] = Layout::vertical([Constraint::Min(8), Constraint::Length(2)]).areas(inner);
        (g, Some(l))
    } else {
        (inner, None)
    };
    let (picture, _half) = cells(ui, &view, grid.width, grid.height);
    f.render_widget(picture, grid);
    if let Some(l) = leg {
        f.render_widget(Paragraph::new(legend(ui)), l);
    }
    match trip {
        Some(t) => side_trip(ui, f, side, t),
        None => side_chart(ui, f, side),
    }
}
