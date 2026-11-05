//! A tiny original 5x5 pixel font for the intro's big letters. Letters, digits and a few marks;
//! anything else is left out, so a board name can never put a glyph here that is not in this
//! table (and the table has no marker glyph or delimiter).

/// One glyph: five rows of five bits (bit 4 is the left column).
const GLYPHS: &[(char, [u8; 5])] = &[
    ('A', [0b01110, 0b10001, 0b11111, 0b10001, 0b10001]),
    ('B', [0b11110, 0b10001, 0b11110, 0b10001, 0b11110]),
    ('C', [0b01111, 0b10000, 0b10000, 0b10000, 0b01111]),
    ('D', [0b11110, 0b10001, 0b10001, 0b10001, 0b11110]),
    ('E', [0b11111, 0b10000, 0b11110, 0b10000, 0b11111]),
    ('F', [0b11111, 0b10000, 0b11110, 0b10000, 0b10000]),
    ('G', [0b01111, 0b10000, 0b10011, 0b10001, 0b01111]),
    ('H', [0b10001, 0b10001, 0b11111, 0b10001, 0b10001]),
    ('I', [0b11111, 0b00100, 0b00100, 0b00100, 0b11111]),
    ('J', [0b00111, 0b00001, 0b00001, 0b10001, 0b01110]),
    ('K', [0b10001, 0b10010, 0b11100, 0b10010, 0b10001]),
    ('L', [0b10000, 0b10000, 0b10000, 0b10000, 0b11111]),
    ('M', [0b10001, 0b11011, 0b10101, 0b10001, 0b10001]),
    ('N', [0b10001, 0b11001, 0b10101, 0b10011, 0b10001]),
    ('O', [0b01110, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('P', [0b11110, 0b10001, 0b11110, 0b10000, 0b10000]),
    ('Q', [0b01110, 0b10001, 0b10101, 0b10010, 0b01101]),
    ('R', [0b11110, 0b10001, 0b11110, 0b10010, 0b10001]),
    ('S', [0b01111, 0b10000, 0b01110, 0b00001, 0b11110]),
    ('T', [0b11111, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('U', [0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('V', [0b10001, 0b10001, 0b10001, 0b01010, 0b00100]),
    ('W', [0b10001, 0b10001, 0b10101, 0b11011, 0b10001]),
    ('X', [0b10001, 0b01010, 0b00100, 0b01010, 0b10001]),
    ('Y', [0b10001, 0b01010, 0b00100, 0b00100, 0b00100]),
    ('Z', [0b11111, 0b00010, 0b00100, 0b01000, 0b11111]),
    ('0', [0b01110, 0b10011, 0b10101, 0b11001, 0b01110]),
    ('1', [0b00100, 0b01100, 0b00100, 0b00100, 0b01110]),
    ('2', [0b01110, 0b10001, 0b00110, 0b01000, 0b11111]),
    ('3', [0b11110, 0b00001, 0b01110, 0b00001, 0b11110]),
    ('4', [0b10010, 0b10010, 0b11111, 0b00010, 0b00010]),
    ('5', [0b11111, 0b10000, 0b11110, 0b00001, 0b11110]),
    ('6', [0b01110, 0b10000, 0b11110, 0b10001, 0b01110]),
    ('7', [0b11111, 0b00001, 0b00010, 0b00100, 0b00100]),
    ('8', [0b01110, 0b10001, 0b01110, 0b10001, 0b01110]),
    ('9', [0b01110, 0b10001, 0b01111, 0b00001, 0b01110]),
    ('-', [0b00000, 0b00000, 0b01110, 0b00000, 0b00000]),
    ('.', [0b00000, 0b00000, 0b00000, 0b00110, 0b00110]),
    ('!', [0b00100, 0b00100, 0b00100, 0b00000, 0b00100]),
];

/// Glyph columns of a space.
const SPACE: u16 = 3;

/// One placed glyph: its rows and the font-pixel column it starts at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Placed {
    pub rows: [u8; 5],
    pub x: u16,
}

/// The glyphs of `text` (upper-cased) that the font has, laid out with one blank column between
/// them, as many as fit in `max_width` font pixels. Returns them and the total width.
pub(crate) fn layout(text: &str, max_width: u16) -> (Vec<Placed>, u16) {
    let mut placed = Vec::new();
    let mut x = 0_u16;
    for ch in text.chars().flat_map(char::to_uppercase) {
        if ch == ' ' {
            x = x.saturating_add(SPACE);
            continue;
        }
        let Some((_, rows)) = GLYPHS.iter().find(|(c, _)| *c == ch) else {
            continue;
        };
        if x.saturating_add(5) > max_width {
            break;
        }
        placed.push(Placed { rows: *rows, x });
        x += 6;
    }
    let width = placed.last().map_or(0, |last| last.x + 5);
    (placed, width)
}

impl Placed {
    /// Whether the pixel at column `cx` (0 to 4), row `cy` (0 to 4) is lit.
    pub(crate) fn lit(self, cx: u16, cy: u16) -> bool {
        cx < 5 && cy < 5 && (self.rows[usize::from(cy)] >> (4 - cx)) & 1 == 1
    }
}

/// The characters of `text` the font can draw, upper-cased, in order (spaces and anything else
/// dropped). Used for the plain-text title, so it can show nothing the font does not have.
pub(crate) fn supported(text: &str) -> Vec<char> {
    text.chars()
        .flat_map(char::to_uppercase)
        .filter(|ch| GLYPHS.iter().any(|(c, _)| c == ch))
        .collect()
}
