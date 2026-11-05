//! The member's choice of layout and theme (M3-A, D82).
//!
//! The layout is one of a closed set the code draws ([`LayoutId`]); a theme is a named pack the
//! sysop supplied or the build ships, so its name is bounded text ([`ThemeName`]) and whether it
//! exists is the server's question (an unknown name falls back to the default theme). Neither
//! choice can change what a security marker looks like or where it is drawn; that is fixed in
//! the server's chrome (D39).

use std::fmt;

/// How menus are arranged. Every layout draws the same menu tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum LayoutId {
    /// A bordered box of `[k] Label` lines. The default and the accessible choice.
    #[default]
    Classic,
    /// A list with a highlighted row moved with the up and down arrow keys, picked with Enter.
    /// Hotkeys still work.
    Lightbar,
    /// One line per item, no border or art, for narrow terminals and screen readers.
    Compact,
    /// Menu on the left, a widget grid on the right (UI-2).
    Dashboard,
    /// Three columns for wide terminals: navigation, content, widgets (UI-2).
    Wide,
    /// Content full-screen under a slim header, widgets in a pop-over (UI-2).
    Focus,
    /// A screen beside a second pane (UI-2).
    Split,
}

impl LayoutId {
    /// Every layout, in the order "next layout" cycles through them.
    pub const ALL: [Self; 7] = [
        Self::Classic,
        Self::Lightbar,
        Self::Compact,
        Self::Dashboard,
        Self::Wide,
        Self::Focus,
        Self::Split,
    ];

    /// The layout after this one, wrapping.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Classic => Self::Lightbar,
            Self::Lightbar => Self::Compact,
            Self::Compact => Self::Dashboard,
            Self::Dashboard => Self::Wide,
            Self::Wide => Self::Focus,
            Self::Focus => Self::Split,
            Self::Split => Self::Classic,
        }
    }

    /// The string-table key of this layout's name, for the settings screen.
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Classic => super::strings::LAYOUT_CLASSIC,
            Self::Lightbar => super::strings::LAYOUT_LIGHTBAR,
            Self::Compact => super::strings::LAYOUT_COMPACT,
            Self::Dashboard => super::strings::LAYOUT_DASHBOARD,
            Self::Wide => super::strings::LAYOUT_WIDE,
            Self::Focus => super::strings::LAYOUT_FOCUS,
            Self::Split => super::strings::LAYOUT_SPLIT,
        }
    }

    /// Whether this layout draws a cursor, so Up, Down and Enter mean something on a menu.
    /// Layouts without one leave those keys alone, so Enter never acts on an unseen selection.
    #[must_use]
    pub const fn has_cursor(self) -> bool {
        !matches!(self, Self::Classic | Self::Compact)
    }
}

/// The stable text stored for a layout. Never rename a value; add a migration instead.
#[must_use]
pub const fn layout_to_stored(layout: LayoutId) -> &'static str {
    match layout {
        LayoutId::Classic => "classic",
        LayoutId::Lightbar => "lightbar",
        LayoutId::Compact => "compact",
        LayoutId::Dashboard => "dashboard",
        LayoutId::Wide => "wide",
        LayoutId::Focus => "focus",
        LayoutId::Split => "split",
    }
}

/// Parse the stored layout text.
#[must_use]
pub fn layout_from_stored(text: &str) -> Option<LayoutId> {
    LayoutId::ALL
        .into_iter()
        .find(|layout| layout_to_stored(*layout) == text)
}

/// A member's colour-depth ceiling (UI-2). It can only lower what the client advertises
/// (`ColorDepth::effective` in the server), never raise it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DepthCap {
    /// Use what the terminal advertises.
    #[default]
    Auto,
    /// At most the sixteen named colours.
    Ansi16,
    /// At most the 256-colour palette.
    Ansi256,
    /// At most 24-bit colour (no cap in practice).
    TrueColor,
}

impl DepthCap {
    /// Every cap, in the order "next" cycles.
    pub const ALL: [Self; 4] = [Self::Auto, Self::Ansi16, Self::Ansi256, Self::TrueColor];

    /// The cap after this one, wrapping.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Auto => Self::Ansi16,
            Self::Ansi16 => Self::Ansi256,
            Self::Ansi256 => Self::TrueColor,
            Self::TrueColor => Self::Auto,
        }
    }

    /// The string-table key of this cap's name.
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Auto => super::strings::DEPTH_AUTO,
            Self::Ansi16 => super::strings::DEPTH_16,
            Self::Ansi256 => super::strings::DEPTH_256,
            Self::TrueColor => super::strings::DEPTH_TRUECOLOR,
        }
    }
}

