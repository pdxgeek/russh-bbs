//! The character scanner behind every sanitizer context.

use std::iter::Peekable;
use std::str::Chars;

use icu_properties::props::{DefaultIgnorableCodePoint, GeneralCategory};
use icu_properties::{CodePointMapData, CodePointSetData};
use unicode_normalization::char::is_combining_mark;

/// Most combining marks kept after one base character. A longer run is truncated (the
/// extra marks are dropped, the base and the first four kept). Four covers every real
/// script's needs (Vietnamese and Indic stacks use up to three); more is a "zalgo" attack on
/// line height and on column arithmetic. ZWJ and ZWNJ neither count nor reset the run, so
/// interleaving them does not buy more marks (QA M0 N2).
pub const MAX_COMBINING_MARKS: usize = 4;

/// Characters reserved for security chrome (decision D39). Content can never produce them:
/// the sanitizer turns each into [`REPLACEMENT`]. The icon registry draws its Unicode-tier
/// markers with exactly these (delimiters and glyphs); a test in `icons` proves the
/// registry stays inside this set.
pub const RESERVED_CHROME: &[char] = &[
    // Reserved delimiter pair: every Unicode-tier security marker is drawn inside it.
    '\u{27E6}', // MATHEMATICAL LEFT WHITE SQUARE BRACKET
    '\u{27E7}', // MATHEMATICAL RIGHT WHITE SQUARE BRACKET
    // The Unicode-tier marker glyphs themselves.
    '\u{2299}', // server-readable
    '\u{229B}', // E2EE
    '\u{270E}', // signature
    '\u{26A0}', // key changed
    '\u{2297}', // unencrypted (telnet)
    '\u{2296}', // low-security mode
];

/// Replacement for private-use codepoints and noncharacters. It is neither a control nor in
/// any private-use range, so output is stable when sanitized again.
pub(super) const REPLACEMENT: char = '\u{FFFD}';

const ESC: char = '\u{1b}';
const BEL: char = '\u{07}';
const CAN: char = '\u{18}';
const SUB: char = '\u{1a}';
/// 8-bit String Terminator.
const C1_ST: char = '\u{9c}';
const ZWNJ: char = '\u{200C}';
const ZWJ: char = '\u{200D}';

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
    // Combining marks emitted since the last base character.
    let mut marks = 0usize;
    // The last character written, for the variation-selector rule.
    let mut last: Option<char> = None;
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
            // LF, VT, FF, NEL, LS and PS all separate lines: a space in one line, a newline
            // in a body. Deleting them would glue the words on either side together.
            '\n' | '\u{0b}' | '\u{0c}' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                Some(line_break(layout))
            }
            '\t' => Some(match layout {
                Layout::SingleLine => ' ',
                Layout::MultiLine => '\t',
            }),
            c if is_unicode_space(c) => Some(' '),
            // U+FE0F keeps emoji presentation, but only straight after an emoji base.
            '\u{FE0F}' if last.is_some_and(is_emoji_base) => Some('\u{FE0F}'),
            // ZWNJ and ZWJ are kept only in a joining context (D45); elsewhere they hide text.
            ZWNJ | ZWJ if last.is_some_and(is_joiner_context) => Some(c),
            ZWNJ | ZWJ => None,
            c if c.is_control() || is_invisible_format(c) => None,
            c if is_private_use(c) || is_noncharacter(c) || RESERVED_CHROME.contains(&c) => {
                Some(REPLACEMENT)
            }
            c if is_combining_mark(c) && marks >= MAX_COMBINING_MARKS => None,
            c => Some(c),
        };
        if let Some(c) = emit {
            // Variation selector and joiners are transparent to the mark run.
            if matches!(c, '\u{FE0F}' | ZWNJ | ZWJ) {
                // keep `marks` as is
            } else if is_combining_mark(c) {
                marks += 1;
            } else {
                marks = 0;
            }
            out.push(c);
            last = Some(c);
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

pub(super) const fn is_noncharacter(c: char) -> bool {
    let v = c as u32;
    matches!(v, 0xFDD0..=0xFDEF) || (v & 0xFFFE) == 0xFFFE
}

/// Unicode space separators other than ASCII space. They are mapped to a plain space so
/// they cannot be used to build blank-looking or look-alike padding.
const fn is_unicode_space(c: char) -> bool {
    matches!(
        c,
        '\u{00A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

/// Characters that start an emoji sequence, so a following U+FE0F is meaningful: the emoji
/// blocks and the few BMP symbols that have an emoji presentation. This is a coarse
/// superset of the Emoji property; a stray U+FE0F after a non-emoji is dropped.
const fn is_emoji_base(c: char) -> bool {
    matches!(
        c,
        '\u{00A9}'
            | '\u{00AE}'
            | '\u{203C}'
            | '\u{2049}'
            | '\u{2122}'
            | '\u{2139}'
            | '\u{2194}'..='\u{21AA}'
            | '\u{231A}'..='\u{23FF}'
            | '\u{24C2}'
            | '\u{25AA}'..='\u{25FE}'
            | '\u{2600}'..='\u{27BF}'
            | '\u{2934}'..='\u{2935}'
            | '\u{2B05}'..='\u{2B55}'
            | '\u{3030}'
            | '\u{303D}'
            | '\u{3297}'
            | '\u{3299}'
            | '\u{1F000}'..='\u{1FAFF}'
    )
}

/// Whether a ZWNJ or ZWJ may follow `prev`: only after a non-ASCII character that is not
/// itself a joiner. That keeps emoji sequences (after an emoji base or U+FE0F), Persian and
/// Indic shaping (after a letter or a virama), and drops a joiner after ASCII, after
/// another joiner, or at the start, where it can only hide or split text (D45).
const fn is_joiner_context(prev: char) -> bool {
    !prev.is_ascii() && !matches!(prev, ZWNJ | ZWJ)
}

/// Invisible and format characters that can hide, reorder or fake text, decided by Unicode
/// property rather than a hand-kept list (D45): general category `Cf`, or
/// `Default_Ignorable_Code_Point` (which also covers assigned fillers and unassigned
/// reserved ranges), plus two visible-blank characters that have neither property.
///
/// This returns true for ZWNJ, ZWJ and U+FE0F as well; `clean` decides those first. Line
/// and paragraph separators (category `Zl`/`Zp`) are not in it; they are line breaks.
pub(super) fn is_invisible_format(c: char) -> bool {
    matches!(c, '\u{2800}' | '\u{FFFC}')
        || CodePointSetData::new::<DefaultIgnorableCodePoint>().contains(c)
        || CodePointMapData::<GeneralCategory>::new().get(c) == GeneralCategory::Format
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
