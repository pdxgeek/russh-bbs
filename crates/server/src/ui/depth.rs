//! Colour depth: what the client can show, and how a colour degrades to fit it (UI-1, D102).
//!
//! **Where the depth comes from.** The client tells us: over ssh, `TERM` arrives in the `pty-req`
//! and `COLORTERM` in an `env` request when the client sends it; over telnet, the terminal type
//! (RFC 1091) is the only hint. [`ColorDepth::detect`] reads those two strings and never trusts
//! them for anything but this choice. A member's own setting can only **lower** the result
//! ([`ColorDepth::effective`]): colour off is [`ColorDepth::Mono`] today, and a stored cap
//! (16, 256 or truecolor) plugs into the same function when UI-2 adds the setting.
//!
//! **How a colour degrades.** A [`Tint`] holds one colour as the author meant it (truecolor, a
//! 256-colour index or a named colour) and [`Tint::resolve`] returns what to send at a given
//! depth: the exact colour, the nearest cell of the 256-colour cube or grey ramp, the nearest of
//! the sixteen, or nothing on a mono terminal. Mono output uses weight and underline only, never
//! reverse video: reverse is the security markers' reserved style (D39).

use ratatui::style::Color;

/// What a terminal can show, from least to most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ColorDepth {
    /// No colour: bold, dim and underline only.
    Mono,
    /// The sixteen named colours.
    Ansi16,
    /// The 256-colour palette.
    Ansi256,
    /// 24-bit colour.
    TrueColor,
}

impl ColorDepth {
    /// Every depth, lowest first.
    pub const ALL: [Self; 4] = [Self::Mono, Self::Ansi16, Self::Ansi256, Self::TrueColor];

    /// The depth a client advertises. `term` is `TERM` (ssh `pty-req`) or the telnet terminal
    /// type; `colorterm` is `COLORTERM` when the client sent it. Unknown or missing values give
    /// [`ColorDepth::Ansi16`]: the sixteen colours are safe on nearly every terminal, and the
    /// result only ever lowers to mono for a terminal that says it has no colour.
    #[must_use]
    pub fn detect(term: Option<&str>, colorterm: Option<&str>) -> Self {
        let colorterm = colorterm.map(str::to_ascii_lowercase);
        if matches!(colorterm.as_deref(), Some("truecolor" | "24bit")) {
            return Self::TrueColor;
        }
        let term = term.unwrap_or("").to_ascii_lowercase();
        let term = term.as_str();
        if term.ends_with("-direct")
            || ["kitty", "alacritty", "wezterm", "ghostty", "foot", "iterm"]
                .iter()
                .any(|name| term.contains(name))
        {
            return Self::TrueColor;
        }
        if term.contains("256color") || term.contains("256-color") {
            return Self::Ansi256;
        }
        if term.is_empty() {
            return Self::Ansi16;
        }
        // Terminals that have no colour at all.
        if term == "dumb"
            || term == "unknown"
            || term == "network"
            || term.starts_with("vt")
            || term == "ansi.sys"
        {
            return Self::Mono;
        }
        Self::Ansi16
    }

    /// The depth to draw with: the detected one, lowered by the member's settings. `color` is
    /// the existing colour on/off setting (off is mono); `cap` is a stored ceiling, `None` for
    /// automatic. A member can lower the depth, never raise it above what the client advertised.
    #[must_use]
    pub fn effective(detected: Self, color: bool, cap: Option<Self>) -> Self {
        if !color {
            return Self::Mono;
        }
        cap.map_or(detected, |cap| detected.min(cap))
    }

    /// Whether any colour is drawn.
    #[must_use]
    pub const fn has_color(self) -> bool {
        !matches!(self, Self::Mono)
    }
}

/// One colour as an author means it, with its fallbacks worked out by [`Tint::resolve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    /// 24-bit colour, with the named colour to use on a 16-colour terminal (the nearest named
    /// colour is often a poor match for a dark shade, so the author picks).
    Rgb(u8, u8, u8, Color),
    /// A cell of the 256-colour palette.
    Indexed(u8),
    /// One of the sixteen named colours.
    Named(Color),
}

