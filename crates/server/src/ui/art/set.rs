//! The art a board has: the pieces that ship with the server and the sysop's own, and the rule
//! for choosing one for a slot.
//!
//! **Where sysop art lives.** In `art/` beside the host key (like `themes/`), no config key:
//!
//! * `*.art.toml`: a set of pieces ([`super::file`]);
//! * `<slot>.ans` or `<style>.<slot>.ans`: one CP437 ANSI picture for a slot (`login.ans`,
//!   `neon.menu.ans`), parsed by [`super::ans`].
//!
//! A bad file is skipped with a logged reason and listed on the sysop preview screen; it never
//! stops the boot. At most [`MAX_FILES`] files are read, and a symbolic link is never followed.
//!
//! **Choosing.** For a slot, a style and the room available: the pieces for that slot made for
//! that style or for any style, the sysop's before the bundled, the style's own before the
//! generic, and among those the largest that fits.

use std::fs;
use std::io::Read;
use std::path::Path;

use russh_bbs_core::terminal_settings::ThemeName;
use tracing::warn;

use super::ans;
use super::bundled;
use super::file;
use super::grid::ArtError;
use super::piece::{ArtPiece, Slot, check_text};

/// Most files read from the art directory.
pub const MAX_FILES: usize = 64;

/// A file that was refused, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The file's name.
    pub file: String,
    /// What was wrong.
    pub reason: String,
}

/// Every piece the board can draw.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArtSet {
    pieces: Vec<ArtPiece>,
    problems: Vec<Problem>,
}

impl ArtSet {
    /// The art that ships with the server.
    ///
    /// # Errors
    ///
    /// [`ArtError`] if a bundled file is invalid, which is a build bug that a test also catches.
    pub fn bundled() -> Result<Self, ArtError> {
        let mut pieces = Vec::new();
        for text in bundled::FILES {
            pieces.extend(file::parse(text, false)?.pieces);
        }
        Ok(Self {
            pieces,
            problems: Vec::new(),
        })
    }

    /// Add the sysop's files from `dir`. Returns how many pieces were added. A missing
    /// directory adds nothing; a bad file is logged and recorded in [`ArtSet::problems`].
    pub fn add_dir(&mut self, dir: &Path) -> usize {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return 0,
            Err(error) => {
                warn!(dir = %dir.display(), %error, "art directory unreadable; using bundled art only");
                return 0;
            }
        };
        let mut names: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
        names.sort();
        let mut added = 0;
        for (seen, path) in names.iter().enumerate() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_owned();
            let lower = name.to_ascii_lowercase();
            let kind = if lower.ends_with(".art.toml") {
                Kind::Toml
            } else if Path::new(&lower).extension().is_some_and(|e| e == "ans") {
                Kind::Ans
            } else {
                continue;
            };
            if seen >= MAX_FILES {
                warn!(dir = %dir.display(), limit = MAX_FILES, "too many art files; the rest are ignored");
                self.problems.push(Problem {
                    file: name,
                    reason: format!("more than {MAX_FILES} files; ignored"),
                });
                break;
            }
            match load(path, &name, kind) {
                Ok(pieces) => {
                    added += pieces.len();
                    self.pieces.extend(pieces);
                }
                Err(error) => {
                    warn!(file = %path.display(), %error, "art refused; the bundled art stays");
                    self.problems.push(Problem {
                        file: name,
                        reason: error.to_string(),
                    });
                }
            }
        }
        added
    }

    /// Every piece, the bundled ones first.
    #[must_use]
    pub fn pieces(&self) -> &[ArtPiece] {
        &self.pieces
    }

    /// The sysop files that were refused, with reasons.
    #[must_use]
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// The pieces for `slot` that suit `style`, best first: the sysop's before the bundled, a
    /// style's own before the generic, then the largest first.
    #[must_use]
    pub fn candidates(&self, slot: Slot, style: ThemeName) -> Vec<&ArtPiece> {
        let mut found: Vec<&ArtPiece> = self
            .pieces
            .iter()
            .filter(|p| p.slot == slot && p.style.is_none_or(|s| s == style))
            .collect();
        found.sort_by_key(|p| {
            (
                std::cmp::Reverse(p.sysop),
                std::cmp::Reverse(p.style.is_some()),
                std::cmp::Reverse(p.grid.width() * p.grid.height()),
            )
        });
        found
    }

    /// The best piece for `slot` and `style` that fits in `width` columns and `height` rows.
    #[must_use]
    pub fn pick(&self, slot: Slot, style: ThemeName, width: u16, height: u16) -> Option<&ArtPiece> {
        self.candidates(slot, style)
            .into_iter()
            .find(|p| p.fits(width, height))
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Toml,
    Ans,
}

