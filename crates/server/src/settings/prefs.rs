//! The member's drawing preferences, read from their settings in one place (UI-2): the colour
//! depth ceiling, ASCII borders and motion.

use russh_bbs_core::terminal_settings::{BorderChoice, DepthCap, TerminalSettings};

use crate::ui::depth::ColorDepth;

/// The member's ceiling on colour depth; `None` is automatic.
pub(super) fn depth_cap(settings: &TerminalSettings) -> Option<ColorDepth> {
    match settings.depth_cap() {
        DepthCap::Auto => None,
        DepthCap::Ansi16 => Some(ColorDepth::Ansi16),
        DepthCap::Ansi256 => Some(ColorDepth::Ansi256),
        DepthCap::TrueColor => Some(ColorDepth::TrueColor),
    }
}

/// Whether the member chose ASCII borders.
pub(super) fn ascii_borders(settings: &TerminalSettings) -> bool {
    settings.borders() == BorderChoice::Ascii
}

/// Whether the member chose the Blocks border.
pub(super) fn blocks_borders(settings: &TerminalSettings) -> bool {
    settings.borders() == BorderChoice::Blocks
}

/// Whether the member wants motion.
pub(super) fn motion(settings: &TerminalSettings) -> bool {
    settings.motion()
}
