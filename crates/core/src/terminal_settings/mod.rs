//! Per-user terminal settings, the glyph test, and the login / logoff screens (M1-E, D42).
//!
//! The rules live here and the SSH front end only draws them (AGENTS.md: business logic in
//! core). The pieces:
//!
//! * [`TerminalSettings`]: size overrides, color, pause and icon tier, with the rules that turn
//!   them into what is drawn. **The safe default is ASCII icons and no color**: until a person
//!   has answered the glyph test, and whenever they skip it or say "no", that is what they get.
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
mod glyph;
mod sessions;
mod settings;
pub mod strings;
mod time;

pub use flow::{MemberEffect, MemberFlow, MemberInput};
pub use glyph::{GlyphAnswer, GlyphProgress, GlyphStep, GlyphTest};
pub use sessions::{MemberSessions, MemberStart, TerminalSettingsStore};
pub use settings::{
    MAX_HEIGHT, MAX_WIDTH, MIN_HEIGHT, MIN_WIDTH, SAFE_TIER, SettingsError, TerminalSettings,
    tier_from_stored, tier_name, tier_to_stored,
};
pub use time::format_utc;

#[cfg(test)]
mod tests;
