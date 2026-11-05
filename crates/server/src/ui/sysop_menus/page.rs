//! Sysop pages (UI-6): bulletins and info screens, art plus text, linked from menus.
//!
//! ```toml
//! version = 1
//! title = "House rules"
//! art = "rules.art.toml"     # optional: a file in the same directory (.art.toml or .ans)
//! text = """
//! # Be kind
//! Plain paragraphs wrap to the room there is.
//! - a bullet
//! > a quotation
//! ---
//! """
//! ```
//!
//! **Content, not chrome.** Every text line passes the shared sanitizer (control characters, ANSI
//! escapes, invisible and private-use characters become nothing or a replacement, and the
//! reserved marker codepoints are neutralized), so a page can never contain the reserved style,
//! a marker glyph or a Nerd Font glyph; and the page is drawn only in the body, with plain
//! palette styles, never over the status row (D39). Caps are enforced before anything is parsed
//! further: file size, line count, line length. A bad page refuses the whole menu load.
//!
//! The text is the sysop's own words. It is not checked for privacy claims (house rules may say
//! what they like), unlike a label or title, which is.

use std::path::Path;

use russh_bbs_core::menu::valid_page_id;
use russh_bbs_core::sanitize::SingleLine;
use serde::Deserialize;

use super::file::{FileError, check_lines, read_capped};
use crate::ui::art::{ArtPiece, page_piece};
use crate::ui::strings::{StringTable, check_board_text};

/// Largest page file, in bytes.
pub(crate) const MAX_BYTES: usize = 16 * 1024;
/// Most lines of text.
pub(crate) const MAX_LINES: usize = 200;
/// Longest line, in characters.
pub(crate) const MAX_LINE_CHARS: usize = 160;
/// Longest title, in characters.
pub(crate) const TITLE_MAX: usize = 32;
/// The only format version.
const VERSION: u32 = 1;

/// Why a page was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PageError {
    /// The file is missing.
    #[error("no file pages/<id>.toml")]
    Missing,
    /// Reading or capping the file failed.
    #[error("{0}")]
    File(#[from] FileError),
    /// More lines of text than the cap.
    #[error("more than {MAX_LINES} lines of text")]
    TooManyLines,
    /// A line of text longer than the cap.
    #[error("text line {0} is longer than {MAX_LINE_CHARS} characters")]
    LongLine(usize),
    /// The title is empty, long, unsafe or a claim.
    #[error("the title must be 1 to {TITLE_MAX} plain characters with no privacy claim")]
    BadTitle,
    /// The art file name is not a plain file name.
    #[error("art must be a plain file name ending in .art.toml or .ans")]
    BadArtName,
    /// The art file was refused.
    #[error("art: {0}")]
    Art(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    version: u32,
    title: String,
    art: Option<String>,
    #[serde(default)]
    text: String,
}

/// One block of a page's text, already sanitized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A heading (`# text`).
    Heading(String),
    /// A line of plain text.
    Text(String),
    /// A bullet (`- text`).
    Bullet(String),
    /// A quotation (`> text`).
    Quote(String),
    /// A rule (`---`).
    Rule,
    /// A blank line.
    Blank,
}

/// A loaded page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    id: String,
    title: String,
    art: Option<ArtPiece>,
    blocks: Vec<Block>,
}

impl Page {
    /// Its id (the file name without `.toml`).
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Its title (sanitized, a plain word or phrase).
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Its art, if it has some.
    #[must_use]
    pub const fn art(&self) -> Option<&ArtPiece> {
        self.art.as_ref()
    }

    /// Its text.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }
}

/// Load `dir/<id>.toml`.
pub(crate) fn load(dir: &Path, id: &str, strings: &StringTable) -> Result<Page, PageError> {
    if !valid_page_id(id) {
        return Err(PageError::Missing);
    }
    let path = dir.join(format!("{id}.toml"));
    let text = read_capped(&path, MAX_BYTES)?.ok_or(PageError::Missing)?;
    parse(id, &text, dir, strings)
}

/// Parse page `text`; `dir` is where its art lives.
pub(crate) fn parse(
    id: &str,
    text: &str,
    dir: &Path,
    strings: &StringTable,
) -> Result<Page, PageError> {
    let raw: Raw = toml::from_str(text)
        .map_err(|error: toml::de::Error| FileError::Syntax(error.message().to_owned()))?;
    if raw.version != VERSION {
        return Err(FileError::Version(raw.version).into());
    }
    let title = raw.title.trim();
    let title_ok = !title.is_empty()
        && title.chars().count() <= TITLE_MAX
        && SingleLine::new(TITLE_MAX + 1).sanitize(title).as_str() == title
        && strings.keys().all(|key| key != title)
        && check_board_text("a page title", title).is_ok();
    if !title_ok {
        return Err(PageError::BadTitle);
    }
    check_lines(&raw.text, MAX_LINES, MAX_LINE_CHARS).map_err(|error| match error {
        FileError::TooManyLines => PageError::TooManyLines,
        FileError::LongLine(at) => PageError::LongLine(at),
        other => PageError::File(other),
    })?;
    let blocks = raw.text.lines().map(block).collect();
    let art = match raw.art.as_deref() {
        None => None,
        Some(name) => {
            let plain = !name.is_empty()
                && name.len() <= 64
                && !name.starts_with('.')
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
                && (name.ends_with(".art.toml")
                    || std::path::Path::new(name)
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("ans")));
            if !plain {
                return Err(PageError::BadArtName);
            }
            Some(page_piece(&dir.join(name), name).map_err(|e| PageError::Art(e.to_string()))?)
        }
    };
    Ok(Page {
        id: id.to_owned(),
        title: title.to_owned(),
        art,
        blocks: trim_blank(blocks),
    })
}

/// Sanitize one source line and read its markup.
fn block(line: &str) -> Block {
    let clean = SingleLine::new(MAX_LINE_CHARS * 2)
        .sanitize(line)
        .into_string();
    let clean = clean.trim_end();
    if clean.trim().is_empty() {
        return Block::Blank;
    }
    if clean.trim() == "---" {
        return Block::Rule;
    }
    if let Some(rest) = clean.strip_prefix("# ") {
        return Block::Heading(rest.trim().to_owned());
    }
    if let Some(rest) = clean.strip_prefix("- ") {
        return Block::Bullet(rest.trim().to_owned());
    }
    if let Some(rest) = clean.strip_prefix("> ") {
        return Block::Quote(rest.trim().to_owned());
    }
    Block::Text(clean.to_owned())
}

/// Drop blank lines at both ends and collapse runs of blank lines to one.
fn trim_blank(blocks: Vec<Block>) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::with_capacity(blocks.len());
    for block in blocks {
        if block == Block::Blank && out.last().is_none_or(|last| *last == Block::Blank) {
            continue;
        }
        out.push(block);
    }
    while out.last() == Some(&Block::Blank) {
        out.pop();
    }
    out
}
