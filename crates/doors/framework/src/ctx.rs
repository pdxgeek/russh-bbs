//! What a door is handed while it handles a key: the injected clock, the seeded RNG, a way to
//! ask for a save, and the sanitizing helpers for text a player types.
//!
//! The context has no store, no path and no network: a save is a request ([`Ctx::request_save`])
//! that the board carries out after the key, with a size cap and a compare-and-swap, and a world
//! change is a pure function the board applies to the stored world ([`Ctx::edit_world`]).

use russh_bbs_core::limits::Day;
use russh_bbs_core::sanitize::{Sanitized, SingleLine, Username, UsernameError, sanitize_username};
use russh_bbs_core::store::UnixSeconds;

use crate::rng::DoorRng;

/// The most characters of text one player may show another in one message.
pub const PLAYER_TEXT_MAX_CHARS: usize = 140;

/// How many messages a player may show others within [`PLAYER_TEXT_WINDOW_SECS`].
pub const PLAYER_TEXT_PER_WINDOW: u32 = 6;

/// The length of the rate-limit window, in seconds.
pub const PLAYER_TEXT_WINDOW_SECS: i64 = 60;

/// A change to the shared world: given the stored bytes (`None` when no world exists yet), the
/// new bytes, or `None` to change nothing. The board may call it more than once (it retries on
/// a conflict), so it must compute from its argument only.
pub type WorldEdit = Box<dyn FnMut(Option<&[u8]>) -> Option<Vec<u8>> + Send>;

/// Why player text was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TextRefused {
    /// Nothing is left after sanitizing.
    #[error("there is nothing to say")]
    Empty,
    /// Too many messages in the last minute.
    #[error("slow down: too many messages")]
    RateLimited,
}

/// Per-session limiter for text one player shows another.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TextBudget {
    window_start: i64,
    used: u32,
}

impl TextBudget {
    fn take(&mut self, now: UnixSeconds) -> bool {
        let now = now.as_raw();
        if now.saturating_sub(self.window_start) >= PLAYER_TEXT_WINDOW_SECS || self.used == 0 {
            self.window_start = now;
            self.used = 0;
        }
        if self.used >= PLAYER_TEXT_PER_WINDOW {
            return false;
        }
        self.used += 1;
        true
    }
}

/// What a key asked the board to do afterwards.
#[derive(Default)]
pub struct Effects {
    /// The save to write, if the door asked for one (the last request wins).
    pub(crate) save: Option<Vec<u8>>,
    /// World changes, in order.
    pub(crate) world: Vec<WorldEdit>,
}

impl Effects {
    /// Whether there is anything to write.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.save.is_none() && self.world.is_empty()
    }

    /// The save the door asked for, if any (tests and replays read it here).
    #[must_use]
    pub fn save(&self) -> Option<&[u8]> {
        self.save.as_deref()
    }
}

impl std::fmt::Debug for Effects {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Effects")
            .field("save_bytes", &self.save.as_ref().map(Vec::len))
            .field("world_edits", &self.world.len())
            .finish()
    }
}

/// The door's view of the world while it handles one key.
pub struct Ctx<'a> {
    pub(crate) now: UnixSeconds,
    pub(crate) season: u32,
    pub(crate) rng: &'a mut DoorRng,
    pub(crate) effects: &'a mut Effects,
    pub(crate) text: &'a mut TextBudget,
}

