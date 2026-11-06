//! The virtual file system SFTP serves, and the one parser that turns a client's path into a
//! place in it (M4-F2, D141).
//!
//! The tree has no real paths behind it: every name is an **id**.
//!
//! ```text
//! /                        areas the member can list, one directory each
//! /<area>/                 approved files of the area
//! /<area>/pending/         the member's own uploads still in quarantine (approvers: all of them)
//! /downloads/              the member's batch download list
//! ```
//!
//! An area directory is `<id>-<slug>` and a file is `<id>-<name>`; the leading **id** is the
//! only part read when a path is resolved, the rest is for people. Parsing is strict and does
//! no I/O:
//!
//! * a component that the shared sanitizer would change (a control character, an escape, an
//!   invisible format character, a private-use code point) is refused, as are `\`, NUL, `..`
//!   (except in [`canonical`], where it is resolved lexically and the result parsed again), a
//!   component over 255 bytes, a path over 1,024 bytes and a tree deeper than three levels;
//! * `.` and empty components are skipped, so `a//b` and `./a/` are `a/b` and `a`;
//! * a leading `/` means the root of this virtual tree; there is no other root, no drive, no
//!   home directory and no symlink (readlink and symlink are refused by the server).
//!
//! Nothing here can name a file system path: the output is a [`VPath`] of ids and, for a new
//! upload, a candidate name that [`super::prepare_file_name`] cleans.

use super::ids::{AreaId, FileId};
use crate::sanitize::SingleLine;

/// Longest whole path accepted, in bytes.
pub const MAX_PATH_BYTES: usize = 1_024;
/// Longest component accepted, in bytes.
pub const MAX_COMPONENT_BYTES: usize = 255;
/// The name of the directory of a member's unapproved uploads.
pub const PENDING_DIR: &str = "pending";
/// The name of the batch download directory.
pub const DOWNLOADS_DIR: &str = "downloads";

/// A name inside an area, a pending directory or the download list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// `<id>` or `<id>-<anything>`: an existing file, by id.
    Id(FileId),
    /// Anything else: the name a new upload would get. Only a create-for-writing may use it.
    Name(String),
}

/// A place in the virtual tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VPath {
    /// `/`.
    Root,
    /// `/<area>`.
    Area(AreaId),
    /// `/<area>/<entry>`.
    AreaEntry(AreaId, Entry),
    /// `/<area>/pending`.
    Pending(AreaId),
    /// `/<area>/pending/<entry>`.
    PendingEntry(AreaId, Entry),
    /// `/downloads`.
    Downloads,
    /// `/downloads/<entry>`.
    DownloadEntry(Entry),
}

/// Why a path was refused. The client sees one answer for all of them ("no such file"); the
/// variants are for tests and logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    /// Longer than [`MAX_PATH_BYTES`] or a component over [`MAX_COMPONENT_BYTES`].
    #[error("path too long")]
    TooLong,
    /// `..` where it is not allowed.
    #[error("parent references are not accepted")]
    Dots,
    /// A character the shared sanitizer would change, a backslash or a NUL.
    #[error("odd characters in the path")]
    OddCharacters,
    /// Not a place in the tree.
    #[error("no such place")]
    NoSuchPlace,
}

/// Split into components, enforcing the character and size rules. `..` is returned as is, for
/// the caller to refuse or resolve.
fn components(path: &str) -> Result<Vec<&str>, PathError> {
    if path.len() > MAX_PATH_BYTES {
        return Err(PathError::TooLong);
    }
    let mut out = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part.len() > MAX_COMPONENT_BYTES {
            return Err(PathError::TooLong);
        }
        if part.contains('\\') || part.contains('\0') {
            return Err(PathError::OddCharacters);
        }
        // Anything the shared sanitizer would alter is an odd encoding (AGENTS.md): refuse it
        // rather than guess what the client meant.
        if SingleLine::new(MAX_COMPONENT_BYTES).sanitize(part).as_str() != part {
            return Err(PathError::OddCharacters);
        }
        out.push(part);
    }
    Ok(out)
}

