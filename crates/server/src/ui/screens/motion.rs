//! Motion, used sparingly (UI-4): a short wipe when the screen changes and a blinking cursor in
//! inputs. Everything here is a pure function of a clock, so it is tested without a terminal.
//!
//! **Gate.** Motion runs only when [`allowed`]: the look allows it (the member wants it, the
//! board allows it and the style has it) and the session is not in accessibility mode. Off
//! means no animation and **no timer**: [`Motion::deadline`] is `None`, so the shell never wakes
//! for it.
//!
//! **Cost (D49).** The shell never draws more often than its redraw interval (50 ms, 20 a
//! second). A wipe takes [`STEPS`] frames [`STEP`] apart (80 ms, 12.5 a second for a third of a
//! second, once per screen change) and the cursor blinks every [`BLINK`] (2 a second, only
//! while an input is on screen). Both are under the cap, and the constants are asserted
//! against it in the tests.
//!
//! **Where it draws.** A wipe changes cells inside the body only: never the header row and
//! never the last row, where the status bar and its marker cell are. It uses no style at all
//! (no reverse video, no bold), so it cannot look like a marker.

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::settings::Look;

/// Frames in a wipe, including the last one that shows the whole screen.
pub(crate) const STEPS: u8 = 4;
/// Time between two wipe frames. Longer than the shell's redraw interval, so a wipe never
/// asks for more redraws than the D49 cap lets through.
pub(crate) const STEP: Duration = Duration::from_millis(80);
/// Half a blink: how long the cursor stays lit, then dark.
pub(crate) const BLINK: Duration = Duration::from_millis(500);

/// Whether motion may run at all: the look allows it and the session is not in accessibility
/// mode (which is always plain).
#[must_use]
pub(crate) const fn allowed(look: Look, accessible: bool) -> bool {
    look.motion() && !accessible
}

/// One session's motion state.
#[derive(Debug, Default)]
pub(crate) struct Motion {
    /// What the last frame showed, so a change starts a wipe.
    shown: Option<u64>,
    /// When the running wipe started, and how many frames it has drawn.
    wipe: Option<(Instant, u8)>,
    /// Accessibility mode (M5): always plain.
    accessible: bool,
    /// Whether an input is on screen, so the cursor needs a timer.
    input: bool,
    /// The blink's zero.
    epoch: Option<Instant>,
}

impl Motion {
    /// Turn accessibility mode on or off. On cancels anything running.
    pub(crate) fn set_accessible(&mut self, on: bool) {
        self.accessible = on;
        if on {
            self.wipe = None;
        }
    }

    /// Whether accessibility mode is on.
    #[cfg(test)]
    pub(crate) const fn accessible(&self) -> bool {
        self.accessible
    }

    /// Note what screen is about to be drawn (`identity`: a hash of which screen it is). A
    /// different one than last time starts a wipe, when motion is allowed. The first screen of
    /// a session does not wipe.
    pub(crate) fn note_screen(&mut self, identity: u64, look: Look, now: Instant) {
        let changed = self.shown.is_some_and(|before| before != identity);
        self.shown = Some(identity);
        if changed && allowed(look, self.accessible) {
            self.wipe = Some((now, 0));
        } else if !allowed(look, self.accessible) {
            self.wipe = None;
        }
    }

    /// Tell the state whether an input line is on screen.
    pub(crate) fn set_input(&mut self, input: bool, now: Instant) {
        if input && !self.input {
            self.epoch = Some(now);
        }
        self.input = input;
    }

    /// How much of the screen the running wipe shows, in `0..=STEPS`, or `None` when there is
    /// no wipe (or it is over). Calling this counts a frame.
    pub(crate) fn wipe_step(&mut self, now: Instant) -> Option<u8> {
        let (start, drawn) = self.wipe?;
        let due = u8::try_from(
            (now.saturating_duration_since(start).as_millis() / STEP.as_millis())
                .min(u128::from(STEPS)),
        )
        .unwrap_or(STEPS);
        let step = due.max(drawn).min(STEPS);
        if step >= STEPS {
            self.wipe = None;
            return None;
        }
        self.wipe = Some((start, step));
        Some(step)
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

    /// When the shell should next wake to draw motion, if anything is moving: the next wipe
    /// frame, or the next blink edge. `None` when motion is off or nothing is moving, which is
    /// the common case (and the only one in accessibility mode).
    pub(crate) fn deadline(&self, look: Look, now: Instant) -> Option<Instant> {
        if !allowed(look, self.accessible) {
            return None;
        }
        if let Some((start, drawn)) = self.wipe {
            return Some((start + STEP * u32::from(drawn.saturating_add(1))).max(now));
        }
        if self.input {
            let epoch = self.epoch?;
            let beats = now.saturating_duration_since(epoch).as_millis() / BLINK.as_millis() + 1;
            let next = epoch + BLINK * u32::try_from(beats).unwrap_or(u32::MAX);
            return Some(next);
        }
        None
    }

    /// Whether a wipe is running (a frame is wanted on the next deadline).
    #[cfg(test)]
    pub(crate) const fn wiping(&self) -> bool {
        self.wipe.is_some()
    }
}

/// Cover the part of the body the wipe has not reached yet: the columns right of the leading
/// edge are blanked, and the edge itself is a shaded column. `step` is `0..STEPS`.
/// Rows `1..height-1` only: the header row and the last row (status bar, marker cell) are never
/// touched, so a wipe cannot cover a marker.
pub(crate) fn wipe(frame: &mut Frame<'_>, look: Look, step: u8) {
    let area = frame.area();
    if area.height < 3 || area.width < 2 {
        return;
    }
    let edge = if look.ascii() { ":" } else { "\u{2592}" };
    let shown = u32::from(step) * u32::from(area.width) / u32::from(STEPS);
    let shown = u16::try_from(shown).unwrap_or(area.width);
    let body = Rect {
        y: area.y + 1,
        height: area.height - 2,
        ..area
    };
    wipe_buffer(frame.buffer_mut(), body, shown, edge, look.muted());
}

fn wipe_buffer(buffer: &mut Buffer, body: Rect, shown: u16, edge: &str, edge_style: Style) {
    for y in body.y..body.y + body.height {
        for x in body.x.saturating_add(shown)..body.x + body.width {
            let Some(cell) = buffer.cell_mut((x, y)) else {
                continue;
            };
            if x == body.x + shown {
                cell.reset();
                cell.set_symbol(edge).set_style(edge_style);
            } else {
                cell.reset();
            }
        }
    }
}
