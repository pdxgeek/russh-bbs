//! Code page 437: the byte-to-Unicode table classic ANSI art is drawn in, and the ASCII
//! stand-ins the ASCII tier uses.
//!
//! Only characters a terminal can show in one column come out of here. None of them is a
//! private-use codepoint or one of the sanitizer's reserved security-marker glyphs (a test
//! checks every entry), so art made from this table can never spell a marker.

/// CP437 to Unicode, index = byte. The first 32 and the last of the first 128 are the
/// "graphic" glyphs a DOS screen shows for those bytes; the ANSI parser treats those bytes as
/// controls and never reads this part of the table for them.
pub(crate) const TO_UNICODE: [char; 256] = [
    '\u{0}', '☺', '☻', '♥', '♦', '♣', '♠', '•', '◘', '○', '◙', '♂', '♀', '♪', '♬', '☼', //
    '►', '◄', '↕', '‼', '¶', '§', '▬', '↨', '↑', '↓', '→', '←', '∟', '↔', '▲', '▼', //
    ' ', '!', '"', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', //
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?', //
    '@', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', //
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '[', '\\', ']', '^', '_', //
    '`', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', //
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '{', '|', '}', '~', '⌂', //
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', //
    'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', //
    'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', //
    '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', //
    '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', //
    '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀', //
    'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', //
    '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', ' ', //
];

/// The Unicode character for a CP437 byte.
pub(crate) fn char_of(byte: u8) -> char {
    TO_UNICODE[usize::from(byte)]
}

/// Characters and the ASCII character that stands in for all of them.
const STAND_INS: &[(&str, char)] = &[
    ("░", '.'),
    ("▒", ':'),
    ("▓█■▬", '#'),
    ("▀", '"'),
    ("▄", '_'),
    ("▌", '['),
    ("▐", ']'),
    ("•∙·○◘◙☼♥♦♣♠♂♀♪♬", '*'),
    ("►→▶»≥", '>'),
    ("◄←◀«≤", '<'),
    ("▲↑", '^'),
    ("▼↓√", 'v'),
    ("↕↔‼¡⌠⌡", '!'),
    ("☺☻°ºôöòóÖ", 'o'),
    ("⌂ⁿπ∩ñ", 'n'),
    ("∞", '8'),
    ("≡═", '='),
    ("≈", '~'),
    ("±", '+'),
    ("÷½¼", '/'),
    ("²", '2'),
    ("¿", '?'),
    ("¢çÇ", 'c'),
    ("£", 'L'),
    ("¥", 'Y'),
    ("₧", 'P'),
    ("ƒ", 'f'),
    ("ßβ", 'B'),
    ("αªäàåáÄÅæÂâ", 'a'),
    ("ΓτT", 't'),
    ("Σσε", 'E'),
    ("µüûùúÜ", 'u'),
    ("ΦφΘΩ", 'O'),
    ("δ", 'd'),
    ("¬⌐", '-'),
    ("∟", 'L'),
    ("¶§", 'S'),
    ("ÉéêëèÆ", 'e'),
    ("ïîìí", 'i'),
    ("ÿ", 'y'),
    ("Ñ", 'N'),
];

/// An ASCII stand-in for `c`, one column wide, for the ASCII tier. ASCII is itself; shades,
/// blocks, lines and accented letters map to the nearest plain character; anything else is `?`.
#[must_use]
pub(crate) fn ascii_of(c: char) -> char {
    if c.is_ascii() && !c.is_ascii_control() {
        return c;
    }
    if ('\u{2500}'..='\u{257F}').contains(&c) {
        return box_ascii(c);
    }
    STAND_INS
        .iter()
        .find(|(set, _)| set.contains(c))
        .map_or('?', |(_, plain)| *plain)
}

/// A box-drawing character as `-`, `|` or `+`.
fn box_ascii(c: char) -> char {
    // Horizontal lines, vertical lines and everything else (corners, crossings, tees).
    const HORIZONTAL: &str = "─━┄┅┈┉═╌╍╴╶╸╺╾╼";
    const VERTICAL: &str = "│┃┆┇┊┋║╎╏╵╷╹╻╽╿";
    if HORIZONTAL.contains(c) {
        '-'
    } else if VERTICAL.contains(c) {
        '|'
    } else {
        '+'
    }
}
