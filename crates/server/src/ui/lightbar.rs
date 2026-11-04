//! The **lightbar** layout: the classic box with a highlighted row that Up and Down move and
//! Enter chooses (M3-A). It draws the same [`MenuPage`](russh_bbs_core::menu::MenuPage) as the
//! classic layout; hotkeys still work, so nothing is lost by choosing it.
//!
//! **The highlight is not the security style.** Markers use bold + reverse video, and nothing
//! else may (D39), so the cursor row is drawn with a pointer character, bold and underline (and
//! the theme's selection colour when colour is on). It never uses reverse video, and it is drawn
//! only inside the content box: the status bar, with its marker cell, is outside the body.

use ratatui::Frame;

use super::classic::{self, MenuScreen};
use super::keys;

/// Draw the screen: the frame's bars, and the menu box with the cursor row highlighted.
pub(crate) fn render(frame: &mut Frame<'_>, screen: &MenuScreen<'_>) {
    let hints = classic::menu_hints(screen.strings, true);
    let body = classic::frame_spec(screen, &hints).draw(frame);
    if screen.page.help {
        classic::draw_help(frame, body, screen, Some(keys::HELP_LIGHTBAR));
    } else {
        classic::draw_menu(frame, body, screen, Some(screen.page.cursor));
    }
}
