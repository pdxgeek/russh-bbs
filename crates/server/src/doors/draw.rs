//! Drawing the door list and a running door into the member's box.
//!
//! A door never touches the real frame. It draws any ratatui widget into its own off-screen
//! `DoorSurface`, the size of the box's inner area, and this module **composites** that surface:
//! the framework builds a new buffer from it, cleaning every cell (the shared sanitizer, no
//! reserved marker style, colours degraded to the member's depth, the ASCII tier's stand-ins for
//! icon characters), and the clean buffer is copied into ordinary lines for the same box and
//! status row every other screen uses. So the box, the title, the security markers and the
//! transport's footer row are never inside the door's reach, and nothing the door drew reaches
//! the screen uncleaned.

use ratatui::Frame;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_door::{Canvas, ColorDepth, Composite, DoorPalette, DoorSurface};

use super::screen::{DoorsScreen, State};
use super::strings;
use crate::settings::{self, Look};
use crate::ui::chrome;
use crate::ui::classic::{self, ContentScreen, MENU_MARKERS, styles};
use crate::ui::strings::StringTable;

/// Rows of the box above the door's area: the heading and a blank line.
const HEADER_ROWS: usize = 2;

/// What drawing needs besides the screen.
pub(crate) struct DrawCtx<'a> {
    pub(crate) strings: &'a StringTable,
    pub(crate) look: Look,
    pub(crate) tier: Tier,
}

/// A door's palette from the member's theme: the theme's colors when color is on, weights only
/// when it is off. No entry can be the reserved marker style (the palette breaks that).
fn palette(look: Look) -> DoorPalette {
    let colors = look.palette();
    DoorPalette::new([
        Style::new(),
        Style::new().add_modifier(Modifier::DIM),
        Style::new().add_modifier(Modifier::BOLD),
        look.accent(Style::new().add_modifier(Modifier::BOLD), colors.heading),
        look.accent(Style::new(), colors.info),
        look.accent(Style::new().add_modifier(Modifier::BOLD), colors.error),
        Style::new().add_modifier(Modifier::REVERSED),
    ])
}

fn clean(text: &str) -> String {
    SingleLine::new(120).sanitize(text).into_string()
}

/// Draw the doors screens.
pub(crate) fn render(frame: &mut Frame<'_>, screen: &DoorsScreen, ctx: &DrawCtx<'_>) {
    let (content, _) = chrome::split_status(frame.area(), &MENU_MARKERS);
    let (width, rows) = settings::inner_size(content);
    let lines = match &screen.state {
        State::Playing(running) => {
            let title = screen
                .current
                .and_then(|at| screen.doors.get(at))
                .map_or(strings::HEADING, |door| door.info().title);
            let mut lines = header(title, ctx);
            let door_rows = rows.saturating_sub(HEADER_ROWS);
            lines.extend(door_lines(width, door_rows, ctx, |canvas| {
                running.render(canvas, &screen.env);
            }));
            lines
        }
        State::Busy => {
            let mut lines = header(strings::HEADING, ctx);
            lines.push(Line::raw(strings::OPENING));
            lines
        }
        State::List => list_lines(screen, ctx),
    };
    classic::render_content(
        frame,
        ContentScreen {
            strings: ctx.strings,
            lines,
            look: ctx.look,
            tier: ctx.tier,
            markers: &MENU_MARKERS,
        },
    );
}

fn header(title: &str, ctx: &DrawCtx<'_>) -> Vec<Line<'static>> {
    let (_, _, heading, _) = styles(ctx.look);
    vec![Line::styled(clean(title), heading), Line::raw("")]
}

fn list_lines(screen: &DoorsScreen, ctx: &DrawCtx<'_>) -> Vec<Line<'static>> {
    let (bold, dim, _, key) = styles(ctx.look);
    let mut lines = header(strings::HEADING, ctx);
    if screen.doors.is_empty() {
        lines.push(Line::raw(strings::NONE));
    } else {
        lines.push(Line::styled(strings::PICK, dim));
        lines.push(Line::raw(""));
        for (n, door) in screen.doors.iter().enumerate() {
            let info = door.info();
            let marker = if n == screen.cursor { ">" } else { " " };
            let row = Style::new().add_modifier(if n == screen.cursor {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
            lines.push(Line::from(vec![
                Span::styled(format!("{marker} "), bold),
                Span::styled(format!("[{}] ", n + 1), key),
                Span::styled(clean(info.title), row),
            ]));
            lines.push(Line::styled(format!("      {}", clean(info.blurb)), dim));
        }
    }
    if let Some(notice) = screen.notice {
        lines.push(Line::raw(""));
        lines.push(Line::styled(notice, bold));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(strings::LEAVE_HINT, dim));
    lines
}

/// The colour depth a door's colours are brought down to. The member's setting is on or off
/// (`TerminalSettings::color`) and the board's own screens use the sixteen named colours, so
/// "on" is sixteen colours; a per-member depth would be a settings change (D104).
const fn depth(look: Look) -> ColorDepth {
    if look.color() {
        ColorDepth::Ansi16
    } else {
        ColorDepth::Mono
    }
}

/// Let `draw` paint a `width` by `rows` off-screen surface, composite it (every cell cleaned)
/// and return it as lines.
pub(super) fn door_lines(
    width: usize,
    rows: usize,
    ctx: &DrawCtx<'_>,
    draw: impl FnOnce(&mut Canvas<'_>),
) -> Vec<Line<'static>> {
    let width = u16::try_from(width).unwrap_or(u16::MAX);
    let rows = u16::try_from(rows).unwrap_or(u16::MAX);
    if width == 0 || rows == 0 {
        return Vec::new();
    }
    let mut surface = DoorSurface::new(width, rows, palette(ctx.look));
    surface.draw(draw);
    surface
        .composite(Composite::new(depth(ctx.look), ctx.tier == Tier::Ascii))
        .to_lines()
}
