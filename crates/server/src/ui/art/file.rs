//! The `.art.toml` format: art for sysops who would rather not draw in an ANSI editor.
//!
//! ```toml
//! format = 1                   # required
//! name = "harbour"             # the set's name: a-z, 0-9, - and _
//! style = "paper"              # optional: made for this style (omit it for any style)
//! credit = "Jo Sysop"          # optional, up to 80 characters
//!
//! [[piece]]
//! slot = "login"               # login, menu, area or logoff
//! fallback = "{board}"         # optional: text shown when the piece does not fit
//! rows = [                     # the picture, one string per row
//!   "  ~~~~  ",
//!   "~ HOME ~",
//! ]
//! fg = [                       # optional colours, one character per cell
//!   "  YYYY  ",
//!   "Y CCCC Y",
//! ]
//! bg = []                      # optional, same shape
//! ascii = [ "..." ]            # optional: the ASCII tier's version of the rows
//! ```
//!
//! Colour characters: `.` the style's own, then `k b g c r m y w` for black, blue, green, cyan,
//! red, magenta, brown and grey, and `K B G C R M Y W` for their bright forms (the VGA palette
//! classic art is drawn in). A row shorter than its widest sibling is padded with blanks.
//!
//! **A piece is untrusted input.** The file is size-capped, unknown keys are refused, every
//! character goes through the shared sanitizer (controls become blanks, marker glyphs and
//! private-use characters become U+FFFD), and a piece over its slot's cap is refused.

use russh_bbs_core::terminal_settings::ThemeName;
use serde::Deserialize;

use super::grid::{ArtCell, ArtError, Grid, cell_for};
use super::piece::{ArtPiece, Slot, check_text};

/// Largest `.art.toml` file, in bytes.
pub const MAX_BYTES: usize = 64 * 1024;
/// Most pieces in one file.
pub(super) const MAX_PIECES: usize = 12;
/// The only format number.
const FORMAT: u32 = 1;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    format: u32,
    name: String,
    style: Option<String>,
    credit: Option<String>,
    #[serde(default)]
    piece: Vec<RawPiece>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPiece {
    slot: String,
    fallback: Option<String>,
    rows: Vec<String>,
    #[serde(default)]
    fg: Vec<String>,
    #[serde(default)]
    bg: Vec<String>,
    #[serde(default)]
    ascii: Vec<String>,
}

/// A set of pieces from one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtFile {
    /// The set's name.
    pub name: String,
    /// The pieces.
    pub pieces: Vec<ArtPiece>,
}

/// Parse a `.art.toml` file. `sysop` marks where the pieces came from.
///
/// # Errors
///
/// [`ArtError`] naming what is wrong: too big, not TOML, an unknown key, a bad name, slot or
/// colour character, a picture over its slot's cap, or no pieces.
pub fn parse(text: &str, sysop: bool) -> Result<ArtFile, ArtError> {
    if text.len() > MAX_BYTES {
        return Err(ArtError::TooManyBytes(text.len(), MAX_BYTES));
    }
    let raw: RawFile =
        toml::from_str(text).map_err(|error| ArtError::Invalid(format!("not valid: {error}")))?;
    if raw.format != FORMAT {
        return Err(invalid(format!(
            "format {} is not understood (this server reads format {FORMAT})",
            raw.format
        )));
    }
    let name = ThemeName::new(&raw.name)
        .map_err(|_| invalid("name must be 1 to 24 characters of a-z, 0-9, _ and -"))?
        .to_string();
    let style = raw
        .style
        .as_deref()
        .map(|s| {
            ThemeName::new(s).map_err(|_| invalid("style must be a style name (a-z, 0-9, _ and -)"))
        })
        .transpose()?;
    let credit = check_text("credit", raw.credit.as_deref().unwrap_or(""))?;
    if raw.piece.is_empty() {
        return Err(invalid("the file has no [[piece]]"));
    }
    if raw.piece.len() > MAX_PIECES {
        return Err(invalid(format!("more than {MAX_PIECES} pieces")));
    }
    let mut pieces = Vec::with_capacity(raw.piece.len());
    for (index, piece) in raw.piece.iter().enumerate() {
        let one = build(piece, &name, style, &credit, sysop)
            .map_err(|error| invalid(format!("piece {}: {error}", index + 1)))?;
        pieces.push(one);
    }
    Ok(ArtFile { name, pieces })
}

fn invalid(message: impl Into<String>) -> ArtError {
    ArtError::Invalid(message.into())
}

