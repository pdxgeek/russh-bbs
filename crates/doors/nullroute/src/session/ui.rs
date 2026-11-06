//! The Nullroute screens, drawn with ratatui into the door's own buffer.
//!
//! Nothing here holds a rule: every number comes from [`Player`](crate::player::Player),
//! [`Run`](crate::run::Run) or the content tables, and every key is handled in `session.rs`. This
//! module only turns that state into pictures: a node graph with braille edges, a trace that
//! closes in from both sides, a scrolling log, gauges, tables and sparklines.
//!
//! **Responsive.** Three layouts, chosen by the size of the door's area ([`Size`]): `Small` is the
//! 72 by 19 area of an 80 by 24 window, `Medium` from 90 by 24 and `Large` from 120 by 34. A
//! window under 56 by 14 gets the plain text of the look-around view.
//!
//! **Tiers and colour.** The framework turns box drawing, blocks and braille into ASCII for the
//! ASCII tier and brings every colour down to the member's depth, so the widgets here are drawn
//! once. Two places choose by tier anyway because a stand-in would look worse than a drawing of
//! their own: the node graph (diagonal edges are `/` and `\` in ASCII, not dots) and the node
//! glyphs. Nothing relies on colour alone: every state also has a word or a distinct glyph.
//!
//! **Palette.** Text and borders use the theme's roles ([`DoorStyle`]). The Nullroute accents are
//! signal green, warning red, cold cyan and amber; the framework degrades them.

// Drawing code: layout rectangles are named by one letter, a cell count is cast to a ratio and a
// ratio back to a cell count (every value is a few hundred at most), and a meter takes its
// colours and widths as arguments.
#![allow(
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_arguments
)]

use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Color, Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Block, BorderType, Paragraph};
use russh_bbs_door::{Canvas, DoorEnv, DoorFrame, DoorStyle, Tier};

use super::{Session, View};

mod fight;
mod gear;
mod hack;
mod map;
mod social;
mod town;

/// Signal green: good, open, fresh.
pub(super) const GREEN: Color = Color::Rgb(0x39, 0xff, 0x8f);
/// Warning red: danger, burned, patched.
pub(super) const RED: Color = Color::Rgb(0xff, 0x4a, 0x55);
/// Cold cyan: you, scans, information.
pub(super) const CYAN: Color = Color::Rgb(0x4d, 0xd2, 0xff);
/// Amber: caution, locks, money.
pub(super) const AMBER: Color = Color::Rgb(0xff, 0xb8, 0x3d);
/// Violet: countermeasures.
pub(super) const VIOLET: Color = Color::Rgb(0xb4, 0x8c, 0xff);
/// Steel: edges and empty gauge cells.
pub(super) const STEEL: Color = Color::Rgb(0x4a, 0x66, 0x78);
/// The faint grid under the map.
pub(super) const GRID: Color = Color::Rgb(0x1f, 0x30, 0x3c);
/// The glow behind the node you stand on.
pub(super) const GLOW: Color = Color::Rgb(0x0b, 0x3a, 0x4a);

/// The smallest area that gets the panels.
const PANELS_MIN: (u16, u16) = (56, 14);

/// Which layout the area gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Size {
    /// 72 by 19 (an 80 by 24 window) up to a little more.
    Small,
    /// 90 by 24.
    Medium,
    /// 120 by 34.
    Large,
}

impl Size {
    fn of(area: Rect) -> Self {
        if area.width >= 120 && area.height >= 34 {
            Self::Large
        } else if area.width >= 88 && area.height >= 23 {
            Self::Medium
        } else {
            Self::Small
        }
    }
}

/// The theme's roles as styles.
#[derive(Debug, Clone, Copy)]
pub(super) struct Ink {
    pub plain: Style,
    pub dim: Style,
    pub strong: Style,
    pub accent: Style,
    pub sel: Style,
}

impl Ink {
    fn of(frame: &DoorFrame<'_>) -> Self {
        Self {
            plain: frame.style(DoorStyle::Plain),
            dim: frame.style(DoorStyle::Dim),
            strong: frame.style(DoorStyle::Strong),
            accent: frame.style(DoorStyle::Accent),
            sel: frame.style(DoorStyle::Selected),
        }
    }
}

/// A pen: the frame, the theme's styles, and what the layout needs to know.
pub(super) struct Pen<'a, 'b> {
    pub f: &'a mut DoorFrame<'b>,
    pub ink: Ink,
    pub size: Size,
    pub ascii: bool,
}

/// A foreground colour on top of the theme's plain style.
pub(super) fn fg(color: Color) -> Style {
    Style::new().fg(color)
}