impl Ctx<'_> {
    /// The board's clock (injected: a test or a replay sets it).
    #[must_use]
    pub const fn now(&self) -> UnixSeconds {
        self.now
    }

    /// The current UTC day. A door that has days compares this with the day in its save.
    #[must_use]
    pub const fn today(&self) -> Day {
        Day::of(self.now)
    }

    /// The door's season number (1 until staff end one).
    #[must_use]
    pub const fn season(&self) -> u32 {
        self.season
    }

    /// The session's seeded random numbers.
    pub fn rng(&mut self) -> &mut DoorRng {
        self.rng
    }

    /// Ask the board to store `bytes` as this player's save once the key is handled. The board
    /// refuses a save over the size cap or one that another session overwrote meanwhile (and
    /// then tells the player); the door cannot see or work around either. Calling it twice in
    /// one key keeps the last.
    pub fn request_save(&mut self, bytes: Vec<u8>) {
        self.effects.save = Some(bytes);
    }

    /// Ask the board to change the shared world with `edit` (see [`WorldEdit`]).
    pub fn edit_world(
        &mut self,
        edit: impl FnMut(Option<&[u8]>) -> Option<Vec<u8>> + Send + 'static,
    ) {
        self.effects.world.push(Box::new(edit));
    }

    /// A name a player typed (a character, a ship, a clan): the board's username rules, which
    /// include the reserved-name rule, so nobody can call themselves `sysop` or a security
    /// marker. Store the returned name, not the raw text.
    pub fn player_name(&self, raw: &str) -> Result<Username, UsernameError> {
        sanitize_username(raw)
    }

    /// A line a player typed that other players will see: sanitized, one line, at most
    /// [`PLAYER_TEXT_MAX_CHARS`] characters, and at most [`PLAYER_TEXT_PER_WINDOW`] per minute
    /// per session. Where a game can offer templates instead, it should.
    pub fn player_text(&mut self, raw: &str) -> Result<Sanitized, TextRefused> {
        let clean = SingleLine::new(PLAYER_TEXT_MAX_CHARS).sanitize(raw);
        if clean.as_str().trim().is_empty() {
            return Err(TextRefused::Empty);
        }
        if !self.text.take(self.now) {
            return Err(TextRefused::RateLimited);
        }
        Ok(clean)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;

    fn with_ctx<R>(now: i64, text: &mut TextBudget, f: impl FnOnce(&mut Ctx<'_>) -> R) -> R {
        let mut rng = DoorRng::from_seed(1);
        let mut effects = Effects::default();
        let mut ctx = Ctx {
            now: UnixSeconds::from_raw(now),
            season: 1,
            rng: &mut rng,
            effects: &mut effects,
            text,
        };
        f(&mut ctx)
    }

    #[test]
    fn player_names_follow_the_reserved_name_rule() {
        let mut budget = TextBudget::default();
        with_ctx(0, &mut budget, |ctx| {
            assert!(ctx.player_name("Brindle").is_ok());
            assert_eq!(
                ctx.player_name("sysop").err(),
                Some(UsernameError::Reserved)
            );
            assert!(ctx.player_name("two words").is_err());
            assert!(ctx.player_name("\u{1b}[31m").is_err());
        });
    }

    #[test]
    fn player_text_is_sanitized_capped_and_rate_limited() {
        let mut budget = TextBudget::default();
        with_ctx(1_000, &mut budget, |ctx| {
            let got = ctx.player_text("hi \u{1b}[31mthere\u{7}\nnext").unwrap();
            assert!(!got.as_str().contains('\u{1b}') && !got.as_str().contains('\n'));
            assert_eq!(ctx.player_text("   ").err(), Some(TextRefused::Empty));
            let long = "x".repeat(1_000);
            assert_eq!(
                ctx.player_text(&long).unwrap().as_str().chars().count(),
                PLAYER_TEXT_MAX_CHARS
            );
            for _ in 0..(PLAYER_TEXT_PER_WINDOW - 2) {
                ctx.player_text("ok").unwrap();
            }
            assert_eq!(ctx.player_text("ok").err(), Some(TextRefused::RateLimited));
        });
        // A new window opens a minute later.
        with_ctx(1_000 + PLAYER_TEXT_WINDOW_SECS, &mut budget, |ctx| {
            assert!(ctx.player_text("again").is_ok());
        });
    }

    #[test]
    fn a_save_request_keeps_the_last_and_the_clock_is_the_injected_one() {
        let mut budget = TextBudget::default();
        let mut rng = DoorRng::from_seed(1);
        let mut effects = Effects::default();
        let mut ctx = Ctx {
            now: UnixSeconds::from_raw(86_400 * 3 + 5),
            season: 4,
            rng: &mut rng,
            effects: &mut effects,
            text: &mut budget,
        };
        assert_eq!(ctx.today(), Day::from_raw(3));
        assert_eq!(ctx.season(), 4);
        ctx.request_save(vec![1]);
        ctx.request_save(vec![2]);
        assert_eq!(effects.save(), Some(&[2][..]));
        assert!(!effects.is_empty());
    }
}
