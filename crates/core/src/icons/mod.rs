//! The single icon registry: logical names to glyphs, per tier, with display widths.
//!
//! UI code names an icon with the closed enums [`Icon`], [`ContentIcon`] and
//! [`SecurityMarker`] and never holds a raw codepoint. This module is the only place in the
//! workspace where Nerd Font (private-use) glyphs come from; the sanitizer neutralizes the
//! private-use range in all user and external text, so content cannot forge them.
//!
//! # Tiers
//!
//! [`Tier::NerdFont`], [`Tier::Unicode`] (plain, East Asian Width neutral symbols only) and
//! [`Tier::Ascii`] (printable ASCII). Every icon fills all three: each is one `match` arm in
//! `ContentIcon::entry` or `SecurityMarker::entry` with no wildcard, so a new variant that
//! lacks glyphs does not compile.
//!
//! # Widths
//!
//! `width` holds one explicit cell width (1 or 2) for every non-ASCII character the
//! registry uses. Nerd Font glyphs are ambiguous (a terminal may draw them in one cell or
//! overflow into two), so the table *reserves* two cells and [`Icon::render`] pads the glyph
//! with a trailing space: correct under both behaviours. A glyph with no table entry falls
//! back to the ASCII tier rather than risking misalignment (a test proves this never
//! happens).
//!
//! # Security chrome
//!
//! [`SecurityMarker`] icons are system chrome. What is and is not guaranteed (decision D39):
//!
//! * **Reserved presentation.** [`Rendered`] carries a [`ChromeStyle`] (bold + reverse
//!   video) for every marker in every tier. Content can never emit terminal styling, because
//!   the sanitizer removes every escape sequence, so a styled marker cannot be forged by
//!   text. [`Rendered::to_terminal`] is the only way to get a terminal-ready string and it
//!   includes the style; [`Rendered::plain`] is for width measurement and for renderers that
//!   apply [`Rendered::chrome_style`] themselves (a ratatui `Style`). Nothing in the
//!   renderer may apply that style to sanitized content.
//! * **Unicode tier: reserved delimiters and glyphs.** Markers are drawn as `⟦glyph⟧`.
//!   The sanitizer turns the *exact codepoints* U+27E6/U+27E7 and the four marker glyphs into
//!   U+FFFD in all content ([`crate::sanitize::RESERVED_CHROME`]). A test proves the registry
//!   uses exactly that set. It does **not** block lookalikes (`〚☉〛`, `◉`, fullwidth
//!   brackets), which may render like a marker in some fonts, so on a terminal that ignores
//!   styling the Unicode tier also depends on the chrome-only position (D45).
//! * **Nerd Font tier:** the glyph is private-use, which the sanitizer replaces.
//! * **ASCII tier: style and position only.** Content can type any ASCII, so the text
//!   `E2EE` or `SERVER-READABLE` is *not* unforgeable. The distinction is the reserved
//!   style plus a reserved screen position (a chrome-only status line or gutter that content
//!   is never drawn into). On a terminal with no styling at all, position is the only cue.
//!   Callers must draw ASCII-tier markers in a chrome-only position.
//!
//! **Which markers are drawn (D93).** Only `UNENCRYPTED` (telnet) and `LOW-SECURITY` (while
//! telnet is on) are drawn today. [`SecurityMarker::ServerReadable`] stays in the registry for
//! future use but no screen draws it: what the server can read is explained in the docs, not on
//! member screens. Nothing the server can read is ever called private, secure or encrypted.
//!
//! The marker *type* is separate from [`ContentIcon`], so layout and theme code that accepts
//! only content icons cannot be handed one. [`Rendered::is_security_chrome`] tells a renderer
//! it must not hide, replace or restyle the marker.

mod width;

#[cfg(test)]
mod tests;

pub use width::{GlyphWidth, char_width};

/// Terminal styling reserved for security chrome: bold plus reverse video.
///
/// Only chrome code applies it. Sanitized content cannot contain it, because the sanitizer
/// removes every escape sequence and control character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChromeStyle;

impl ChromeStyle {
    /// SGR sequence that turns the reserved style on.
    pub const SGR_ON: &'static str = "\u{1b}[1;7m";
    /// SGR sequence that resets all attributes.
    pub const SGR_OFF: &'static str = "\u{1b}[0m";
    /// Whether the style is bold.
    pub const BOLD: bool = true;
    /// Whether the style is reverse video.
    pub const REVERSE: bool = true;
}

/// Which glyph set to draw from. The server cannot detect the client font, so this is a
/// per-user setting chosen on the glyph test screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    /// Private-use Nerd Font glyphs. Needs a patched client font.
    NerdFont,
    /// Plain Unicode symbols that render in common fonts.
    Unicode,
    /// Printable ASCII only.
    Ascii,
}

impl Tier {
    /// Every tier, in preference order.
    pub const ALL: [Self; 3] = [Self::NerdFont, Self::Unicode, Self::Ascii];
}