/// A bold foreground colour.
pub(super) fn bold(color: Color) -> Style {
    Style::new().fg(color).add_modifier(Modifier::BOLD)
}

impl Pen<'_, '_> {
    /// A bordered panel with a title; returns the room inside.
    pub(super) fn panel(&mut self, rect: Rect, title: &str, tone: Color) -> Rect {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(fg(tone))
            .title(Line::from(vec![
                Span::styled(" ", fg(tone)),
                Span::styled(title.to_owned(), bold(tone)),
                Span::styled(" ", fg(tone)),
            ]));
        let inner = block.inner(rect);
        self.f.render_widget(block, rect);
        inner
    }

    /// A panel with only a rule and a title above it, for the rows a full box would eat.
    pub(super) fn ruled(&mut self, rect: Rect, title: &str, tone: Color) -> Rect {
        let block = Block::new()
            .borders(russh_bbs_door::widgets::Borders::TOP)
            .border_type(BorderType::Plain)
            .border_style(fg(STEEL))
            .title(Line::from(vec![
                Span::styled(" ", fg(tone)),
                Span::styled(title.to_owned(), bold(tone)),
                Span::styled(" ", fg(tone)),
            ]));
        let inner = block.inner(rect);
        self.f.render_widget(block, rect);
        inner
    }

    /// Lines of text in `rect`.
    pub(super) fn text(&mut self, rect: Rect, lines: Vec<Line<'static>>) {
        self.f.render_widget(Paragraph::new(lines), rect);
    }

    /// Lines of text wrapped to `rect`.
    pub(super) fn wrapped(&mut self, rect: Rect, lines: Vec<Line<'static>>) {
        self.f.render_widget(
            Paragraph::new(lines).wrap(russh_bbs_door::widgets::Wrap { trim: false }),
            rect,
        );
    }

    /// The key hints, one row: `[key] what`, the key in the accent style.
    pub(super) fn hint(&mut self, rect: Rect, keys: &[(&str, &str)]) {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (key, what) in keys {
            spans.push(Span::styled(format!("[{key}]"), self.ink.accent));
            spans.push(Span::styled(format!("{what}  "), self.ink.dim));
        }
        self.f
            .render_widget(Paragraph::new(Line::from(spans)), rect);
    }

    /// The notes the last action left (success in green, refusals in red), one per row.
    pub(super) fn notes(&mut self, rect: Rect, s: &Session) {
        let lines = s
            .msgs
            .iter()
            .map(|(text, style)| {
                let style = match style {
                    DoorStyle::Good => bold(GREEN),
                    DoorStyle::Bad => bold(RED),
                    DoorStyle::Accent => bold(CYAN),
                    _ => self.ink.dim,
                };
                Line::styled(text.clone(), style)
            })
            .collect();
        self.wrapped(rect, lines);
    }
}

/// Clip `text` to `max` characters, with an ellipsis stand-in that is ASCII.
pub(super) fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
        out.push('~');
        out
    }
}

/// The colour of a point along a bar: green, then amber, then red.
pub(super) fn heat_color(frac: f64) -> Color {
    if frac < 0.5 {
        GREEN
    } else if frac < 0.75 {
        AMBER
    } else {
        RED
    }
}

/// A smooth bar: `width` cells, `frac` of them filled in eighths. Filled cells take `color` (or
/// a gradient when `gradient`), empty cells are steel shade.
pub(super) fn bar(width: usize, frac: f64, color: Color, gradient: bool) -> Vec<Span<'static>> {
    const PARTS: [char; 8] = [
        ' ', '\u{258F}', '\u{258E}', '\u{258D}', '\u{258C}', '\u{258B}', '\u{258A}', '\u{2589}',
    ];
    let frac = frac.clamp(0.0, 1.0);
    let eighths = (frac * width as f64 * 8.0).round() as usize;
    let (full, part) = (eighths / 8, eighths % 8);
    let mut spans = Vec::new();
    for i in 0..full.min(width) {
        let c = if gradient {
            heat_color(i as f64 / width.max(1) as f64)
        } else {
            color
        };
        spans.push(Span::styled("\u{2588}", fg(c)));
    }
    let mut used = full.min(width);
    if part > 0 && used < width {
        let c = if gradient {
            heat_color(used as f64 / width.max(1) as f64)
        } else {
            color
        };
        spans.push(Span::styled(PARTS[part].to_string(), fg(c)));
        used += 1;
    }
    if used < width {
        spans.push(Span::styled("\u{2591}".repeat(width - used), fg(STEEL)));
    }
    spans
}

