//! The large-letter fallback: the board's name in a block font drawn by this project.
//!
//! Used when no art fits the terminal, when the board has no art, and in the ASCII tier when a
//! piece has no ASCII form. Each glyph is five rows by five columns of "ink", with one blank
//! column between letters. In the Unicode tiers the ink is a full block; in the ASCII tier it is
//! `#`. The text is the board's name, already checked by `Board::from_config`; a character the
//! font does not have is drawn as a small box so nothing is silently dropped.

use super::grid::{ArtCell, Grid};

/// Rows in a glyph.
pub const HEIGHT: usize = 5;
/// Columns in a glyph (without the gap).
const GLYPH_WIDTH: usize = 5;

const fn g(rows: [&'static str; HEIGHT]) -> [&'static str; HEIGHT] {
    rows
}

/// A glyph's rows: `#` is ink, `.` is blank.
fn glyph(c: char) -> [&'static str; HEIGHT] {
    match c.to_ascii_uppercase() {
        'A' => g([".###.", "#...#", "#####", "#...#", "#...#"]),
        'B' => g(["####.", "#...#", "####.", "#...#", "####."]),
        'C' => g([".####", "#....", "#....", "#....", ".####"]),
        'D' => g(["####.", "#...#", "#...#", "#...#", "####."]),
        'E' => g(["#####", "#....", "####.", "#....", "#####"]),
        'F' => g(["#####", "#....", "####.", "#....", "#...."]),
        'G' => g([".####", "#....", "#..##", "#...#", ".###."]),
        'H' => g(["#...#", "#...#", "#####", "#...#", "#...#"]),
        'I' => g(["#####", "..#..", "..#..", "..#..", "#####"]),
        'J' => g(["..###", "...#.", "...#.", "#..#.", ".##.."]),
        'K' => g(["#...#", "#..#.", "###..", "#..#.", "#...#"]),
        'L' => g(["#....", "#....", "#....", "#....", "#####"]),
        'M' => g(["#...#", "##.##", "#.#.#", "#...#", "#...#"]),
        'N' => g(["#...#", "##..#", "#.#.#", "#..##", "#...#"]),
        'O' => g([".###.", "#...#", "#...#", "#...#", ".###."]),
        'P' => g(["####.", "#...#", "####.", "#....", "#...."]),
        'Q' => g([".###.", "#...#", "#.#.#", "#..#.", ".##.#"]),
        'R' => g(["####.", "#...#", "####.", "#..#.", "#...#"]),
        'S' => g([".####", "#....", ".###.", "....#", "####."]),
        'T' => g(["#####", "..#..", "..#..", "..#..", "..#.."]),
        'U' => g(["#...#", "#...#", "#...#", "#...#", ".###."]),
        'V' => g(["#...#", "#...#", "#...#", ".#.#.", "..#.."]),
        'W' => g(["#...#", "#...#", "#.#.#", "##.##", "#...#"]),
        'X' => g(["#...#", ".#.#.", "..#..", ".#.#.", "#...#"]),
        'Y' => g(["#...#", ".#.#.", "..#..", "..#..", "..#.."]),
        'Z' => g(["#####", "...#.", "..#..", ".#...", "#####"]),
        '0' => g([".###.", "#..##", "#.#.#", "##..#", ".###."]),
        '1' => g(["..#..", ".##..", "..#..", "..#..", ".###."]),
        '2' => g([".###.", "#...#", "..##.", ".#...", "#####"]),
        '3' => g(["####.", "....#", ".###.", "....#", "####."]),
        '4' => g(["#..#.", "#..#.", "#####", "...#.", "...#."]),
        '5' => g(["#####", "#....", "####.", "....#", "####."]),
        '6' => g([".###.", "#....", "####.", "#...#", ".###."]),
        '7' => g(["#####", "....#", "...#.", "..#..", "..#.."]),
        '8' => g([".###.", "#...#", ".###.", "#...#", ".###."]),
        '9' => g([".###.", "#...#", ".####", "....#", ".###."]),
        ' ' => g([".....", ".....", ".....", ".....", "....."]),
        '.' => g([".....", ".....", ".....", ".....", "..#.."]),
        ',' => g([".....", ".....", ".....", "..#..", ".#..."]),
        '-' => g([".....", ".....", ".###.", ".....", "....."]),
        '_' => g([".....", ".....", ".....", ".....", "#####"]),
        '\'' => g(["..#..", "..#..", ".....", ".....", "....."]),
        '!' => g(["..#..", "..#..", "..#..", ".....", "..#.."]),
        '?' => g([".###.", "#...#", "..##.", ".....", "..#.."]),
        ':' => g([".....", "..#..", ".....", "..#..", "....."]),
        '&' => g([".##..", "#..#.", ".##.#", "#..#.", ".##.#"]),
        '/' => g(["....#", "...#.", "..#..", ".#...", "#...."]),
        '+' => g([".....", "..#..", ".###.", "..#..", "....."]),
        '#' => g([".#.#.", "#####", ".#.#.", "#####", ".#.#."]),
        // A character the font does not have: a small box, never silence.
        _ => g(["#####", "#...#", "#...#", "#...#", "#####"]),
    }
}

/// Columns the text takes in the font (glyphs plus the gaps between them).
#[must_use]
pub fn width_of(text: &str) -> usize {
    let count = text.chars().count();
    if count == 0 {
        0
    } else {
        count * (GLYPH_WIDTH + 1) - 1
    }
}

/// The text as a grid in the font. `ink` is the cell for a filled pixel and `ascii_ink` its
/// ASCII-tier form. `None` for empty text.
#[must_use]
pub fn render(text: &str, ink: char, fg: Option<u8>) -> Option<Grid> {
    let width = width_of(text);
    if width == 0 {
        return None;
    }
    let mut grid = Grid::blank(width, HEIGHT);
    for (index, c) in text.chars().enumerate() {
        let x0 = index * (GLYPH_WIDTH + 1);
        for (y, row) in glyph(c).iter().enumerate() {
            for (dx, pixel) in row.chars().enumerate() {
                if pixel == '#' {
                    grid.set(
                        x0 + dx,
                        y,
                        ArtCell {
                            ch: ink,
                            ascii: '#',
                            fg,
                            bg: None,
                        },
                    );
                }
            }
        }
    }
    Some(grid)
}
