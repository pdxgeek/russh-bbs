//! The text rules of mail: one place that sanitizes and bounds a subject and a body before they
//! are sealed.

use crate::drafts::{Body, BodyError, BodyLimits};
use crate::sanitize::{Sanitized, SingleLine};

/// Longest subject, in characters.
pub const SUBJECT_MAX_CHARS: usize = 120;
/// Longest body, in characters.
pub const BODY_MAX_CHARS: usize = 8_000;
/// Most lines in a body.
pub const BODY_MAX_LINES: usize = 200;

/// Why typed text was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MailTextError {
    /// Nothing was left after cleaning.
    #[error("that cannot be empty")]
    Empty,
    /// Over the limit; refused, not cut.
    #[error("that is longer than {max} characters")]
    TooLong {
        /// The limit.
        max: usize,
    },
    /// The body is over its size or line limit.
    #[error("the message is over its size limit")]
    Body,
}

/// The bounds a body must fit; the editor is opened with the same.
#[must_use]
pub fn body_limits() -> BodyLimits {
    BodyLimits::new(BODY_MAX_CHARS, BODY_MAX_LINES)
}

/// A subject: single line, 1 to [`SUBJECT_MAX_CHARS`], sanitized by the shared sanitizer.
///
/// # Errors
/// [`MailTextError`] for empty or over-long text.
pub fn prepare_subject(raw: &str) -> Result<Sanitized, MailTextError> {
    // One character past the limit, so "at the limit" and "over it" differ and nothing is cut.
    let max = SUBJECT_MAX_CHARS;
    let cleaned = SingleLine::new(max + 1).sanitize(raw);
    let text = SingleLine::new(max + 1).sanitize(cleaned.as_str().trim());
    if text.as_str().chars().count() > max {
        return Err(MailTextError::TooLong { max });
    }
    if text.as_str().is_empty() {
        return Err(MailTextError::Empty);
    }
    Ok(text)
}

/// A body from the editor, checked against the mail bounds. Blank is refused.
///
/// # Errors
/// [`MailTextError::Empty`] for blank text, [`MailTextError::Body`] when over the bounds.
pub fn check_body(body: &Body) -> Result<Body, MailTextError> {
    if body.is_blank() {
        return Err(MailTextError::Empty);
    }
    Body::from_sanitized(body_limits(), body.sanitized())
        .map_err(|_: BodyError| MailTextError::Body)
}