/// The leading decimal id of `text` (`12` or `12-name`), if it has one that fits.
fn leading_id(text: &str) -> Option<i64> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 18 {
        return None;
    }
    let rest = &text[digits..];
    if !(rest.is_empty() || rest.starts_with('-')) {
        return None;
    }
    let id: i64 = text[..digits].parse().ok()?;
    (id > 0).then_some(id)
}

fn entry(component: &str) -> Entry {
    leading_id(component).map_or_else(
        || Entry::Name(component.to_owned()),
        |id| Entry::Id(FileId::from_raw(id)),
    )
}

/// Resolve a client's path.
///
/// # Errors
/// [`PathError`]; the caller answers every one the same way.
pub fn parse(path: &str) -> Result<VPath, PathError> {
    let parts = components(path)?;
    if parts.contains(&"..") {
        return Err(PathError::Dots);
    }
    let mut it = parts.iter().copied();
    let Some(first) = it.next() else {
        return Ok(VPath::Root);
    };
    if first == DOWNLOADS_DIR {
        return match (it.next(), it.next()) {
            (None, _) => Ok(VPath::Downloads),
            (Some(name), None) => Ok(VPath::DownloadEntry(entry(name))),
            _ => Err(PathError::NoSuchPlace),
        };
    }
    let area = leading_id(first)
        .map(AreaId::from_raw)
        .ok_or(PathError::NoSuchPlace)?;
    let Some(second) = it.next() else {
        return Ok(VPath::Area(area));
    };
    if second == PENDING_DIR {
        return match (it.next(), it.next()) {
            (None, _) => Ok(VPath::Pending(area)),
            (Some(name), None) => Ok(VPath::PendingEntry(area, entry(name))),
            _ => Err(PathError::NoSuchPlace),
        };
    }
    match it.next() {
        None => Ok(VPath::AreaEntry(area, entry(second))),
        Some(_) => Err(PathError::NoSuchPlace),
    }
}

/// Resolve `..` lexically for `realpath` (a client's `cd ..`), then parse the result strictly.
/// Returns the normalized path, which always starts with `/`. Climbing above the root stays at
/// the root. The output is made only of components that passed [`parse`]'s own checks.
///
/// # Errors
/// [`PathError`] as for [`parse`].
pub fn canonical(path: &str) -> Result<String, PathError> {
    let mut stack: Vec<&str> = Vec::new();
    for part in components(path)? {
        if part == ".." {
            stack.pop();
        } else {
            stack.push(part);
        }
    }
    let joined = format!("/{}", stack.join("/"));
    parse(&joined)?;
    Ok(joined)
}

/// A name safe to put in a directory listing: ascii letters, digits and `-`, cut at 32.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 32 {
            break;
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// The directory name an area is listed under: `<id>-<slug>`, or just `<id>` when the name has
/// no letters or digits.
#[must_use]
pub fn area_dir_name(id: AreaId, name: &str) -> String {
    let slug = slug(name);
    if slug.is_empty() {
        id.to_string()
    } else {
        format!("{id}-{slug}")
    }
}

/// The name a file is listed under: `<id>-<display name>`. The display name is already
/// sanitized and free of `/`; it is cut so the whole stays within a component.
#[must_use]
pub fn file_entry_name(id: FileId, display_name: &str) -> String {
    let mut name = format!("{id}-{display_name}");
    while name.len() > MAX_COMPONENT_BYTES {
        name.pop();
    }
    name
}

/// The path to show a member who wants to download an approved file.
#[must_use]
pub fn download_path(area: AreaId, area_name: &str, file: FileId, file_name: &str) -> String {
    format!(
        "/{}/{}",
        area_dir_name(area, area_name),
        file_entry_name(file, file_name)
    )
}
