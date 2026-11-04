//! Colour depth: what a member's terminal can show, and how a door's colours are brought down
//! to it.
//!
//! A door may use the theme's roles ([`crate::DoorStyle`]) and colours of its own (truecolor
//! `Color::Rgb`, the 256-colour `Color::Indexed`, or the sixteen names). The framework brings
//! every colour down to the member's depth when it composites the door onto the screen, so a
//! door never has to ask what the terminal can do and never draws a colour the terminal cannot
//! show. Degradation is integer arithmetic only, so the result is the same on every machine.

use ratatui::style::Color;

/// How many colours the member's terminal shows. Ordered from richest to poorest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ColorDepth {
    /// 24-bit colour: nothing is changed.
    TrueColor,
    /// The 256-colour palette: `Rgb` is mapped to the nearest cube or grey-ramp entry.
    Ansi256,
    /// The sixteen named colours: `Rgb` and `Indexed` are mapped to the nearest name.
    Ansi16,
    /// No colour: every colour is dropped, weights and reverse video stay.
    Mono,
}

/// The sixteen named colours in terminal order (index 0 to 15) with the red, green and blue that
/// xterm shows for them. The mapping from an arbitrary colour is to the nearest of these.
const NAMED: [(Color, [u8; 3]); 16] = [
    (Color::Black, [0, 0, 0]),
    (Color::Red, [205, 0, 0]),
    (Color::Green, [0, 205, 0]),
    (Color::Yellow, [205, 205, 0]),
    (Color::Blue, [0, 0, 238]),
    (Color::Magenta, [205, 0, 205]),
    (Color::Cyan, [0, 205, 205]),
    (Color::Gray, [229, 229, 229]),
    (Color::DarkGray, [127, 127, 127]),
    (Color::LightRed, [255, 0, 0]),
    (Color::LightGreen, [0, 255, 0]),
    (Color::LightYellow, [255, 255, 0]),
    (Color::LightBlue, [92, 92, 255]),
    (Color::LightMagenta, [255, 0, 255]),
    (Color::LightCyan, [0, 255, 255]),
    (Color::White, [255, 255, 255]),
];

/// The six levels of each channel in the 6x6x6 colour cube (indices 16 to 231).
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

fn distance(a: [u8; 3], b: [u8; 3]) -> u32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| {
            let d = u32::from(*x).abs_diff(u32::from(y));
            d * d
        })
        .sum()
}

/// The red, green and blue a palette index stands for.
fn index_rgb(index: u8) -> [u8; 3] {
    match index {
        0..=15 => NAMED[usize::from(index)].1,
        16..=231 => {
            let n = usize::from(index - 16);
            [CUBE[n / 36], CUBE[(n / 6) % 6], CUBE[n % 6]]
        }
        232..=255 => {
            let grey = 8 + 10 * (index - 232);
            [grey, grey, grey]
        }
    }
}

/// The red, green and blue of a colour, or `None` for `Reset` (the terminal's own colour).
fn rgb(color: Color) -> Option<[u8; 3]> {
    match color {
        Color::Reset => None,
        Color::Rgb(r, g, b) => Some([r, g, b]),
        Color::Indexed(i) => Some(index_rgb(i)),
        named => NAMED.iter().find(|(c, _)| *c == named).map(|(_, v)| *v),
    }
}

/// The nearest of the sixteen names.
fn nearest_named(target: [u8; 3]) -> Color {
    NAMED
        .iter()
        .min_by_key(|(_, v)| distance(*v, target))
        .map_or(Color::Reset, |(c, _)| *c)
}

/// The index of the cube level nearest to one channel value.
fn cube_level(value: u8) -> usize {
    CUBE.iter()
        .enumerate()
        .min_by_key(|(_, level)| u32::from(**level).abs_diff(u32::from(value)))
        .map_or(0, |(at, _)| at)
}

/// The nearest entry of the 256-colour palette beyond the sixteen names (the cube or the grey
/// ramp), whichever is closer.
fn nearest_indexed(target: [u8; 3]) -> u8 {
    let [r, g, b] = target.map(cube_level);
    let cube_rgb = [CUBE[r], CUBE[g], CUBE[b]];
    // `r`, `g` and `b` are below 6, so this is at most 231.
    let cube_index = u8::try_from(16 + 36 * r + 6 * g + b).unwrap_or(231);
    let average = (u32::from(target[0]) + u32::from(target[1]) + u32::from(target[2])) / 3;
    // Grey ramp entry n (0 to 23) is 8 + 10n; round to the nearest.
    let step = u8::try_from(average.saturating_sub(8).saturating_add(5) / 10)
        .unwrap_or(23)
        .min(23);
    let grey = 8 + 10 * step;
    if distance([grey; 3], target) < distance(cube_rgb, target) {
        232 + step
    } else {
        cube_index
    }
}