/// Glyphs for one icon in each tier, plus its text label.
#[derive(Debug, Clone, Copy)]
struct Entry {
    nerd: &'static str,
    unicode: &'static str,
    ascii: &'static str,
    label: &'static str,
}

/// Ordinary icons. Layouts and themes may place, color and replace these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentIcon {
    /// A message board or forum.
    Board,
    /// Access restriction (a closed board, invite-only). **Not a privacy claim**: it is
    /// deliberately a different glyph from [`SecurityMarker::E2eeLock`], and every tier says
    /// "restricted" or draws a key or a prohibition sign, never a padlock.
    Restricted,
    /// Something unread.
    Unread,
    /// An RSS feed.
    Feed,
    /// Internal mail.
    Mail,
    /// An invite.
    Invite,
}

impl ContentIcon {
    /// Every content icon.
    pub const ALL: [Self; 6] = [
        Self::Board,
        Self::Restricted,
        Self::Unread,
        Self::Feed,
        Self::Mail,
        Self::Invite,
    ];

    // Nerd Font codepoints are Font Awesome (BMP private use, U+F000..). They are written as
    // escapes because the raw characters are invisible in editors and diffs.
    const fn entry(self) -> Entry {
        match self {
            Self::Board => Entry {
                nerd: "\u{f086}", // fa-comments
                unicode: "\u{2263}",
                ascii: "[#]",
                label: "board",
            },
            Self::Restricted => Entry {
                nerd: "\u{f084}",    // fa-key (not the padlock: that is E2EE chrome)
                unicode: "\u{2298}", // circled division slash: "not open to everyone"
                ascii: "[restricted]",
                label: "restricted",
            },
            Self::Unread => Entry {
                nerd: "\u{f111}", // fa-circle
                unicode: "\u{25c9}",
                ascii: "(*)",
                label: "unread",
            },
            Self::Feed => Entry {
                nerd: "\u{f09e}", // fa-rss
                unicode: "\u{223f}",
                ascii: "[~]",
                label: "feed",
            },
            Self::Mail => Entry {
                nerd: "\u{f0e0}", // fa-envelope
                unicode: "\u{2709}",
                ascii: "[@]",
                label: "mail",
            },
            Self::Invite => Entry {
                nerd: "\u{f234}", // fa-user-plus
                unicode: "\u{2731}",
                ascii: "[+]",
                label: "invite",
            },
        }
    }
}

/// Security chrome. Drawn only by chrome code; see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecurityMarker {
    /// The server operator can read this content. Not drawn by any screen (D93).
    ServerReadable,
    /// End-to-end encrypted (future; arrives with the native client, decisions D3 to D5).
    E2eeLock,
    /// A signature badge.
    Signature,
    /// A member's key changed; warn loudly.
    KeyChanged,
    /// This session is not encrypted: anyone on the network path can read it (the telnet
    /// transport, D27). Drawn on every telnet screen.
    Unencrypted,
    /// The board is in low-security mode: the sysop turned an unencrypted transport on (D27).
    /// Drawn on staff screens while it is on.
    LowSecurity,
}

impl SecurityMarker {
    /// Every security marker.
    pub const ALL: [Self; 6] = [
        Self::ServerReadable,
        Self::E2eeLock,
        Self::Signature,
        Self::KeyChanged,
        Self::Unencrypted,
        Self::LowSecurity,
    ];

    /// The text label, also the ASCII-tier form.
    #[must_use]
    pub const fn label(self) -> &'static str {
        self.entry().label
    }

    // The ASCII tier is the honest label itself, and equals `label`. It is *not*
    // unforgeable by content; see the module docs. Unicode forms sit inside the reserved
    // delimiters U+27E6 and U+27E7.
    const fn entry(self) -> Entry {
        match self {
            Self::ServerReadable => Entry {
                nerd: "\u{f06e}",                    // fa-eye
                unicode: "\u{27e6}\u{2299}\u{27e7}", // circled dot: an eye, "someone can look"
                ascii: "SERVER-READABLE",
                label: "SERVER-READABLE",
            },
            Self::E2eeLock => Entry {
                nerd: "\u{f023}", // fa-lock (the padlock)
                unicode: "\u{27e6}\u{229b}\u{27e7}",
                ascii: "E2EE",
                label: "E2EE",
            },
            Self::Signature => Entry {
                nerd: "\u{f0a3}", // fa-certificate
                unicode: "\u{27e6}\u{270e}\u{27e7}",
                ascii: "SIGNED",
                label: "SIGNED",
            },
            Self::KeyChanged => Entry {
                nerd: "\u{f071}", // fa-warning
                unicode: "\u{27e6}\u{26a0}\u{27e7}",
                ascii: "KEY-CHANGED",
                label: "KEY-CHANGED",
            },
            Self::Unencrypted => Entry {
                nerd: "\u{f09c}",                    // fa-unlock
                unicode: "\u{27e6}\u{2297}\u{27e7}", // circled times: "crossed out"
                ascii: "UNENCRYPTED",
                label: "UNENCRYPTED",
            },
            Self::LowSecurity => Entry {
                nerd: "\u{f132}",                    // fa-shield
                unicode: "\u{27e6}\u{2296}\u{27e7}", // circled minus: "less protection"
                ascii: "LOW-SECURITY",
                label: "LOW-SECURITY",
            },
        }
    }
}

