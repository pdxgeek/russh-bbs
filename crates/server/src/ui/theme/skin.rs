//! How a style draws, beyond its colours: borders, title treatment, header decoration, the
//! backdrop behind the panels and gradients (UI-2).
//!
//! Everything here is **chrome decoration**. It is drawn from fixed characters in this file,
//! never from member text, and none of it can use the reserved marker style (bold plus reverse):
//! a skin has no way to say "bold", "reverse" or any attribute at all, only which of a few fixed
//! drawings to use. The ASCII tier replaces every decoration with printable ASCII.

use ratatui::symbols::border;
use ratatui::widgets::BorderType;

use super::ThemeError;
use crate::ui::depth::{ColorDepth, Tint, nearest_16};

/// Printable-ASCII box: the border for the ASCII tier.
pub(crate) const ASCII_BORDER: border::Set = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

/// A border drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderSet {
    /// Thin straight lines.
    Plain,
    /// Thin lines with rounded corners.
    Rounded,
    /// Double lines.
    Double,
    /// Thick lines.
    Thick,
    /// `+ - |`.
    Ascii,
    /// No border: the panel is told apart by its background and its title.
    None,
}

impl BorderSet {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "plain" => Self::Plain,
            "rounded" => Self::Rounded,
            "double" => Self::Double,
            "thick" => Self::Thick,
            "ascii" => Self::Ascii,
            "none" => Self::None,
            _ => return None,
        })
    }

    /// The ratatui border type, or `None` for the ASCII set and for no border.
    pub(crate) const fn border_type(self) -> Option<BorderType> {
        match self {
            Self::Plain => Some(BorderType::Plain),
            Self::Rounded => Some(BorderType::Rounded),
            Self::Double => Some(BorderType::Double),
            Self::Thick => Some(BorderType::Thick),
            Self::Ascii | Self::None => None,
        }
    }
}

/// Which element a border belongs to; a style can give each its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element {
    /// The main box of a screen.
    Frame,
    /// A pane inside a layout.
    Pane,
    /// A popup.
    Popup,
}

/// How a box title is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleTreatment {
    /// ` Title `.
    Plain,
    /// ` TITLE `.
    Caps,
    /// ` T I T L E `.
    Spaced,
    /// `[ Title ]`.
    Bracket,
    /// A title set into the border: `╡ Title ╞`.
    Rule,
    /// `« Title »`.
    Angle,
}

impl TitleTreatment {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "plain" => Self::Plain,
            "caps" => Self::Caps,
            "spaced" => Self::Spaced,
            "bracket" => Self::Bracket,
            "rule" => Self::Rule,
            "angle" => Self::Angle,
            _ => return None,
        })
    }

    /// `title` written in this treatment. The title is chrome (string table or fixed text), so
    /// changing its case or spacing is safe.
    pub(crate) fn render(self, title: &str, ascii: bool) -> String {
        match self {
            Self::Plain => format!(" {title} "),
            Self::Caps => format!(" {} ", title.to_uppercase()),
            Self::Spaced => {
                let shout = title.to_uppercase();
                if shout.chars().count() > 18 {
                    return format!(" {shout} ");
                }
                let spaced: Vec<String> = shout.chars().map(String::from).collect();
                format!(" {} ", spaced.join(" "))
            }
            Self::Bracket => format!("[ {title} ]"),
            Self::Rule if ascii => format!("| {title} |"),
            Self::Rule => format!("\u{2561} {title} \u{255e}"),
            Self::Angle if ascii => format!("<< {title} >>"),
            Self::Angle => format!("\u{ab} {title} \u{bb}"),
        }
    }
}

/// Where a box title sits on its border.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleAlign {
    /// At the left.
    Left,
    /// In the middle.
    Center,
}

/// What flanks the board name in the header bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderDeco {
    /// Nothing.
    None,
    /// `░▒▓ name ▓▒░`.
    Blocks,
    /// `» name «`.
    Chevrons,
    /// `◆ name ◆`.
    Diamonds,
}

impl HeaderDeco {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "none" => Self::None,
            "blocks" => Self::Blocks,
            "chevrons" => Self::Chevrons,
            "diamonds" => Self::Diamonds,
            _ => return None,
        })
    }

    /// The text before and after the name.
    pub(crate) const fn flank(self, ascii: bool) -> (&'static str, &'static str) {
        match (self, ascii) {
            (Self::None, _) => ("", ""),
            (Self::Blocks, false) => ("\u{2591}\u{2592}\u{2593} ", " \u{2593}\u{2592}\u{2591}"),
            (Self::Blocks, true) => ("## ", " ##"),
            (Self::Chevrons, false) => ("\u{bb} ", " \u{ab}"),
            (Self::Chevrons, true) => (">> ", " <<"),
            (Self::Diamonds, false) => ("\u{25c6} ", " \u{25c6}"),
            (Self::Diamonds, true) => ("* ", " *"),
        }
    }
}

