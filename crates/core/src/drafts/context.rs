//! The key a calling slice chooses to say what a draft is for.

use std::fmt;

/// Longest context key, in characters.
pub const MAX_CONTEXT_CHARS: usize = 64;

/// Why a context key was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ContextKeyError {
    /// The key is empty.
    #[error("a draft context key cannot be empty")]
    Empty,
    /// The key is longer than [`MAX_CONTEXT_CHARS`].
    #[error("a draft context key is at most {MAX_CONTEXT_CHARS} characters")]
    TooLong,
    /// The key holds a character outside `A-Z a-z 0-9 . _ : / -`.
    #[error("a draft context key uses only letters, digits and . _ : / -")]
    BadCharacter,
}

/// An opaque key for one draft of one member, chosen by the slice that opens the editor
/// (for example `forum:reply:1234` or `mail:new`).
///
/// The draft store never interprets it. It is limited to a short ASCII alphabet so it is safe
/// to store, log and compare without sanitizing, and it never holds member-typed text: a
/// calling slice builds it from ids, not from what a member wrote.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContextKey(String);

impl ContextKey {
    /// Check and wrap a key.
    ///
    /// # Errors
    /// [`ContextKeyError`] when the key is empty, too long or uses a character outside the
    /// allowed alphabet.
    pub fn parse(key: &str) -> Result<Self, ContextKeyError> {
        if key.is_empty() {
            return Err(ContextKeyError::Empty);
        }
        // Count bytes first: every allowed character is ASCII, so a longer string is refused
        // before it is scanned.
        if key.len() > MAX_CONTEXT_CHARS {
            return Err(ContextKeyError::TooLong);
        }
        if !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'/' | b'-'))
        {
            return Err(ContextKeyError::BadCharacter);
        }
        Ok(Self(key.to_owned()))
    }

    /// The key as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContextKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
