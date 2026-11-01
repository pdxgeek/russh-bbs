//! Terminal facts from the client: window size and keypresses.
//!
//! Both come from an untrusted peer, so sizes are clamped (a 65535x65535 window would allocate
//! gigabytes of cells) and the key parser never fails.

/// Size used when the client reports 0 (unknown) for a dimension.
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
/// Largest size honored. Bigger windows are clamped, bounding render memory per session.
pub const MAX_COLS: u16 = 500;
/// See [`MAX_COLS`].
pub const MAX_ROWS: u16 = 200;

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
    /// Anything else: escape sequences, arrows, non-ASCII bytes. Consumed and ignored.
    Other,
}

impl Key {
    /// Whether this key asks to leave: `q`, `Q`, `Ctrl-C` or `Ctrl-D`.
    #[must_use]
    pub fn is_quit(self) -> bool {
        matches!(self, Key::Char('q' | 'Q') | Key::Ctrl('c' | 'd'))
    }
}

/// Split a chunk of input bytes into keypresses.
///
/// Escape sequences (`ESC [ ... final`, `ESC O x`, `ESC x`) are consumed whole so an arrow key
/// is never read as the letters in it. A sequence cut off at the end of a chunk is dropped; the
/// parser is stateless between chunks, which is fine for the placeholder screen.
#[must_use]
pub fn parse_keys(bytes: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut rest = bytes;
    while let Some((&first, tail)) = rest.split_first() {
        rest = tail;
        match first {
            0x1b => {
                rest = skip_escape(rest);
                keys.push(Key::Other);
            }
            0x01..=0x1a => keys.push(Key::Ctrl(char::from(first + 0x60))),
            0x20..=0x7e => keys.push(Key::Char(char::from(first))),
            _ => keys.push(Key::Other),
        }
    }
    keys
}

/// Skip the body of an escape sequence whose ESC was just consumed.
fn skip_escape(rest: &[u8]) -> &[u8] {
    match rest.split_first() {
        Some((b'[', tail)) => {
            // CSI: parameter and intermediate bytes, then one final byte in 0x40..=0x7e.
            let end = tail.iter().position(|b| (0x40..=0x7e).contains(b));
            end.map_or(&[][..], |i| tail.get(i + 1..).unwrap_or(&[]))
        }
        // SS3 (`ESC O x`, F-keys on many terminals): the final byte follows the `O`.
        Some((b'O', tail)) => tail.get(1..).unwrap_or(&[]),
        // Alt+key: exactly one more byte.
        Some((_, tail)) => tail,
        None => rest,
    }
}

#[cfg(test)]
mod tests;
