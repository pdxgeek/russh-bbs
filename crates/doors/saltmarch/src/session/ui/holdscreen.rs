//! The hold: the plot grid as a styled map, with the build menu and the upkeep gauges. The
//! home screen is the same picture with the main menu beside it.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Block, BorderType, Paragraph, Row, Table, Wrap};

use super::{Size, Ui, fit};
use crate::hold::{COLS, PLOTS, ROWS};
use crate::types::{Resource, Track};

/// The style of a building track.
fn track_style(ui: &Ui<'_>, t: Track) -> Style {
    match t {
        Track::Shelter => ui.pal.peat(),
        Track::Work => ui.pal.sea(),
        Track::Defence => ui.pal.rust(),
    }
}

/// What stands on a plot, for the grid.
struct Plot {
    /// Short name.
    name: String,
    /// Second line: crew posted, labour left, or a word.
    note: String,
    track: Option<Track>,
    state: State,
    /// What it makes, for a cell with room for a third line.
    makes: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Locked,
    Empty,
    Building,
    Built,
}

fn plots(ui: &Ui<'_>) -> Vec<Plot> {
    let hold = &ui.clan.hold;
    let cat = &ui.s.cat;
    let open = hold.unlocked(cat);
    (0..PLOTS)
        .map(|i| {
            if i >= open {
                return Plot {
                    name: String::new(),
                    note: "locked".to_owned(),
                    track: None,
                    state: State::Locked,
                    makes: String::new(),
                };
            }
            if let Some(id) = hold.plots.get(i).and_then(|p| p.as_ref())
                && let Some(def) = cat.building(id)
            {
                let posted = ui
                    .clan
                    .crew
                    .iter()
                    .filter(|c| usize::from(c.post.unwrap_or(255)) == i)
                    .count();
                let note = if def.slots > 0 {
                    let slots = usize::from(def.slots);
                    format!(
                        "{}{}",
                        ui.look.g("●", "o").repeat(posted.min(slots)),
                        ui.look.g("○", ".").repeat(slots.saturating_sub(posted))
                    )
                } else if def.crew_cap > 0 {
                    format!("beds {}", def.crew_cap)
                } else if def.defence > 0 || def.flood_defence > 0 {
                    "guard".to_owned()
                } else if def.morale > 0 {
                    format!("joy +{}", def.morale)
                } else {
                    def.track.name().to_owned()
                };
                return Plot {
                    name: def.name.clone(),
                    note,
                    track: Some(def.track),
                    state: State::Built,
                    makes: if def.produce.is_empty() {
                        String::new()
                    } else {
                        format!("makes {}", def.produce.describe())
                    },
                };
            }
            if let Some(c) = hold
                .building
                .iter()
                .flatten()
                .find(|c| usize::from(c.plot) == i)
                && let Some(def) = cat.building(&c.building)
            {
                return Plot {
                    name: def.name.clone(),
                    note: format!("{} {} left", ui.look.g("◆", "~"), c.remaining),
                    track: Some(def.track),
                    state: State::Building,
                    makes: String::new(),
                };
            }
            Plot {
                name: String::new(),
                note: "free".to_owned(),
                track: None,
                state: State::Empty,
                makes: String::new(),
            }
        })
        .collect()
}

