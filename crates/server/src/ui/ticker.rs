//! The **live header ticker** (UI-9): a line in the header bar with the sysop's words and,
//! if the sysop allows it, the newest headlines of the feed boards the member may read.
//!
//! - **Budget.** The ticker is one row of the header, diffed like everything else, so a frame
//!   that moves it sends only the cells that changed. With motion on it advances one step per
//!   [`STEP`] (once a second, far under the D49 redraw cap, and it shares the shell's cycle, so
//!   it can never raise the rate); with motion off it is **static**: the first line, shown
//!   whole, changing only when the data does, and it asks for no timer-driven redraws at all.
//! - **Chrome position only.** It is drawn by the frame, in the header bar, never in the status
//!   bar and never beside the marker cell. The frame takes it as [`TickerText`], which can be
//!   built only from the board's checked lines ([`BoardText`](super::board::BoardText)) or
//!   from headlines that have been through the shared sanitizer; plain text cannot be passed.
//! - **Honest.** Headlines are content (feed items): sanitized, one line, length-capped, and
//!   drawn plain. They are not styled as anything the board says itself.

use std::time::{Duration, Instant};

use russh_bbs_core::sanitize::SingleLine;

use super::board::{Board, BoardText};

/// How often the marquee moves one cell.
pub(crate) const STEP: Duration = Duration::from_secs(1);
/// How often feed headlines are fetched again.
pub(crate) const REFRESH: Duration = Duration::from_secs(300);
/// Most headlines kept.
pub(crate) const MAX_HEADLINES: usize = 8;
/// Longest headline kept, in characters.
const HEADLINE_MAX: usize = 80;
/// The gap between two items in the marquee.
const SEPARATOR: &str = "  \u{00b7}  ";
const SEPARATOR_ASCII: &str = "  *  ";

/// Text the frame may draw in the ticker slot: the board's own checked words, or content that
/// was sanitized on the way in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TickerText(String);

impl TickerText {
    /// The text.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// One session's ticker.
#[derive(Debug, Clone)]
pub(crate) struct Ticker {
    sysop: Vec<String>,
    headlines: Vec<String>,
    feeds: bool,
    /// Whether the member and the board allow motion.
    moving: bool,
    started: Instant,
    fetched: Option<Instant>,
}

impl Ticker {
    /// A ticker for `board`, started at `now`. `moving` is the member's motion allowance.
    pub(crate) fn new(board: &Board, moving: bool, now: Instant) -> Self {
        Self {
            sysop: board
                .ticker()
                .iter()
                .map(BoardText::as_str)
                .map(str::to_owned)
                .collect(),
            headlines: Vec::new(),
            feeds: board.ticker_feeds(),
            moving,
            started: now,
            fetched: None,
        }
    }

    /// Whether there is anything to show beyond the clock.
    pub(crate) fn has_text(&self) -> bool {
        !self.sysop.is_empty() || !self.headlines.is_empty()
    }

    /// Whether the sysop wants feed headlines in it.
    #[cfg(test)]
    pub(crate) fn wants_headlines(&self) -> bool {
        self.feeds
    }

    /// Whether the headlines are due to be fetched again.
    pub(crate) fn headlines_due(&self, now: Instant) -> bool {
        self.feeds
            && self
                .fetched
                .is_none_or(|at| now.duration_since(at) >= REFRESH)
    }

    /// Replace the headlines (feed item subjects the member may read). Each is sanitized here.
    pub(crate) fn set_headlines<'a>(
        &mut self,
        subjects: impl IntoIterator<Item = &'a str>,
        now: Instant,
    ) {
        self.fetched = Some(now);
        self.headlines = subjects
            .into_iter()
            .map(|subject| {
                SingleLine::new(HEADLINE_MAX)
                    .sanitize(subject)
                    .into_string()
            })
            .filter(|subject| !subject.trim().is_empty())
            .take(MAX_HEADLINES)
            .collect();
    }

    /// Update the motion allowance (the member can change it while connected).
    pub(crate) fn set_moving(&mut self, moving: bool) {
        self.moving = moving;
    }

    /// When the ticker next needs a frame, if it is moving.
    pub(crate) fn next_wake(&self, now: Instant) -> Option<Instant> {
        if !(self.moving && self.has_text()) {
            return None;
        }
        let ticks = now.duration_since(self.started).as_secs() + 1;
        Some(self.started + STEP * u32::try_from(ticks).unwrap_or(u32::MAX))
    }

    /// The step the marquee is on at `now` (always 0 when it is not moving).
    pub(crate) fn step(&self, now: Instant) -> usize {
        if self.moving {
            usize::try_from(now.duration_since(self.started).as_secs()).unwrap_or(0)
        } else {
            0
        }
    }

    /// What the header can draw at `now`, or `None` when there is nothing to show. Static
    /// (motion off): the first item. Moving: the items joined in a loop, rotated one cell per
    /// step. The frame asks the view for as many columns as the header has room for.
    pub(crate) fn view(&self, now: Instant, ascii: bool) -> Option<TickerView> {
        if !self.has_text() {
            return None;
        }
        let items: Vec<&str> = self
            .sysop
            .iter()
            .chain(self.headlines.iter())
            .map(String::as_str)
            .collect();
        let separator = if ascii { SEPARATOR_ASCII } else { SEPARATOR };
        let (whole, step) = if self.moving {
            (items.join(separator), self.step(now))
        } else {
            (items.first().copied().unwrap_or("").to_owned(), 0)
        };
        let mut chars: Vec<char> = whole.chars().collect();
        chars.extend(separator.chars());
        Some(TickerView {
            chars,
            whole,
            step,
            moving: self.moving,
            ascii,
        })
    }
}

/// What the header draws: a loop of characters and where in it the window starts. The frame
/// reads it through [`current`]; it holds only text that passed the checks above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TickerView {
    /// The items joined and a trailing gap, as a loop.
    chars: Vec<char>,
    /// The items joined, for when they fit without scrolling.
    whole: String,
    step: usize,
    moving: bool,
    ascii: bool,
}

impl TickerView {
    /// The text for a slot `width` columns wide.
    pub(crate) fn window(&self, width: usize) -> TickerText {
        if width == 0 || self.chars.is_empty() {
            return TickerText(String::new());
        }
        let len = self.chars.len();
        if !self.moving || self.whole.chars().count() <= width {
            return TickerText(fit(&self.whole, width, self.ascii));
        }
        let start = self.step % len;
        let shown: String = (0..width)
            .filter_map(|at| self.chars.get((start + at) % len).copied())
            .collect();
        TickerText(shown)
    }
}

thread_local! {
    static CURRENT: std::cell::RefCell<Option<TickerView>> = const { std::cell::RefCell::new(None) };
}

/// Run `draw` with `view` as the header's ticker. A thread-local, set and cleared inside one
/// synchronous draw, so the ticker reaches the header without a new field on every screen's
/// draw context (each screen builds its own frame), and nothing is shared between sessions.
pub(crate) fn scope<R>(view: Option<TickerView>, draw: impl FnOnce() -> R) -> R {
    let previous = CURRENT.with(|current| current.replace(view));
    let result = draw();
    CURRENT.with(|current| current.replace(previous));
    result
}

/// The ticker the header should draw now, if any.
pub(crate) fn current() -> Option<TickerView> {
    CURRENT.with(|current| current.borrow().clone())
}

/// `text` cut to `width` columns, with an ellipsis (or `~` in the ASCII tier) when cut.
fn fit(text: &str, width: usize, ascii: bool) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push(if ascii { '~' } else { '\u{2026}' });
    out
}

#[cfg(test)]
mod tests;
