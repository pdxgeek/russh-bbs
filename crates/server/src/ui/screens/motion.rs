//! Motion that is not an effect (UI-4): the **blinking cursor** in text inputs. The short wipe
//! between menus and the other effects are UI-8's engine (`ui::fx`); this module shares its
//! gate, so one switch turns all motion off.
//!
//! **Gate.** The cursor blinks only when [`allowed`]: the look allows motion (the member wants
//! it, the board allows it and the style has it) and the session is not in accessibility mode.
//! Off means a steady `_` and **no timer**: [`Motion::deadline`] is `None`, so the shell never
//! wakes for it.
//!
//! **Cost (D49).** The shell never draws more often than its redraw interval (50 ms, 20 a
//! second). The cursor blinks every [`BLINK`] (2 redraws a second, and only while an input is on
//! screen), far under the cap; the constant is asserted against it in the tests. The cursor is
//! a character in the input line ([`Look::cursor`]), never a style, so it cannot look like a
//! marker.

use std::time::{Duration, Instant};

use crate::settings::Look;

/// Half a blink: how long the cursor stays lit, then dark.
pub(crate) const BLINK: Duration = Duration::from_millis(500);

/// Whether motion may run at all: the look allows it and the session is not in accessibility
/// mode (which is always plain). The same gate as the effect engine's.
#[must_use]
pub(crate) fn allowed(look: Look, accessible: bool) -> bool {
    crate::ui::fx::allowed(look, accessible)
}

/// One session's motion state.
#[derive(Debug, Default)]
pub(crate) struct Motion {
    /// Accessibility mode (M5): always plain.
    accessible: bool,
    /// Whether an input is on screen, so the cursor needs a timer.
    input: bool,
    /// The blink's zero.
    epoch: Option<Instant>,
    /// Whether the last frame drawn showed the cursor dark, so a tick knows the phase moved.
    shown_dark: bool,
}

impl Motion {
    /// Turn accessibility mode on or off.
    pub(crate) fn set_accessible(&mut self, on: bool) {
        self.accessible = on;
    }

    /// Whether accessibility mode is on.
    pub(crate) const fn accessible(&self) -> bool {
        self.accessible
    }

    /// Tell the state whether an input line is on screen.
    pub(crate) fn set_input(&mut self, input: bool, now: Instant) {
        if input && !self.input {
            self.epoch = Some(now);
        }
        self.input = input;
    }

    /// Note which half of the blink the frame just drawn showed.
    pub(crate) fn shown(&mut self, dark: bool) {
        self.shown_dark = dark;
    }

    /// Whether the blink has moved to the other half since the last frame drawn, so the screen
    /// needs a redraw. [`Motion::deadline`] cannot answer this: it is always the *next* beat,
    /// later than `now` (the FIX-M35-7 bug: the tick never found it due).
    pub(crate) fn blink_due(&self, look: Look, now: Instant) -> bool {
        self.input && self.cursor_dark(look, now) != self.shown_dark
    }

    /// Whether the cursor is in the dark half of its blink at `now`.
    pub(crate) fn cursor_dark(&self, look: Look, now: Instant) -> bool {
        if !allowed(look, self.accessible) || !self.input {
            return false;
        }
        let Some(epoch) = self.epoch else {
            return false;
        };
        (now.saturating_duration_since(epoch).as_millis() / BLINK.as_millis()) % 2 == 1
    }

    /// When the shell should next wake to blink the cursor, if one is blinking. `None` when
    /// motion is off or no input is on screen, which is the common case (and the only one in
    /// accessibility mode).
    pub(crate) fn deadline(&self, look: Look, now: Instant) -> Option<Instant> {
        if !allowed(look, self.accessible) || !self.input {
            return None;
        }
        let epoch = self.epoch?;
        let beats = now.saturating_duration_since(epoch).as_millis() / BLINK.as_millis() + 1;
        Some(epoch + BLINK * u32::try_from(beats).unwrap_or(u32::MAX))
    }
}
