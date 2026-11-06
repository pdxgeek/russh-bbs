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
        message: one_line(error.message()),
    })?;
    let problems = table.validate();
    if problems.is_empty() {
        Ok(table)
    } else {
        Err(ContentError::Invalid {
            name,
            problems: bounded_problems(problems),
        })
    }
}

/// The longest single message kept, in characters.
const MESSAGE_MAX: usize = 160;
/// The most problems kept.
const PROBLEMS_MAX: usize = 20;

/// A parser or validator message made safe to log: one line, control characters escaped, cut at
/// [`MESSAGE_MAX`] characters. It may quote the file, which can be any length (H-4).
fn one_line(text: &str) -> String {
    let mut out = String::new();
    for (count, c) in text.chars().enumerate() {
        if count >= MESSAGE_MAX {
            out.push_str("...");
            break;
        }
        match c {
            '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_control() => out.extend(c.escape_default()),
            c => out.push(c),
        }
    }
    out
}

/// At most [`PROBLEMS_MAX`] problems, each one line, with a count of the rest.
fn bounded_problems(problems: Vec<String>) -> Vec<String> {
    let extra = problems.len().saturating_sub(PROBLEMS_MAX);
    let mut out: Vec<String> = problems
        .into_iter()
        .take(PROBLEMS_MAX)
        .map(|p| one_line(&p))
        .collect();
    if extra > 0 {
        out.push(format!("...and {extra} more"));
    }
    out
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
    #![allow(clippy::unwrap_used, clippy::panic)]
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
                    problems.push(format!(
                        "phrase `{}` has characters the board strips",
                        phrase.id
                    ));
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

    // ---- H-4: hostile input ----

    use std::fmt::Write as _;

    use proptest::prelude::*;

    const GOOD: &str =
        "[[phrase]]\nid = 'a'\ntext = 'Hello'\n\n[[phrase]]\nid = 'b'\ntext = 'World'\n";

    /// Reserved marker codepoints, a Nerd Font glyph, an escape, a bidi override and the
    /// fragments that have broken parsers.
    const BAD: &[&str] = &[
        "\u{1b}[31m",
        "\u{e0a0}",
        "\u{202e}",
        "\u{0}",
        "\u{200b}",
        "\\u001b[2J",
        "[[[[",
        "'''",
        "nan",
        "inf",
        "-1",
        "1e999",
        "99999999999999999999",
        "{ a = { b = 1 } }",
        "= [",
        "\n[[phrase]]\n",
    ];

    fn floor(t: &str, n: u32) -> usize {
        let mut i = n as usize % (t.len() + 1);
        while !t.is_char_boundary(i) {
            i -= 1;
        }
        i
    }

    fn hostile() -> impl Strategy<Value = String> {
        let edits =
            prop::collection::vec((0_u8..6, any::<u32>(), any::<u32>()), 1..6).prop_map(|ops| {
                let mut text = GOOD.to_owned();
                for (kind, a, b) in ops {
                    let i = floor(&text, a);
                    match kind {
                        0 => text.truncate(i),
                        1 => text.insert_str(i, BAD[b as usize % BAD.len()]),
                        2 => {
                            let span: String = text[i..].chars().take(40).collect();
                            let end = i + span.len();
                            text.insert_str(end, &span.repeat(1 + b as usize % 300));
                        }
                        3 => text.insert_str(i, &"x".repeat(1 + b as usize % 60_000)),
                        4 => text = text.replace("'Hello'", "[1, 2, [3, [4]]]"),
                        _ => {
                            let _ = write!(text, "\nextra{a} = {b}\n");
                        }
                    }
                }
                text
            });
        let garbage = prop_oneof![
            (1_usize..20_000).prop_map(|n| format!("a = {}{}", "[".repeat(n), "]".repeat(n))),
            (1_usize..20_000).prop_map(|n| format!("a = {}", "{ a = ".repeat(n))),
            (1_usize..4_000).prop_map(|n| "[[phrase]]\n".repeat(n)),
            (1_usize..4_000).prop_map(|n| format!("[{}]\n", vec!["phrase"; n].join("."))),
            prop::collection::vec(any::<u8>(), 0..2048)
                .prop_map(|b| String::from_utf8_lossy(&b).into_owned()),
        ];
        prop_oneof![3 => edits, 1 => garbage]
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

        /// Nothing panics; a refusal is short and one line; what loads passes the validator, so
        /// it holds no character the board strips.
        #[test]
        fn hostile_tables_never_panic_and_refusals_are_short(text in hostile()) {
            match load::<Phrases>("p", &text) {
                Ok(table) => {
                    prop_assert!(table.phrase.iter().all(|p| plain_text(&p.text)));
                }
                Err(error) => {
                    let shown = error.to_string();
                    prop_assert!(shown.chars().count() < 4_000, "{} chars", shown.chars().count());
                    prop_assert!(!shown.chars().any(char::is_control), "{shown:?}");
                }
            }
        }
    }

    #[test]
    fn a_table_over_the_cap_is_refused_before_it_is_parsed() {
        let text = format!("{GOOD}# {}", "x".repeat(MAX_TABLE_BYTES));
        assert!(matches!(
            load::<Phrases>("p", &text),
            Err(ContentError::TooLarge { .. })
        ));
    }

    #[test]
    fn a_long_unknown_key_is_not_repeated_whole() {
        let text = format!(
            "[[phrase]]\nid = 'a'\ntext = 'x'\n{} = 1\n",
            "k".repeat(50_000)
        );
        let shown = load::<Phrases>("p", &text).unwrap_err().to_string();
        assert!(
            shown.chars().count() < 400,
            "{} chars",
            shown.chars().count()
        );
    }

    #[test]
    fn a_validator_with_many_problems_is_summarised() {
        let text = "[[phrase]]\nid = 'a'\ntext = ''\n".repeat(200);
        match load::<Phrases>("p", &text) {
            Err(ContentError::Invalid { problems, .. }) => {
                assert!(problems.len() <= PROBLEMS_MAX + 1, "{}", problems.len());
                assert!(problems.last().unwrap().contains("more"));
            }
            other => panic!("{other:?}"),
        }
    }
}
