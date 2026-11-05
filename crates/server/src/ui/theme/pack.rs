//! Theme packs (UI-7): one versioned file that carries a style and its art, so a sysop can move
//! a look between boards.
//!
//! ```toml
//! pack = 1                    # the container version
//! theme = '''                 # a theme file, exactly as it would sit in themes/
//! name = "harbour"
//! [colors]
//! ...
//! '''
//! [[art]]
//! kind = "art.toml"           # an .art.toml file, as it would sit in art/
//! text = '''...'''
//! [[art]]
//! kind = "ans"                # a CP437 .ans picture, written in hex
//! slot = "menu"
//! hex = "1b5b..."
//! ```
//!
//! **A pack holds files, and every file goes through the loader that reads the same file from
//! disk.** The `theme` text is parsed by [`Theme::parse`] (so the schema, the sizes, the
//! contrast floor, the string checks and the version migrations are the ones a theme file gets),
//! each `art.toml` by the art loader, each `.ans` by the `.ans` parser. Nothing in a pack is
//! executed, fetched or interpreted as anything but data. A pack cannot name a security marker
//! because neither a theme file nor art has any way to: there is no key, style or glyph for one
//! (D39), and the sanitizer replaces the reserved characters in every string and cell.
//!
//! **Caps.** The whole file is at most [`MAX_PACK_BYTES`]; at most [`MAX_ART_ENTRIES`] art
//! entries; a `.ans` is read from hex only up to the `.ans` cap before it is decoded.
//!
//! **Art belongs to its style.** Every art piece in a pack must be written for the pack's own
//! style (`style = "<name>"`, or the `.ans` slot, which is installed as `<name>.<slot>.ans`), so
//! importing a pack can never replace the board's generic art or another style's.
//!
//! **Install.** [`install`] writes the validated files into `themes/` and `art/` with
//! `create_new` (nothing is overwritten), and takes effect when the server next starts, like
//! any file the sysop drops there. **Export** writes only the style's own text and art: no
//! secret, no member data and no board setting is in either source.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use russh_bbs_core::terminal_settings::ThemeName;
use serde::Deserialize;

use super::{MAX_THEME_FILES, Theme, ThemeError, ThemeSet};
use crate::ui::art::{self, ANS_MAX_BYTES, ART_TOML_MAX_BYTES, ArtError, ArtSet, Slot};
use crate::ui::strings::StringTable;

/// The pack container version this server reads and writes.
pub const PACK_VERSION: u32 = 1;

/// Largest pack file, in bytes.
pub const MAX_PACK_BYTES: usize = 512 * 1024;

