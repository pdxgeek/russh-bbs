//! **Toasts** (UI-9): short notices that appear over the body for a few seconds (new mail, a
//! page, a door event) and go away by themselves.
//!
//! - **Rate limited.** At most [`MAX_SHOWN`] show at once, [`MAX_QUEUED`] wait, a repeat of a
//!   notice already showing or waiting is folded into it, and no more than
//!   [`MAX_PER_MINUTE`] are accepted per minute, so a chatty source cannot turn the screen
//!   into a wall of notices or force redraws (each change is one frame, inside the D49 cap).
//! - **Never over chrome.** They are drawn in the area the caller passes, which is the body with
//!   the header row and the status row (the marker cell lives in it) taken out; [`place`]
//!   clamps every toast to that area, and a test checks that none can reach those rows.
//! - **Sanitized.** A toast's words come from the string table ([`Text::Key`]) or from
//!   somebody else ([`Text::Member`]), which goes through the shared sanitizer, one line, and a
//!   length cap. The kind is a word first (`Mail:`), never colour alone, so a toast reads the
//!   same with no colour and to a screen reader ([`Toast::announce`] is that text form).
//! - **Not a marker.** A toast never uses the reserved style (bold plus reverse).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use russh_bbs_core::sanitize::SingleLine;

use super::strings::StringTable;
use crate::settings::Look;

/// Toasts on screen at once.
pub(crate) const MAX_SHOWN: usize = 3;
/// Toasts waiting for room.
pub(crate) const MAX_QUEUED: usize = 8;
/// Toasts accepted in any one minute.
pub(crate) const MAX_PER_MINUTE: usize = 6;
/// How long a toast stays.
pub(crate) const LIFETIME: Duration = Duration::from_secs(5);
/// Widest a toast is drawn, in columns.
const MAX_WIDTH: u16 = 48;
/// Longest text kept, in characters.
const MAX_TEXT: usize = 120;

/// What a toast is about. The word comes from the string table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Notice,
    Mail,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the page toast: its source (live chat pages) arrives with UI-4's split chat pane"
        )
    )]
    Page,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the door-event toast: doors report events through UI-5's API when it carries them"
        )
    )]
    Door,
}

impl Kind {
    const fn key(self) -> &'static str {
        match self {
            Self::Notice => "toast.notice",
            Self::Mail => "toast.mail",
            Self::Page => "toast.page",
            Self::Door => "toast.door",
        }
    }
}

/// The string-table keys toasts use.
pub(crate) const TEXT_KEYS: [&str; 10] = [
    "toast.notice",
    "toast.mail",
    "toast.page",
    "toast.door",
    "toast.mouse_on",
    "toast.mouse_off",
    "toast.new_mail",
    "toast.dock_open",
    "toast.dock_narrow",
    "toast.ascii_only",
];

/// The words of a toast. A toast cannot be made from a bare `String`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Text<'a> {
    /// A string-table key, resolved here: chrome is only ever words the table holds.
    Key(&'a str),
    /// A string-table key whose `{count}` is filled in.
    Count(&'a str, u64),
    /// Words from a member or the board's content: sanitized on the way in.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "used by the page and door toasts, once those sources exist"
        )
    )]
    Member(&'a str),
}

/// One toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Toast {
    kind: Kind,
    word: String,
    text: String,
}

impl Toast {
    /// The plain text form: the kind word, a colon, the words. What a screen reader or a
    /// colourless terminal gets.
    pub(crate) fn announce(&self) -> String {
        format!("{}: {}", self.word, self.text)
    }
}

/// The toasts of one session.
#[derive(Debug, Default)]
pub(crate) struct Toasts {
    shown: Vec<(Toast, Instant)>,
    queued: VecDeque<Toast>,
    /// When each recently accepted toast arrived, for the per-minute limit.
    accepted: VecDeque<Instant>,
    /// A toast came or went since the last [`Toasts::take_changed`].
    changed: bool,
}

