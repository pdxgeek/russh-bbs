//! Content as data: events, items, dialogue and the like live in TOML tables embedded in the
//! door's crate and are checked by a schema test, so writers can add content without touching
//! the engine (`docs/doors/README.md`).
//!
//! A door defines a typed table (`#[derive(Deserialize)]` with `deny_unknown_fields`), gives it
//! a [`Table::validate`] that checks what the types cannot (ranges, duplicate ids, empty text),
//! embeds the file with `include_str!` and loads it with [`load`]. A test calls [`check`] on the
//! same text so a bad edit fails in CI, not on a live board. TOML is the one format here; RON
//! would add a dependency and no capability.
//!
//! Content is the crate author's text, but it is still drawn through the canvas, which
//! sanitizes; [`plain_text`] lets a validator reject text the sanitizer would change, so the
//! author hears about it.

use russh_bbs_core::sanitize::MultiLine;
use serde::de::DeserializeOwned;

/// The largest table the loader accepts, in bytes.
pub const MAX_TABLE_BYTES: usize = 512 * 1024;

/// A typed content table.
pub trait Table: DeserializeOwned {
    /// Check what the types cannot. Return every problem found, each naming the entry.
    fn validate(&self) -> Vec<String>;
}

/// Why a table was not loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContentError {
    /// The text is over [`MAX_TABLE_BYTES`].
    #[error("content table `{name}` is too large")]
    TooLarge {
        /// The table's name.
        name: &'static str,
    },
    /// The text is not valid TOML for the table's type.
    #[error("content table `{name}` does not parse: {message}")]
    Parse {
        /// The table's name.
        name: &'static str,
        /// The parser's message.
        message: String,
    },
    /// The table parsed but failed its own checks.
    #[error("content table `{name}` is invalid: {}", problems.join("; "))]
    Invalid {
        /// The table's name.
        name: &'static str,
        /// Every problem.
        problems: Vec<String>,
    },
}

/// Parse and validate an embedded table.
///
/// # Errors
/// See [`ContentError`].
pub fn load<T: Table>(name: &'static str, text: &str) -> Result<T, ContentError> {
    if text.len() > MAX_TABLE_BYTES {
        return Err(ContentError::TooLarge { name });
    }
    let table: T = toml::from_str(text).map_err(|error| ContentError::Parse {
        name,
        message: error.message().to_owned(),
    })?;
    let problems = table.validate();
    if problems.is_empty() {
        Ok(table)
    } else {
        Err(ContentError::Invalid { name, problems })
    }
}

/// The schema-test hook: `Ok(())` when `text` loads as `T`, otherwise the error to print.
/// A door's test is `assert_eq!(content::check::<MyTable>("events", EVENTS), Ok(()))`.
pub fn check<T: Table>(name: &'static str, text: &str) -> Result<(), ContentError> {
    load::<T>(name, text).map(|_| ())
}

/// Whether `text` survives the sanitizer unchanged (no control characters, escapes, invisible
/// or reserved characters). For validators.
#[must_use]
pub fn plain_text(text: &str) -> bool {
    MultiLine::new(usize::MAX / 4).sanitize(text).as_str() == text
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Phrases {
        phrase: Vec<Phrase>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Phrase {
        id: String,
        text: String,
    }

    impl Table for Phrases {
        fn validate(&self) -> Vec<String> {
            let mut problems = Vec::new();
            for (n, phrase) in self.phrase.iter().enumerate() {
                if phrase.text.trim().is_empty() {
                    problems.push(format!("phrase {n} is empty"));
                }
                if !plain_text(&phrase.text) {
                    problems.push(format!("phrase `{}` has characters the board strips", phrase.id));
                }
                if self.phrase[..n].iter().any(|p| p.id == phrase.id) {
                    problems.push(format!("phrase id `{}` is used twice", phrase.id));
                }
            }
            problems
        }
    }

    #[test]
    fn a_good_table_loads_and_every_kind_of_bad_one_is_named() {
        let good = "[[phrase]]\nid = 'a'\ntext = 'Hello'\n";
        assert_eq!(load::<Phrases>("p", good).unwrap().phrase.len(), 1);
        assert_eq!(check::<Phrases>("p", good), Ok(()));

        let unknown = "[[phrase]]\nid = 'a'\ntext = 'x'\nextra = 1\n";
        assert!(matches!(
            load::<Phrases>("p", unknown),
            Err(ContentError::Parse { .. })
        ));
        assert!(matches!(
            load::<Phrases>("p", "not toml ["),
            Err(ContentError::Parse { .. })
        ));
        let dupes = "[[phrase]]\nid='a'\ntext='x'\n[[phrase]]\nid='a'\ntext=''\n";
        match load::<Phrases>("p", dupes) {
            Err(ContentError::Invalid { problems, .. }) => assert_eq!(problems.len(), 2),
            other => panic!("{other:?}"),
        }
        let escape = "[[phrase]]\nid='a'\ntext=\"\\u001b[31mred\"\n";
        assert!(matches!(
            load::<Phrases>("p", escape),
            Err(ContentError::Invalid { .. })
        ));
        let huge = " ".repeat(MAX_TABLE_BYTES + 1);
        assert!(matches!(
            load::<Phrases>("p", &huge),
            Err(ContentError::TooLarge { .. })
        ));
    }
}
