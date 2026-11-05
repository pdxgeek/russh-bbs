//! Per-user terminal settings, the glyph test, and the login / logoff screens (M1-E, D42).
//!
//! The rules live here and the SSH front end only draws them (AGENTS.md: business logic in
//! core). The pieces:
//!
//! * [`TerminalSettings`]: size overrides, color, pause and icon tier, with the rules that turn
//!   them into what is drawn. **The safe default is ASCII icons and no color**: until a person
//!   has answered the glyph test, and whenever they skip it or say "no", that is what they get.
//! * [`Pager`]: pages long text and honors the pause setting (D65).
//! * [`GlyphTest`]: asks "do these look like icons?" per tier, using only registry glyphs.
//! * [`MemberFlow`]: the signed-in session as a state machine (login screen, glyph test on
//!   first login, a placeholder home, the settings screen, logoff) producing a [`View`].
//! * [`MemberSessions`]: what the SSH front end needs from the store, kept as a core trait so
//!   it can be tested against any backend.
//! * [`strings`]: every fixed string, out of rendering code so a theme can replace them later.
//!
//! **Text.** The only user-derived text shown is the account handle, and it goes through the
//! shared sanitizer before it is placed in a view.
//!
//! [`View`]: crate::waiting_room::View

mod flow;
mod gallery;
mod glyph;
mod layout;
mod packs;
mod pager;
mod preview;
mod sessions;
mod settings;
pub mod strings;
mod time;

pub use flow::{MemberEffect, MemberFlow, MemberInput};
pub use gallery::{Gallery, GalleryKey, GalleryOutcome};
pub use glyph::{GlyphAnswer, GlyphProgress, GlyphStep, GlyphTest};
pub use layout::{
    BorderChoice, DEFAULT_THEME, DepthCap, LayoutId, THEME_NAME_MAX, ThemeName, ThemeNameError,
    borders_from_stored, borders_to_stored, depth_cap_from_stored, depth_cap_to_stored,
    layout_from_stored, layout_to_stored,
};
pub use packs::{FileRow, PackKey, PackListing, PackRequest, PackScreen, PackStep, StyleRow};
pub use pager::{Pager, PagerEffect, PagerKey, PagerPrompt, PagerView};
pub use preview::{PreviewKey, PreviewSample, SIZES as PREVIEW_SIZES, ThemePreview};
pub use sessions::{MemberSessions, MemberStart, SaveError, TerminalSettingsStore};
pub use settings::{
    MAX_HEIGHT, MAX_WIDTH, MIN_HEIGHT, MIN_WIDTH, SAFE_TIER, SettingsError, TerminalSettings,
    tier_from_stored, tier_name, tier_to_stored,
};
pub use time::format_utc;

#[cfg(test)]
mod tests;
