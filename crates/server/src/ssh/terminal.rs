//! Terminal facts from the client: window size and keypresses.
//!
//! Both come from an untrusted peer, so sizes are clamped (a 65535x65535 window would allocate
//! gigabytes of cells) and the key parser never fails.
//!
//! Keys are parsed by [`KeyParser`], which keeps its state between SSH data packets (D50): an
//! escape sequence cut in two by the network is still one key, and a lone `Esc` becomes the
//! Escape key after a short timeout instead of being guessed from where a packet ends.

use std::time::{Duration, Instant};

/// Size used when the client reports 0 (unknown) for a dimension.
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
/// Largest size honored. Bigger windows are clamped, bounding render memory per session.
pub const MAX_COLS: u16 = 500;
/// See [`MAX_COLS`].
pub const MAX_ROWS: u16 = 200;

/// Widest and tallest area a session draws into (D53). A session's render buffers hold at most
/// `2 * cols * rows` cells however large a window the client asks for; the rest of a bigger
/// window stays blank. The sysop sets it (`ssh.render_cols`, `ssh.render_rows`); the default is
/// the whole clamped window, so by default nothing is cut off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderCap {
    cols: u16,
    rows: u16,
}

impl RenderCap {
    /// Smallest cap accepted: the classic 80x24 screen.
    pub const MIN_COLS: u16 = 80;
    /// See [`RenderCap::MIN_COLS`].
    pub const MIN_ROWS: u16 = 24;

    /// A cap of `cols` x `rows` cells, kept within `MIN..=MAX` in each dimension.
    #[must_use]
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols.clamp(Self::MIN_COLS, MAX_COLS),
            rows: rows.clamp(Self::MIN_ROWS, MAX_ROWS),
        }
    }

    /// Width in cells.
    #[must_use]
    pub fn cols(self) -> u16 {
        self.cols
    }

    /// Height in cells.
    #[must_use]
    pub fn rows(self) -> u16 {
        self.rows
    }

    /// Cells in one render buffer at the cap.
    #[must_use]
    pub fn cells(self) -> usize {
        usize::from(self.cols) * usize::from(self.rows)
    }

    /// Bytes one session's two render buffers take at the cap. For the sysop's memory bill:
    /// multiply by the connection cap.
    #[must_use]
    pub fn bytes_per_session(self) -> usize {
        2 * self.cells() * std::mem::size_of::<ratatui::buffer::Cell>()
    }
}

impl Default for RenderCap {
    /// The whole clamped window.
    fn default() -> Self {
        Self::new(MAX_COLS, MAX_ROWS)
    }
}

/// A terminal size in character cells, always within `1..=MAX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    cols: u16,
    rows: u16,
}

impl PtySize {
    /// Build a size from the raw `pty-req` or `window-change` values.
    ///
    /// Zero means "unknown" in SSH and becomes the 80x24 default for that dimension; values
    /// above the limits are clamped.
    #[must_use]
    pub fn from_request(cols: u32, rows: u32) -> Self {
        Self {
            cols: clamp(cols, DEFAULT_COLS, MAX_COLS),
            rows: clamp(rows, DEFAULT_ROWS, MAX_ROWS),
        }
    }

    /// Width in cells.
    #[must_use]
    pub fn cols(self) -> u16 {
        self.cols
    }

    /// Height in cells.
    #[must_use]
    pub fn rows(self) -> u16 {
        self.rows
    }
}

fn clamp(value: u32, default: u16, max: u16) -> u16 {
    if value == 0 {
        return default;
    }
    u16::try_from(value).map_or(max, |v| v.min(max))
}

/// A keypress, as far as the waiting room cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A printable ASCII character.
    Char(char),
    /// Control plus a letter, lowercase (`Ctrl-C` is `Ctrl('c')`).
    Ctrl(char),
    /// Return (CR or LF).
    Enter,
    /// Backspace (BS or DEL).
    Backspace,
    /// The Escape key: an `Esc` byte that no sequence followed within [`ESC_TIMEOUT`].
    Escape,
    /// The up arrow (a lightbar cursor key, M3-A).
    Up,
    /// The down arrow.
    Down,
    /// Anything else: other arrows, function keys, Alt chords, mouse reports, non-ASCII text. Consumed
    /// and ignored.
    Other,
}