/// The stable text stored for a depth cap.
#[must_use]
pub const fn depth_cap_to_stored(cap: DepthCap) -> &'static str {
    match cap {
        DepthCap::Auto => "auto",
        DepthCap::Ansi16 => "16",
        DepthCap::Ansi256 => "256",
        DepthCap::TrueColor => "truecolor",
    }
}

/// Parse the stored depth cap text.
#[must_use]
pub fn depth_cap_from_stored(text: &str) -> Option<DepthCap> {
    DepthCap::ALL
        .into_iter()
        .find(|cap| depth_cap_to_stored(*cap) == text)
}

/// Which borders a member draws (UI-2): the style's own, plain ASCII, or block elements
/// (FIX-UI-7a: edges that fill the cell, for terminals that draw box lines from the font).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderChoice {
    /// The style's border set.
    #[default]
    ThemeDefault,
    /// Plain `+ - |` borders whatever the style says.
    Ascii,
    /// Block-element borders (`▐ ▌ ▄ ▀`), whatever the style says. The ASCII icon tier still
    /// draws `+ - |`.
    Blocks,
}

impl BorderChoice {
    /// Every choice, in the order "next" cycles.
    pub const ALL: [Self; 3] = [Self::ThemeDefault, Self::Ascii, Self::Blocks];

    /// The other choice.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::ThemeDefault => Self::Ascii,
            Self::Ascii => Self::Blocks,
            Self::Blocks => Self::ThemeDefault,
        }
    }

    /// The string-table key of this choice's name.
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::ThemeDefault => super::strings::BORDERS_THEME,
            Self::Ascii => super::strings::BORDERS_ASCII,
            Self::Blocks => super::strings::BORDERS_BLOCKS,
        }
    }
}

/// The stable text stored for a border choice.
#[must_use]
pub const fn borders_to_stored(borders: BorderChoice) -> &'static str {
    match borders {
        BorderChoice::ThemeDefault => "theme",
        BorderChoice::Ascii => "ascii",
        BorderChoice::Blocks => "blocks",
    }
}

/// Parse the stored border choice text.
#[must_use]
pub fn borders_from_stored(text: &str) -> Option<BorderChoice> {
    BorderChoice::ALL
        .into_iter()
        .find(|b| borders_to_stored(*b) == text)
}

/// The most characters a theme name may have.
pub const THEME_NAME_MAX: usize = 24;

/// The name of the theme every member starts with and every failure falls back to.
pub const DEFAULT_THEME: &str = "default";

/// A theme's name: 1 to [`THEME_NAME_MAX`] characters of `a-z`, `0-9`, `_` and `-`, starting
/// with a letter or digit. Held inline so settings stay `Copy`. The name is shown on screen and
/// stored, so it is restricted to characters that need no sanitizing.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThemeName {
    bytes: [u8; THEME_NAME_MAX],
    len: u8,
}

/// Why a theme name was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "a theme name is 1 to {THEME_NAME_MAX} characters of a-z, 0-9, _ and -, starting with a letter or digit"
)]
pub struct ThemeNameError;

impl ThemeName {
    /// Check and keep `name`.
    pub fn new(name: &str) -> Result<Self, ThemeNameError> {
        let valid = !name.is_empty()
            && name.len() <= THEME_NAME_MAX
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
            && name
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        if !valid {
            return Err(ThemeNameError);
        }
        let mut bytes = [0; THEME_NAME_MAX];
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        let len = u8::try_from(name.len()).map_err(|_| ThemeNameError)?;
        Ok(Self { bytes, len })
    }

    /// The default theme's name.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "DEFAULT_THEME is 7 bytes, checked against THEME_NAME_MAX at compile time"
    )]
    pub const fn default_theme() -> Self {
        const { assert!(DEFAULT_THEME.len() <= THEME_NAME_MAX) };
        let name = DEFAULT_THEME.as_bytes();
        let mut bytes = [0; THEME_NAME_MAX];
        let mut at = 0;
        while at < name.len() {
            bytes[at] = name[at];
            at += 1;
        }
        Self {
            bytes,
            len: name.len() as u8,
        }
    }

    /// The name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Only ASCII bytes are ever stored, so this cannot fail; the fallback keeps it total.
        std::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or(DEFAULT_THEME)
    }
}

impl Default for ThemeName {
    fn default() -> Self {
        Self::default_theme()
    }
}

impl fmt::Debug for ThemeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ThemeName({:?})", self.as_str())
    }
}

impl fmt::Display for ThemeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
