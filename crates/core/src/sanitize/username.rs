//! Username normalization, strict validation and the confusable skeleton.

use std::fmt;

use unicode_normalization::UnicodeNormalization;

use crate::icons::SecurityMarker;

/// Names nobody may register, compared on the skeleton (so `Sy50p` is blocked as well as
/// `sysop`): staff words, the default role and group names from `docs/access.md`, and the
/// security-marker labels (added from the icon registry in [`is_reserved`], so they cannot
/// drift). Reserved means a member cannot pose as staff or as a security cue in a list or
/// byline.
const RESERVED_NAMES: &[&str] = &[
    "sysop",
    "cosysop",
    "admin",
    "root",
    "system",
    "staff",
    "everyone",
    "moderator",
    // Default roles (docs/access.md) not already listed.
    "waiting",
    "member",
    "trusted",
];

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
        skeleton_of(&self.0)
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
    /// A well-formed name that is reserved (staff, role and group names, security-marker
    /// labels), including lookalikes of one.
    Reserved,
}

impl fmt::Display for UsernameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooShort => "username is too short",
            Self::TooLong => "username is too long",
            Self::DisallowedCharacter => "username contains a disallowed character",
            Self::BadFirstCharacter => "username must start with a letter or digit",
            Self::Reserved => "username is reserved",
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
    if is_reserved(&normalized) {
        return Err(UsernameError::Reserved);
    }
    Ok(Username(normalized))
}

/// Whether `name` (already validated ASCII) is, or looks like, a reserved name.
fn is_reserved(name: &str) -> bool {
    let skeleton = skeleton_of(name);
    RESERVED_NAMES
        .iter()
        .copied()
        .chain(SecurityMarker::ALL.map(SecurityMarker::label))
        .any(|reserved| skeleton_of(reserved) == skeleton)
}

/// The confusable skeleton of a validated name: ASCII lowercase with common lookalikes
/// folded. Private on purpose: callers get it from [`Username::skeleton`], so it is only
/// ever computed from text that already passed validation.
///
/// Single characters: `0`→`o`, `1` and `i`→`l`, `5`→`s`, `2`→`z`, `8`→`b`, `q`→`g`, `-`→`_`.
/// Pairs (after the single folds, scanning left to right): `vv`→`w`, `cl`→`d`, `nn`→`m`,
/// `rn`→`m`. This is a best-effort table, not a confusables database: it errs toward
/// collisions (blocking a few honest names) and cross-script lookalikes never reach it
/// because the allowed set is ASCII only.
pub(super) fn skeleton_of(name: &str) -> String {
    let folded: Vec<char> = name
        .chars()
        .map(|c| match c.to_ascii_lowercase() {
            '0' => 'o',
            '1' | 'i' => 'l',
            '5' => 's',
            '2' => 'z',
            '8' => 'b',
            'q' => 'g',
            '-' => '_',
            other => other,
        })
        .collect();
    let mut out = String::with_capacity(folded.len());
    let mut i = 0;
    while i < folded.len() {
        let pair = folded.get(i..i + 2);
        let merged = match pair {
            Some(['v', 'v']) => Some('w'),
            Some(['c', 'l']) => Some('d'),
            Some(['n' | 'r', 'n']) => Some('m'),
            _ => None,
        };
        if let Some(m) = merged {
            out.push(m);
            i += 2;
        } else if let Some(&c) = folded.get(i) {
            out.push(c);
            i += 1;
        } else {
            break;
        }
    }
    out
}