/// Most art entries in one pack.
pub const MAX_ART_ENTRIES: usize = 16;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPack {
    pack: u32,
    theme: String,
    #[serde(default)]
    art: Vec<RawArt>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArt {
    kind: String,
    slot: Option<String>,
    text: Option<String>,
    hex: Option<String>,
}

/// Why a pack was refused. Every message names what to fix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackError {
    /// The file is larger than [`MAX_PACK_BYTES`].
    #[error("pack is {0} bytes; the limit is {MAX_PACK_BYTES}")]
    TooLarge(usize),
    /// Not valid TOML, or a field of the wrong shape, or an unknown field.
    #[error("pack is not valid: {0}")]
    Syntax(String),
    /// A pack version this server does not read.
    #[error("pack version {0} is not understood (this server reads {PACK_VERSION})")]
    Version(u32),
    /// The style inside is refused.
    #[error("the theme in the pack is refused: {0}")]
    Theme(#[from] ThemeError),
    /// An art entry is refused.
    #[error("art entry {entry}: {reason}")]
    Art {
        /// 1-based position in the pack.
        entry: usize,
        /// What is wrong.
        reason: String,
    },
    /// More than [`MAX_ART_ENTRIES`] art entries.
    #[error("more than {MAX_ART_ENTRIES} art entries")]
    TooManyArt,
    /// A style of that name is already installed.
    #[error("a style called `{0}` is already installed; rename it in the pack and import again")]
    Exists(String),
    /// The themes or art directory is full (the loaders read at most so many files).
    #[error("the {0} directory already holds as many files as the board reads")]
    Full(&'static str),
    /// A file could not be written.
    #[error("could not write {file}: {reason}")]
    Write {
        /// The file.
        file: String,
        /// The operating system's reason.
        reason: String,
    },
    /// The style has no file to export (the built-in default).
    #[error("`{0}` is built into the server and has no file to export")]
    NotExportable(String),
    /// There is no such style.
    #[error("no style called `{0}` is installed")]
    Unknown(String),
}

/// A validated pack, ready to install.
#[derive(Debug)]
pub struct Pack {
    theme: Theme,
    theme_text: String,
    art: Vec<ArtFileOut>,
}

/// One art file a pack will write.
#[derive(Debug)]
struct ArtFileOut {
    /// The file name inside `art/`.
    file: String,
    bytes: Vec<u8>,
}

impl Pack {
    /// The style the pack carries.
    #[must_use]
    pub const fn theme(&self) -> &Theme {
        &self.theme
    }

    /// How many art files it carries.
    #[must_use]
    pub fn art_files(&self) -> usize {
        self.art.len()
    }
}

/// Parse and validate a pack over `base` (the default string table). Pure: it reads and writes
/// nothing.
///
/// # Errors
///
/// [`PackError`] naming the first thing wrong.
pub fn parse(text: &str, base: &StringTable) -> Result<Pack, PackError> {
    if text.len() > MAX_PACK_BYTES {
        return Err(PackError::TooLarge(text.len()));
    }
    let raw: RawPack =
        toml::from_str(text).map_err(|error| PackError::Syntax(error.message().to_owned()))?;
    if raw.pack != PACK_VERSION {
        return Err(PackError::Version(raw.pack));
    }
    let theme = Theme::parse(&raw.theme, base)?;
    let Some(theme_text) = theme.export_text() else {
        // `parse` always records its source; a theme without one cannot come from a file.
        return Err(PackError::Syntax("the theme has no text".to_owned()));
    };
    if raw.art.len() > MAX_ART_ENTRIES {
        return Err(PackError::TooManyArt);
    }
    let style = theme.name();
    let mut art = Vec::new();
    let mut seen_slots: Vec<Slot> = Vec::new();
    for (index, entry) in raw.art.iter().enumerate() {
        let at = index + 1;
        let fail = |reason: String| PackError::Art { entry: at, reason };
        match entry.kind.as_str() {
            "art.toml" => {
                if entry.slot.is_some() || entry.hex.is_some() {
                    return Err(fail("an art.toml entry has only `text`".to_owned()));
                }
                let body = entry
                    .text
                    .as_deref()
                    .ok_or_else(|| fail("an art.toml entry needs `text`".to_owned()))?;
                if body.len() > ART_TOML_MAX_BYTES {
                    return Err(fail(ArtError::TooManyBytes(body.len(), ART_TOML_MAX_BYTES).to_string()));
                }
                let parsed = art::parse_art_toml(body, true).map_err(|e| fail(e.to_string()))?;
                if parsed.pieces.iter().any(|p| p.style() != Some(style)) {
                    return Err(fail(format!(
                        "every piece must be made for this style: put `style = \"{style}\"` in the file"
                    )));
                }
                art.push(ArtFileOut {
                    file: format!("{style}.pack{at}.art.toml"),
                    bytes: body.as_bytes().to_vec(),
                });
            }
            "ans" => {
                if entry.text.is_some() {
                    return Err(fail("an ans entry has `slot` and `hex`, not `text`".to_owned()));
                }
                let slot = entry
                    .slot
                    .as_deref()
                    .and_then(Slot::from_name)
                    .ok_or_else(|| fail("slot must be login, menu, area or logoff".to_owned()))?;
                if seen_slots.contains(&slot) {
                    return Err(fail(format!("a second .ans for the {} slot", slot.name())));
                }
                seen_slots.push(slot);
                let hex = entry
                    .hex
                    .as_deref()
                    .ok_or_else(|| fail("an ans entry needs `hex`".to_owned()))?;
                let bytes = decode_hex(hex, ANS_MAX_BYTES).map_err(fail)?;
                let file = format!("{style}.{}.ans", slot.name());
                // The same function that loads a `.ans` from disk.
                art::ans_piece(&file, &bytes).map_err(|e| fail(e.to_string()))?;
                art.push(ArtFileOut { file, bytes });
            }
            other => {
                return Err(fail(format!("kind {other:?} is not art.toml or ans")));
            }
        }
    }
    Ok(Pack {
        theme,
        theme_text,
        art,
    })
}

/// Decode hex, refusing more than `cap` bytes **before** decoding so a bomb costs nothing.
fn decode_hex(hex: &str, cap: usize) -> Result<Vec<u8>, String> {
    let digits: Vec<u8> = hex.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if digits.len() > cap * 2 {
        return Err(format!("the picture is more than the {cap}-byte cap"));
    }
    if digits.len() % 2 != 0 || !digits.iter().all(u8::is_ascii_hexdigit) {
        return Err("`hex` must be an even number of hex digits".to_owned());
    }
    digits
        .chunks(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|e| e.to_string())?;
            u8::from_str_radix(text, 16).map_err(|e| e.to_string())
        })
        .collect()
}

