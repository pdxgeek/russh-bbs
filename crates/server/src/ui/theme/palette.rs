//! The semantic palette: about twenty colour roles a style fills in (UI-2, D82 extended).
//!
//! Every role is a [`Tint`]: a colour as the author meant it (truecolor, a 256-colour index or
//! one of the sixteen names) with its fallbacks worked out by the colour-depth code, so a style
//! written in truecolor still reads on a 16-colour terminal. **There is no role for a security
//! marker**, and no way to name one: markers are drawn with the reserved style and no colour
//! (D39, D109), and nothing in this module is an input to that drawing.
//!
//! A style may set only the six original roles (the D82 form); the others are then derived from
//! them, so older theme files keep their look. [`check_contrast`] refuses a palette whose text
//! cannot be read against its own backgrounds, so a hostile or careless theme cannot make the
//! screen unreadable.

use ratatui::style::Color;

use super::ThemeError;
use crate::ui::depth::{Surfaces, Tint};

/// Every colour role, in the order the docs list them.
pub const ROLES: [&str; 24] = [
    "surface",
    "panel",
    "popup",
    "stripe",
    "head",
    "bar",
    "bar_text",
    "text",
    "muted",
    "border",
    "heading",
    "key",
    "accent",
    "accent2",
    "selected",
    "select_bg",
    "info",
    "success",
    "warning",
    "error",
    "link",
    "quote",
    "timestamp",
    "title",
];

/// The colours a style sets. Nothing here can draw a security marker, whose style is fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// The screen's background behind everything. `Reset` leaves the terminal's own.
    pub surface: Tint,
    /// The background inside boxes and panes.
    pub panel: Tint,
    /// A popup's background.
    pub popup: Tint,
    /// Every other table row.
    pub stripe: Tint,
    /// A table's header row background.
    pub head: Tint,
    /// The header and status bar background.
    pub bar: Tint,
    /// The header and status bar text.
    pub bar_text: Tint,
    /// Ordinary text.
    pub text: Tint,
    /// Quiet text: hints, timestamps, secondary lines.
    pub muted: Tint,
    /// Box borders.
    pub border: Tint,
    /// Headings and box titles.
    pub heading: Tint,
    /// Hotkeys.
    pub key: Tint,
    /// The board's accent: the name in the header, pointers, rules.
    pub accent: Tint,
    /// A second accent for gradients and secondary decoration.
    pub accent2: Tint,
    /// The text of the row under the cursor.
    pub selected: Tint,
    /// The background of the row under the cursor.
    pub select_bg: Tint,
    /// An informational notice.
    pub info: Tint,
    /// Good news.
    pub success: Tint,
    /// A warning.
    pub warning: Tint,
    /// An error notice.
    pub error: Tint,
    /// A link.
    pub link: Tint,
    /// A quoted block.
    pub quote: Tint,
    /// A timestamp.
    pub timestamp: Tint,
    /// A box's title text (the heading unless a style says otherwise).
    pub title: Tint,
}

impl Default for Palette {
    /// The colours the screens used before styles existed, with the UI-1 surfaces.
    fn default() -> Self {
        Self::from_base(Base::DEFAULT)
    }
}

/// The six roles the D82 theme files set. The rest of a palette is derived from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Base {
    /// Headings and box titles.
    pub heading: Tint,
    /// Hotkeys.
    pub key: Tint,
    /// Box borders.
    pub border: Tint,
    /// An informational notice.
    pub info: Tint,
    /// An error notice.
    pub error: Tint,
    /// The row under the cursor.
    pub selected: Tint,
}

impl Base {
    /// The original default colours.
    pub const DEFAULT: Self = Self {
        heading: Tint::Named(Color::Cyan),
        key: Tint::Named(Color::Yellow),
        border: Tint::Named(Color::Cyan),
        info: Tint::Named(Color::Green),
        error: Tint::Named(Color::Red),
        selected: Tint::Named(Color::Yellow),
    };
}

const RESET: Tint = Tint::Named(Color::Reset);

impl Palette {
    /// A full palette from the six base roles: the surfaces are shades of the border colour (as
    /// UI-1 drew them) and the terminal keeps its own background and text colour.
    #[must_use]
    pub fn from_base(base: Base) -> Self {
        let surfaces = surfaces_of(base.border);
        Self {
            surface: RESET,
            panel: RESET,
            popup: surfaces.popup_bg,
            stripe: surfaces.stripe_bg,
            head: surfaces.head_bg,
            bar: surfaces.bar_bg,
            bar_text: surfaces.bar_fg,
            text: RESET,
            muted: RESET,
            border: base.border,
            heading: base.heading,
            key: base.key,
            accent: base.heading,
            accent2: base.key,
            selected: base.selected,
            select_bg: surfaces.select_bg,
            info: base.info,
            success: base.info,
            warning: base.key,
            error: base.error,
            link: base.info,
            quote: base.info,
            timestamp: RESET,
            title: base.heading,
        }
    }

