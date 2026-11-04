//! The member's drawing preferences, read from their settings in one place (UI-2): the colour
//! depth ceiling, ASCII borders and motion.

use russh_bbs_core::terminal_settings::TerminalSettings;

use crate::ui::depth::ColorDepth;

/// The member's ceiling on colour depth; `None` is automatic.
pub(super) fn depth_cap(_settings: &TerminalSettings) -> Option<ColorDepth> {
    None
}

/// Whether the member chose ASCII borders.
pub(super) fn ascii_borders(_settings: &TerminalSettings) -> bool {
    false
}

/// Whether the member wants motion.
pub(super) fn motion(_settings: &TerminalSettings) -> bool {
    true
}
