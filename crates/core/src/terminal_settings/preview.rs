//! The sysop's theme preview (UI-3): what is being previewed and the keys that change it.
//!
//! The screen itself is drawn by the server (it owns the styles, layouts and art); this is the
//! small piece of state the member flow keeps, so the keys, the permission re-check and the
//! return to Sysop tools work like every other page. Nothing here is stored.

use super::layout::{LayoutId, ThemeName};

/// The terminal sizes a preview can be drawn at, smallest first.
pub const SIZES: [(u16, u16); 3] = [(80, 24), (100, 30), (132, 43)];

/// Which screen is previewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSample {
    /// The main menu with its header art.
    Menu,
    /// The login screen with its banner.
    Login,
}

/// A key on the preview screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKey {
    /// Next style.
    Style,
    /// Next layout.
    Layout,
    /// Next size.
    Size,
    /// Unicode or ASCII icon tier.
    Tier,
    /// Colour or mono.
    Colour,
    /// Menu or login sample.
    Sample,
    /// Art or large letters only.
    Art,
    /// Leave.
    Back,
}

/// What the preview shows now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemePreview {
    style: ThemeName,
    layout: LayoutId,
    size: usize,
    ascii: bool,
    mono: bool,
    sample: PreviewSample,
    art: bool,
}

impl ThemePreview {
    /// Start on the style and layout the sysop is using, at the smallest size.
    #[must_use]
    pub const fn open(style: ThemeName, layout: LayoutId) -> Self {
        Self {
            style,
            layout,
            size: 0,
            ascii: false,
            mono: false,
            sample: PreviewSample::Menu,
            art: true,
        }
    }

    /// The style being previewed.
    #[must_use]
    pub const fn style(&self) -> ThemeName {
        self.style
    }

    /// The layout being previewed.
    #[must_use]
    pub const fn layout(&self) -> LayoutId {
        self.layout
    }

    /// The size being previewed, columns by rows.
    #[must_use]
    pub const fn size(&self) -> (u16, u16) {
        SIZES[self.size]
    }

    /// Whether the ASCII tier is previewed.
    #[must_use]
    pub const fn ascii(&self) -> bool {
        self.ascii
    }

    /// Whether colour is off.
    #[must_use]
    pub const fn mono(&self) -> bool {
        self.mono
    }

    /// The sample screen.
    #[must_use]
    pub const fn sample(&self) -> PreviewSample {
        self.sample
    }

    /// Whether installed art is drawn (off shows the large-letter fallback).
    #[must_use]
    pub const fn art(&self) -> bool {
        self.art
    }

    /// Apply `key`. `themes` and `layouts` are what may be cycled through (empty: every layout,
    /// and the current style only). Returns `true` to leave the screen.
    pub fn press(&mut self, key: PreviewKey, themes: &[ThemeName], layouts: &[LayoutId]) -> bool {
        match key {
            PreviewKey::Style => self.style = next(themes, self.style),
            PreviewKey::Layout => {
                self.layout = if layouts.is_empty() {
                    self.layout.next()
                } else {
                    next(layouts, self.layout)
                };
            }
            PreviewKey::Size => self.size = (self.size + 1) % SIZES.len(),
            PreviewKey::Tier => self.ascii = !self.ascii,
            PreviewKey::Colour => self.mono = !self.mono,
            PreviewKey::Sample => {
                self.sample = match self.sample {
                    PreviewSample::Menu => PreviewSample::Login,
                    PreviewSample::Login => PreviewSample::Menu,
                };
            }
            PreviewKey::Art => self.art = !self.art,
            PreviewKey::Back => return true,
        }
        false
    }
}

/// The entry after `current` in `list` (the first if `current` is not in it or the list is
/// empty, in which case `current` stays).
fn next<T: Copy + PartialEq>(list: &[T], current: T) -> T {
    match list.iter().position(|item| *item == current) {
        Some(at) => list[(at + 1) % list.len()],
        None => list.first().copied().unwrap_or(current),
    }
}