    /// The role called `name`, for the theme loader.
    pub(super) fn role_mut(&mut self, name: &str) -> Option<&mut Tint> {
        Some(match name {
            "surface" => &mut self.surface,
            "panel" => &mut self.panel,
            "popup" => &mut self.popup,
            "stripe" => &mut self.stripe,
            "head" => &mut self.head,
            "bar" => &mut self.bar,
            "bar_text" => &mut self.bar_text,
            "text" => &mut self.text,
            "muted" => &mut self.muted,
            "border" => &mut self.border,
            "heading" => &mut self.heading,
            "key" => &mut self.key,
            "accent" => &mut self.accent,
            "accent2" => &mut self.accent2,
            "selected" => &mut self.selected,
            "select_bg" => &mut self.select_bg,
            "info" => &mut self.info,
            "success" => &mut self.success,
            "warning" => &mut self.warning,
            "error" => &mut self.error,
            "link" => &mut self.link,
            "quote" => &mut self.quote,
            "timestamp" => &mut self.timestamp,
            "title" => &mut self.title,
            _ => return None,
        })
    }

    /// The frame's surface colours (bars, stripes, selection, popups) from the roles.
    #[must_use]
    pub const fn surfaces(self) -> Surfaces {
        Surfaces {
            bar_bg: self.bar,
            bar_fg: self.bar_text,
            stripe_bg: self.stripe,
            select_bg: self.select_bg,
            select_fg: self.selected,
            head_bg: self.head,
            popup_bg: self.popup,
        }
    }

    /// The board's own accent replaces the style's.
    #[must_use]
    pub const fn with_accent(mut self, accent: Tint) -> Self {
        self.accent = accent;
        self
    }
}

/// Shades of `border` for the bars and the selection, as UI-1 derived them.
fn surfaces_of(border: Tint) -> Surfaces {
    match border {
        Tint::Named(color) => Surfaces::of(color),
        other => {
            let (r, g, b) = other.rgb().unwrap_or((0, 160, 160));
            let shade = |percent: u16| {
                let scale =
                    |c: u8| u8::try_from((u16::from(c) * percent / 100).min(255)).unwrap_or(u8::MAX);
                (scale(r), scale(g), scale(b))
            };
            let tint = |percent, named| {
                let (r, g, b) = shade(percent);
                Tint::Rgb(r, g, b, named)
            };
            Surfaces {
                bar_bg: tint(22, Color::Blue),
                bar_fg: Tint::Rgb(235, 240, 245, Color::White),
                stripe_bg: tint(10, Color::Reset),
                select_bg: tint(52, Color::Cyan),
                select_fg: Tint::Rgb(255, 255, 255, Color::Black),
                head_bg: tint(34, Color::Blue),
                popup_bg: tint(14, Color::Reset),
            }
        }
    }
}

/// Relative luminance of an sRGB colour (WCAG 2.x).
fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    let channel = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// The WCAG contrast ratio of two colours, 1.0 to 21.0.
#[must_use]
pub fn contrast_ratio(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// The text-on-background pairs a style must keep readable, with the least contrast ratio each
/// needs. A pair whose background is the terminal's own (`Reset`) is skipped: its colour is the
/// member's, not ours.
const READABLE: [(&str, &str, f64); 14] = [
    ("text", "surface", 4.5),
    ("text", "panel", 4.5),
    ("heading", "panel", 3.0),
    ("heading", "surface", 3.0),
    ("key", "panel", 3.0),
    ("info", "panel", 3.0),
    ("warning", "panel", 3.0),
    ("error", "panel", 3.0),
    ("muted", "panel", 2.5),
    ("bar_text", "bar", 4.5),
    ("selected", "select_bg", 4.5),
    ("text", "popup", 4.5),
    ("accent", "panel", 3.0),
    ("border", "panel", 2.0),
];

fn role_of(palette: &Palette, name: &str) -> Tint {
    let mut copy = *palette;
    copy.role_mut(name).copied().unwrap_or(RESET)
}

/// Refuse a palette whose text cannot be read against its own backgrounds. A pair is checked
/// when both colours are known and the style set the background itself (`explicit` lists the
/// roles the file wrote): a background the server derived is the server's to get right, and an
/// older colour-only theme keeps its look. A `panel` of `Reset` falls back to the `surface`.
pub fn check_contrast(palette: &Palette, explicit: &[&str]) -> Result<(), ThemeError> {
    for (fg, bg, least) in READABLE {
        let set = |role: &str| explicit.contains(&role);
        if !(set(bg) || (bg == "panel" && set("surface"))) {
            continue;
        }
        let mut back = role_of(palette, bg);
        if back.rgb().is_none() && bg == "panel" {
            back = palette.surface;
        }
        let (Some(front), Some(back)) = (role_of(palette, fg).rgb(), back.rgb()) else {
            continue;
        };
        let ratio = contrast_ratio(front, back);
        if ratio < least {
            return Err(ThemeError::Contrast {
                role: fg.to_owned(),
                against: bg.to_owned(),
                ratio: format!("{ratio:.1}"),
                least: format!("{least:.1}"),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_on_white_is_the_maximum() {
        let ratio = contrast_ratio((0, 0, 0), (255, 255, 255));
        assert!((ratio - 21.0).abs() < 0.01);
    }

    #[test]
    fn invisible_text_is_refused() {
        let mut palette = Palette::default();
        palette.panel = Tint::Rgb(10, 10, 10, Color::Black);
        palette.text = Tint::Rgb(12, 12, 12, Color::Black);
        assert!(matches!(
            check_contrast(&palette, &ROLES),
            Err(ThemeError::Contrast { .. })
        ));
    }

    #[test]
    fn every_role_name_is_known() {
        let mut palette = Palette::default();
        for role in ROLES {
            assert!(palette.role_mut(role).is_some(), "{role}");
        }
        assert!(palette.role_mut("marker").is_none());
    }
}
