//! The text rules of the forum: one place that sanitizes and bounds everything a member types.

use crate::drafts::{Body, BodyError, BodyLimits};
use crate::sanitize::{Sanitized, SingleLine};

/// Longest board name, in characters.
pub const NAME_MAX_CHARS: usize = 40;
/// Longest board description.
pub const DESCRIPTION_MAX_CHARS: usize = 160;
/// Longest thread subject.
pub const SUBJECT_MAX_CHARS: usize = 120;
/// Longest post body, in characters (the editor's ceiling is higher; a post is shorter).
pub const BODY_MAX_CHARS: usize = 8_000;
/// Most lines in a post body.
pub const BODY_MAX_LINES: usize = 200;

/// Why typed text was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TextError {
    /// Nothing was left after cleaning.
    #[error("that cannot be empty")]
    Empty,
    /// The text is over its limit; it is refused, not cut.
    #[error("that is longer than {max} characters")]
    TooLong {
        /// The limit.
        max: usize,
    },
    /// The body is over its size or line limit.
    #[error("the post is over its size limit")]
    Body,
}

/// The bounds a post body must fit.
#[must_use]
pub fn body_limits() -> BodyLimits {
    BodyLimits::new(BODY_MAX_CHARS, BODY_MAX_LINES)
}

fn single(raw: &str, max: usize, allow_empty: bool) -> Result<Sanitized, TextError> {
    // Sanitize one character past the limit: the sanitizer cuts at its limit, and a silent cut
    // would change what the member meant, so text that reaches the extra character is refused.
    let cleaned = SingleLine::new(max.saturating_add(1)).sanitize(raw);
    let text = SingleLine::new(max.saturating_add(1)).sanitize(cleaned.as_str().trim());
    if text.as_str().chars().count() > max {
        return Err(TextError::TooLong { max });
    }
    if text.as_str().is_empty() && !allow_empty {
        return Err(TextError::Empty);
    }
    Ok(text)
}

/// A board name: single line, 1 to [`NAME_MAX_CHARS`].
///
/// # Errors
/// [`TextError`] for empty or over-long text.
pub fn prepare_name(raw: &str) -> Result<Sanitized, TextError> {
    single(raw, NAME_MAX_CHARS, false)
}

/// A board description: single line, up to [`DESCRIPTION_MAX_CHARS`], may be empty.
///
/// # Errors
/// [`TextError::TooLong`] for over-long text.
pub fn prepare_description(raw: &str) -> Result<Sanitized, TextError> {
    single(raw, DESCRIPTION_MAX_CHARS, true)
}

/// A thread subject: single line, 1 to [`SUBJECT_MAX_CHARS`].
///
/// # Errors
/// [`TextError`] for empty or over-long text.
pub fn prepare_subject(raw: &str) -> Result<Sanitized, TextError> {
    single(raw, SUBJECT_MAX_CHARS, false)
}

/// A body already prepared by the editor, checked against the post bounds. Blank is refused.
///
/// # Errors
/// [`TextError::Empty`] for blank text, [`TextError::Body`] when it is over the post bounds.
pub(super) fn check_body(body: &Body) -> Result<Body, TextError> {
    if body.is_blank() {
        return Err(TextError::Empty);
    }
    Body::from_sanitized(body_limits(), body.sanitized()).map_err(|_: BodyError| TextError::Body)
}