/// The grid of plots as a map of the high ground.
fn grid(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let outer = ui.panel(&format!("The hold of {}", ui.clan.name));
    let inner = outer.inner(area);
    f.render_widget(outer, area);
    let cols = usize::from(COLS);
    let rows = usize::from(ROWS);
    let legend_h = 1;
    let grid_h = inner.height.saturating_sub(legend_h);
    let (cols_n, rows_n) = (u16::from(COLS), u16::from(ROWS));
    let cw = inner.width / cols_n;
    let ch = grid_h / rows_n;
    let boxes = cw >= 8 && ch >= 4;
    let col_rects = Layout::horizontal(vec![Constraint::Length(cw); cols]).split(Rect::new(
        inner.x,
        inner.y,
        cw * cols_n,
        grid_h,
    ));
    let plots = plots(ui);
    for r in 0..rows {
        let y = inner.y + u16::try_from(r).unwrap_or(0) * ch;
        for c in 0..cols {
            let cell = Rect::new(col_rects[c].x, y, cw, ch);
            let pl = &plots[r * cols + c];
            let line_style = match pl.state {
                State::Locked => p.slate(),
                State::Empty => p.border(),
                State::Building => p.gold(),
                State::Built => pl.track.map_or(p.plain, |t| track_style(ui, t)),
            };
            let name_style = match pl.state {
                State::Locked => p.slate(),
                State::Empty => p.dim,
                _ => p.salt(),
            };
            let name = if pl.state == State::Locked {
                ui.look.g("░░░░", "////").to_owned()
            } else if pl.state == State::Empty {
                ui.look.g("·  ·", ". .").to_owned()
            } else {
                pl.name.clone()
            };
            let w = usize::from(cw).saturating_sub(if boxes { 2 } else { 1 });
            // A long name shows its last word (the noun): "Fish weir" is "weir".
            let name = if name.chars().count() > w {
                name.rsplit(' ').next().unwrap_or(&name).to_owned()
            } else {
                name
            };
            let lines = vec![
                Line::from(Span::styled(
                    fit(&name, w),
                    name_style.add_modifier(if pl.state == State::Built {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ))
                .centered(),
                Line::from(Span::styled(fit(&pl.note, w), line_style)).centered(),
            ];
            let lines = if ch >= 5 && !pl.makes.is_empty() {
                let mut l = lines;
                l.push(Line::from(Span::styled(fit(&pl.makes, w), p.dim)).centered());
                l
            } else {
                lines
            };
            let widget = if boxes {
                Paragraph::new(lines).block(
                    Block::bordered()
                        .border_type(if ui.look.ascii {
                            BorderType::Plain
                        } else if pl.state == State::Built {
                            BorderType::Rounded
                        } else {
                            BorderType::Plain
                        })
                        .border_style(line_style),
                )
            } else {
                Paragraph::new(lines)
            };
            f.render_widget(widget, cell);
        }
    }
    // The legend: the three tracks, in their colours and in words.
    let legend = Line::from(vec![
        Span::styled(format!("{} shelter ", ui.look.g("■", "#")), p.peat()),
        Span::styled(format!("{} work ", ui.look.g("■", "#")), p.sea()),
        Span::styled(format!("{} defence ", ui.look.g("■", "#")), p.rust()),
        Span::styled(format!("{} building", ui.look.g("■", "#")), p.gold()),
    ]);
    f.render_widget(
        Paragraph::new(legend),
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
    );
}

/// Nights of food and of peat in the stores, against a week.
fn nights(ui: &Ui<'_>) -> (u32, u32) {
    let clan = ui.clan;
    let mouths = u32::try_from(clan.crew.len()).unwrap_or(1).max(1);
    let food = clan.stock.get(Resource::Food) + clan.stock.get(Resource::Fish) / 2;
    let peat = clan.stock.get(Resource::Peat);
    let upkeep = clan.hold.upkeep_peat(&ui.s.cat).max(1);
    (food / mouths, peat / upkeep)
}

/// The upkeep gauges and the stores.
fn gauges(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, stores_table: bool) {
    let p = &ui.pal;
    let clan = ui.clan;
    let block = ui.panel("Upkeep");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let (food, peat) = nights(ui);
    let w = inner.width;
    let week = 7;
    let tone = |n: u32| {
        if n <= 1 {
            p.rust()
        } else if n <= 3 {
            p.gold()
        } else {
            p.moss()
        }
    };
    let mut lines = vec![
        ui.gauge_with(
            "Larder",
            food.min(week),
            week,
            w,
            tone(food),
            format!(" {food} night{}", if food == 1 { "" } else { "s" }),
        ),
        ui.gauge_with(
            "Peat",
            peat.min(week),
            week,
            w,
            tone(peat),
            format!(" {peat} night{}", if peat == 1 { "" } else { "s" }),
        ),
        ui.gauge_line(
            "Morale",
            u32::from(clan.morale),
            100,
            w,
            if clan.morale < 35 { p.rust() } else { p.moss() },
        ),
        ui.gauge_line(
            "Crew",
            u32::try_from(clan.crew.len()).unwrap_or(0),
            u32::try_from(clan.hold.crew_cap(&ui.s.cat)).unwrap_or(1),
            w,
            p.sea(),
        ),
    ];
    if ui.clan.hold.built(&ui.s.cat).any(|(_, b)| b.is_sluice()) {
        lines.push(Line::styled(
            ui.clan
                .hold
                .sluice_level
                .map_or("Sluice: open".to_owned(), |l| {
                    format!("Sluice: floods the approach at water {l}")
                }),
            p.sea(),
        ));
    }
    if !stores_table {
        let goods: Vec<String> = Resource::ALL
            .iter()
            .filter(|r| clan.stock.get(**r) > 0)
            .map(|r| format!("{} {}", clan.stock.get(*r), r.name()))
            .collect();
        lines.push(Line::from(Span::styled(
            if goods.is_empty() {
                "Stores: empty".to_owned()
            } else {
                format!("Stores: {}", goods.join(", "))
            },
            p.dim,
        )));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

/// The stores as a two-column table.
fn stores(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let have: Vec<(Resource, u32)> = Resource::ALL
        .iter()
        .map(|&r| (r, ui.clan.stock.get(r)))
        .collect();
    let half = have.len().div_ceil(2);
    let rows: Vec<Row<'static>> = (0..half)
        .map(|i| {
            let cell = |slot: Option<&(Resource, u32)>| match slot {
                Some((r, n)) => vec![
                    Span::styled(r.name(), if *n == 0 { p.slate() } else { p.plain }),
                    Span::styled(format!("{n}"), if *n == 0 { p.slate() } else { p.salt() }),
                ],
                None => vec![Span::raw(""), Span::raw("")],
            };
            let mut cells = cell(have.get(i));
            cells.extend(cell(have.get(i + half)));
            Row::new(cells)
        })
        .collect();
    f.render_widget(
        Table::new(
            rows,
            [
                Constraint::Min(8),
                Constraint::Length(4),
                Constraint::Min(8),
                Constraint::Length(4),
            ],
        )
        .block(ui.panel("Stores")),
        area,
    );
}

pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, home: bool) {
    let roomy = ui.look.size >= Size::Roomy;
    let left_w = if roomy {
        (area.width * 11 / 20).clamp(48, 80)
    } else {
        // Five plots of eight columns and the frame.
        46.min(area.width / 2 + 6)
    };
    let [left, right] =
        Layout::horizontal([Constraint::Length(left_w), Constraint::Min(26)]).areas(area);
    grid(ui, f, left);
    let g_h = if roomy { 6 } else { 8 };
    let stores_h = if roomy { 9 } else { 0 };
    let [top, mid, orders] = Layout::vertical([
        Constraint::Length(g_h),
        Constraint::Length(stores_h),
        Constraint::Min(4),
    ])
    .areas(right);
    gauges(ui, f, top, roomy);
    if roomy {
        stores(ui, f, mid);
    }
    let _ = home;
    let items: Vec<_> = ui.entries.iter().enumerate().collect();
    ui.orders(
        f,
        orders,
        if home { "Orders" } else { "Build and work" },
        &items,
    );
}