/// Any icon: ordinary content or security chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    /// An ordinary icon.
    Content(ContentIcon),
    /// Security chrome.
    Security(SecurityMarker),
}

impl From<ContentIcon> for Icon {
    fn from(icon: ContentIcon) -> Self {
        Self::Content(icon)
    }
}

impl From<SecurityMarker> for Icon {
    fn from(marker: SecurityMarker) -> Self {
        Self::Security(marker)
    }
}

/// An icon ready to write to a terminal.
///
/// For a security marker the reserved [`ChromeStyle`] is part of the value: there is no
/// accessor that returns a marker's text as a bare `&str` under a name that suggests it is
/// ready to print. Use [`Rendered::to_terminal`] (styled) or [`Rendered::plain`] plus
/// [`Rendered::chrome_style`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    text: String,
    width: usize,
    chrome: Option<ChromeStyle>,
    tier: Tier,
}

impl Rendered {
    /// The text without any styling, including alignment padding and (for Unicode-tier
    /// markers) the reserved delimiters. For measuring and for renderers that apply
    /// [`Rendered::chrome_style`] through their own style type. Never print a marker's
    /// `plain()` text unstyled.
    #[must_use]
    pub fn plain(&self) -> &str {
        &self.text
    }

    /// The text to write to a terminal. For a security marker this wraps the text in the
    /// reserved style; content icons are returned as is.
    #[must_use]
    pub fn to_terminal(&self) -> String {
        match self.chrome {
            Some(_) => format!(
                "{}{}{}",
                ChromeStyle::SGR_ON,
                self.text,
                ChromeStyle::SGR_OFF
            ),
            None => self.text.clone(),
        }
    }

    /// The terminal cells the text occupies (style sequences take none).
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// The reserved style a renderer must apply, if this is security chrome.
    #[must_use]
    pub const fn chrome_style(&self) -> Option<ChromeStyle> {
        self.chrome
    }

    /// Whether this is security chrome that a layout or theme must not hide, replace or
    /// restyle misleadingly.
    #[must_use]
    pub const fn is_security_chrome(&self) -> bool {
        self.chrome.is_some()
    }

    /// The tier actually used (the ASCII tier when a wider tier had no known width).
    #[must_use]
    pub const fn tier(&self) -> Tier {
        self.tier
    }
}

impl Icon {
    /// Every icon.
    pub fn all() -> impl Iterator<Item = Self> {
        ContentIcon::ALL
            .into_iter()
            .map(Self::Content)
            .chain(SecurityMarker::ALL.into_iter().map(Self::Security))
    }

    /// Whether this is security chrome.
    #[must_use]
    pub const fn is_security_chrome(self) -> bool {
        matches!(self, Self::Security(_))
    }

    const fn entry(self) -> Entry {
        match self {
            Self::Content(icon) => icon.entry(),
            Self::Security(marker) => marker.entry(),
        }
    }

    /// The bare glyph text for `tier`, without alignment padding.
    #[must_use]
    pub const fn glyph(self, tier: Tier) -> &'static str {
        let entry = self.entry();
        match tier {
            Tier::NerdFont => entry.nerd,
            Tier::Unicode => entry.unicode,
            Tier::Ascii => entry.ascii,
        }
    }

    /// The plain-text fallback: the honest label for security markers
    /// (`SERVER-READABLE`, `E2EE`), a short word for the rest. Always ASCII.
    #[must_use]
    pub const fn text_label(self) -> &'static str {
        self.entry().label
    }

    /// Render for `tier`, padded so the width is exact. If any character of the glyph has
    /// no width entry, the ASCII tier is used instead.
    #[must_use]
    pub fn render(self, tier: Tier) -> Rendered {
        let (glyph, tier) = match width::str_width(self.glyph(tier)) {
            Some(_) => (self.glyph(tier), tier),
            None => (self.glyph(Tier::Ascii), Tier::Ascii),
        };
        // ASCII always has a width, so the fallback cannot fail; `unwrap_or` avoids a panic.
        let width = width::str_width(glyph).unwrap_or_else(|| glyph.chars().count());
        let padding = width.saturating_sub(glyph.chars().count());
        let mut text = String::with_capacity(glyph.len() + padding);
        text.push_str(glyph);
        text.extend(std::iter::repeat_n(' ', padding));
        Rendered {
            text,
            width,
            chrome: self.is_security_chrome().then_some(ChromeStyle),
            tier,
        }
    }
}