impl ColorDepth {
    /// `color` brought down to this depth.
    ///
    /// `Reset` stays `Reset` at every depth (it means the terminal's own colour). The sixteen
    /// names are kept at every depth but [`Self::Mono`], which drops every colour.
    #[must_use]
    pub fn degrade(self, color: Color) -> Color {
        match self {
            Self::TrueColor => color,
            Self::Mono => Color::Reset,
            Self::Ansi256 => match color {
                Color::Rgb(r, g, b) => Color::Indexed(nearest_indexed([r, g, b])),
                other => other,
            },
            Self::Ansi16 => match color {
                Color::Rgb(..) | Color::Indexed(_) => {
                    rgb(color).map_or(Color::Reset, nearest_named)
                }
                other => other,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const NAMES: [Color; 16] = [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
        Color::DarkGray,
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::White,
    ];

    #[test]
    fn truecolor_changes_nothing_and_mono_drops_everything() {
        for color in [
            Color::Reset,
            Color::Red,
            Color::Rgb(1, 2, 3),
            Color::Indexed(77),
        ] {
            assert_eq!(ColorDepth::TrueColor.degrade(color), color);
            assert_eq!(ColorDepth::Mono.degrade(color), Color::Reset);
        }
    }

    #[test]
    fn reset_and_the_sixteen_names_survive_every_depth_but_mono() {
        for depth in [
            ColorDepth::TrueColor,
            ColorDepth::Ansi256,
            ColorDepth::Ansi16,
        ] {
            assert_eq!(depth.degrade(Color::Reset), Color::Reset);
            for name in NAMES {
                assert_eq!(depth.degrade(name), name, "{depth:?}");
            }
        }
    }

    #[test]
    fn rgb_goes_to_the_nearest_palette_entry() {
        let d256 = ColorDepth::Ansi256;
        assert_eq!(d256.degrade(Color::Rgb(255, 0, 0)), Color::Indexed(196));
        assert_eq!(d256.degrade(Color::Rgb(0, 0, 0)), Color::Indexed(16));
        assert_eq!(d256.degrade(Color::Rgb(255, 255, 255)), Color::Indexed(231));
        // A mid grey is nearer the grey ramp than any cube entry.
        assert_eq!(d256.degrade(Color::Rgb(128, 128, 128)), Color::Indexed(244));
        // An indexed colour is already 256-colour.
        assert_eq!(d256.degrade(Color::Indexed(99)), Color::Indexed(99));

        let d16 = ColorDepth::Ansi16;
        assert_eq!(d16.degrade(Color::Rgb(255, 0, 0)), Color::LightRed);
        assert_eq!(d16.degrade(Color::Rgb(10, 10, 10)), Color::Black);
        assert_eq!(d16.degrade(Color::Rgb(250, 250, 250)), Color::White);
        assert_eq!(d16.degrade(Color::Rgb(0, 0, 200)), Color::Blue);
        assert_eq!(d16.degrade(Color::Indexed(196)), Color::LightRed);
        assert_eq!(d16.degrade(Color::Indexed(2)), Color::Green);
        assert_eq!(d16.degrade(Color::Indexed(232)), Color::Black);
        assert_eq!(d16.degrade(Color::Indexed(255)), Color::Gray);
    }

    #[test]
    fn degrading_never_produces_a_richer_colour() {
        for r in (0..=255u8).step_by(15) {
            for g in (0..=255u8).step_by(15) {
                for b in (0..=255u8).step_by(15) {
                    let c = Color::Rgb(r, g, b);
                    assert!(matches!(
                        ColorDepth::Ansi256.degrade(c),
                        Color::Indexed(16..=255)
                    ));
                    assert!(NAMES.contains(&ColorDepth::Ansi16.degrade(c)));
                    assert_eq!(ColorDepth::Mono.degrade(c), Color::Reset);
                }
            }
        }
        for i in 0..=255u8 {
            let c = Color::Indexed(i);
            assert!(NAMES.contains(&ColorDepth::Ansi16.degrade(c)));
            assert_eq!(ColorDepth::Ansi256.degrade(c), c);
        }
    }

    #[test]
    fn depth_is_ordered_from_richest_to_poorest() {
        assert!(ColorDepth::TrueColor < ColorDepth::Ansi256);
        assert!(ColorDepth::Ansi256 < ColorDepth::Ansi16);
        assert!(ColorDepth::Ansi16 < ColorDepth::Mono);
        // Index 232 is the first grey (8), 255 the last (238).
        assert_eq!(index_rgb(232), [8, 8, 8]);
        assert_eq!(index_rgb(255), [238, 238, 238]);
        assert_eq!(index_rgb(231), [255, 255, 255]);
        assert_eq!(nearest_indexed([8, 8, 8]), 232);
        assert_eq!(nearest_indexed([238, 238, 238]), 255);
        assert!(rgb(Color::Reset).is_none());
        assert_eq!(rgb(Color::Red).unwrap(), [205, 0, 0]);
    }
}
