//! The member's style gallery (UI-7): every style the board allows, previewed live, and a key
//! to pick one.
//!
//! The server draws it (it owns the styles, layouts and art); this is the state the member flow
//! keeps so the keys, the board's limits and the save work like every other settings page.
//! **The board's limits are the list.** The choices are exactly the styles the sysop allows
//! (`board.styles`); one entry is a lock, which the gallery shows and does not let the member
//! change. Nothing here reads or writes the store: picking returns the style, and the flow saves
//! it through the same path as the settings key.

use super::layout::ThemeName;
use super::preview::PreviewSample;

/// A key on the gallery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryKey {
    /// The previous style.
    Prev,
    /// The next style.
    Next,
    /// Use the style shown.
    Pick,
    /// Menu or login sample.
    Sample,
    /// Leave.
    Back,
}

/// What a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GalleryOutcome {
    /// Nothing for the flow to do (the screen redraws).
    Stay,
    /// The member picked this style.
    Pick(ThemeName),
    /// Leave the gallery.
    Leave,
}

/// The gallery's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gallery {
    choices: Vec<ThemeName>,
    at: usize,
    current: ThemeName,
    sample: PreviewSample,
}

impl Gallery {
    /// Open on `current`. `choices` is what the board allows, in the order the member cycles
    /// through; an empty list is treated as `current` alone (nothing to choose between).
    #[must_use]
    pub fn open(choices: &[ThemeName], current: ThemeName) -> Self {
        let mut list = choices.to_vec();
        if list.is_empty() {
            list.push(current);
        }
        let at = list.iter().position(|name| *name == current).unwrap_or(0);
        Self {
            choices: list,
            at,
            current,
            sample: PreviewSample::Menu,
        }
    }

    /// The styles on offer.
    #[must_use]
    pub fn choices(&self) -> &[ThemeName] {
        &self.choices
    }

    /// The style being previewed.
    #[must_use]
    pub fn selected(&self) -> ThemeName {
        self.choices[self.at]
    }

    /// The position of the style being previewed.
    #[must_use]
    pub const fn index(&self) -> usize {
        self.at
    }

    /// The style the member uses now.
    #[must_use]
    pub const fn current(&self) -> ThemeName {
        self.current
    }

    /// Whether the board offers no choice (the sysop locked the style).
    #[must_use]
    pub fn locked(&self) -> bool {
        self.choices.len() < 2
    }

    /// The sample screen.
    #[must_use]
    pub const fn sample(&self) -> PreviewSample {
        self.sample
    }

    /// Record that the member now uses `style` (after a save).
    pub fn set_current(&mut self, style: ThemeName) {
        self.current = style;
    }

    /// Apply `key`.
    pub fn press(&mut self, key: GalleryKey) -> GalleryOutcome {
        let count = self.choices.len();
        match key {
            GalleryKey::Next => self.at = (self.at + 1) % count,
            GalleryKey::Prev => self.at = (self.at + count - 1) % count,
            GalleryKey::Sample => {
                self.sample = match self.sample {
                    PreviewSample::Menu => PreviewSample::Login,
                    PreviewSample::Login => PreviewSample::Menu,
                };
            }
            GalleryKey::Pick => {
                let style = self.selected();
                // A lock, or the style already in use: nothing to save.
                if !self.locked() && style != self.current {
                    return GalleryOutcome::Pick(style);
                }
            }
            GalleryKey::Back => return GalleryOutcome::Leave,
        }
        GalleryOutcome::Stay
    }
}
