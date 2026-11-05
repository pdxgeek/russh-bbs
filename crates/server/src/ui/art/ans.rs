//! The `.ans` parser: CP437 bytes with ANSI colour in, a [`Grid`] of cells out.
//!
//! **Nothing is passed through.** The parser does not produce a byte stream; it produces cells.
//! A sequence it understands changes the pen or moves the cursor inside the grid; every other
//! sequence is consumed and forgotten, so no escape code in an art file can ever reach a
//! terminal.
//!
//! | Understood | Effect |
//! |---|---|
//! | SGR `0`, `1`, `22`, `5`, `25`, `7`, `27`, `30-37`, `39`, `40-47`, `49`, `90-97`, `100-107` | the pen's colours. Bold becomes the bright colour; reverse swaps the colours; blink is the bright background only when the file's SAUCE flag asks for iCE colours, and is ignored otherwise. Extended colours (`38`, `48`) are skipped whole |
//! | Cursor `A B C D E F G H f s u` | moves inside the grid, clamped to its width and to [`MAX_ROWS`] |
//! | CR, LF, TAB, BS | the usual, inside the grid |
//! | `ESC [ 2 J`, `K` and every other CSI | dropped |
//! | OSC, DCS, SOS, PM, APC, two-byte escapes | dropped whole, payload included |
//! | other control bytes, DEL | dropped |
//!
//! **Caps.** [`MAX_BYTES`] bytes in, [`MAX_COLS`] columns and [`MAX_ROWS`] rows out. Input over
//! the byte cap, or content that would need more than the row cap, is refused with a reason; the
//! parser never allocates more than the grid cap, whatever the input. SAUCE records (and the
//! `COMNT` block) are read and stripped; a `Ctrl-Z` ends the picture.

use super::cp437;
use super::grid::{ArtCell, ArtError, Grid, MAX_COLS, MAX_ROWS, cell_for};

/// Most bytes of `.ans` input.
pub const MAX_BYTES: usize = 64 * 1024;
/// Widest picture when the file does not say (the classic width).
const DEFAULT_COLS: usize = 80;
/// Most numbers in one CSI; the rest are ignored.
const MAX_PARAMS: usize = 16;
/// Longest escape sequence read before it is abandoned (the introducer stays dropped).
const MAX_SEQUENCE: usize = 256;
/// Largest number a CSI parameter can hold.
const PARAM_MAX: usize = 9999;

const SAUCE_LEN: usize = 128;
const COMMENT_LINE: usize = 64;

/// What a SAUCE record said, sanitized.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sauce {
    /// The title field.
    pub title: String,
    /// The author field.
    pub author: String,
    /// The group field.
    pub group: String,
}

/// What [`parse`] makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The picture.
    pub grid: Grid,
    /// The SAUCE record, if the file had one.
    pub sauce: Option<Sauce>,
}

/// Turn `.ans` bytes into a picture.
///
/// # Errors
///
/// [`ArtError::TooManyBytes`] over the byte cap, [`ArtError::TooWide`] if SAUCE asks for more
/// than [`MAX_COLS`], [`ArtError::TooTall`] if the picture needs more than [`MAX_ROWS`] rows,
/// [`ArtError::Empty`] if nothing visible was drawn.
pub fn parse(bytes: &[u8]) -> Result<Parsed, ArtError> {
    if bytes.len() > MAX_BYTES {
        return Err(ArtError::TooManyBytes(bytes.len(), MAX_BYTES));
    }
    let (body, record) = split_sauce(bytes);
    let width = record
        .as_ref()
        .and_then(|r| r.columns)
        .map_or(DEFAULT_COLS, usize::from);
    if width > MAX_COLS {
        return Err(ArtError::TooWide);
    }
    let ice = record.as_ref().is_some_and(|r| r.ice);
    let grid = Canvas::new(width.max(1), ice).run(body)?;
    if grid.is_empty() {
        return Err(ArtError::Empty);
    }
    Ok(Parsed {
        grid,
        sauce: record.map(|r| r.sauce),
    })
}

