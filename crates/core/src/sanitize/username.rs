//! Username normalization, strict validation and the confusable skeleton.

use std::fmt;

use unicode_normalization::UnicodeNormalization;

/// Fewest characters in a username.
pub const USERNAME_MIN_CHARS: usize = 2;
/// Most characters in a username.
pub const USERNAME_MAX_CHARS: usize = 24;

/// A validated username: NFKC-normalized, ASCII letters, digits, `_` and `-`, and starting
/// with a letter or digit.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Username(String);

impl Username {
    /// The username text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The comparison key used to block lookalike names. Two usernames with equal skeletons
    /// must not both be registered.
    #[must_use]
    pub fn skeleton(&self) -> String {
        username_skeleton(&self.0)
    }
}

impl AsRef<str> for Username {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a username was rejected. Usernames are rejected, never silently repaired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsernameError {
    /// Shorter than [`USERNAME_MIN_CHARS`] after normalization.
    TooShort,
    /// Longer than [`USERNAME_MAX_CHARS`] after normalization.
    TooLong,
    /// Contains a character outside the allowed set (this includes controls, escapes, bidi
    /// controls, private-use codepoints, whitespace and non-ASCII letters).
    DisallowedCharacter,
    /// Starts with `_` or `-`.
    BadFirstCharacter,
}

impl fmt::Display for UsernameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooShort => "username is too short",
            Self::TooLong => "username is too long",
            Self::DisallowedCharacter => "username contains a disallowed character",
            Self::BadFirstCharacter => "username must start with a letter or digit",
        })
    }
}

impl std::error::Error for UsernameError {}

/// Normalize (NFKC) and validate a username.
///
/// NFKC runs first so compatibility forms such as fullwidth `Ａ` collapse to `A` before the
/// strict check. Anything left outside `[A-Za-z0-9_-]` is rejected.
pub fn sanitize_username(input: &str) -> Result<Username, UsernameError> {
    // Bound the work on hostile input: no valid name is longer than this many raw chars,
    // even allowing for NFKC expansion of a few characters.
    let bounded: String = input.chars().take(USERNAME_MAX_CHARS * 4).collect();
    let normalized: String = bounded.nfkc().collect();

    let mut count = 0usize;
    for (i, c) in normalized.chars().enumerate() {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(UsernameError::DisallowedCharacter);
        }
        if i == 0 && !c.is_ascii_alphanumeric() {
            return Err(UsernameError::BadFirstCharacter);
        }
        count += 1;
    }
    if count < USERNAME_MIN_CHARS {
        return Err(UsernameError::TooShort);
    }
    if count > USERNAME_MAX_CHARS {
        return Err(UsernameError::TooLong);
    }
    Ok(Username(normalized))
}

/// The confusable skeleton of a name: ASCII lowercase with the common lookalikes folded
/// (`0`→`o`, `1`, `i`, `l`→`l`, `-`→`_`).
///
/// This is a deliberately minimal table. Cross-script confusables never reach it because
/// the allowed set is ASCII only. Multi-letter lookalikes (`rn` vs `m`) are not handled.
#[must_use]
pub fn username_skeleton(name: &str) -> String {
    name.chars()
        .map(|c| match c.to_ascii_lowercase() {
            '0' => 'o',
            '1' | 'i' => 'l',
            '-' => '_',
            other => other,
        })
        .collect()
}