impl Key {
    /// Whether this key asks to leave: `q`, `Q`, `Ctrl-C` or `Ctrl-D`.
    #[must_use]
    pub fn is_quit(self) -> bool {
        matches!(self, Key::Char('q' | 'Q') | Key::Ctrl('c' | 'd'))
    }
}

/// How long a held `Esc` (or the start of a sequence) waits for the rest before it is read as
/// the Escape key. Longer than a network hiccup between two packets of one key press, shorter
/// than a person notices.
pub const ESC_TIMEOUT: Duration = Duration::from_millis(100);

/// How long a bracketed paste may sit without input before the parser stops waiting for its end
/// marker. A paste whose end marker was lost must not leave a session stuck in paste mode.
pub const PASTE_IDLE: Duration = Duration::from_secs(1);

/// Most bytes one bracketed paste may contain before the parser leaves paste mode by itself.
const MAX_PASTE: usize = 16 * 1024;

const ESC: u8 = 0x1b;
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// A stateful key parser for the bytes of a PTY session.
///
/// The escape-sequence, UTF-8 and control-character handling is the `anes` crate's parser (D50).
/// This wrapper adds the two things a network needs on top of it:
///
/// - **Holding a partial start.** An `Esc` byte, or the first bytes of a sequence that might be a
///   bracketed-paste marker, are held back until the next byte settles what they are. If the rest
///   arrives in another packet it is joined; if nothing arrives, [`KeyParser::expire`] (called at
///   [`KeyParser::deadline`]) turns a lone `Esc` into [`Key::Escape`].
/// - **Bracketed paste.** Between `ESC [ 200 ~` and `ESC [ 201 ~` the bytes are text, not
///   commands: printable ASCII becomes [`Key::Char`], and everything else (newlines, tabs,
///   escapes, controls, non-ASCII) is dropped, so a paste can neither press Enter nor quit.
///   Where the text is used it still passes the shared sanitizer.
///
/// A sequence that anes accepted into its own state and that is then cut off (for example
/// `ESC [ 1 ;` and silence) is not timed out: it ends at its final byte, whenever that arrives.
pub struct KeyParser {
    parser: anes::parser::Parser,
    /// Bytes held back because they are a proper prefix of a paste marker (and always start
    /// with `Esc`).
    held: Vec<u8>,
    paste: bool,
    paste_len: usize,
    /// When the last input arrived, to time out `held` and an open paste.
    last_input: Option<Instant>,
}

impl Default for KeyParser {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for KeyParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyParser")
            .field("held", &self.held.len())
            .field("paste", &self.paste)
            .finish_non_exhaustive()
    }
}

impl KeyParser {
    /// A parser with nothing pending.
    #[must_use]
    pub fn new() -> Self {
        Self {
            parser: anes::parser::Parser::default(),
            held: Vec::new(),
            paste: false,
            paste_len: 0,
            last_input: None,
        }
    }

    /// Feed one packet of input received at `now`. Returns the keys it completed; a trailing
    /// partial sequence stays inside the parser for the next call.
    pub fn feed(&mut self, bytes: &[u8], now: Instant) -> Vec<Key> {
        let mut keys = Vec::new();
        for &byte in bytes {
            if self.paste {
                self.paste_byte(byte, &mut keys);
            } else {
                self.plain_byte(byte, &mut keys);
            }
        }
        if !bytes.is_empty() {
            self.last_input = Some(now);
        }
        keys
    }