struct Record {
    sauce: Sauce,
    columns: Option<u16>,
    ice: bool,
}

/// Split off a SAUCE record (and its comment block) from the end of the file.
fn split_sauce(bytes: &[u8]) -> (&[u8], Option<Record>) {
    let end = bytes.iter().position(|b| *b == 0x1a).unwrap_or(bytes.len());
    let Some(start) = bytes.len().checked_sub(SAUCE_LEN) else {
        return (&bytes[..end], None);
    };
    let tail = &bytes[start..];
    if !tail.starts_with(b"SAUCE00") {
        return (&bytes[..end], None);
    }
    let field = |from: usize, to: usize| clean_field(&tail[from..to]);
    let read_u16 = |at: usize| u16::from_le_bytes([tail[at], tail[at + 1]]);
    let data_type = tail[94];
    let file_type = tail[95];
    // ANSi (character data, type 1) carries its width in TInfo1; other types say nothing here.
    let columns = (data_type == 1 && file_type <= 2)
        .then(|| read_u16(96))
        .filter(|c| *c > 0);
    let ice = data_type == 1 && file_type <= 1 && tail[105] & 1 == 1;
    let sauce = Sauce {
        title: field(7, 42),
        author: field(42, 62),
        group: field(62, 82),
    };
    // The picture ends where the EOF marker, the comment block or the record begins.
    let comments = usize::from(tail[104]);
    let block = comments
        .checked_mul(COMMENT_LINE)
        .and_then(|n| n.checked_add(5))
        .and_then(|n| start.checked_sub(n))
        .filter(|at| bytes[*at..].starts_with(b"COMNT"));
    let cut = block.unwrap_or(start).min(end);
    (
        &bytes[..cut],
        Some(Record {
            sauce,
            columns,
            ice,
        }),
    )
}

/// A SAUCE text field (CP437, space or NUL padded) as sanitized text.
fn clean_field(raw: &[u8]) -> String {
    let text: String = raw
        .iter()
        .map(|b| if *b == 0 { ' ' } else { cp437::char_of(*b) })
        .collect();
    let cleaned = russh_bbs_core::sanitize::SingleLine::new(raw.len()).sanitize(&text);
    cleaned.as_str().trim().to_owned()
}

/// The pen: the colours new cells are drawn in.
#[derive(Debug, Clone, Copy, Default)]
struct Pen {
    fg: Option<u8>,
    bg: Option<u8>,
    bold: bool,
    blink: bool,
    reverse: bool,
}

impl Pen {
    /// The colours a cell gets: bold is the bright half, iCE blink the bright background, and
    /// reverse swaps the two (no cell has a reverse attribute).
    fn colours(self, ice: bool) -> (Option<u8>, Option<u8>) {
        let fg = match (self.fg, self.bold) {
            (Some(i), true) if i < 8 => Some(i + 8),
            (None, true) => Some(15),
            (fg, _) => fg,
        };
        let bg = match (self.bg, self.blink && ice) {
            (Some(i), true) if i < 8 => Some(i + 8),
            (None, true) => Some(8),
            (bg, _) => bg,
        };
        if self.reverse {
            // A reversed default is the terminal's own pair: light grey on black.
            (Some(bg.unwrap_or(0)), Some(fg.unwrap_or(7)))
        } else {
            (fg, bg)
        }
    }
}

struct Canvas {
    width: usize,
    ice: bool,
    cells: Vec<ArtCell>,
    rows_used: usize,
    cols_used: usize,
    x: usize,
    y: usize,
    saved: (usize, usize),
    pen: Pen,
}

impl Canvas {
    fn new(width: usize, ice: bool) -> Self {
        Self {
            width,
            ice,
            cells: Vec::new(),
            rows_used: 0,
            cols_used: 0,
            x: 0,
            y: 0,
            saved: (0, 0),
            pen: Pen::default(),
        }
    }