impl Tint {
    /// The colour to send at `depth`; `None` on a mono terminal.
    #[must_use]
    pub fn resolve(self, depth: ColorDepth) -> Option<Color> {
        match depth {
            ColorDepth::Mono => None,
            ColorDepth::TrueColor => Some(match self {
                Self::Rgb(r, g, b, _) => Color::Rgb(r, g, b),
                Self::Indexed(i) => {
                    let (r, g, b) = indexed_rgb(i);
                    Color::Rgb(r, g, b)
                }
                Self::Named(color) => color,
            }),
            ColorDepth::Ansi256 => Some(match self {
                Self::Rgb(r, g, b, _) => Color::Indexed(nearest_256(r, g, b)),
                Self::Indexed(i) => Color::Indexed(i),
                Self::Named(color) => color,
            }),
            ColorDepth::Ansi16 => Some(match self {
                Self::Rgb(_, _, _, named) | Self::Named(named) => named,
                Self::Indexed(i) => {
                    let (r, g, b) = indexed_rgb(i);
                    nearest_16(r, g, b)
                }
            }),
        }
    }
}

/// The xterm default RGB of palette cell `i`.
#[must_use]
pub fn indexed_rgb(i: u8) -> (u8, u8, u8) {
    const BASE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match i {
        0..=15 => BASE[usize::from(i)],
        16..=231 => {
            let n = usize::from(i - 16);
            (LEVELS[n / 36], LEVELS[(n / 6) % 6], LEVELS[n % 6])
        }
        232..=255 => {
            let level = 8 + (i - 232) * 10;
            (level, level, level)
        }
    }
}

fn distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    let d = |x: u8, y: u8| u32::from(x.abs_diff(y)).pow(2);
    d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
}

/// The nearest cell of the 6x6x6 cube or the grey ramp (cells 16 to 255).
#[must_use]
pub fn nearest_256(r: u8, g: u8, b: u8) -> u8 {
    (16..=255_u8)
        .min_by_key(|i| distance(indexed_rgb(*i), (r, g, b)))
        .unwrap_or(16)
}

/// The nearest of the sixteen named colours.
#[must_use]
pub fn nearest_16(r: u8, g: u8, b: u8) -> Color {
    const NAMED: [Color; 16] = [
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
    let best = (0..16_u8)
        .min_by_key(|i| distance(indexed_rgb(*i), (r, g, b)))
        .unwrap_or(0);
    NAMED[usize::from(best)]
}

/// The surfaces the frame paints (bars, table header, stripes, the selected row), worked out
/// from a theme's palette. Colours are chosen per depth by [`Tint::resolve`]; a mono terminal
/// gets none of them and the widgets fall back to weight, underline and a pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Surfaces {
    /// Header and status bar background.
    pub bar_bg: Tint,
    /// Header and status bar text.
    pub bar_fg: Tint,
    /// Every other table row.
    pub stripe_bg: Tint,
    /// The selected row's background.
    pub select_bg: Tint,
    /// The selected row's text.
    pub select_fg: Tint,
    /// The table header row's background.
    pub head_bg: Tint,
    /// A popup's background.
    pub popup_bg: Tint,
}

impl Surfaces {
    /// The surfaces for a palette whose box and heading colour is `base`. A named colour gives
    /// a family of shades of it; the sixteen-colour fallbacks are fixed so they stay readable
    /// whatever the family.
    #[must_use]
    pub fn of(base: Color) -> Self {
        let (r, g, b) = named_rgb(base);
        let shade = |percent: u16| {
            let scale =
                |c: u8| u8::try_from((u16::from(c) * percent / 100).min(255)).unwrap_or(u8::MAX);
            (scale(r), scale(g), scale(b))
        };
        let (br, bg, bb) = shade(22);
        let (sr, sg, sb) = shade(10);
        let (hr, hg, hb) = shade(34);
        let (pr, pg, pb) = shade(14);
        let (er, eg, eb) = shade(52);
        Self {
            bar_bg: Tint::Rgb(br, bg, bb, Color::Blue),
            bar_fg: Tint::Rgb(235, 240, 245, Color::White),
            stripe_bg: Tint::Rgb(sr, sg, sb, Color::Reset),
            select_bg: Tint::Rgb(er, eg, eb, Color::Cyan),
            select_fg: Tint::Rgb(255, 255, 255, Color::Black),
            head_bg: Tint::Rgb(hr, hg, hb, Color::Blue),
            popup_bg: Tint::Rgb(pr, pg, pb, Color::Reset),
        }
    }
}

