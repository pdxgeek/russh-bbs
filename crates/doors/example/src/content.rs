//! The flavor lines, as data: `content/phrases.toml`, embedded and validated at load.

use russh_bbs_door::content::{self, Table, plain_text};
use serde::Deserialize;

/// The embedded table's text.
pub const PHRASES_TOML: &str = include_str!("../content/phrases.toml");

/// One line of flavor.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Phrase {
    /// A unique id, for writers and for tests.
    pub id: String,
    /// The text shown.
    pub text: String,
}

/// The table: lines for a win and for a loss.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Phrases {
    /// Shown after a win.
    pub win: Vec<Phrase>,
    /// Shown after a loss.
    pub lose: Vec<Phrase>,
}

impl Table for Phrases {
    fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for (name, list) in [("win", &self.win), ("lose", &self.lose)] {
            if list.is_empty() {
                problems.push(format!("[[{name}]] needs at least one entry"));
            }
            for (n, phrase) in list.iter().enumerate() {
                if phrase.id.is_empty() || !phrase.id.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
                    problems.push(format!("[[{name}]] #{n}: id must be a-z and -"));
                }
                if list[..n].iter().any(|p| p.id == phrase.id) {
                    problems.push(format!("[[{name}]] id `{}` is used twice", phrase.id));
                }
                if phrase.text.trim().is_empty() || phrase.text.chars().count() > 70 {
                    problems.push(format!("[[{name}]] `{}`: text must be 1 to 70 characters", phrase.id));
                }
                if !plain_text(&phrase.text) {
                    problems.push(format!("[[{name}]] `{}`: text has characters the board strips", phrase.id));
                }
            }
        }
        problems
    }
}

impl Phrases {
    /// The embedded table. If it were broken (a test rules that out) the door still works with
    /// one plain line each.
    #[must_use]
    pub fn embedded() -> Self {
        content::load::<Self>("phrases", PHRASES_TOML).unwrap_or_else(|_| Self {
            win: vec![Phrase {
                id: "win".into(),
                text: "You win.".into(),
            }],
            lose: vec![Phrase {
                id: "lose".into(),
                text: "You lose.".into(),
            }],
        })
    }

    /// A line for a win or a loss, picked by `roll` (any number; it wraps).
    #[must_use]
    pub fn pick(&self, won: bool, roll: u32) -> &str {
        let list = if won { &self.win } else { &self.lose };
        list.get(roll as usize % list.len().max(1))
            .map_or("", |p| p.text.as_str())
    }
}