fn load(path: &Path, name: &str, kind: Kind) -> Result<Vec<ArtPiece>, ArtError> {
    let meta = fs::symlink_metadata(path).map_err(|e| ArtError::Invalid(e.to_string()))?;
    if !meta.file_type().is_file() {
        return Err(ArtError::Invalid(
            "not a regular file (links are not followed)".to_owned(),
        ));
    }
    let cap = match kind {
        Kind::Toml => file::MAX_BYTES,
        Kind::Ans => ans::MAX_BYTES,
    };
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| ArtError::Invalid(e.to_string()))?
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ArtError::Invalid(e.to_string()))?;
    match kind {
        Kind::Toml => {
            let text = String::from_utf8(bytes)
                .map_err(|_| ArtError::Invalid("not valid UTF-8".to_owned()))?;
            Ok(file::parse(&text, true)?.pieces)
        }
        Kind::Ans => Ok(vec![ans_piece(name, &bytes)?]),
    }
}

/// The picture a sysop page shows (UI-6): one file named by the page, read with the same caps and
/// parsers as `art/` (no link followed, bounded bytes, bounded grid), drawn in the area-header
/// size. An `.art.toml` file gives its first `area` piece; an `.ans` file is the picture itself.
pub(crate) fn page_piece(path: &Path, name: &str) -> Result<ArtPiece, ArtError> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".art.toml") {
        let pieces = load(path, name, Kind::Toml)?;
        return pieces
            .into_iter()
            .find(|piece| piece.slot == Slot::Area)
            .ok_or_else(|| {
                ArtError::Invalid("the file has no piece for the `area` slot".to_owned())
            });
    }
    if Path::new(&lower).extension().is_none_or(|e| e != "ans") {
        return Err(ArtError::Invalid(
            "page art must be an .art.toml or .ans file".to_owned(),
        ));
    }
    let meta = fs::symlink_metadata(path).map_err(|e| ArtError::Invalid(e.to_string()))?;
    if !meta.file_type().is_file() {
        return Err(ArtError::Invalid(
            "not a regular file (links are not followed)".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| ArtError::Invalid(e.to_string()))?
        .take(ans::MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ArtError::Invalid(e.to_string()))?;
    let parsed = ans::parse(&bytes)?;
    ArtPiece::check(Slot::Area, &parsed.grid)?;
    Ok(ArtPiece {
        slot: Slot::Area,
        style: None,
        set: name.trim_end_matches(".ans").to_owned(),
        credit: String::new(),
        fallback: None,
        grid: parsed.grid,
        ascii_given: false,
        sysop: true,
    })
}

/// A piece from an `.ans` file named `<slot>.ans` or `<style>.<slot>.ans`.
pub(crate) fn ans_piece(file_name: &str, bytes: &[u8]) -> Result<ArtPiece, ArtError> {
    let stem = file_name.trim_end_matches(".ans");
    let (style, slot_name) = match stem.split_once('.') {
        Some((style, slot)) => (Some(style), slot),
        None => (None, stem),
    };
    let slot = Slot::from_name(slot_name).ok_or_else(|| {
        ArtError::Invalid(format!(
            "the file name must be <slot>.ans or <style>.<slot>.ans with a slot of login, menu, area or logoff, not {file_name:?}"
        ))
    })?;
    let style = style
        .map(|s| {
            ThemeName::new(s).map_err(|_| {
                ArtError::Invalid("the style in the file name is not a style name".to_owned())
            })
        })
        .transpose()?;
    let parsed = ans::parse(bytes)?;
    ArtPiece::check(slot, &parsed.grid)?;
    let credit = parsed
        .sauce
        .as_ref()
        .map(|s| {
            let who = [s.author.as_str(), s.group.as_str()]
                .iter()
                .filter(|part| !part.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" / ");
            check_text("credit", &who).unwrap_or_default()
        })
        .unwrap_or_default();
    Ok(ArtPiece {
        slot,
        style,
        set: stem.to_owned(),
        credit,
        fallback: None,
        grid: parsed.grid,
        ascii_given: false,
        sysop: true,
    })
}