/// A TOML string for `text` that a person can edit in place: a multi-line literal string when
/// the text allows one, else an escaped basic string.
fn block(text: &str) -> String {
    let plain = !text.contains("'''")
        && !text.ends_with('\'')
        && text.chars().all(|c| c == '\n' || c == '\t' || !c.is_control());
    if plain {
        format!("'''\n{text}'''")
    } else {
        art::quote(text)
    }
}

/// The pack for the style `name`: its theme file and its art, as pack text. Only what the style
/// authored: no board setting, no member data, no path.
///
/// # Errors
///
/// [`PackError::Unknown`] if no such style, [`PackError::NotExportable`] for the built-in one.
pub fn export(themes: &ThemeSet, art_set: &ArtSet, name: ThemeName) -> Result<String, PackError> {
    if !themes.contains(name) {
        return Err(PackError::Unknown(name.to_string()));
    }
    let theme = themes.get(name);
    let text = theme
        .export_text()
        .ok_or_else(|| PackError::NotExportable(name.to_string()))?;
    let mut out = format!("pack = {PACK_VERSION}\ntheme = {}\n", block(&text));
    let pieces: Vec<&art::ArtPiece> = art_set
        .pieces()
        .iter()
        .filter(|p| p.style() == Some(name))
        .collect();
    // One art.toml per set the style's pieces came from, so credits stay with their pieces.
    let mut sets: Vec<&str> = Vec::new();
    for piece in &pieces {
        if !sets.contains(&piece.set()) {
            sets.push(piece.set());
        }
    }
    for set in sets {
        let mine: Vec<&art::ArtPiece> = pieces.iter().copied().filter(|p| p.set() == set).collect();
        let credit = mine.first().map(|p| p.credit()).unwrap_or_default();
        let body = art::write_art_toml(name.as_str(), name, credit, &mine);
        out.push_str(&format!("\n[[art]]\nkind = \"art.toml\"\ntext = {}\n", block(&body)));
    }
    Ok(out)
}