/// The texture drawn on the screen behind the panels. Panels clear their own area, so a backdrop
/// never shows through text. It is dropped on a mono terminal and in the ASCII tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backdrop {
    /// Plain background.
    None,
    /// Every other row a shade darker, like a CRT's scanlines.
    Scanlines,
    /// Sparse stars.
    Stars,
    /// A faint dot grid.
    Grid,
}

impl Backdrop {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "none" => Self::None,
            "scanlines" => Self::Scanlines,
            "stars" => Self::Stars,
            "grid" => Self::Grid,
            _ => return None,
        })
    }
}

/// A style's drawing choices (its colours are the [`Palette`](super::Palette)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Skin {
    /// Border drawing of a screen's main box.
    pub frame: BorderSet,
    /// Border drawing of a layout's panes.
    pub pane: BorderSet,
    /// Border drawing of popups.
    pub popup: BorderSet,
    /// How box titles are written.
    pub title: TitleTreatment,
    /// Where box titles sit; `None` leaves each screen's own choice.
    pub align: Option<TitleAlign>,
    /// What flanks the board name in the header.
    pub deco: HeaderDeco,
    /// The texture behind the panels.
    pub backdrop: Backdrop,
    /// Draw the board name and rules as an accent gradient where the colour depth allows.
    pub gradient: bool,
    /// Whether the style allows motion (a ceiling on the member's setting, UI-4 and UI-8 use it).
    pub motion: bool,
}

impl Default for Skin {
    /// What the screens looked like before styles: plain thin boxes, plain titles at the left.
    fn default() -> Self {
        Self {
            frame: BorderSet::Plain,
            pane: BorderSet::Plain,
            popup: BorderSet::Plain,
            title: TitleTreatment::Plain,
            align: None,
            deco: HeaderDeco::None,
            backdrop: Backdrop::None,
            gradient: false,
            motion: true,
        }
    }
}

impl Skin {
    /// The border drawing for `element`.
    #[must_use]
    pub const fn border(self, element: Element) -> BorderSet {
        match element {
            Element::Frame => self.frame,
            Element::Pane => self.pane,
            Element::Popup => self.popup,
        }
    }

    /// Read the `[style]` table of a theme file. Every key is checked; unknown ones are refused.
    pub(super) fn from_table(table: &toml::Table) -> Result<Self, ThemeError> {
        let mut skin = Self::default();
        for (section, value) in table {
            let toml::Value::Table(inner) = value else {
                return Err(style_error(format!("`style.{section}` must be a table")));
            };
            for (key, value) in inner {
                let toml::Value::String(text) = value else {
                    if let (("motion", "default"), toml::Value::Boolean(on)) =
                        ((section.as_str(), key.as_str()), value)
                    {
                        skin.motion = *on;
                        continue;
                    }
                    if let (("header", "gradient"), toml::Value::Boolean(on)) =
                        ((section.as_str(), key.as_str()), value)
                    {
                        skin.gradient = *on;
                        continue;
                    }
                    return Err(style_error(format!(
                        "`style.{section}.{key}` has the wrong type"
                    )));
                };
                let bad =
                    || style_error(format!("`style.{section}.{key}` = {text:?} is not known"));
                match (section.as_str(), key.as_str()) {
                    ("borders", "frame") => skin.frame = BorderSet::parse(text).ok_or_else(bad)?,
                    ("borders", "pane") => skin.pane = BorderSet::parse(text).ok_or_else(bad)?,
                    ("borders", "popup") => skin.popup = BorderSet::parse(text).ok_or_else(bad)?,
                    ("title", "treatment") => {
                        skin.title = TitleTreatment::parse(text).ok_or_else(bad)?;
                    }
                    ("title", "align") => {
                        skin.align = Some(match text.as_str() {
                            "left" => TitleAlign::Left,
                            "center" => TitleAlign::Center,
                            _ => return Err(bad()),
                        });
                    }
                    ("header", "decoration") => {
                        skin.deco = HeaderDeco::parse(text).ok_or_else(bad)?;
                    }
                    ("backdrop", "kind") => {
                        skin.backdrop = Backdrop::parse(text).ok_or_else(bad)?
                    }
                    _ => {
                        return Err(style_error(format!(
                            "`style.{section}.{key}` is not a style setting"
                        )));
                    }
                }
            }
        }
        Ok(skin)
    }
}

fn style_error(message: String) -> ThemeError {
    ThemeError::Style(message)
}

