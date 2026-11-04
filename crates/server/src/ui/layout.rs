//! Choosing and drawing a layout (M3-A, D82).
//!
//! A layout is a way to arrange the **same** menu page; the page, the string table and the
//! reserved status row come from outside and are identical for every layout. This module is the
//! one place that maps a [`LayoutId`] to its drawing, with an exhaustive `match` (no wildcard),
//! so a new layout does not compile until it is drawn.
//!
//! **Minimum sizes and fallback** (`docs/ui.md`): each layout declares the smallest area it is
//! drawn in and the layout used below that, so a small terminal never gets a broken box. The
//! chain ends at the compact layout, which has no minimum.

use ratatui::Frame;
use ratatui::layout::Rect;
use russh_bbs_core::terminal_settings::LayoutId;

use super::classic::{self, MenuScreen};
use super::{compact, lightbar, panes};

/// The smallest area `layout` is drawn in, as `(columns, rows)`, and the layout used below it.
/// `None` for a layout with no minimum. The chain from every layout ends at compact, so a tiny
/// terminal always gets a drawing; the pane layouts fall back toward the lightbar first because
/// it keeps the cursor the pane layouts also have.
const fn requirement(layout: LayoutId) -> Option<((u16, u16), LayoutId)> {
    match layout {
        LayoutId::Classic => Some(((24, 9), LayoutId::Compact)),
        LayoutId::Lightbar => Some(((44, 12), LayoutId::Classic)),
        LayoutId::Compact => None,
        LayoutId::Dashboard | LayoutId::Wide | LayoutId::Focus | LayoutId::Split => {
            Some(((0, 0), LayoutId::Classic))
        }
    }
}

/// The layout actually drawn for `chosen` in `area`: `chosen` if it fits, else its fallback,
/// repeating until one fits. Terminates at the compact layout.
#[must_use]
pub(crate) fn effective(chosen: LayoutId, area: Rect) -> LayoutId {
    let mut layout = chosen;
    while let Some(((cols, rows), fallback)) = requirement(layout) {
        if area.width >= cols && area.height >= rows {
            break;
        }
        layout = fallback;
    }
    layout
}

/// Draw a menu screen in `layout`. Call with the layout from [`effective`].
pub(crate) fn render_menu(frame: &mut Frame<'_>, layout: LayoutId, screen: &MenuScreen<'_>) {
    match layout {
        LayoutId::Classic
        | LayoutId::Dashboard
        | LayoutId::Wide
        | LayoutId::Focus
        | LayoutId::Split => classic::render(frame, screen),
        LayoutId::Lightbar => lightbar::render(frame, screen),
        LayoutId::Compact => compact::render(frame, screen),
        LayoutId::Dashboard | LayoutId::Wide | LayoutId::Focus | LayoutId::Split => {
            panes::render(frame, layout, screen);
        }
    }
}
