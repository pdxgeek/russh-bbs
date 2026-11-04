//! Small pictures made of characters: the marsh at a place, a time of day and a water level,
//! for the title screen and the event scenes. They are decoration: nothing here reads a rule.
//! Each picture is a pure function of its inputs and a seed, so a scene looks the same every
//! time it is drawn, and different scenes (different seeds) look different.

use russh_bbs_door::style::{Color, Style};

use super::{Cells, Look, Pal, hue};
use crate::lore::{Daypart, Water};
use crate::map::TileKind;

/// A small hash of the seed and a cell, for scattering stars and reeds.
fn mix(seed: u64, x: usize, y: usize) -> u32 {
    let mut v = seed
        ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    v ^= v >> 29;
    v = v.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    v ^= v >> 32;
    u32::try_from(v & 0xFFFF_FFFF).unwrap_or(0)
}

/// What the picture is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ground {
    /// A tile of the marsh.
    Tile(TileKind),
    /// The hold on its high ground.
    Hold,
    /// The Tidecourt on its stilts.
    Court,
}

/// Draw a landscape `w` by `h` cells.
#[allow(clippy::too_many_lines)]
pub(super) fn landscape(
    look: Look,
    pal: &Pal,
    (w, h): (u16, u16),
    ground: Ground,
    day: Daypart,
    water: Water,
    seed: u64,
) -> Cells {
    let (w, h) = (usize::from(w), usize::from(h));
    let horizon = (h * 5 / 12).max(2);
    let night = matches!(day, Daypart::Night);
    // Sky colours by the time of day: deep blue at night, rose at dusk and dawn, pale by day.
    let sky = match day {
        Daypart::Night => hue::FOG,
        Daypart::Dawn => hue::SEA_DEEP,
        Daypart::Day => hue::SLATE,
        Daypart::Dusk => hue::SEA_DEEP,
    };
    let sun_col = (mix(seed, 1, 1) as usize) % (w.max(4) - 2) + 1;
    let mut rows = Vec::with_capacity(h);
    for y in 0..h {
        let mut row: Vec<(&'static str, Style)> = Vec::with_capacity(w);
        for x in 0..w {
            let m = mix(seed, x, y);
            let cell: (&'static str, Style) = if y < horizon {
                // The sky.
                let near = horizon - y;
                if y == 1 && x == sun_col {
                    match day {
                        Daypart::Night => (look.g("●", "O"), pal.salt()),
                        Daypart::Day => (look.g("●", "O"), pal.gold()),
                        _ => (look.g("◐", "O"), pal.gold()),
                    }
                } else if night && m % 23 == 0 {
                    (
                        if m % 7 == 0 {
                            look.g("*", "*")
                        } else {
                            look.g("·", ".")
                        },
                        pal.salt(),
                    )
                } else if !night && y >= 1 && y <= 2 && m % 9 == 0 && (x + y) % 3 != 0 {
                    (look.g("▒", "~"), pal.fg(hue::SALT))
                } else if near == 1 {
                    (look.g("▁", "_"), pal.fg(sky))
                } else {
                    (" ", Style::new())
                }
            } else {
                ground_cell(look, pal, ground, water, (x, y), (w, h), horizon, m)
            };
            row.push(cell);
        }
        rows.push(row);
    }
    // The time of day dims or lifts every colour: the same marsh at night is darker.
    let percent = match day {
        Daypart::Night => 50,
        Daypart::Dusk | Daypart::Dawn => 80,
        Daypart::Day => 100,
    };
    for row in &mut rows {
        for (_, style) in row.iter_mut() {
            style.fg = style.fg.map(|c| shade(c, percent));
        }
    }
    Cells { rows }
}

/// `color` scaled to `percent` of its brightness (other kinds of colour are left alone).
fn shade(color: Color, percent: u16) -> Color {
    match color {
        Color::Rgb(r, g, b) => {
            let f = |v: u8| u8::try_from(u16::from(v) * percent / 100).unwrap_or(255);
            Color::Rgb(f(r), f(g), f(b))
        }
        other => other,
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn ground_cell(
    look: Look,
    pal: &Pal,
    ground: Ground,
    water: Water,
    (x, y): (usize, usize),
    (w, h): (usize, usize),
    horizon: usize,
    m: u32,
) -> (&'static str, Style) {
    let depth = y - horizon; // 0 at the horizon
    let span = (h - horizon).max(1);
    let wet_rows = match water {
        Water::Low => 0,
        Water::Mid => span / 4,
        Water::High => span / 2,
    };
    let from_bottom = h - 1 - y;
    // Water covers the bottom rows, with a ripple.
    if from_bottom < wet_rows {
        let ripple = (x + y + (m as usize % 3)) % 4 == 0;
        return (
            if ripple {
                look.g("~", "~")
            } else {
                look.g("≈", "-")
            },
            pal.fg(hue::WET),
        );
    }
    let t = |a: &'static str, b: &'static str| look.g(a, b);
    match ground {
        Ground::Tile(TileKind::Reedbed) => {
            if m % 5 < 3 && depth > 0 {
                let glyph = match m % 4 {
                    0 => t("│", "|"),
                    1 => t("/", "/"),
                    2 => t("\\", "\\"),
                    _ => t("╿", "|"),
                };
                (glyph, if m % 2 == 0 { pal.reed() } else { pal.moss() })
            } else {
                (t("'", "'"), pal.peat())
            }
        }
        Ground::Tile(TileKind::PeatBog) => {
            if m % 11 == 0 {
                (t("°", "o"), pal.fg(hue::SEA))
            } else if m % 3 == 0 {
                (t(",", ","), pal.peat())
            } else {
                (" ", Style::new())
            }
        }
        Ground::Tile(TileKind::SaltPan) => {
            if m % 4 == 0 {
                (t("·", "."), pal.salt())
            } else if m % 3 == 0 {
                (t(":", ":"), pal.salt())
            } else {
                (" ", Style::new())
            }
        }
        Ground::Tile(TileKind::Channel | TileKind::Mere) => {
            if (x + y * 2 + m as usize % 2) % 5 < 2 {
                (t("≈", "~"), pal.sea())
            } else {
                (t("~", "-"), pal.fg(hue::WET))
            }
        }
        Ground::Tile(TileKind::Island) | Ground::Hold => {
            let hill = (w / 2).abs_diff(x) < (depth + 1) * 2;
            if hill && depth >= 1 {
                if Ground::Hold == ground && depth == 1 && x % 7 == 3 {
                    (t("▲", "A"), pal.peat())
                } else if m % 6 == 0 {
                    (t("\"", "\""), pal.moss())
                } else {
                    (t("▒", "^"), pal.fg(hue::MOSS))
                }
            } else if m % 4 == 0 {
                (t(".", "."), pal.fg(hue::MUD))
            } else {
                (" ", Style::new())
            }
        }
        Ground::Court => {
            let walk = (h - horizon) / 2;
            let tower = x % 11 < 4 && depth < walk;
            if depth == walk {
                (t("═", "="), pal.peat())
            } else if tower {
                if (x % 11 == 1 || x % 11 == 2) && depth % 2 == 1 && m % 3 != 0 {
                    (t("▪", "."), pal.gold())
                } else {
                    (t("█", "#"), pal.fg(hue::SLATE))
                }
            } else if depth > walk && x % 4 == 0 {
                (t("╿", "|"), pal.peat())
            } else if depth > walk && m % 5 == 0 {
                (t("~", "~"), pal.fg(hue::WET))
            } else {
                (" ", Style::new())
            }
        }
        Ground::Tile(TileKind::DrownedTown) => {
            let tower = (x % 9 < 3) && depth < 1 + (x / 9 % 3) * 2 + 2;
            if tower && depth > 0 {
                if m % 6 == 0 {
                    (t("▫", "o"), pal.fg(hue::FOG))
                } else {
                    (t("█", "#"), pal.fg(hue::SLATE))
                }
            } else if m % 7 == 0 {
                (t(".", "."), pal.fg(hue::MUD))
            } else {
                (" ", Style::new())
            }
        }
        Ground::Tile(TileKind::Wreck) => {
            let rib = x % 4 == 0 && depth < 4;
            if rib {
                (t(")", ")"), pal.peat())
            } else if depth == 3 {
                (t("▂", "_"), pal.peat())
            } else {
                (" ", Style::new())
            }
        }
        Ground::Tile(TileKind::Deep) => {
            if x % 11 == 5 && depth > 0 {
                (t("▓", "#"), pal.fg(hue::SEA_DEEP))
            } else if m % 9 == 0 {
                (t("·", "."), pal.sea())
            } else {
                (" ", Style::new())
            }
        }
        Ground::Tile(TileKind::Causeway) => {
            if depth == 2 {
                (t("▀", "="), pal.peat())
            } else {
                (t("░", "."), pal.fg(hue::MUD))
            }
        }
        Ground::Tile(TileKind::Mudflat) => {
            if m % 8 == 0 {
                (t("~", "~"), pal.fg(hue::WET))
            } else if m % 3 == 0 {
                (t(".", "."), pal.fg(hue::MUD))
            } else {
                (" ", Style::new())
            }
        }
    }
}
