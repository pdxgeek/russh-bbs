//! Nick colours: a handle always gets the same colour from a hash of its name (UI-4).
//!
//! The colours are the theme's own content roles (accent, link, key and so on), which every
//! style already keeps readable on its panel, so a nick is as legible as the rest of the text
//! in a dark style and in Paper. **A nick is never given the reserved marker style**: the
//! style built here sets a foreground colour and, on a mono terminal, nothing at all, and
//! never sets reverse video. A handle is content: callers draw it through the shared
//! sanitizer, and the colour is only a hint, never the way to tell who said what (the
//! handle is in the text).

use ratatui::style::Style;

use crate::settings::Look;
use crate::ui::depth::Tint;

/// How many different colours a nick can have.
pub(crate) const HUES: usize = 8;

/// FNV-1a of the handle, lower-cased so `Ann` and `ann` match (handles compare that way).
fn hash(handle: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for c in handle.chars().flat_map(char::to_lowercase) {
        let mut buf = [0_u8; 4];
        for byte in c.encode_utf8(&mut buf).bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

/// Which of the [`HUES`] colours `handle` gets.
#[must_use]
pub(crate) fn hue_of(handle: &str) -> usize {
    usize::try_from(hash(handle) % HUES as u64).unwrap_or(0)
}

/// The tint for hue `index`, from the theme's palette.
fn tint(look: Look, index: usize) -> Tint {
    let palette = look.palette();
    [
        palette.accent,
        palette.accent2,
        palette.info,
        palette.success,
        palette.warning,
        palette.link,
        palette.key,
        palette.quote,
    ][index % HUES]
}

/// The style for a nick: its hue as a foreground colour. Mono gets no colour (the handle
/// itself says who it is). No modifier is ever set, reverse video least of all.
#[must_use]
pub(crate) fn style(look: Look, handle: &str) -> Style {
    look.fg(tint(look, hue_of(handle)))
}
