//! `menus.toml`: the schema, the caps and the reader (UI-6).
//!
//! ```toml
//! version = 1
//!
//! [[menu]]                 # a built-in menu: main, sysop, invites, members, community
//! id = "main"
//! title = "Main Hall"      # optional
//!
//!   [[menu.item]]
//!   target = "main.mail"   # a built-in item id, "group:<id>" or "page:<id>"
//!   key = "x"              # optional for built-in items, required for groups and pages
//!   label = "Messages"     # optional text
//!   hidden = false         # true leaves it out for everyone
//!
//! [[menu]]                 # a new group, linked from above with target = "group:talk"
//! id = "talk"
//! ```
//!
//! Unknown fields are refused. The file is untrusted input: bounded bytes, lines and line
//! length, read through `take` so an endless file cannot be slurped, no symbolic link followed.

use std::fs;
use std::io::Read;
use std::path::Path;

use russh_bbs_core::menu::{ItemSpec, MenuSpec, Spec, valid_page_id};
use serde::Deserialize;

/// Largest `menus.toml`, in bytes.
pub(crate) const MAX_BYTES: usize = 32 * 1024;
/// Most lines.
pub(crate) const MAX_LINES: usize = 600;
/// Longest line, in characters.
pub(crate) const MAX_LINE_CHARS: usize = 200;
/// The only format version.
const VERSION: u32 = 1;
/// Most `[[menu]]` tables (the built-in menus plus the groups).
const MAX_MENUS: usize = 16;

/// Why a file was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum FileError {
    /// The file could not be read.
    #[error("cannot read the file: {0}")]
    Io(String),
    /// Not a regular file (a link, a directory, a device).
    #[error("not a regular file (links are not followed)")]
    NotRegular,
    /// Over the size cap.
    #[error("larger than {MAX_BYTES} bytes")]
    TooLarge,
    /// Not UTF-8.
    #[error("not valid UTF-8")]
    NotUtf8,
    /// More lines than the cap.
    #[error("more than {MAX_LINES} lines")]
    TooManyLines,
    /// A line longer than the cap.
    #[error("line {0} is longer than {MAX_LINE_CHARS} characters")]
    LongLine(usize),
    /// Not valid TOML for the schema.
    #[error("not valid: {0}")]
    Syntax(String),
    /// A version this server does not read.
    #[error("version {0} is not understood (this server reads version {VERSION})")]
    Version(u32),
    /// Too many `[[menu]]` tables.
    #[error("more than {MAX_MENUS} menus")]
    TooManyMenus,
    /// A hotkey is not one character.
    #[error("key {0:?} must be exactly one character")]
    BadKey(String),
    /// A target that is not an item id, `group:` or `page:` reference (before the core check).
    #[error("target {0:?} is not valid")]
    BadTarget(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    version: u32,
    #[serde(default)]
    menu: Vec<RawMenu>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMenu {
    id: String,
    title: Option<String>,
    #[serde(default)]
    item: Vec<RawItem>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawItem {
    target: String,
    key: Option<String>,
    label: Option<String>,
    #[serde(default)]
    hidden: bool,
}

/// Read a capped text file. `Ok(None)` when it does not exist.
pub(crate) fn read_capped(path: &Path, max_bytes: usize) -> Result<Option<String>, FileError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(FileError::Io(error.kind().to_string())),
    };
    if !meta.file_type().is_file() {
        return Err(FileError::NotRegular);
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|error| FileError::Io(error.kind().to_string()))?
        .take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| FileError::Io(error.kind().to_string()))?;
    if bytes.len() > max_bytes {
        return Err(FileError::TooLarge);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| FileError::NotUtf8)
}

/// Read `menus.toml` (`Ok(None)` when absent), with its line caps.
pub(crate) fn read(path: &Path) -> Result<Option<String>, FileError> {
    let Some(text) = read_capped(path, MAX_BYTES)? else {
        return Ok(None);
    };
    check_lines(&text, MAX_LINES, MAX_LINE_CHARS)?;
    Ok(Some(text))
}

/// Refuse text with too many lines or a line that is too long.
pub(crate) fn check_lines(text: &str, lines: usize, width: usize) -> Result<(), FileError> {
    let mut count = 0;
    for (at, line) in text.lines().enumerate() {
        count += 1;
        if count > lines {
            return Err(FileError::TooManyLines);
        }
        if line.chars().count() > width {
            return Err(FileError::LongLine(at + 1));
        }
    }
    Ok(())
}

/// Parse the text into a [`Spec`] for `core::menu::build`.
pub(crate) fn parse(text: &str) -> Result<Spec, FileError> {
    let raw: RawFile = toml::from_str(text)
        .map_err(|error: toml::de::Error| FileError::Syntax(error.message().to_owned()))?;
    if raw.version != VERSION {
        return Err(FileError::Version(raw.version));
    }
    if raw.menu.len() > MAX_MENUS {
        return Err(FileError::TooManyMenus);
    }
    let mut menus = Vec::with_capacity(raw.menu.len());
    for menu in raw.menu {
        let mut items = Vec::with_capacity(menu.item.len());
        for item in menu.item {
            let key = match item.key {
                None => None,
                Some(text) => {
                    let mut chars = text.chars();
                    match (chars.next(), chars.next()) {
                        (Some(c), None) => Some(c),
                        _ => return Err(FileError::BadKey(clip(&text))),
                    }
                }
            };
            if item.target.is_empty() || item.target.chars().count() > 64 {
                return Err(FileError::BadTarget(clip(&item.target)));
            }
            items.push(ItemSpec {
                target: item.target,
                key,
                label: item.label,
                hidden: item.hidden,
            });
        }
        menus.push(MenuSpec {
            id: menu.id,
            title: menu.title,
            items,
        });
    }
    Ok(Spec { menus })
}

/// A short escaped rendering of file text for an error.
fn clip(text: &str) -> String {
    text.chars().take(20).flat_map(char::escape_debug).collect()
}

/// The ids of the pages a spec links (`page:<id>`), in file order, without repeats. An id that is
/// not a valid page id is left for `core::menu::build` to refuse.
pub(crate) fn linked_pages(spec: &Spec) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for item in spec.menus.iter().flat_map(|menu| &menu.items) {
        if let Some(id) = item.target.strip_prefix("page:")
            && valid_page_id(id)
            && !ids.iter().any(|seen| seen == id)
        {
            ids.push(id.to_owned());
        }
    }
    ids
}
