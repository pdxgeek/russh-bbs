//! The theme-file version and its forward migrations (UI-7).
//!
//! A theme file may start with `version = N`. A file without one is **version 1**: every file
//! written before UI-7, including the shipped ones. The loader migrates an older file forward
//! in memory, one step at a time, so a sysop's old theme keeps loading and an export always
//! writes the current version. A file newer than this server reads is refused, never guessed at.
//!
//! * **1 to 2** adds the optional `[meta]` table (`title`, `credit`). Nothing in a version-1 file
//!   changes meaning; the step only stamps the version.
//!
//! A migration works on the parsed table and runs before any validation, so the checks that
//! follow (schema, sizes, strings, contrast) are the same for every version.

use super::ThemeError;
use crate::ui::strings::check_board_text;

/// The version this server writes.
pub(super) const CURRENT: u32 = 2;

/// The oldest version a file may be: files without `version`.
const OLDEST: u32 = 1;

/// Longest title, in characters.
const TITLE_MAX: usize = 40;

/// Longest credit, in characters.
const CREDIT_MAX: usize = 80;

/// Read and remove `version` from `root`, then migrate the table forward to [`CURRENT`].
/// Returns the version the file was written in.
pub(super) fn migrate(root: &mut toml::Table) -> Result<u32, ThemeError> {
    let found = match root.remove("version") {
        None => OLDEST,
        Some(toml::Value::Integer(n)) => u32::try_from(n)
            .ok()
            .filter(|n| (OLDEST..=CURRENT).contains(n))
            .ok_or_else(|| ThemeError::Version(n.to_string()))?,
        Some(other) => return Err(ThemeError::Version(other.type_str().to_owned())),
    };
    let mut at = found;
    while at < CURRENT {
        step(root, at);
        at += 1;
    }
    Ok(found)
}

/// One migration step: the table as version `from` to the table as version `from + 1`.
fn step(_root: &mut toml::Table, from: u32) {
    match from {
        // 1 to 2: `[meta]` is new and optional; a version-1 table is already valid.
        // Later versions add their step here; `migrate` never asks for one at or above CURRENT.
        _ => {}
    }
}

/// Read `[meta]`: a title and a credit, both optional, both checked like any sysop string.
pub(super) fn meta(table: Option<toml::Value>) -> Result<(String, String), ThemeError> {
    let bad = |message: String| ThemeError::Meta(message);
    let table = match table {
        None => return Ok((String::new(), String::new())),
        Some(toml::Value::Table(table)) => table,
        Some(_) => return Err(bad("`meta` must be a table".to_owned())),
    };
    let mut title = String::new();
    let mut credit = String::new();
    for (key, value) in &table {
        let toml::Value::String(text) = value else {
            return Err(bad(format!("`meta.{key}` must be text")));
        };
        let (max, slot) = match key.as_str() {
            "title" => (TITLE_MAX, &mut title),
            "credit" => (CREDIT_MAX, &mut credit),
            _ => return Err(bad(format!("`meta.{key}` is not a meta key (title, credit)"))),
        };
        if text.chars().count() > max {
            return Err(bad(format!("`meta.{key}` is longer than {max} characters")));
        }
        check_board_text(&format!("meta.{key}"), text)
            .map_err(|error| bad(error.to_string()))?;
        text.clone_into(slot);
    }
    Ok((title, credit))
}
