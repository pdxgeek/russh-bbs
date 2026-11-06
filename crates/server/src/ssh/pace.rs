//! Delivery pacing for effect frames (FIX-M35-3, QA M3.5 M3; builds D124).
//!
//! A write that returns says only that the bytes reached the server's socket buffer or the
//! network's queue, not that the client has them. On a slow link the whole animated intro is
//! accepted at once, and a key press then waits behind everything already written. So effect
//! frames are paced by an acknowledgement from the client's terminal itself: every effect frame
//! ends with a cursor-position request ([`PROBE`]), and the terminal's reply arrives only after
//! it has taken every byte before it, in order. The [`Pacer`] counts the bytes sent since the
//! last reply and:
//!
//! - holds the next effect frame back while [`WINDOW`] bytes or more are unacknowledged (the
//!   frame is dropped, as a late tick already drops frames; the effect is keyed on the clock), so
//!   at most about a window plus one frame is ever waiting in front of a key press; and
//! - gives up on motion for the session when the oldest probe has gone [`DEADLINE`] without a
//!   reply, which is what a link that cannot carry the effect (or a terminal that never answers)
//!   looks like.
//!
//! Only effect frames are probed. Static frames are never held back, and the static screen
//! that ends an effect is always sent.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Written after an effect frame: park the cursor on the last row and ask where it is. The
/// terminal's answer is `ESC [ row ; col R` with a row of at least 2, which the key parser reads
/// as an acknowledgement and never as a key. The cursor is hidden while an effect runs, and
/// every draw moves it explicitly, so parking it changes nothing visible.
pub(crate) const PROBE: &[u8] = b"\x1b[999;1H\x1b[6n";

/// Unacknowledged effect bytes at which the next effect frame is held back. One frame may still
/// go out below this, so the most that waits in front of a key is a window plus a frame.
pub(crate) const WINDOW: usize = 4 * 1024;

/// How long the oldest probe may go unanswered before motion is given up for the session. A
/// 56 kbit link takes about 0.9 s to carry the largest intro frame, so this is not triggered by
/// it, while a link slower than about 25 kbit gives up on the first frame.
pub(crate) const DEADLINE: Duration = Duration::from_millis(1500);

/// Most probes remembered. Past this the oldest is forgotten (it counts as answered), which
/// cannot happen while [`WINDOW`] holds: an effect frame is at least a few bytes, so the window
/// closes long before this many frames are in flight, and the cap only bounds a client that sends
/// replies it was never asked for.
const MAX_PROBES: usize = 64;

/// What the pacer says about the next effect frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Draw and send it.
    Open,
    /// The client is behind: skip this frame (the effect carries on from the clock).
    Held,
    /// The client is too slow for motion: end it and draw the static screen.
    TooSlow,
}

/// Effect bytes sent and not yet acknowledged.
#[derive(Debug, Default)]
pub(crate) struct Pacer {
    /// One entry per probe sent and not answered: when, and how many bytes came with it.
    probes: VecDeque<(Instant, usize)>,
    /// The sum of the bytes in `probes`.
    in_flight: usize,
}

impl Pacer {
    /// May the next effect frame go out at `now`?
    pub(crate) fn gate(&self, now: Instant) -> Gate {
        if let Some((sent, _)) = self.probes.front()
            && now.saturating_duration_since(*sent) >= DEADLINE
        {
            return Gate::TooSlow;
        }
        if self.in_flight >= WINDOW {
            Gate::Held
        } else {
            Gate::Open
        }
    }

    /// An effect frame of `len` bytes (probe included) was handed to the connection at `now`.
    pub(crate) fn sent(&mut self, now: Instant, len: usize) {
        if self.probes.len() >= MAX_PROBES
            && let Some((_, bytes)) = self.probes.pop_front()
        {
            self.in_flight = self.in_flight.saturating_sub(bytes);
        }
        self.probes.push_back((now, len));
        self.in_flight = self.in_flight.saturating_add(len);
    }

    /// The client answered `count` probes. Replies nobody asked for are ignored.
    pub(crate) fn acked(&mut self, count: u32) {
        for _ in 0..count {
            let Some((_, bytes)) = self.probes.pop_front() else {
                return;
            };
            self.in_flight = self.in_flight.saturating_sub(bytes);
        }
    }

    /// Forget everything (motion was given up; nothing is waited for any more).
    pub(crate) fn clear(&mut self) {
        self.probes.clear();
        self.in_flight = 0;
    }

    /// Bytes sent for effects and not yet acknowledged (tests and logs).
    #[cfg(test)]
    pub(crate) fn in_flight(&self) -> usize {
        self.in_flight
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64, base: Instant) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn a_fresh_pacer_is_open() {
        assert_eq!(Pacer::default().gate(Instant::now()), Gate::Open);
    }

    #[test]
    fn frames_are_held_once_a_window_is_unacknowledged_and_released_by_a_reply() {
        let t0 = Instant::now();
        let mut pacer = Pacer::default();
        pacer.sent(at(0, t0), 3000);
        assert_eq!(pacer.gate(at(80, t0)), Gate::Open, "under the window");
        pacer.sent(at(80, t0), 3000);
        assert_eq!(pacer.gate(at(160, t0)), Gate::Held);
        assert_eq!(pacer.in_flight(), 6000);
        pacer.acked(1);
        assert_eq!(pacer.in_flight(), 3000);
        assert_eq!(pacer.gate(at(200, t0)), Gate::Open);
    }

    #[test]
    fn an_unanswered_probe_gives_up_at_the_deadline_and_not_before() {
        let t0 = Instant::now();
        let mut pacer = Pacer::default();
        pacer.sent(t0, 100);
        let before = t0 + DEADLINE.saturating_sub(Duration::from_millis(1));
        assert_eq!(pacer.gate(before), Gate::Open);
        assert_eq!(pacer.gate(t0 + DEADLINE), Gate::TooSlow);
        pacer.acked(1);
        assert_eq!(pacer.gate(t0 + DEADLINE * 2), Gate::Open);
    }

    #[test]
    fn replies_nobody_asked_for_change_nothing() {
        let mut pacer = Pacer::default();
        pacer.acked(1000);
        assert_eq!(pacer.in_flight(), 0);
        assert_eq!(pacer.gate(Instant::now()), Gate::Open);
    }

    /// H-2: a client can forge any number of replies; the count is attacker-controlled
    /// (`u32`), the work is bounded by the probes actually sent, and nothing underflows.
    #[test]
    fn a_forged_flood_of_replies_is_bounded_and_only_opens_the_gate() {
        let t0 = Instant::now();
        let mut pacer = Pacer::default();
        for _ in 0..MAX_PROBES {
            pacer.sent(t0, 4096);
        }
        assert_eq!(pacer.gate(t0), Gate::Held);
        pacer.acked(u32::MAX);
        assert_eq!(pacer.in_flight(), 0);
        assert_eq!(pacer.gate(t0), Gate::Open);
        pacer.acked(u32::MAX);
        assert_eq!(pacer.in_flight(), 0);
    }

    #[test]
    fn the_probe_list_is_bounded() {
        let t0 = Instant::now();
        let mut pacer = Pacer::default();
        for _ in 0..(MAX_PROBES * 3) {
            pacer.sent(t0, 1);
        }
        assert!(pacer.in_flight() <= MAX_PROBES);
        pacer.clear();
        assert_eq!(pacer.in_flight(), 0);
    }

    #[test]
    fn the_probe_is_a_cursor_report_request_on_the_last_row() {
        assert!(PROBE.ends_with(b"\x1b[6n"));
        assert!(PROBE.starts_with(b"\x1b[999;1H"));
    }
}
