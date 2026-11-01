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
//! [`SecurityMarker`] icons are system chrome. They are a separate type, so layout and theme
//! code that accepts only [`ContentIcon`] cannot be handed one, and [`Rendered`] carries
//! [`Rendered::is_security_chrome`] so a renderer must not restyle or drop it. Their ASCII
//! tier is the honest text label (`SERVER-READABLE`, `E2EE`), also available from
//! [`Icon::text_label`] for every icon.

mod width;

#[cfg(test)]
mod tests;

pub use width::{GlyphWidth, char_width};

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
    /// deliberately a different glyph from [`SecurityMarker::E2eeLock`].
    Lock,
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
        Self::Lock,
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
            Self::Lock => Entry {
                nerd: "\u{f084}", // fa-key (not the padlock: that is E2EE chrome)
                unicode: "\u{26b7}",
                ascii: "[x]",
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
    /// The server operator can read this content.
    ServerReadable,
    /// End-to-end encrypted (future; arrives with the native client, decisions D3 to D5).
    E2eeLock,
    /// A signature badge.
    Signature,
    /// A member's key changed; warn loudly.
    KeyChanged,
}

impl SecurityMarker {
    /// Every security marker.
    pub const ALL: [Self; 4] = [
        Self::ServerReadable,
        Self::E2eeLock,
        Self::Signature,
        Self::KeyChanged,
    ];

    // The ASCII tier is the honest label itself, and equals `label`.
    const fn entry(self) -> Entry {
        match self {
            Self::ServerReadable => Entry {
                nerd: "\u{f06e}", // fa-eye
                unicode: "\u{2298}",
                ascii: "SERVER-READABLE",
                label: "SERVER-READABLE",
            },
            Self::E2eeLock => Entry {
                nerd: "\u{f023}", // fa-lock (the padlock)
                unicode: "\u{229b}",
                ascii: "E2EE",
                label: "E2EE",
            },
            Self::Signature => Entry {
                nerd: "\u{f0a3}", // fa-certificate
                unicode: "\u{270e}",
                ascii: "SIGNED",
                label: "SIGNED",
            },
            Self::KeyChanged => Entry {
                nerd: "\u{f071}", // fa-warning
                unicode: "\u{26a0}",
                ascii: "KEY-CHANGED",
                label: "KEY-CHANGED",
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    text: String,
    width: usize,
    security_chrome: bool,
    tier: Tier,
}

impl Rendered {
    /// The text to write, including any alignment padding.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The terminal cells `as_str` occupies.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Whether this is security chrome that a layout or theme must not hide, replace or
    /// restyle misleadingly.
    #[must_use]
    pub const fn is_security_chrome(&self) -> bool {
        self.security_chrome
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
            security_chrome: self.is_security_chrome(),
            tier,
        }
    }
}
