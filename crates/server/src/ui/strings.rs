//! The string table: every fixed, user-facing string of the framework's screens, loaded from
//! one TOML file instead of written inline in rendering code (`docs/ui.md`, "Themes and
//! strings").
//!
//! The default table is embedded ([`DEFAULT_TOML`]). [`StringTable::parse`] is the loader a
//! theme or translation will use: it flattens the TOML tables into dotted keys and **validates**
//! the result against the keys the code needs, so a missing key is a clear error at boot and
//! never a blank on a member's screen.
//!
//! Table values are UI chrome (they come from the sysop or the build, never from members), but
//! they are still checked with the shared sanitizer: a value the sanitizer would change has a
//! control character, an escape sequence, an invisible or private-use character or a reserved
//! security-marker character in it, and is refused. A theme therefore cannot smuggle styling or
//! a forged marker through a string.

use std::collections::BTreeMap;

use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::terminal_settings::strings::fill;

/// The table that ships with the server.
pub(crate) const DEFAULT_TOML: &str = include_str!("strings.toml");

/// Longest table file accepted, so a mistake cannot load a huge file.
pub(crate) const MAX_BYTES: usize = 64 * 1024;

/// Longest single value, in characters.
pub(crate) const MAX_VALUE_CHARS: usize = 240;

/// Why a table was refused. Every message names the key, so the sysop can fix the file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StringError {
    /// The file is larger than [`MAX_BYTES`].
    #[error("string table is {0} bytes; the limit is {MAX_BYTES}")]
    TooLarge(usize),
    /// The file is not valid TOML.
    #[error("string table is not valid TOML: {0}")]
    Syntax(String),
    /// A value is not a string (a number, a list, ...).
    #[error("string table key `{0}` must be text")]
    NotText(String),
    /// A key the code needs is not in the table.
    #[error("string table is missing key `{0}`")]
    Missing(String),
    /// A key the code does not use (usually a typo).
    #[error("string table has unknown key `{0}`")]
    Unknown(String),
    /// A value is empty.
    #[error("string table key `{0}` is empty")]
    Empty(String),
    /// A value has characters that are not allowed in a string (see the module docs).
    #[error("string table key `{0}` contains control, invisible or reserved characters")]
    Unsafe(String),
    /// A value is longer than [`MAX_VALUE_CHARS`].
    #[error("string table key `{0}` is longer than {MAX_VALUE_CHARS} characters")]
    TooLong(String),
}

/// A validated table of strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringTable {
    entries: BTreeMap<String, String>,
}

impl StringTable {
    /// Parse and validate `text`. `required` is every key the code uses; the table must hold
    /// exactly those.
    pub fn parse(text: &str, required: &[&str]) -> Result<Self, StringError> {
        if text.len() > MAX_BYTES {
            return Err(StringError::TooLarge(text.len()));
        }
        let root: toml::Table = text
            .parse()
            .map_err(|error: toml::de::Error| StringError::Syntax(error.message().to_owned()))?;
        let mut entries = BTreeMap::new();
        flatten(&root, "", &mut entries)?;
        for key in required {
            if !entries.contains_key(*key) {
                return Err(StringError::Missing((*key).to_owned()));
            }
        }
        if let Some(key) = entries.keys().find(|key| !required.contains(&key.as_str())) {
            return Err(StringError::Unknown(key.clone()));
        }
        let checker = SingleLine::new(MAX_VALUE_CHARS + 1);
        for (key, value) in &entries {
            if value.is_empty() {
                return Err(StringError::Empty(key.clone()));
            }
            if value.chars().count() > MAX_VALUE_CHARS {
                return Err(StringError::TooLong(key.clone()));
            }
            if checker.sanitize(value).as_str() != value {
                return Err(StringError::Unsafe(key.clone()));
            }
        }
        Ok(Self { entries })
    }

    /// The text for `key`. A key that is not in the table cannot happen after
    /// [`StringTable::parse`] with the right `required` list; if the code ever asks for one the
    /// key itself is returned, so the bug is visible on screen instead of a panic.
    #[must_use]
    pub fn text<'a>(&'a self, key: &'a str) -> &'a str {
        self.entries.get(key).map_or(key, String::as_str)
    }

    /// The text for `key` with `{name}` placeholders filled. Values must already be sanitized.
    #[must_use]
    pub fn filled(&self, key: &str, values: &[(&str, &str)]) -> String {
        fill(self.text(key), values)
    }

    /// Every key, sorted.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }
}

fn flatten(
    table: &toml::Table,
    prefix: &str,
    out: &mut BTreeMap<String, String>,
) -> Result<(), StringError> {
    for (name, value) in table {
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            toml::Value::String(text) => {
                out.insert(key, text.clone());
            }
            toml::Value::Table(inner) => flatten(inner, &key, out)?,
            _ => return Err(StringError::NotText(key)),
        }
    }
    Ok(())
}