/// Write a validated pack into `themes_dir` and `art_dir`. Nothing is overwritten (`create_new`),
/// and a failure part-way removes what this call wrote. Returns the files written.
///
/// # Errors
///
/// [`PackError::Exists`] if the style is installed or a target file is there,
/// [`PackError::Full`] if a directory is at the loaders' file cap, [`PackError::Write`] if the
/// disk refuses.
pub fn install(
    pack: &Pack,
    installed: &ThemeSet,
    themes_dir: &Path,
    art_dir: &Path,
) -> Result<Vec<PathBuf>, PackError> {
    let name = pack.theme.name();
    if installed.contains(name) {
        return Err(PackError::Exists(name.to_string()));
    }
    for (dir, extra, cap, what) in [
        (themes_dir, 1, MAX_THEME_FILES, "themes"),
        (art_dir, pack.art.len(), art::MAX_FILES, "art"),
    ] {
        let present = fs::read_dir(dir).map_or(0, Iterator::count);
        if present + extra > cap {
            return Err(PackError::Full(what));
        }
    }
    fs::create_dir_all(themes_dir).map_err(|e| write_error(themes_dir, &e))?;
    if !pack.art.is_empty() {
        fs::create_dir_all(art_dir).map_err(|e| write_error(art_dir, &e))?;
    }
    let mut written: Vec<PathBuf> = Vec::new();
    // Art first, the theme last: the style appears only when everything it needs is there.
    let files = pack
        .art
        .iter()
        .map(|file| (art_dir.join(&file.file), file.bytes.as_slice()))
        .chain([(
            themes_dir.join(format!("{name}.toml")),
            pack.theme_text.as_bytes(),
        )]);
    for (path, bytes) in files {
        let result = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut file| file.write_all(bytes));
        match result {
            Ok(()) => written.push(path),
            Err(error) => {
                let failure = if error.kind() == std::io::ErrorKind::AlreadyExists {
                    PackError::Exists(name.to_string())
                } else {
                    write_error(&path, &error)
                };
                // Remove only what this call created.
                for done in &written {
                    let _ = fs::remove_file(done);
                }
                return Err(failure);
            }
        }
    }
    Ok(written)
}

fn write_error(path: &Path, error: &std::io::Error) -> PackError {
    PackError::Write {
        file: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_owned(),
        reason: error.to_string(),
    }
}

/// Write `text` to `dir/<name>.pack.toml` (creating `dir`), refusing to replace a file.
///
/// # Errors
///
/// [`PackError::Write`] (also when the file is already there).
pub fn write_export(dir: &Path, name: ThemeName, text: &str) -> Result<PathBuf, PackError> {
    fs::create_dir_all(dir).map_err(|e| write_error(dir, &e))?;
    let path = dir.join(format!("{name}.pack.toml"));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|e| write_error(&path, &e))?;
    Ok(path)
}

/// A pack file found in the packs directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackFile {
    /// The file name.
    pub file: String,
    /// The style it carries, or why it is refused.
    pub verdict: Result<ThemeName, String>,
}

/// Most pack files listed.
pub const MAX_PACK_FILES: usize = 32;

/// Read one pack file, bounded (`take`), never following a link.
///
/// # Errors
///
/// [`PackError`] if it is not a regular file, is too large or is not valid UTF-8.
pub fn read_file(path: &Path) -> Result<String, PackError> {
    let meta = fs::symlink_metadata(path).map_err(|e| write_error(path, &e))?;
    if !meta.file_type().is_file() {
        return Err(PackError::Syntax("not a regular file (links are not followed)".to_owned()));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| write_error(path, &e))?
        .take(MAX_PACK_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| write_error(path, &e))?;
    if bytes.len() > MAX_PACK_BYTES {
        return Err(PackError::TooLarge(bytes.len()));
    }
    String::from_utf8(bytes).map_err(|_| PackError::Syntax("not valid UTF-8".to_owned()))
}

/// The `*.pack.toml` files in `dir`, sorted, each checked (nothing is installed). A missing
/// directory is an empty list.
#[must_use]
pub fn list_dir(dir: &Path, base: &StringTable) -> Vec<PackFile> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".pack.toml"))
        })
        .collect();
    paths.sort();
    paths
        .iter()
        .take(MAX_PACK_FILES)
        .map(|path| PackFile {
            file: path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_owned(),
            verdict: read_file(path)
                .and_then(|text| parse(&text, base))
                .map(|pack| pack.theme.name())
                .map_err(|e| e.to_string()),
        })
        .collect()
}

#[cfg(test)]
mod tests;