    fn run(mut self, bytes: &[u8]) -> Result<Grid, ArtError> {
        let mut at = 0;
        while at < bytes.len() {
            let byte = bytes[at];
            at += 1;
            match byte {
                0x1b => at = self.escape(bytes, at),
                0x0d => self.x = 0,
                0x0a => {
                    self.x = 0;
                    self.y = (self.y + 1).min(MAX_ROWS);
                }
                0x09 => self.x = ((self.x / 8 + 1) * 8).min(self.width - 1),
                0x08 => self.x = self.x.saturating_sub(1),
                0x1a => break,
                0x00..=0x1f | 0x7f => {}
                _ => self.put(cp437::char_of(byte))?,
            }
        }
        Ok(self.finish())
    }

    /// Draw one character at the cursor and advance, wrapping at the right edge.
    fn put(&mut self, c: char) -> Result<(), ArtError> {
        if self.x >= self.width {
            self.x = 0;
            self.y += 1;
        }
        if self.y >= MAX_ROWS {
            // A blank cell below the cap is not a picture; only ink counts.
            return if c == ' ' && self.pen.bg.is_none() {
                Ok(())
            } else {
                Err(ArtError::TooTall)
            };
        }
        let (fg, bg) = self.pen.colours(self.ice);
        let cell = cell_for(c, fg, bg);
        let at = self.y * self.width + self.x;
        if self.cells.len() <= at {
            self.cells.resize(at + 1, ArtCell::BLANK);
        }
        self.cells[at] = cell;
        if cell != ArtCell::BLANK {
            self.rows_used = self.rows_used.max(self.y + 1);
            self.cols_used = self.cols_used.max(self.x + 1);
        }
        self.x += 1;
        Ok(())
    }

    /// After an `ESC` at `bytes[at - 1]`: handle the sequence and return where reading goes on.
    fn escape(&mut self, bytes: &[u8], at: usize) -> usize {
        match bytes.get(at) {
            Some(b'[') => self.csi(bytes, at + 1),
            // OSC, DCS, SOS, PM, APC: a string that runs to BEL (OSC only) or ST (`ESC \`).
            Some(b']' | b'P' | b'X' | b'^' | b'_') => skip_string(bytes, at + 1),
            // Two-byte escapes, with any intermediates (`ESC ( B`, `ESC # 8`).
            Some(0x20..=0x2f) => {
                let mut end = at;
                while bytes.get(end).is_some_and(|b| (0x20..=0x2f).contains(b)) {
                    end += 1;
                }
                (end + 1).min(bytes.len())
            }
            Some(_) => at + 1,
            None => at,
        }
    }

    /// A CSI whose parameters start at `from`.
    fn csi(&mut self, bytes: &[u8], from: usize) -> usize {
        let mut params: Vec<usize> = Vec::new();
        let mut current: Option<usize> = None;
        let mut private = false;
        let mut at = from;
        let limit = bytes.len().min(from + MAX_SEQUENCE);
        while at < limit {
            let b = bytes[at];
            at += 1;
            match b {
                b'0'..=b'9' => {
                    let digit = usize::from(b - b'0');
                    current = Some(
                        current
                            .unwrap_or(0)
                            .saturating_mul(10)
                            .saturating_add(digit)
                            .min(PARAM_MAX),
                    );
                }
                b';' => {
                    if params.len() < MAX_PARAMS {
                        params.push(current.take().unwrap_or(0));
                    } else {
                        current = None;
                    }
                }
                b'<'..=b'?' => private = true,
                0x20..=0x2f => {}
                0x40..=0x7e => {
                    if params.len() < MAX_PARAMS {
                        params.push(current.unwrap_or(0));
                    }
                    if !private {
                        self.apply(b, &params);
                    }
                    return at;
                }
                // Anything else ends the sequence, unread: it is an ordinary byte again.
                _ => return at - 1,
            }
        }
        limit
    }

