//! The **bounded mouse-report parser** (UI-9).
//!
//! A terminal with mouse reporting on sends reports as escape sequences in the same byte stream
//! as the keys. The member's client is an untrusted peer, so this parser has three promises that
//! the tests (`tests.rs`, including property tests over arbitrary bytes) check:
//!
//! - **Bounded.** A report is at most [`MAX_REPORT`] bytes. A longer run that still looks like
//!   one is dropped to its final byte and discarded; nothing is buffered without limit.
//! - **Dropped, never echoed.** An invalid or oversized report produces no event and no key: its
//!   bytes are not handed to the key parser, so they cannot appear in a text field or be read as
//!   commands.
//! - **Never panics.** Any byte string, cut anywhere, is handled.
//!
//! Two encodings are read: **SGR 1006** (`ESC [ < b ; x ; y M` for a press or motion,
//! `... m` for a release; the one the board asks for) and the legacy **X10** encoding
//! (`ESC [ M` and three bytes). Coordinates are 1-based on the wire and 0-based in [`Mouse`].
//! The parser only reads; whether a report acts on anything is the screen's decision, so a
//! report from a client that never turned the mouse on is dropped there.

/// Longest SGR report accepted, in bytes, `ESC [ <` and the final byte included. The longest
/// legitimate report is `ESC [ < 67 ; 65535 ; 65535 M`, 19 bytes.
pub(crate) const MAX_REPORT: usize = 24;

/// Largest coordinate read (one-based on the wire). A report beyond it is dropped; no window
/// the board draws is this large (`MAX_COLS` is 500).
pub(crate) const MAX_COORD: u32 = 4096;

/// How many bytes of a dropped report are skipped looking for its final byte before the parser
/// goes back to reading keys, so a stuck run cannot swallow the session's input.
pub(crate) const MAX_DISCARD: usize = 64;

/// The most bytes of one dropped report skipped in all (FIX-M35-5). Past [`MAX_DISCARD`] only
/// report-body bytes ([`is_report_body`]) are skipped, so a long run of digits is consumed to
/// its `M` or `m` and its tail never becomes keys, while a letter ends the skip at once.
pub(crate) const MAX_RESYNC: usize = 64 * 1024;

/// A byte that can belong to the body of a mouse report: a digit or a separator.
pub(crate) const fn is_report_body(byte: u8) -> bool {
    matches!(byte, b'0'..=b'9' | b';' | b'<' | b':' | b'-')
}

const ESC: u8 = 0x1b;

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Button {
    /// The primary button.
    Left,
    /// The wheel button.
    Middle,
    /// The secondary button.
    Right,
}

/// What the mouse did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A button went down.
    Down(Button),
    /// A button came up.
    Up(Button),
    /// The mouse moved with a button held.
    Drag(Button),
    /// The wheel turned away from the person (content moves up the screen).
    ScrollUp,
    /// The wheel turned towards the person.
    ScrollDown,
}

/// One mouse report, in zero-based cell coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mouse {
    pub(crate) kind: Kind,
    pub(crate) col: u16,
    pub(crate) row: u16,
}

/// What the bytes held so far are, as far as mouse reports go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Class {
    /// Not a mouse report: the caller treats the bytes as keys.
    NotMouse,
    /// The start of a report; more bytes are needed.
    Prefix,
    /// A complete report. `None` is a report the parser understood and chose not to act on
    /// (motion with no button, other buttons, modifiers it ignores): dropped, not an error.
    Done(Option<Mouse>),
    /// A report that is invalid or too long: drop what was held and skip to its final byte.
    Invalid,
}

/// Classify `held` (the bytes since an `Esc`), which the caller keeps for as long as the answer
/// is [`Class::Prefix`]. `held` must be what the key parser has not yet used; it can be any
/// bytes at all.
pub(crate) fn classify(held: &[u8]) -> Class {
    let [ESC, b'[', third, rest @ ..] = held else {
        return Class::NotMouse;
    };
    match *third {
        b'<' => classify_sgr(rest),
        b'M' => classify_x10(rest),
        _ => Class::NotMouse,
    }
}

/// `rest` is what follows `ESC [ <`.
fn classify_sgr(rest: &[u8]) -> Class {
    if rest.len() + 3 > MAX_REPORT {
        return Class::Invalid;
    }
    let Some((last, body)) = rest.split_last() else {
        return Class::Prefix;
    };
    match *last {
        b'0'..=b'9' | b';' => Class::Prefix,
        b'M' | b'm' => match sgr_fields(body) {
            Some((code, x, y)) => Class::Done(decode(code, x, y, *last == b'm')),
            None => Class::Invalid,
        },
        _ => Class::Invalid,
    }
}

/// `b;x;y` as three numbers, nothing else.
fn sgr_fields(body: &[u8]) -> Option<(u32, u32, u32)> {
    let mut parts = body.split(|&byte| byte == b';');
    let code = number(parts.next()?)?;
    let x = number(parts.next()?)?;
    let y = number(parts.next()?)?;
    parts.next().is_none().then_some((code, x, y))
}

fn number(digits: &[u8]) -> Option<u32> {
    if digits.is_empty() || digits.len() > 5 {
        return None;
    }
    let mut value = 0_u32;
    for &digit in digits {
        let step = digit.checked_sub(b'0').filter(|d| *d <= 9)?;
        value = value * 10 + u32::from(step);
    }
    Some(value)
}

/// `rest` is what follows `ESC [ M`: three bytes `Cb Cx Cy`, each offset by 32.
fn classify_x10(rest: &[u8]) -> Class {
    match rest {
        [] | [_] | [_, _] => Class::Prefix,
        [button, x, y] => {
            let fields = (
                u32::from(*button).checked_sub(32),
                u32::from(*x).checked_sub(32),
                u32::from(*y).checked_sub(32),
            );
            match fields {
                (Some(code), Some(x), Some(y)) => {
                    // The legacy encoding says "a button was released" (code 3) without saying
                    // which, so it decodes to nothing; presses and the wheel decode as usual.
                    Class::Done(decode(code, x, y, false))
                }
                _ => Class::Invalid,
            }
        }
        _ => Class::Invalid,
    }
}

/// Turn a button code and one-based coordinates into an event, or `None` for what the board
/// does not act on.
fn decode(code: u32, x: u32, y: u32, release: bool) -> Option<Mouse> {
    if x == 0 || y == 0 || x > MAX_COORD || y > MAX_COORD {
        return None;
    }
    let col = u16::try_from(x - 1).ok()?;
    let row = u16::try_from(y - 1).ok()?;
    // Bits 2 to 4 are Shift, Meta and Ctrl. A chord is not a click: the board leaves those to
    // the terminal (selection, paste) and keys.
    if code & 0b1_1100 != 0 {
        return None;
    }
    let motion = code & 32 != 0;
    let wheel = code & 64 != 0;
    let low = code & 3;
    let kind = if wheel {
        match low {
            0 => Kind::ScrollUp,
            1 => Kind::ScrollDown,
            _ => return None,
        }
    } else {
        let button = match low {
            0 => Button::Left,
            1 => Button::Middle,
            2 => Button::Right,
            // Code 3 is "no button": a legacy release, or motion with nothing held.
            _ => return None,
        };
        match (release, motion) {
            (true, _) => Kind::Up(button),
            (false, true) => Kind::Drag(button),
            (false, false) => Kind::Down(button),
        }
    };
    Some(Mouse { kind, col, row })
}

#[cfg(test)]
mod tests;
