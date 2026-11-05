//! The effect engine (UI-8): fades, wipes, a typewriter reveal, glow and gradients, and the
//! animated login intro, timed on the session's own ticker and paid for out of a per-session
//! frame budget.
//!
//! **The rules every effect keeps** (each has a test):
//!
//! * **Static is the truth.** A screen draws exactly as it always does; an effect is a rewrite
//!   of the finished frame ([`Overlay`]). At the end of an effect, and whenever it is not
//!   running, nothing rewrites the frame, so the final frame is the static screen.
//! * **Motion off is one static frame.** [`allowed`] is the single gate: the member's setting,
//!   the board's and the style's ceilings (all folded into `Look::motion()`), and the plain
//!   accessible path (always off). When it says no, [`Fx::start`] does nothing, no timer is
//!   armed and no extra frame is ever drawn.
//! * **Input is never delayed.** [`Fx::finish`] ends the effect at once; the screen calls it on
//!   any key and on a resize, and the key is handled as usual.
//! * **Bounded cost.** An effect draws at most [`Budget::frames_per_effect`] frames, no more
//!   than [`Budget::frames_per_window`] in any [`Budget::window`], and its frame interval is
//!   [`Budget::interval`]. The shell still redraws no faster than the D49 cap
//!   (`redraw_interval`, 50 ms), because an effect frame is an ordinary redraw cycle; the
//!   interval here is longer than that cap, so effects use at most about 12 frames a second.
//!   An effect also has a byte budget ([`Budget::bytes_per_effect`]): it ends early rather than
//!   queue more than that on a slow link.
//!   Frames are keyed on wall-clock progress, so a slow cycle drops frames; it never makes the
//!   effect last longer.
//! * **The status row is never touched.** See [`effects::Stage`].
//!
//! **Why hand-written and not `tachyonfx` (D47).** The effects here must know the colour-depth
//! model (D109), leave the last row and any reserved-style cell alone, and end on exactly the
//! static frame; a general effects crate knows none of that, and each of these effects is a few
//! lines. No new dependency for effects.

pub(crate) mod effects;
mod font;
pub(crate) mod intro;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ratatui::Frame;

use crate::settings::Look;
use crate::ui::canvas::Rgb;

/// The effects the screens use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The animated login intro.
    Intro,
    /// The short wipe between menus.
    Wipe,
    /// A typewriter reveal of a whole screen (for screens UI-4 moves onto the engine).
    #[allow(dead_code, reason = "used by UI-4's screens; tested here")]
    Reveal,
}

impl Kind {
    /// How long the effect runs.
    pub(crate) const fn duration(self) -> Duration {
        match self {
            Self::Intro => Duration::from_millis(1800),
            Self::Wipe => Duration::from_millis(240),
            Self::Reveal => Duration::from_millis(700),
        }
    }
}

/// What a session may spend on effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Budget {
    /// The time between effect frames.
    pub interval: Duration,
    /// The most frames one effect may draw.
    pub frames_per_effect: u16,
    /// The most effect frames in any [`Budget::window`].
    pub frames_per_window: u16,
    /// The span the window budget is counted over.
    pub window: Duration,
    /// The most bytes one effect may send. An effect that reaches it ends and the screen is
    /// drawn static, so a slow link is never left with a long queue in front of the next key.
    pub bytes_per_effect: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            interval: Duration::from_millis(80),
            frames_per_effect: 28,
            frames_per_window: 96,
            window: Duration::from_secs(60),
            bytes_per_effect: 160 * 1024,
        }
    }
}

/// Whether motion may run at all for this look. `accessible` is the plain accessibility path
/// (M5): always off, whatever else is set.
pub(crate) fn allowed(look: Look, accessible: bool) -> bool {
    look.motion() && !accessible
}

#[derive(Debug, Clone, Copy)]
struct Running {
    kind: Kind,
    started: Instant,
    due: Instant,
    frames: u16,
    bytes: usize,
}

/// One session's effect state.
#[derive(Debug, Clone)]
pub(crate) struct Fx {
    budget: Budget,
    running: Option<Running>,
    /// Effects are off for the rest of the session (a slow link).
    disabled: bool,
    /// When recent effect frames were drawn, for the window budget.
    spent: VecDeque<Instant>,
}

impl Default for Fx {
    fn default() -> Self {
        Self::new(Budget::default())
    }
}

impl Fx {
    /// An idle engine with `budget`.
    pub(crate) fn new(budget: Budget) -> Self {
        Self {
            budget,
            running: None,
            disabled: false,
            spent: VecDeque::new(),
        }
    }