fn build(
    raw: &RawPiece,
    set: &str,
    style: Option<ThemeName>,
    credit: &str,
    sysop: bool,
) -> Result<ArtPiece, ArtError> {
    let slot = Slot::from_name(raw.slot.trim())
        .ok_or_else(|| invalid("slot must be login, menu, area or logoff"))?;
    let (max_cols, max_rows) = slot.cap();
    if raw.rows.len() > max_rows {
        return Err(invalid(format!(
            "{} rows is taller than the {max_rows}-row cap for the {} slot",
            raw.rows.len(),
            slot.name()
        )));
    }
    let rows: Vec<Vec<char>> = raw.rows.iter().map(|r| r.chars().collect()).collect();
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width > max_cols {
        return Err(invalid(format!(
            "{width} columns is wider than the {max_cols}-column cap for the {} slot",
            slot.name()
        )));
    }
    let fg = colour_rows("fg", &raw.fg, rows.len(), width)?;
    let bg = colour_rows("bg", &raw.bg, rows.len(), width)?;
    let ascii = ascii_rows(&raw.ascii, rows.len(), width)?;
    let mut grid = Grid::blank(width, rows.len());
    for (y, row) in rows.iter().enumerate() {
        for x in 0..width {
            let c = row.get(x).copied().unwrap_or(' ');
            let mut cell = cell_for(c, fg[y][x], bg[y][x]);
            if let Some(ascii) = &ascii {
                cell.ascii = ascii[y][x];
            }
            grid.set(x, y, cell);
        }
    }
    trim_blank_rows(&mut grid);
    ArtPiece::check(slot, &grid)?;
    let fallback = raw
        .fallback
        .as_deref()
        .map(|text| check_text("fallback", text))
        .transpose()?
        .filter(|text| !text.is_empty());
    Ok(ArtPiece {
        slot,
        style,
        set: set.to_owned(),
        credit: credit.to_owned(),
        fallback,
        grid,
        ascii_given: ascii.is_some(),
        sysop,
    })
}

/// Drop blank rows from the bottom, so a stray empty line does not claim a row of the screen.
fn trim_blank_rows(grid: &mut Grid) {
    let mut height = grid.height();
    while height > 0 && (0..grid.width()).all(|x| grid.get(x, height - 1) == ArtCell::BLANK) {
        height -= 1;
    }
    if height != grid.height() {
        let mut trimmed = Grid::blank(grid.width(), height);
        for y in 0..height {
            for x in 0..grid.width() {
                trimmed.set(x, y, grid.get(x, y));
            }
        }
        *grid = trimmed;
    }
}

type Colours = Vec<Vec<Option<u8>>>;

fn colour_rows(
    what: &str,
    rows: &[String],
    height: usize,
    width: usize,
) -> Result<Colours, ArtError> {
    if rows.len() > height {
        return Err(invalid(format!("{what} has more rows than rows")));
    }
    let mut out = vec![vec![None; width]; height];
    for (y, row) in rows.iter().enumerate() {
        let chars: Vec<char> = row.chars().collect();
        if chars.len() > width {
            return Err(invalid(format!("{what} row {} is wider than rows", y + 1)));
        }
        for (x, c) in chars.into_iter().enumerate() {
            out[y][x] = colour_of(c).map_err(|bad| {
                invalid(format!(
                    "{what} row {} has {bad:?}: use . k b g c r m y w K B G C R M Y W",
                    y + 1
                ))
            })?;
        }
    }
    Ok(out)
}

/// A colour character as a palette index: `.` is the style's own. `Err` carries the character
/// that is not a colour.
fn colour_of(c: char) -> Result<Option<u8>, char> {
    const NAMES: &str = "kbgcrmyw";
    if c == '.' {
        return Ok(None);
    }
    let base = NAMES
        .find(c.to_ascii_lowercase())
        .and_then(|at| u8::try_from(at).ok())
        .ok_or(c)?;
    Ok(Some(if c.is_ascii_uppercase() {
        base + 8
    } else {
        base
    }))
}

fn ascii_rows(
    rows: &[String],
    height: usize,
    width: usize,
) -> Result<Option<Vec<Vec<char>>>, ArtError> {
    if rows.is_empty() {
        return Ok(None);
    }
    if rows.len() > height {
        return Err(invalid("ascii has more rows than rows"));
    }
    let mut out = vec![vec![' '; width]; height];
    for (y, row) in rows.iter().enumerate() {
        let chars: Vec<char> = row.chars().collect();
        if chars.len() > width {
            return Err(invalid(format!("ascii row {} is wider than rows", y + 1)));
        }
        for (x, c) in chars.into_iter().enumerate() {
            if !(c == ' ' || c.is_ascii_graphic()) {
                return Err(invalid(format!(
                    "ascii row {} has {c:?}: only printable ASCII",
                    y + 1
                )));
            }
            out[y][x] = c;
        }
    }
    Ok(Some(out))
}
