//! The bounded, sanitized body of a message or draft.

use crate::sanitize::{MultiLine, Sanitized};

/// Most characters any body may hold, whatever a caller asks for. The store enforces the same
/// number, so a bug in a caller cannot fill the database.
pub const MAX_BODY_CHARS: usize = 16_000;
/// Most lines (separated by `\n`) any body may hold.
pub const MAX_BODY_LINES: usize = 400;

/// How big a body may be: a caller's own bounds, never above [`MAX_BODY_CHARS`] and
/// [`MAX_BODY_LINES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyLimits {
    max_chars: usize,
    max_lines: usize,
}

impl BodyLimits {
    /// The largest body the board accepts.
    pub const MAX: Self = Self {
        max_chars: MAX_BODY_CHARS,
        max_lines: MAX_BODY_LINES,
    };

    /// Limits of `max_chars` characters and `max_lines` lines, each kept within `1` and the
    /// board-wide ceiling (so a caller can narrow the bounds, never widen them).
    #[must_use]
    pub fn new(max_chars: usize, max_lines: usize) -> Self {
        Self {
            max_chars: max_chars.clamp(1, MAX_BODY_CHARS),
            max_lines: max_lines.clamp(1, MAX_BODY_LINES),
        }
    }

    /// Most characters.
    #[must_use]
    pub const fn max_chars(self) -> usize {
        self.max_chars
    }

    /// Most lines.
    #[must_use]
    pub const fn max_lines(self) -> usize {
        self.max_lines
    }
}

impl Default for BodyLimits {
    fn default() -> Self {
        Self::MAX
    }
}

/// Why a text is not an acceptable body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BodyError {
    /// More characters than the limit, after sanitizing.
    #[error("the text is longer than {max} characters")]
    TooLong {
        /// The limit.
        max: usize,
    },
    /// More lines than the limit, after sanitizing.
    #[error("the text is longer than {max} lines")]
    TooManyLines {
        /// The limit.
        max: usize,
    },
}

/// A body that has passed the shared multi-line sanitizer and is within its limits. It cannot
/// be built any other way, so a [`DraftStore`](super::DraftStore) can only ever be handed
/// sanitized, bounded text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body(Sanitized);

impl Body {
    /// Sanitize `raw` with the shared multi-line sanitizer and check it against `limits`.
    ///
    /// Over-long text is refused, not cut: a draft or post that silently lost its end would be
    /// worse than one that is refused. (The sanitizer is asked for one character more than the
    /// limit so that "exactly at the limit" and "over it" can be told apart.)
    ///
    /// # Errors
    /// [`BodyError::TooLong`] or [`BodyError::TooManyLines`].
    pub fn prepare(limits: BodyLimits, raw: &str) -> Result<Self, BodyError> {
        let clean = MultiLine::new(limits.max_chars.saturating_add(1)).sanitize(raw);
        if clean.as_str().chars().count() > limits.max_chars {
            return Err(BodyError::TooLong {
                max: limits.max_chars,
            });
        }
        if line_count(clean.as_str()) > limits.max_lines {
            return Err(BodyError::TooManyLines {
                max: limits.max_lines,
            });
        }
        Ok(Self(clean))
    }

    /// Wrap text the sanitizer already produced (a quote, or a caller's own fixed text).
    ///
    /// # Errors
    /// The same as [`Body::prepare`].
    pub fn from_sanitized(limits: BodyLimits, text: &Sanitized) -> Result<Self, BodyError> {
        Self::prepare(limits, text.as_str())
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// The sanitized text.
    #[must_use]
    pub fn sanitized(&self) -> &Sanitized {
        &self.0
    }

    /// Characters in the body.
    #[must_use]
    pub fn chars(&self) -> usize {
        self.0.as_str().chars().count()
    }

    /// Lines in the body (an empty body has one).
    #[must_use]
    pub fn lines(&self) -> usize {
        line_count(self.0.as_str())
    }

    /// Whether the body holds nothing but whitespace.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.0.as_str().trim().is_empty()
    }

    /// The text without trailing whitespace and blank lines, which is what a post stores.
    #[must_use]
    pub fn trimmed(&self) -> &str {
        self.0.as_str().trim_end()
    }
}

/// Lines in `text`, counted by `\n`; empty text is one line.
#[must_use]
pub(crate) fn line_count(text: &str) -> usize {
    text.bytes().filter(|&b| b == b'\n').count() + 1
}
