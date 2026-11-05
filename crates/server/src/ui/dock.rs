//! The **dock** (UI-W, builds UI-9 part 2): a second pane beside any framed screen, with a
//! divider the member can move with the mouse or with keys.
//!
//! This module is geometry and the one hook the frame calls; it draws nothing and holds no
//! session state. The rules it keeps:
//!
//! - **Only the body gives way.** The dock takes columns from the body between the header and
//!   the status bar. The header, the status bar and the marker cell (the status bar's left
//!   cell) are drawn by [`super::frame::FrameSpec`] over the full width as before, so no pane,
//!   divider or drag can reach them. [`split`] is a pure function of the body, tested to stay
//!   inside it.
//! - **Both sides keep a minimum.** The screen keeps at least [`MIN_MAIN`] columns and the pane
//!   at least [`MIN_PANE`]. A terminal too narrow for both has no dock ([`bounds`] is `None`),
//!   and the member is told so; the screen is never squeezed below what its tables can draw.
//! - **A drag is a resize.** The mouse sets the width through the same function the `<` and `>`
//!   keys use ([`drag_target`], [`resized`]), clamped to the same bounds, so a drag can do
//!   nothing a key cannot (D125 (1)).
//!
//! The frame learns about the dock through a thread-local request set around one synchronous
//! draw ([`with`]), the way the hit map and the ticker are passed (`ui::input::capture`,
//! `ui::ticker`): `FrameSpec::draw` calls [`carve`] on the body it would hand to the screen and
//! the screen's own code needs no change. What was carved is read back with [`with`] so the
//! caller can draw the pane and record the divider.

use std::cell::Cell;

use ratatui::layout::Rect;

use crate::ssh::terminal::Key;

/// The fewest columns the screen beside the dock keeps.
pub(crate) const MIN_MAIN: u16 = 40;
/// The fewest columns the dock pane keeps.
pub(crate) const MIN_PANE: u16 = 24;
/// The divider's width.
pub(crate) const DIVIDER: u16 = 1;
/// Columns one `<` or `>` moves the divider.
pub(crate) const STEP: u16 = 1;
/// Columns one `{` or `}` moves it.
pub(crate) const BIG_STEP: u16 = 8;

/// Where the dock puts things inside the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Geometry {
    /// The screen's share of the body.
    pub(crate) main: Rect,
    /// The divider column.
    pub(crate) divider: Rect,
    /// The pane.
    pub(crate) pane: Rect,
}

/// The least and the most columns the pane may have in a body `total` columns wide. `None` when
/// the body cannot hold a screen, a divider and a pane at their minimums.
#[must_use]
pub(crate) fn bounds(total: u16) -> Option<(u16, u16)> {
    let most = total.checked_sub(MIN_MAIN + DIVIDER)?;
    (most >= MIN_PANE).then_some((MIN_PANE, most))
}

/// `wanted` columns for the pane, held between the bounds. `None` when there is no dock.
#[must_use]
pub(crate) fn clamp(total: u16, wanted: u16) -> Option<u16> {
    let (least, most) = bounds(total)?;
    Some(wanted.clamp(least, most))
}

/// The pane's width when nobody has moved the divider: about two fifths of the body.
#[must_use]
pub(crate) fn default_width(total: u16) -> u16 {
    let wanted = u16::try_from(u32::from(total) * 2 / 5).unwrap_or(MIN_PANE);
    clamp(total, wanted).unwrap_or(MIN_PANE)
}

/// Split `body` into the screen, the divider and a pane of (about) `wanted` columns. The three
/// rectangles are disjoint and inside `body`; `None` when the body is too narrow.
#[must_use]
pub(crate) fn split(body: Rect, wanted: u16) -> Option<Geometry> {
    let pane_width = clamp(body.width, wanted)?;
    let main_width = body.width - pane_width - DIVIDER;
    let main = Rect {
        width: main_width,
        ..body
    };
    let divider = Rect {
        x: body.x + main_width,
        width: DIVIDER,
        ..body
    };
    let pane = Rect {
        x: divider.x + DIVIDER,
        width: pane_width,
        ..body
    };
    Some(Geometry {
        main,
        divider,
        pane,
    })
}

/// The width the screen beside a dock of `wanted` columns is drawn at, given a terminal
/// `total` columns wide. The key path uses this so paging and layout match the drawing.
#[must_use]
pub(crate) fn main_width(total: u16, wanted: u16) -> u16 {
    clamp(total, wanted).map_or(total, |pane| total - pane - DIVIDER)
}

/// The pane width that puts the divider on column `to`, held between the bounds. A drag calls
/// the same resize the `<` and `>` keys call, with this width (D125 (1): the mouse does nothing
/// a key cannot).
#[must_use]
pub(crate) fn drag_target(body: Rect, to: u16) -> Option<u16> {
    let right = body.x.saturating_add(body.width);
    clamp(body.width, right.saturating_sub(to).saturating_sub(DIVIDER))
}

/// The pane width after a resize key, or `None` when `key` is not one.
#[must_use]
pub(crate) fn resized(total: u16, current: u16, key: Key) -> Option<u16> {
    let wanted = match key {
        Key::Char('<') => current.saturating_add(STEP),
        Key::Char('>') => current.saturating_sub(STEP),
        Key::Char('{') => current.saturating_add(BIG_STEP),
        Key::Char('}') => current.saturating_sub(BIG_STEP),
        _ => return None,
    };
    clamp(total, wanted)
}

/// Whether `key` is one of the four resize keys.
#[must_use]
pub(crate) fn is_resize_key(key: Key) -> bool {
    matches!(key, Key::Char('<' | '>' | '{' | '}'))
}

thread_local! {
    /// The pane width the draw in progress should reserve, and what it reserved.
    static REQUEST: Cell<Option<Request>> = const { Cell::new(None) };
}

#[derive(Debug, Clone, Copy)]
struct Request {
    wanted: u16,
    done: Option<Geometry>,
}

/// Run `draw` with a dock of `wanted` columns requested (`None` for no dock) and return what
/// the frame reserved. The result is `None` when no dock was asked for, when the body was too
/// narrow, or when the frame drew no body at all.
pub(crate) fn with<R>(wanted: Option<u16>, draw: impl FnOnce() -> R) -> (R, Option<Geometry>) {
    let previous =
        REQUEST.with(|cell| cell.replace(wanted.map(|wanted| Request { wanted, done: None })));
    let result = draw();
    let used = REQUEST.with(|cell| cell.replace(previous));
    (result, used.and_then(|request| request.done))
}

/// What the draw in progress has reserved so far, for the code that draws the pane after the
/// screen. `None` outside [`with`], when no dock was asked for, or before the frame's body.
#[must_use]
pub(crate) fn reserved() -> Option<Geometry> {
    REQUEST.with(|cell| cell.get().and_then(|request| request.done))
}

/// The body a screen should draw in: `body` less the dock, when one is requested for this
/// draw. Called by `FrameSpec::draw` for the body between the bars, so the bars keep the full
/// width and only the body gives way.
#[must_use]
pub(crate) fn carve(body: Rect) -> Rect {
    REQUEST.with(|cell| {
        let Some(mut request) = cell.get() else {
            return body;
        };
        // One body per draw: a second frame spec in the same draw (a popup's own) is not carved.
        if request.done.is_some() {
            return body;
        }
        match split(body, request.wanted) {
            Some(geometry) => {
                request.done = Some(geometry);
                cell.set(Some(request));
                geometry.main
            }
            None => body,
        }
    })
}

#[cfg(test)]
mod tests;