impl Toasts {
    /// Offer a toast. `false` when it was dropped: over the rate limit, or a repeat.
    pub(crate) fn push(
        &mut self,
        kind: Kind,
        text: Text<'_>,
        strings: &StringTable,
        now: Instant,
    ) -> bool {
        // Table words are chrome; anything else is somebody's. Both are cleaned: one rule for
        // whatever is drawn here.
        let words = match text {
            Text::Key(key) => strings.text(key).to_owned(),
            Text::Count(key, count) => strings.filled(key, &[("count", &count.to_string())]),
            Text::Member(words) => words.to_owned(),
        };
        let words = SingleLine::new(MAX_TEXT).sanitize(&words).into_string();
        if words.trim().is_empty() {
            return false;
        }
        let toast = Toast {
            kind,
            word: strings.text(kind.key()).to_owned(),
            text: words,
        };
        // A repeat of a notice still showing or waiting changes nothing.
        if self.shown.iter().any(|(shown, _)| *shown == toast) || self.queued.contains(&toast) {
            return false;
        }
        while self
            .accepted
            .front()
            .is_some_and(|at| now.duration_since(*at) >= Duration::from_secs(60))
        {
            self.accepted.pop_front();
        }
        if self.accepted.len() >= MAX_PER_MINUTE {
            return false;
        }
        self.accepted.push_back(now);
        if self.queued.len() >= MAX_QUEUED {
            self.queued.pop_front();
        }
        self.queued.push_back(toast);
        self.promote(now);
        self.changed = true;
        true
    }

    fn promote(&mut self, now: Instant) {
        while self.shown.len() < MAX_SHOWN {
            let Some(toast) = self.queued.pop_front() else {
                break;
            };
            self.shown.push((toast, now + LIFETIME));
        }
    }

    /// Retire toasts whose time is up and bring waiting ones in. `true` when the screen must be
    /// redrawn.
    pub(crate) fn tick(&mut self, now: Instant) -> bool {
        let before = self.shown.len();
        self.shown.retain(|(_, until)| *until > now);
        let expired = self.shown.len() != before;
        let queued_before = self.queued.len();
        self.promote(now);
        let promoted = self.queued.len() != queued_before;
        let changed = std::mem::take(&mut self.changed);
        expired || promoted || changed
    }

    /// When the next toast expires, if any is showing.
    pub(crate) fn next_wake(&self) -> Option<Instant> {
        self.shown.iter().map(|(_, until)| *until).min()
    }

    /// Whether anything is showing.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    /// What is showing, as plain text, newest last.
    #[cfg(test)]
    pub(crate) fn announcements(&self) -> Vec<String> {
        self.shown
            .iter()
            .map(|(toast, _)| toast.announce())
            .collect()
    }

    /// Where each showing toast is drawn inside `area`: stacked up from its bottom-right
    /// corner, one row each, clamped so none leaves `area`.
    pub(crate) fn place(&self, area: Rect) -> Vec<Rect> {
        let width = area.width.min(MAX_WIDTH);
        if width < 12 || area.height == 0 {
            return Vec::new();
        }
        let mut rects = Vec::new();
        for (index, _) in self.shown.iter().enumerate() {
            let Ok(up) = u16::try_from(index) else {
                break;
            };
            if up >= area.height {
                break;
            }
            rects.push(Rect {
                x: area.x + area.width - width,
                y: area.y + area.height - 1 - up,
                width,
                height: 1,
            });
        }
        rects
    }

    /// Draw the showing toasts in `area`.
    pub(crate) fn render(&self, frame: &mut Frame<'_>, area: Rect, look: Look) {
        for ((toast, _), rect) in self.shown.iter().zip(self.place(area)) {
            let tone = match toast.kind {
                Kind::Notice => look.palette().info,
                Kind::Mail => look.palette().success,
                Kind::Page => look.palette().warning,
                Kind::Door => look.palette().accent,
            };
            let width = usize::from(rect.width);
            let mut line = toast.announce();
            if line.chars().count() > width.saturating_sub(2) {
                let keep = width.saturating_sub(3);
                line = line.chars().take(keep).collect::<String>();
                line.push(if look.ascii() { '~' } else { '\u{2026}' });
            }
            frame.render_widget(Clear, rect);
            let style = look
                .accent(Style::new().add_modifier(Modifier::BOLD), tone)
                .patch(look.bg(look.surfaces().popup_bg));
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(format!(" {line} "), style))),
                rect,
            );
        }
    }
}

#[cfg(test)]
mod tests;