/// The xterm RGB of a named colour (anything else is a neutral grey).
fn named_rgb(color: Color) -> (u8, u8, u8) {
    let index = match color {
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Indexed(i) => i,
        Color::Rgb(r, g, b) => return (r, g, b),
        Color::Gray | Color::Reset => 7,
    };
    indexed_rgb(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_reads_what_the_client_says() {
        let d = ColorDepth::detect;
        assert_eq!(d(Some("xterm-256color"), None), ColorDepth::Ansi256);
        assert_eq!(
            d(Some("xterm-256color"), Some("truecolor")),
            ColorDepth::TrueColor
        );
        assert_eq!(d(Some("xterm"), Some("24bit")), ColorDepth::TrueColor);
        assert_eq!(d(Some("xterm-kitty"), None), ColorDepth::TrueColor);
        assert_eq!(d(Some("xterm-direct"), None), ColorDepth::TrueColor);
        assert_eq!(d(Some("tmux-256color"), None), ColorDepth::Ansi256);
        assert_eq!(d(Some("xterm"), None), ColorDepth::Ansi16);
        assert_eq!(d(Some("linux"), None), ColorDepth::Ansi16);
        assert_eq!(d(Some("vt100"), None), ColorDepth::Mono);
        assert_eq!(d(Some("dumb"), None), ColorDepth::Mono);
        assert_eq!(d(Some("ANSI"), None), ColorDepth::Ansi16);
        assert_eq!(d(None, None), ColorDepth::Ansi16);
        assert_eq!(d(Some(""), Some("")), ColorDepth::Ansi16);
        // COLORTERM is only believed for the two truecolor words.
        assert_eq!(d(Some("vt100"), Some("yes")), ColorDepth::Mono);
    }

    #[test]
    fn a_member_can_lower_the_depth_and_never_raise_it() {
        use ColorDepth::*;
        assert_eq!(ColorDepth::effective(TrueColor, true, None), TrueColor);
        assert_eq!(ColorDepth::effective(TrueColor, false, None), Mono);
        assert_eq!(ColorDepth::effective(TrueColor, true, Some(Ansi16)), Ansi16);
        assert_eq!(ColorDepth::effective(Ansi16, true, Some(TrueColor)), Ansi16);
        assert_eq!(ColorDepth::effective(Mono, true, Some(TrueColor)), Mono);
    }

    #[test]
    fn a_tint_degrades_down_the_tiers() {
        let tint = Tint::Rgb(0, 51, 51, Color::Blue);
        assert_eq!(
            tint.resolve(ColorDepth::TrueColor),
            Some(Color::Rgb(0, 51, 51))
        );
        assert!(matches!(
            tint.resolve(ColorDepth::Ansi256),
            Some(Color::Indexed(_))
        ));
        assert_eq!(tint.resolve(ColorDepth::Ansi16), Some(Color::Blue));
        assert_eq!(tint.resolve(ColorDepth::Mono), None);
        // An indexed tint on a 16-colour terminal lands on a named colour of the same family.
        assert_eq!(
            Tint::Indexed(196).resolve(ColorDepth::Ansi16),
            Some(Color::LightRed)
        );
        assert!(Tint::Indexed(21).resolve(ColorDepth::Ansi16).is_some());
        // Only the exact colour survives where it can; named colours pass through.
        assert_eq!(
            Tint::Named(Color::Red).resolve(ColorDepth::Ansi256),
            Some(Color::Red)
        );
    }

    #[test]
    fn the_256_search_finds_exact_cells() {
        assert_eq!(indexed_rgb(16), (0, 0, 0));
        assert_eq!(indexed_rgb(231), (255, 255, 255));
        assert_eq!(indexed_rgb(232), (8, 8, 8));
        assert_eq!(nearest_256(255, 0, 0), 196);
        assert_eq!(nearest_256(8, 8, 8), 232);
    }

    #[test]
    fn surfaces_are_dark_shades_of_the_base_with_fixed_sixteen_colour_fallbacks() {
        let s = Surfaces::of(Color::Cyan);
        assert_eq!(s.bar_bg.resolve(ColorDepth::Ansi16), Some(Color::Blue));
        assert_eq!(s.bar_fg.resolve(ColorDepth::Ansi16), Some(Color::White));
        let truecolor = s.bar_bg.resolve(ColorDepth::TrueColor);
        assert!(
            matches!(truecolor, Some(Color::Rgb(r, g, b)) if r < 60 && g < 60 && b < 60),
            "a dark shade: {truecolor:?}"
        );
        assert_eq!(s.select_bg.resolve(ColorDepth::Mono), None);
    }
}
