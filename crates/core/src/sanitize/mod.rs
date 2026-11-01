//! The one shared sanitizer for every string that is not UI chrome.
//!
//! User posts, usernames, feed items, mail, file names and descriptions and door output must
//! pass through this module before they reach a terminal, a log line or a layout. Front ends
//! never write their own filter.
//!
//! # Design
//!
//! * Callers pick an explicit context: [`SingleLine`], [`MultiLine`] or [`sanitize_username`].
//! * The result is a [`Sanitized`] (or [`Username`]) newtype with no public constructor from
//!   raw text, so an API that takes `&Sanitized` cannot be handed unsanitized input.
//! * Escape sequences are removed whole (parameter bytes included), not just their `ESC`.
//! * Limits count Unicode scalar values (`char`), never bytes, so a UTF-8 sequence is never
//!   split. Grapheme clusters are deliberately not used; see `docs/decisions.md` (D16).
//!
//! # What is removed or replaced
//!
//! | Class | Treatment |
//! |---|---|
//! | C0 controls, DEL, C1 controls | removed (newline and tab per context) |
//! | `ESC` + CSI/OSC/DCS/SOS/PM/APC/nF/Fe sequences, 8-bit C1 equivalents | removed entirely |
//! | Bidi overrides, embeddings, isolates, marks; zero-width space, BOM, word joiner, line and paragraph separators, tag characters | removed |
//! | Private-use ranges (BMP and planes 15 and 16) and noncharacters | replaced with U+FFFD |

mod scan;
mod username;

#[cfg(test)]
mod tests;

use std::fmt;

pub use username::{
    USERNAME_MAX_CHARS, USERNAME_MIN_CHARS, Username, UsernameError, sanitize_username,
    username_skeleton,
};

/// Text that has passed through the sanitizer.
///
/// There is deliberately no `From<String>` or public constructor taking raw text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sanitized(String);

impl Sanitized {
    /// The sanitized text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the wrapper and return the sanitized `String`.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }

    /// Length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the text is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl AsRef<str> for Sanitized {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sanitized {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Context for text that must stay on one line (subjects, titles, file names, one-liners).
///
/// Newlines, carriage returns and tabs each become one space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SingleLine {
    max_chars: usize,
}

impl SingleLine {
    /// A single-line context that keeps at most `max_chars` characters.
    #[must_use]
    pub const fn new(max_chars: usize) -> Self {
        Self { max_chars }
    }

    /// Sanitize `input` for this context.
    #[must_use]
    pub fn sanitize(&self, input: &str) -> Sanitized {
        Sanitized(scan::clean(input, scan::Layout::SingleLine, self.max_chars))
    }
}

/// Context for bodies (posts, mail, descriptions, door output).
///
/// Keeps `\n` and `\t`. `\r\n` and lone `\r` become `\n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultiLine {
    max_chars: usize,
}

impl MultiLine {
    /// A multi-line context that keeps at most `max_chars` characters.
    #[must_use]
    pub const fn new(max_chars: usize) -> Self {
        Self { max_chars }
    }

    /// Sanitize `input` for this context.
    #[must_use]
    pub fn sanitize(&self, input: &str) -> Sanitized {
        Sanitized(scan::clean(input, scan::Layout::MultiLine, self.max_chars))
    }
}
