//! A typed secret and the rules for a new password.
//!
//! [`Secret`] holds text a person typed that must never be shown or logged. Its `Debug` prints
//! a fixed placeholder, it has no `Display`, `Clone` or `PartialEq`, and the only way to read it
//! is [`Secret::expose`], which the hashing code calls. Nothing here is an audit, log or error
//! field.
//!
//! A new password must be 12 to 128 characters and hold no control characters. It is **not**
//! normalized or trimmed: what was typed is what is hashed, so a space at either end counts.
//! A login attempt is not held to the length rules (a short wrong guess is simply wrong).

use std::fmt;

/// Fewest characters in a new password. Long enough that a guess budget limited to a few
/// attempts a minute (D81) is hopeless, short enough to type on a phone.
pub const MIN_CHARS: usize = 12;

/// Most characters in a new password. Bounds the work and the screen buffer.
pub const MAX_CHARS: usize = 128;

/// Most bytes of a login attempt that are looked at. A longer attempt is treated as wrong
/// (without being hashed at its full length).
pub const MAX_ATTEMPT_BYTES: usize = 1024;

/// Text that must not be shown or logged.
pub struct Secret(String);

impl Secret {
    /// Wrap typed text.
    #[must_use]
    pub fn new(text: String) -> Self {
        Self(text)
    }

    /// The text, for hashing. Callers must not log it.
    #[must_use]
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }

    /// Characters held.
    #[must_use]
    pub fn chars(&self) -> usize {
        self.0.chars().count()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// Why a new password was refused. Says what to change, never repeats the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    /// Fewer than [`MIN_CHARS`] characters.
    #[error("password is too short (at least {MIN_CHARS} characters)")]
    TooShort,
    /// More than [`MAX_CHARS`] characters.
    #[error("password is too long (at most {MAX_CHARS} characters)")]
    TooLong,
    /// A control character, which a terminal cannot carry reliably.
    #[error("password holds a control character")]
    ControlCharacter,
}

/// Check a new password against the rules.
///
/// # Errors
/// The first rule it breaks.
pub fn check_new(text: &str) -> Result<(), PolicyError> {
    if text.chars().any(char::is_control) {
        return Err(PolicyError::ControlCharacter);
    }
    let length = text.chars().count();
    if length < MIN_CHARS {
        Err(PolicyError::TooShort)
    } else if length > MAX_CHARS {
        Err(PolicyError::TooLong)
    } else {
        Ok(())
    }
}
