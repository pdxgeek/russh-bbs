//! The settings themselves and the rules that turn them into what is drawn.

use crate::icons::Tier;

/// Narrowest width override accepted, in cells.
pub const MIN_WIDTH: u16 = 20;
/// Widest width override accepted. Matches the server's PTY clamp (D19).
pub const MAX_WIDTH: u16 = 500;
/// Shortest height override accepted, in rows.
pub const MIN_HEIGHT: u16 = 6;
/// Tallest height override accepted. Matches the server's PTY clamp (D19).
pub const MAX_HEIGHT: u16 = 200;

/// The tier used until a person has chosen one, and whenever they skip or reject the glyph
/// test. Printable ASCII renders on every terminal and every screen reader (`docs/ui.md`,
/// "Accessibility").
pub const SAFE_TIER: Tier = Tier::Ascii;

/// A setting outside its allowed range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    /// Width override outside [`MIN_WIDTH`]..=[`MAX_WIDTH`].
    #[error("width override is out of range")]
    Width,
    /// Height override outside [`MIN_HEIGHT`]..=[`MAX_HEIGHT`].
    #[error("height override is out of range")]
    Height,
}

/// One account's terminal settings. Fields are private so every instance is in range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSettings {
    width: Option<u16>,
    height: Option<u16>,
    color: bool,
    pause: bool,
    tier: Option<Tier>,
}

impl Default for TerminalSettings {
    /// Follow the PTY size, color on once a tier is chosen, pause on, no tier chosen yet.
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            color: true,
            pause: true,
            tier: None,
        }
    }
}

impl TerminalSettings {
    /// Build settings from stored values, checking the ranges.
    pub fn new(
        width: Option<u16>,
        height: Option<u16>,
        color: bool,
        pause: bool,
        tier: Option<Tier>,
    ) -> Result<Self, SettingsError> {
        if width.is_some_and(|w| !(MIN_WIDTH..=MAX_WIDTH).contains(&w)) {
            return Err(SettingsError::Width);
        }
        if height.is_some_and(|h| !(MIN_HEIGHT..=MAX_HEIGHT).contains(&h)) {
            return Err(SettingsError::Height);
        }
        Ok(Self {
            width,
            height,
            color,
            pause,
            tier,
        })
    }

    /// The width override, or `None` to follow the PTY.
    #[must_use]
    pub const fn width(&self) -> Option<u16> {
        self.width
    }

    /// The height override, or `None` to follow the PTY.
    #[must_use]
    pub const fn height(&self) -> Option<u16> {
        self.height
    }

    /// The stored color preference. Use [`Self::use_color`] to decide what to draw.
    #[must_use]
    pub const fn color(&self) -> bool {
        self.color
    }

    /// Whether long text is shown a page at a time with a "more" prompt. [`Pager`] honors it
    /// (D65); no screen has text long enough to page yet, so today it changes no screen.
    ///
    /// [`Pager`]: super::Pager
    #[must_use]
    pub const fn pause(&self) -> bool {
        self.pause
    }

    /// The tier the person chose, or `None` if they have not answered the glyph test.
    #[must_use]
    pub const fn chosen_tier(&self) -> Option<Tier> {
        self.tier
    }

    /// Whether the glyph test must be shown: no tier has been stored.
    #[must_use]
    pub const fn needs_glyph_test(&self) -> bool {
        self.tier.is_none()
    }

    /// The tier to draw: the chosen one, else [`SAFE_TIER`].
    #[must_use]
    pub const fn icon_tier(&self) -> Tier {
        match self.tier {
            Some(tier) => tier,
            None => SAFE_TIER,
        }
    }

    /// Whether to draw color. Off until a tier has been chosen, so the unanswered-test default
    /// is the plain accessible layout.
    #[must_use]
    pub const fn use_color(&self) -> bool {
        self.color && self.tier.is_some()
    }

    /// The size to draw at: each override if set, else the PTY's value, and never larger than
    /// the PTY. An override that does not fit the real terminal would be drawn off screen and
    /// garble the layout (FIX-M1-L7, D64), so it is clamped here and the settings screen says
    /// so ([`Self::overrides_clamped`]). The stored value is left alone: it applies again in a
    /// bigger window.
    #[must_use]
    pub fn size(&self, pty_cols: u16, pty_rows: u16) -> (u16, u16) {
        (
            self.width.map_or(pty_cols, |w| w.min(pty_cols)),
            self.height.map_or(pty_rows, |h| h.min(pty_rows)),
        )
    }

    /// Whether the stored width and height overrides, in that order, are larger than the
    /// terminal and so cut down by [`Self::size`].
    #[must_use]
    pub fn overrides_clamped(&self, pty_cols: u16, pty_rows: u16) -> (bool, bool) {
        (
            self.width.is_some_and(|w| w > pty_cols),
            self.height.is_some_and(|h| h > pty_rows),
        )
    }

    /// Set or clear the width override.
    pub fn with_width(mut self, width: Option<u16>) -> Result<Self, SettingsError> {
        self = Self::new(width, self.height, self.color, self.pause, self.tier)?;
        Ok(self)
    }

    /// Set or clear the height override.
    pub fn with_height(mut self, height: Option<u16>) -> Result<Self, SettingsError> {
        self = Self::new(self.width, height, self.color, self.pause, self.tier)?;
        Ok(self)
    }

    /// Turn color on or off.
    #[must_use]
    pub const fn with_color(mut self, color: bool) -> Self {
        self.color = color;
        self
    }

    /// Turn pause on or off.
    #[must_use]
    pub const fn with_pause(mut self, pause: bool) -> Self {
        self.pause = pause;
        self
    }

    /// Choose a tier (a "yes" in the glyph test). Color is left as it was.
    #[must_use]
    pub const fn with_tier(mut self, tier: Tier) -> Self {
        self.tier = Some(tier);
        self
    }

    /// The safe fallback for a skipped or rejected glyph test: [`SAFE_TIER`] and no color.
    #[must_use]
    pub const fn safe_fallback(mut self) -> Self {
        self.tier = Some(SAFE_TIER);
        self.color = false;
        self
    }
}

/// The stable text stored for a tier. Never rename a value; add a migration instead.
#[must_use]
pub const fn tier_to_stored(tier: Tier) -> &'static str {
    match tier {
        Tier::NerdFont => "nerd_font",
        Tier::Unicode => "unicode",
        Tier::Ascii => "ascii",
    }
}

/// Parse the stored tier text.
#[must_use]
pub fn tier_from_stored(text: &str) -> Option<Tier> {
    match text {
        "nerd_font" => Some(Tier::NerdFont),
        "unicode" => Some(Tier::Unicode),
        "ascii" => Some(Tier::Ascii),
        _ => None,
    }
}

/// The display name of a tier, for screens.
#[must_use]
pub const fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::NerdFont => super::strings::TIER_NERD,
        Tier::Unicode => super::strings::TIER_UNICODE,
        Tier::Ascii => super::strings::TIER_ASCII,
    }
}
