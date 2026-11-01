//! The character scanner behind every sanitizer context.

use std::iter::Peekable;
use std::str::Chars;

/// Replacement for private-use codepoints and noncharacters. It is neither a control nor in
/// any private-use range, so output is stable when sanitized again.
pub(super) const REPLACEMENT: char = '\u{FFFD}';

const ESC: char = '\u{1b}';
const BEL: char = '\u{07}';
const CAN: char = '\u{18}';
const SUB: char = '\u{1a}';
/// 8-bit String Terminator.
const C1_ST: char = '\u{9c}';

/// How line breaks and tabs are treated.
#[derive(Debug, Clone, Copy)]
pub(super) enum Layout {
    SingleLine,
    MultiLine,
}

type Input<'a> = Peekable<Chars<'a>>;

/// Clean `input`, keeping at most `max_chars` output characters.
pub(super) fn clean(input: &str, layout: Layout, max_chars: usize) -> String {
    let mut out = String::with_capacity(input.len().min(max_chars.saturating_mul(4)));
    let mut kept = 0usize;
    let mut chars = input.chars().peekable();

    while kept < max_chars {
        let Some(c) = chars.next() else { break };
        let emit = match c {
            ESC => {
                skip_escape(&mut chars);
                None
            }
            '\u{9b}' => {
                skip_csi(&mut chars);
                None
            }
            '\u{9d}' => {
                skip_string(&mut chars, true);
                None
            }
            '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => {
                skip_string(&mut chars, false);
                None
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                Some(line_break(layout))
            }
            '\n' => Some(line_break(layout)),
            '\t' => Some(match layout {
                Layout::SingleLine => ' ',
                Layout::MultiLine => '\t',
            }),
            c if c.is_control() || is_invisible_format(c) => None,
            c if is_private_use(c) || is_noncharacter(c) => Some(REPLACEMENT),
            c => Some(c),
        };
        if let Some(c) = emit {
            out.push(c);
            kept += 1;
        }
    }
    out
}

const fn line_break(layout: Layout) -> char {
    match layout {
        Layout::SingleLine => ' ',
        Layout::MultiLine => '\n',
    }
}

/// Private-use areas: BMP, plane 15 and plane 16 (Nerd Fonts live in the BMP area and plane 15).
pub(super) const fn is_private_use(c: char) -> bool {
    matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}' | '\u{100000}'..='\u{10FFFD}')
}

const fn is_noncharacter(c: char) -> bool {
    let v = c as u32;
    matches!(v, 0xFDD0..=0xFDEF) || (v & 0xFFFE) == 0xFFFE
}

/// Bidi controls and invisible characters that can hide or reorder text.
/// Keeps U+200C and U+200D, which real scripts and emoji sequences need.
pub(super) const fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{061C}'
            | '\u{200B}'
            | '\u{200E}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

/// Called after `ESC` was consumed.
fn skip_escape(chars: &mut Input<'_>) {
    match chars.peek().copied() {
        Some('[') => {
            chars.next();
            skip_csi(chars);
        }
        Some(']') => {
            chars.next();
            skip_string(chars, true);
        }
        Some('P' | 'X' | '^' | '_') => {
            chars.next();
            skip_string(chars, false);
        }
        Some('\u{20}'..='\u{2F}') => {
            // nF sequence: intermediates then one final byte (e.g. ESC ( B).
            while matches!(chars.peek(), Some('\u{20}'..='\u{2F}')) {
                chars.next();
            }
            if matches!(chars.peek(), Some('\u{30}'..='\u{7E}')) {
                chars.next();
            }
        }
        Some('\u{30}'..='\u{7E}') => {
            chars.next();
        }
        // End of input, a control, or non-ASCII: only the ESC itself is dropped.
        _ => {}
    }
}

/// Skip CSI parameter and intermediate bytes up to and including the final byte.
/// An out-of-grammar character aborts the sequence and is left for the main loop.
fn skip_csi(chars: &mut Input<'_>) {
    while let Some(&c) = chars.peek() {
        match c {
            '\u{20}'..='\u{3F}' => {
                chars.next();
            }
            '\u{40}'..='\u{7E}' => {
                chars.next();
                return;
            }
            _ => return,
        }
    }
}

/// Skip a control string (OSC, DCS, SOS, PM, APC). It ends at ST (`ESC \` or U+009C), or at
/// BEL for OSC. CAN and SUB abort. An `ESC` that is not part of ST starts a new escape and
/// is left for the main loop. Unterminated strings swallow the rest of the input, as a
/// terminal would.
fn skip_string(chars: &mut Input<'_>, bel_ends: bool) {
    while let Some(&c) = chars.peek() {
        match c {
            C1_ST | CAN | SUB => {
                chars.next();
                return;
            }
            BEL if bel_ends => {
                chars.next();
                return;
            }
            ESC => {
                chars.next();
                if chars.peek() == Some(&'\\') {
                    chars.next();
                } else {
                    // Not ST: hand the sequence start back by re-handling it here.
                    skip_escape(chars);
                }
                return;
            }
            _ => {
                chars.next();
            }
        }
    }
}
