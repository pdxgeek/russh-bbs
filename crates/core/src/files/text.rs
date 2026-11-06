//! The text rules of file areas: one place that sanitizes and bounds everything a member types
//! or a client sends. File names and descriptions are member text (AGENTS.md): they go through
//! the shared sanitizer before they are stored as the display form, and the name as the client
//! sent it is kept only as data (`original_name`), never drawn and never used as a path.

use crate::sanitize::{Sanitized, SingleLine};

/// Longest area name, in characters.
pub const AREA_NAME_MAX_CHARS: usize = 40;
/// Longest area description.
pub const AREA_DESCRIPTION_MAX_CHARS: usize = 160;
/// Longest display file name, in characters.
pub const FILE_NAME_MAX_CHARS: usize = 120;
/// Longest file name a client may send, in bytes (the usual file system limit).
pub const ORIGINAL_NAME_MAX_BYTES: usize = 255;
/// Longest file description.
pub const FILE_DESCRIPTION_MAX_CHARS: usize = 480;
/// Longest tag.
pub const TAG_MAX_CHARS: usize = 24;
/// Most tags on one file.
pub const MAX_TAGS_PER_FILE: u32 = 8;

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
    /// A tag may hold only lowercase letters, digits, `_` and `-`.
    #[error("tags use only letters, digits, - and _")]
    BadTag,
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

/// An area name: single line, 1 to [`AREA_NAME_MAX_CHARS`].
///
/// # Errors
/// [`TextError`] for empty or over-long text.
pub fn prepare_area_name(raw: &str) -> Result<Sanitized, TextError> {
    single(raw, AREA_NAME_MAX_CHARS, false)
}

/// An area description: single line, up to [`AREA_DESCRIPTION_MAX_CHARS`], may be empty.
///
/// # Errors
/// [`TextError::TooLong`] for over-long text.
pub fn prepare_area_description(raw: &str) -> Result<Sanitized, TextError> {
    single(raw, AREA_DESCRIPTION_MAX_CHARS, true)
}

/// A file description: single line, up to [`FILE_DESCRIPTION_MAX_CHARS`], may be empty.
///
/// # Errors
/// [`TextError::TooLong`] for over-long text.
pub fn prepare_file_description(raw: &str) -> Result<Sanitized, TextError> {
    single(raw, FILE_DESCRIPTION_MAX_CHARS, true)
}

/// The two forms of an uploaded file's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileName {
    /// What members see and transfer under: sanitized, no path separators, 1 to
    /// [`FILE_NAME_MAX_CHARS`] characters.
    pub display: Sanitized,
    /// The name as the client sent it, kept as data only.
    pub original: String,
}

/// Prepare a name taken from an SFTP path (the last component). The display form goes through
/// the shared sanitizer and loses `/` and `\`; a name that sanitizes to nothing, or to more than
/// [`FILE_NAME_MAX_CHARS`], is refused.
///
/// # Errors
/// [`TextError`] for an empty, or over-long name.
pub fn prepare_file_name(raw: &str) -> Result<FileName, TextError> {
    if raw.len() > ORIGINAL_NAME_MAX_BYTES {
        return Err(TextError::TooLong {
            max: ORIGINAL_NAME_MAX_BYTES,
        });
    }
    let flat: String = raw
        .chars()
        .map(|c| if c == '/' || c == '\\' { '_' } else { c })
        .collect();
    let display = single(&flat, FILE_NAME_MAX_CHARS, false)?;
    if matches!(display.as_str(), "." | "..") {
        return Err(TextError::Empty);
    }
    Ok(FileName {
        display,
        original: raw.to_owned(),
    })
}

/// A tag: trimmed, lower-cased, 1 to [`TAG_MAX_CHARS`] of `a-z 0-9 _ -`. Anything else is
/// refused, not rewritten, so a tag means what its owner typed.
///
/// # Errors
/// [`TextError`] for an empty, over-long or odd tag.
pub fn prepare_tag(raw: &str) -> Result<String, TextError> {
    let tag = raw.trim().to_lowercase();
    if tag.is_empty() {
        return Err(TextError::Empty);
    }
    if tag.chars().count() > TAG_MAX_CHARS {
        return Err(TextError::TooLong { max: TAG_MAX_CHARS });
    }
    if !tag
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(TextError::BadTag);
    }
    Ok(tag)
}