    /// When [`KeyParser::expire`] should be called if no input arrives first, or `None` when
    /// nothing is waiting.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        let wait = if self.paste {
            PASTE_IDLE
        } else if self.held.is_empty() {
            return None;
        } else {
            ESC_TIMEOUT
        };
        self.last_input.map(|at| at + wait)
    }

    /// Give up waiting for the rest of a held sequence: a lone `Esc` is the Escape key, a longer
    /// stub is dropped (one [`Key::Other`]), and an open paste is closed.
    pub fn expire(&mut self) -> Vec<Key> {
        let mut keys = Vec::new();
        if self.paste {
            self.paste = false;
        }
        match self.held.as_slice() {
            [] => {}
            [ESC] => keys.push(Key::Escape),
            _ => keys.push(Key::Other),
        }
        self.held.clear();
        self.parser = anes::parser::Parser::default();
        keys
    }

    fn plain_byte(&mut self, byte: u8, keys: &mut Vec<Key>) {
        let mut candidate = std::mem::take(&mut self.held);
        candidate.push(byte);
        if PASTE_START.starts_with(&candidate) {
            if candidate.len() == PASTE_START.len() {
                self.paste = true;
                self.paste_len = 0;
            } else {
                self.held = candidate;
            }
            return;
        }
        if byte == ESC {
            // The held bytes were not a marker; they stand as they are, and this Esc starts a
            // new candidate.
            candidate.pop();
            self.run_parser(&candidate, keys);
            self.held.push(ESC);
        } else {
            self.run_parser(&candidate, keys);
        }
    }

    fn paste_byte(&mut self, byte: u8, keys: &mut Vec<Key>) {
        let mut candidate = std::mem::take(&mut self.held);
        candidate.push(byte);
        if PASTE_END.starts_with(&candidate) {
            if candidate.len() == PASTE_END.len() {
                self.paste = false;
            } else {
                self.held = candidate;
            }
            return;
        }
        // Not the end marker: what was held is paste text, and this byte may start a new
        // candidate.
        candidate.pop();
        for text in candidate {
            self.paste_text(text, keys);
        }
        if byte == ESC {
            self.held.push(ESC);
        } else {
            self.paste_text(byte, keys);
        }
    }

    fn paste_text(&mut self, byte: u8, keys: &mut Vec<Key>) {
        if self.paste_len >= MAX_PASTE {
            self.paste = false;
            return;
        }
        self.paste_len += 1;
        if (0x20..=0x7e).contains(&byte) {
            keys.push(Key::Char(char::from(byte)));
        }
    }

    /// Hand a settled run of bytes to anes as a complete chunk, so it never keeps a bare `Esc`.
    fn run_parser(&mut self, bytes: &[u8], keys: &mut Vec<Key>) {
        if bytes.is_empty() {
            return;
        }
        self.parser.advance(bytes, false);
        for sequence in self.parser.by_ref() {
            keys.push(key_of(sequence));
        }
    }
}

/// Map an anes sequence to our small key set. Control characters arrive from anes as plain
/// characters, so they are classified here.
fn key_of(sequence: anes::parser::Sequence) -> Key {
    use anes::parser::{KeyCode, Sequence};
    let Sequence::Key(code, modifiers) = sequence else {
        return Key::Other;
    };
    if !modifiers.is_empty() {
        return Key::Other;
    }
    match code {
        KeyCode::Enter => Key::Enter,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Esc => Key::Escape,
        KeyCode::Tab => Key::Ctrl('i'),
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Char(c) => match c {
            '\x1b' => Key::Escape,
            '\r' | '\n' => Key::Enter,
            '\x08' | '\x7f' => Key::Backspace,
            '\x01'..='\x1a' => u8::try_from(u32::from(c) + 0x60)
                .map_or(Key::Other, |letter| Key::Ctrl(char::from(letter))),
            ' '..='~' => Key::Char(c),
            _ => Key::Other,
        },
        _ => Key::Other,
    }
}

/// Parse a complete input stream in one go, treating its end as the timeout. For tests and
/// one-shot use; a session uses a long-lived [`KeyParser`].
#[must_use]
pub fn parse_keys(bytes: &[u8]) -> Vec<Key> {
    let mut parser = KeyParser::new();
    let mut keys = parser.feed(bytes, Instant::now());
    keys.extend(parser.expire());
    keys
}

#[cfg(test)]
mod tests;