    fn apply(&mut self, final_byte: u8, params: &[usize]) {
        // A missing or zero count means one for the movement commands.
        let count = params.first().copied().filter(|n| *n > 0).unwrap_or(1);
        let last = self.width - 1;
        match final_byte {
            b'm' => self.sgr(params),
            b'A' => self.y = self.y.saturating_sub(count),
            b'B' => self.y = (self.y + count).min(MAX_ROWS - 1),
            b'C' => self.x = (self.x + count).min(last),
            b'D' => self.x = self.x.saturating_sub(count),
            b'E' => {
                self.x = 0;
                self.y = (self.y + count).min(MAX_ROWS - 1);
            }
            b'F' => {
                self.x = 0;
                self.y = self.y.saturating_sub(count);
            }
            b'G' => self.x = (count - 1).min(last),
            b'H' | b'f' => {
                let row = params.first().copied().filter(|n| *n > 0).unwrap_or(1);
                let col = params.get(1).copied().filter(|n| *n > 0).unwrap_or(1);
                self.y = (row - 1).min(MAX_ROWS - 1);
                self.x = (col - 1).min(last);
            }
            b's' => self.saved = (self.x, self.y),
            b'u' => (self.x, self.y) = self.saved,
            // Clears, erases, scrolls, modes, reports: dropped.
            _ => {}
        }
    }

    fn sgr(&mut self, params: &[usize]) {
        let mut at = 0;
        while at < params.len() {
            let code = params[at];
            at += 1;
            match code {
                0 => self.pen = Pen::default(),
                1 => self.pen.bold = true,
                22 => self.pen.bold = false,
                5 | 6 => self.pen.blink = true,
                25 => self.pen.blink = false,
                7 => self.pen.reverse = true,
                27 => self.pen.reverse = false,
                30..=37 => self.pen.fg = Some(ansi_to_vga(code - 30)),
                39 => self.pen.fg = None,
                40..=47 => self.pen.bg = Some(ansi_to_vga(code - 40)),
                49 => self.pen.bg = None,
                90..=97 => self.pen.fg = Some(ansi_to_vga(code - 90) + 8),
                100..=107 => self.pen.bg = Some(ansi_to_vga(code - 100) + 8),
                // Extended colours: skip `5;n` or `2;r;g;b` so the numbers are not read as codes.
                38 | 48 => {
                    at += match params.get(at) {
                        Some(5) => 2,
                        Some(2) => 4,
                        _ => 1,
                    };
                }
                _ => {}
            }
        }
    }

    fn finish(self) -> Grid {
        let (width, height) = (self.cols_used, self.rows_used);
        let mut grid = Grid::blank(width, height);
        for y in 0..height {
            for x in 0..width {
                if let Some(cell) = self.cells.get(y * self.width + x) {
                    grid.set(x, y, *cell);
                }
            }
        }
        grid
    }
}

/// ANSI colour order (black, red, green, yellow, blue, magenta, cyan, white) to the palette
/// index the grid uses, which is the VGA order art is drawn in (black, blue, green, cyan, red,
/// magenta, brown, grey).
const fn ansi_to_vga(n: usize) -> u8 {
    const MAP: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];
    MAP[n & 7]
}

/// Skip a string sequence whose body starts at `from`, up to and including its terminator
/// (`BEL` or `ESC \`), or to the cap. Returns where reading goes on.
fn skip_string(bytes: &[u8], from: usize) -> usize {
    let limit = bytes.len().min(from + MAX_SEQUENCE * 16);
    let mut at = from;
    while at < limit {
        match bytes[at] {
            0x07 => return at + 1,
            0x1b if bytes.get(at + 1) == Some(&b'\\') => return at + 2,
            // A bare ESC starts something new; leave it for the main loop.
            0x1b => return at,
            _ => at += 1,
        }
    }
    limit
}