/// `label  [bar]  tail`, one line. The label is padded to `label_w`.
pub(super) fn meter(
    label: &str,
    label_w: usize,
    value: u32,
    max: u32,
    width: usize,
    color: Color,
    tail: &str,
    ink: &Ink,
) -> Line<'static> {
    let mut spans = vec![Span::styled(format!("{label:<label_w$} "), ink.dim)];
    let frac = if max == 0 {
        0.0
    } else {
        f64::from(value) / f64::from(max)
    };
    spans.extend(bar(width, frac, color, false));
    spans.push(Span::styled(format!(" {tail}"), ink.plain));
    Line::from(spans)
}

/// The trace closing in from both sides. `width` cells: an arm from each edge grows toward the
/// middle as the trace rises, the middle says how far it is, and at 100 the arms meet.
pub(super) fn trace_line(trace: u32, width: usize, ink: &Ink) -> Line<'static> {
    let trace = trace.min(100);
    let mid = if trace >= 100 {
        " BURNED ".to_owned()
    } else if trace >= 70 {
        format!(" TRACE {trace:>3}! ")
    } else {
        format!(" TRACE {trace:>3} ")
    };
    let mid_w = mid.chars().count();
    let half = width.saturating_sub(mid_w) / 2;
    let fill = if trace == 0 {
        0
    } else {
        ((trace as usize * half).div_ceil(100)).clamp(1, half)
    };
    let arm = |left: bool| -> Vec<Span<'static>> {
        (0..half)
            .map(|i| {
                // `i` counts from the outer edge toward the middle.
                let filled = i < fill;
                let tip = filled && i + 1 == fill && fill < half;
                let c = heat_color(i as f64 / half.max(1) as f64);
                let (ch, style) = if tip {
                    (if left { '\u{25B6}' } else { '\u{25C0}' }, bold(c))
                } else if filled {
                    ('\u{2588}', fg(c))
                } else {
                    ('\u{2591}', fg(STEEL))
                };
                Span::styled(ch.to_string(), style)
            })
            .collect()
    };
    let left = arm(true);
    let mut right = arm(false);
    right.reverse();
    let mid_style = if trace >= 70 {
        bold(RED)
    } else if trace >= 40 {
        bold(AMBER)
    } else {
        ink.strong
    };
    let mut spans = left;
    spans.push(Span::styled(mid, mid_style));
    spans.extend(right);
    Line::from(spans)
}

/// Draw the current view.
pub(super) fn draw(s: &Session, canvas: &mut Canvas<'_>, env: &DoorEnv) {
    let (w, h) = (canvas.width(), canvas.height());
    if w < PANELS_MIN.0 || h < PANELS_MIN.1 {
        s.draw_text(canvas);
        return;
    }
    let mut frame = canvas.frame();
    let area = frame.area();
    let ink = Ink::of(&frame);
    let mut pen = Pen {
        f: &mut frame,
        ink,
        size: Size::of(area),
        ascii: env.tier == Tier::Ascii,
    };
    match &s.view {
        View::Run(active) => {
            if active.run.encounter().is_some() {
                fight::draw(&mut pen, s, active, area);
            } else {
                hack::draw(&mut pen, s, active, area);
            }
        }
        View::Debrief(report) => hack::debrief(&mut pen, s, report, area),
        View::Home => town::home(&mut pen, s, area),
        View::Contracts => town::contracts(&mut pen, s, area),
        View::Brief(contract) => town::brief(&mut pen, s, contract, area),
        View::Fixers => town::fixers(&mut pen, s, area),
        View::Heat => town::heat(&mut pen, s, area),
        View::Profile => town::profile(&mut pen, s, area),
        View::Deck(page) => gear::deck(&mut pen, s, *page, area),
        View::Shop(page) => gear::market(&mut pen, s, *page, area),
        View::Black => gear::black_market(&mut pen, s, area),
        View::Rig => gear::rig(&mut pen, s, area),
        View::Hideout => gear::hideout(&mut pen, s, area),
        View::Code(draft) => gear::code(&mut pen, s, *draft, area),
        View::Rivals
        | View::Input(_)
        | View::Crew
        | View::Arcs
        | View::Scene(_)
        | View::Street(_) => social::draw(&mut pen, s, area),
    }
}

/// Split a rectangle into rows by constraints.
pub(super) fn rows<const N: usize>(area: Rect, c: [Constraint; N]) -> [Rect; N] {
    Layout::vertical(c).areas(area)
}

/// Split a rectangle into columns by constraints.
pub(super) fn cols<const N: usize>(area: Rect, c: [Constraint; N]) -> [Rect; N] {
    Layout::horizontal(c).areas(area)
}