/// Parse one colour as a theme file writes it: `"#rrggbb"`, a 256-colour index (`208`), one of
/// the sixteen names, `"reset"` for the terminal's own, or `{ rgb = "#rrggbb", named = "red" }`
/// to choose the sixteen-colour fallback. A hex colour without a fallback gets the nearest of
/// the sixteen.
pub(crate) fn parse_tint(value: &toml::Value) -> Result<Tint, String> {
    match value {
        toml::Value::Integer(index) => u8::try_from(*index)
            .map(Tint::Indexed)
            .map_err(|_| "a colour index is 0 to 255".to_owned()),
        toml::Value::String(text) => parse_text(text),
        toml::Value::Table(table) => {
            let rgb = match table.get("rgb") {
                Some(toml::Value::String(text)) => hex(text)?,
                _ => return Err("`rgb` must be \"#rrggbb\"".to_owned()),
            };
            let named = match table.get("named") {
                Some(toml::Value::String(name)) => {
                    named(name).ok_or_else(|| "`named` is not a known colour name".to_owned())?
                }
                None => nearest_16(rgb.0, rgb.1, rgb.2),
                Some(_) => return Err("`named` must be a colour name".to_owned()),
            };
            if table.keys().any(|k| k != "rgb" && k != "named") {
                return Err("a colour table has only `rgb` and `named`".to_owned());
            }
            Ok(Tint::Rgb(rgb.0, rgb.1, rgb.2, named))
        }
        _ => Err("a colour is a name, \"#rrggbb\" or a 0-255 index".to_owned()),
    }
}

fn parse_text(text: &str) -> Result<Tint, String> {
    if text.starts_with('#') {
        let (r, g, b) = hex(text)?;
        return Ok(Tint::Rgb(r, g, b, nearest_16(r, g, b)));
    }
    named(text)
        .map(Tint::Named)
        .ok_or_else(|| format!("{text:?} is not a known colour"))
}

fn hex(text: &str) -> Result<(u8, u8, u8), String> {
    let digits = text.strip_prefix('#').unwrap_or("");
    let ok = digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit());
    if !ok {
        return Err(format!("{text:?} is not \"#rrggbb\""));
    }
    let byte = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).map_err(|e| e.to_string());
    Ok((byte(0)?, byte(2)?, byte(4)?))
}

/// The sixteen named terminal colours, plus `reset` for the terminal's own.
pub(super) fn named(name: &str) -> Option<ratatui::style::Color> {
    use ratatui::style::Color;
    Some(match name {
        "reset" | "default" => Color::Reset,
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" => Color::Gray,
        "darkgray" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        "white" => Color::White,
        _ => return None,
    })
}

/// The colour `t` (0 to 255) of the way from `from` to `to`, as a truecolor tint. `None` when
/// either end has no known RGB (the terminal's own colour), so the caller draws it flat.
#[must_use]
pub(crate) fn blend(from: Tint, to: Tint, t: u8) -> Option<Tint> {
    let (a, b) = (from.rgb()?, to.rgb()?);
    let mix = |x: u8, y: u8| {
        let (x, y, t) = (i32::from(x), i32::from(y), i32::from(t));
        u8::try_from((x * (255 - t) + y * t) / 255).unwrap_or(u8::MAX)
    };
    let (r, g, bl) = (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2));
    Some(Tint::Rgb(r, g, bl, nearest_16(r, g, bl)))
}

/// Whether a gradient is drawn at `depth`: the 256-colour and truecolor tiers have enough
/// steps; sixteen colours would band, so those draw the flat accent.
#[must_use]
pub(crate) fn gradient_depth(depth: ColorDepth) -> bool {
    depth >= ColorDepth::Ansi256
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_in_every_form() {
        assert_eq!(
            parse_tint(&toml::Value::String("#102030".into()))
                .unwrap()
                .rgb(),
            Some((0x10, 0x20, 0x30))
        );
        assert_eq!(
            parse_tint(&toml::Value::Integer(208)).unwrap(),
            Tint::Indexed(208)
        );
        assert!(parse_tint(&toml::Value::Integer(300)).is_err());
        assert!(parse_tint(&toml::Value::String("#12345".into())).is_err());
        assert!(parse_tint(&toml::Value::String("#12345g".into())).is_err());
        assert!(parse_tint(&toml::Value::String("chartreuse".into())).is_err());
        assert!(parse_tint(&toml::Value::Boolean(true)).is_err());
    }

    #[test]
    fn titles_have_an_ascii_form() {
        for treatment in [
            TitleTreatment::Plain,
            TitleTreatment::Caps,
            TitleTreatment::Spaced,
            TitleTreatment::Bracket,
            TitleTreatment::Rule,
            TitleTreatment::Angle,
        ] {
            assert!(treatment.render("Forums", true).is_ascii(), "{treatment:?}");
        }
        for deco in [
            HeaderDeco::None,
            HeaderDeco::Blocks,
            HeaderDeco::Chevrons,
            HeaderDeco::Diamonds,
        ] {
            let (a, b) = deco.flank(true);
            assert!(a.is_ascii() && b.is_ascii(), "{deco:?}");
        }
    }

    #[test]
    fn a_blend_runs_between_its_ends() {
        let from = Tint::Rgb(0, 0, 0, ratatui::style::Color::Black);
        let to = Tint::Rgb(255, 255, 255, ratatui::style::Color::White);
        assert_eq!(blend(from, to, 0).unwrap().rgb(), Some((0, 0, 0)));
        assert_eq!(blend(from, to, 255).unwrap().rgb(), Some((255, 255, 255)));
        let middle = blend(from, to, 128).unwrap().rgb().unwrap();
        assert!((126..=129).contains(&middle.0));
        assert!(blend(from, Tint::Named(ratatui::style::Color::Reset), 10).is_none());
    }
}
