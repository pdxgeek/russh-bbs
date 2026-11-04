//! The ASCII tier for a door's drawing (D18, D45).
//!
//! Ratatui's widgets draw with box-drawing characters, block elements, braille and a few
//! geometric shapes. A member in the ASCII tier has said their terminal cannot be trusted with
//! those, so when the board composites a door onto an ASCII-tier screen it replaces each of
//! these *icon-like* characters with an ASCII stand-in. A door therefore draws a table, a gauge
//! or a braille chart once and every tier gets a legible version of it.
//!
//! Only these ranges are changed. A letter with an accent, or text in another script, is
//! content and stays as it is: the tier is about icons, not about what players write. (The
//! replacement character U+FFFD that the sanitizer leaves for a neutralized character becomes
//! `?` in this tier, so no oddly shaped glyph is left either.)

/// The ASCII stand-in for `c`, or `c` itself when it is not an icon-like character.
pub(crate) fn ascii_glyph(c: char) -> char {
    match u32::from(c) {
        0x2500..=0x257F => box_drawing(c),
        0x2580..=0x259F => block_element(c),
        0x2800..=0x28FF => braille(c),
        0x2190 | 0x25C0 | 0x25C4 => '<',
        0x2191 | 0x25B2 | 0x25B3 => '^',
        0x2192 | 0x25B6 | 0x25BA => '>',
        0x2193 | 0x25BC | 0x25BD => 'v',
        0x2022 | 0x25CF | 0x25C6 | 0x25C8 | 0x2605 | 0x2606 => '*',
        0x25E6 | 0x25CB | 0x25A1 | 0x25AB | 0x25C7 => 'o',
        0x2219 => '.',
        0x25A0 | 0x25AA => '#',
        0xFFFD => '?',
        _ => c,
    }
}

fn box_drawing(c: char) -> char {
    match u32::from(c) {
        // Horizontal lines: light, heavy, dashed and double, and the half lines.
        0x2500 | 0x2501 | 0x2504 | 0x2505 | 0x2508 | 0x2509 | 0x254C | 0x254D | 0x2550 | 0x2574
        | 0x2576 | 0x2578 | 0x257A | 0x257C | 0x257E => '-',
        // Vertical lines.
        0x2502 | 0x2503 | 0x2506 | 0x2507 | 0x250A | 0x250B | 0x254E | 0x254F | 0x2551 | 0x2575
        | 0x2577 | 0x2579 | 0x257B | 0x257D | 0x257F => '|',
        0x2571 => '/',
        0x2572 => '\\',
        0x2573 => 'X',
        // Corners, junctions and rounded corners.
        _ => '+',
    }
}

fn block_element(c: char) -> char {
    match u32::from(c) {
        // Lower eighths, from a sliver to the full block (a sparkline or bar chart), and the
        // light shade.
        0x2581 | 0x2582 | 0x2591 => '.',
        0x2583 | 0x2592 => ':',
        0x2585 | 0x258D => '=',
        0x2586 => '+',
        0x2587 => '*',
        // Full block, left blocks of four eighths or more, the right half, quadrants, the dark
        // shade.
        0x2588..=0x258C | 0x2590 | 0x2593 | 0x2596..=0x259F => '#',
        0x258E | 0x258F | 0x2595 => '|',
        // The upper half, the lower half (a half-block chart) and the upper eighth.
        _ => '-',
    }
}

/// A braille cell is eight dots; the more are raised, the heavier the stand-in.
fn braille(c: char) -> char {
    let dots = (u32::from(c) & 0xFF).count_ones();
    match dots {
        0 => ' ',
        1 | 2 => '.',
        3..=5 => ':',
        _ => '#',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_ranges_become_ascii_and_everything_else_is_left_alone() {
        for code in 0x2500..=0x259Fu32 {
            let c = char::from_u32(code).unwrap_or('?');
            assert!(ascii_glyph(c).is_ascii(), "U+{code:04X}");
        }
        for code in 0x2800..=0x28FFu32 {
            let c = char::from_u32(code).unwrap_or('?');
            assert!(ascii_glyph(c).is_ascii(), "U+{code:04X}");
        }
        assert_eq!(ascii_glyph('─'), '-');
        assert_eq!(ascii_glyph('│'), '|');
        assert_eq!(ascii_glyph('┌'), '+');
        assert_eq!(ascii_glyph('╭'), '+');
        assert_eq!(ascii_glyph('█'), '#');
        assert_eq!(ascii_glyph('▁'), '.');
        assert_eq!(ascii_glyph('⣿'), '#');
        assert_eq!(ascii_glyph('⠁'), '.');
        assert_eq!(ascii_glyph('•'), '*');
        assert_eq!(ascii_glyph('▲'), '^');
        assert_eq!(ascii_glyph('\u{FFFD}'), '?');
        // Content stays content.
        for c in ['a', 'é', '日', 'ß', '😀', '…'] {
            assert_eq!(ascii_glyph(c), c);
        }
    }

    #[test]
    fn sparkline_levels_rise() {
        let levels: Vec<char> = "▁▂▃▄▅▆▇█".chars().map(ascii_glyph).collect();
        assert_eq!(levels.iter().collect::<String>(), "..:-=+*#");
    }
}