    fn forget_old(&mut self, now: Instant) {
        while self
            .spent
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) > self.budget.window)
        {
            self.spent.pop_front();
        }
    }

    /// Start `kind` at `now` if motion is `allowed` and the budget has room for all of its
    /// frames. Returns whether it started; when it did not, the screen is simply static.
    pub(crate) fn start(&mut self, kind: Kind, now: Instant, allowed: bool) -> bool {
        self.running = None;
        if !allowed || self.disabled {
            return false;
        }
        self.forget_old(now);
        let frames = Self::planned_frames(&self.budget, kind);
        if usize::from(self.budget.frames_per_window) < self.spent.len() + usize::from(frames) {
            return false;
        }
        self.running = Some(Running {
            kind,
            started: now,
            due: now + self.budget.interval,
            frames: 0,
            bytes: 0,
        });
        true
    }

    fn planned_frames(budget: &Budget, kind: Kind) -> u16 {
        let n = kind.duration().as_millis() / budget.interval.as_millis().max(1) + 1;
        u16::try_from(n)
            .unwrap_or(u16::MAX)
            .min(budget.frames_per_effect)
    }

    /// Ask to spend `frames` frames on something the engine does not draw itself (the typed
    /// farewell, which prints on the main screen). `true` when motion is `allowed`, effects are
    /// not switched off and the window budget has room; the frames are then counted.
    pub(crate) fn reserve(&mut self, frames: u16, now: Instant, allowed: bool) -> bool {
        if !allowed || self.disabled {
            return false;
        }
        self.forget_old(now);
        if usize::from(self.budget.frames_per_window) < self.spent.len() + usize::from(frames) {
            return false;
        }
        self.spent
            .extend(std::iter::repeat_n(now, usize::from(frames)));
        true
    }

    /// The time between frames, for work paced on the same ticker.
    pub(crate) const fn interval(&self) -> Duration {
        self.budget.interval
    }

    /// Turn effects off for the rest of the session and end any running one (the client's link
    /// is slow: the shell calls this when a frame took too long to hand over).
    pub(crate) fn disable(&mut self) {
        self.disabled = true;
        self.running = None;
    }

    /// End the running effect now (a key, a resize). Returns whether one was running, so the
    /// screen knows to draw the static frame.
    pub(crate) fn finish(&mut self) -> bool {
        self.running.take().is_some()
    }

    /// Which effect is running (tests read this).
    #[cfg(test)]
    pub(crate) fn running_kind(&self) -> Option<Kind> {
        self.running.map(|r| r.kind)
    }

    /// When the next effect frame is due; `None` when idle (so no timer is armed).
    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.running.map(|r| r.due)
    }

    /// Advance the clock to `now`. Returns whether a redraw is due: a frame of the running
    /// effect, or the final static frame when it has just ended.
    pub(crate) fn tick(&mut self, now: Instant) -> bool {
        let Some(running) = &mut self.running else {
            return false;
        };
        if now < running.due {
            return false;
        }
        if now.saturating_duration_since(running.started) >= running.kind.duration() {
            self.running = None;
            return true;
        }
        // Frames are keyed on time, so a late tick skips ahead rather than catching up.
        running.due = now + self.budget.interval;
        true
    }

    /// Count `bytes` just sent for the running effect. Returns `true` when that used up its
    /// byte budget and the effect ended, so the screen must draw the static frame next.
    pub(crate) fn charge(&mut self, bytes: usize) -> bool {
        let Some(running) = &mut self.running else {
            return false;
        };
        running.bytes = running.bytes.saturating_add(bytes);
        if running.bytes > self.budget.bytes_per_effect {
            self.running = None;
            return true;
        }
        false
    }

    /// The effect to draw into the frame being built at `now`, with its progress (0 to 1,
    /// below 1 while running). Counts the frame against the budget; ends the effect when the
    /// frame cap is reached. `None` means draw the static screen.
    pub(crate) fn frame(&mut self, now: Instant) -> Option<(Kind, f32)> {
        let running = self.running.as_mut()?;
        let elapsed = now.saturating_duration_since(running.started);
        let total = running.kind.duration();
        if elapsed >= total || running.frames >= self.budget.frames_per_effect {
            self.running = None;
            return None;
        }
        running.frames += 1;
        let kind = running.kind;
        self.spent.push_back(now);
        Some((kind, elapsed.as_secs_f32() / total.as_secs_f32()))
    }
}

/// An effect to apply to the frame being built.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Overlay<'a> {
    /// Which effect.
    pub kind: Kind,
    /// Its progress, 0 up to (not including) 1.
    pub t: f32,
    /// How the screen is drawn.
    pub look: Look,
    /// The board's name, for the intro's big letters.
    pub title: &'a str,
}

impl Overlay<'_> {
    /// Rewrite the finished frame. Call last in the draw closure.
    pub(crate) fn apply(self, frame: &mut Frame<'_>) {
        let stage = effects::Stage::new(frame.area(), self.look);
        let buf = frame.buffer_mut();
        match self.kind {
            Kind::Intro => {
                let palette = self.look.palette();
                let rgb =
                    |tint: crate::ui::depth::Tint, fallback: Rgb| tint.rgb().unwrap_or(fallback);
                let colors = intro::Colors {
                    surface: rgb(palette.surface, (8, 10, 20)),
                    accent: rgb(palette.accent, (90, 200, 255)),
                    accent2: rgb(palette.accent2, (230, 90, 220)),
                };
                intro::paint(buf, &stage, self.t, self.title, colors);
            }
            Kind::Wipe => effects::wipe(buf, &stage, self.t),
            Kind::Reveal => effects::typewriter(buf, &stage, self.t),
        }
    }
}

#[cfg(test)]
mod tests;
