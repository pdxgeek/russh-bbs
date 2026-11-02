//! The founder claim: how the first person to present the one-time setup token becomes the
//! sysop (M1-D, decision D41).
//!
//! The pieces:
//!
//! * [`SetupToken`] / [`TokenHash`]: a 160-bit random token and its SHA-256 hash. Only the
//!   hash is kept, and only in memory, so nothing secret reaches the database or a log.
//! * [`FounderGate`]: the in-memory "is the claim open, and what hash opens it" state, with a
//!   constant-time check that behaves the same open or closed.
//! * [`claim_founder`]: the one function that turns a token and a key into a sysop, using the
//!   store's atomic [`FounderStore::claim_founder`].
//! * [`ClaimForm`]: the screen state machine for typing the token.
//!
//! **No oracle.** A wrong token, a token after the claim, a store error and a lost race all
//! return [`ClaimOutcome::Rejected`], and the screen shows the same words for all of them.

mod claim;
mod form;
mod gate;
mod token;

pub use claim::{ClaimOutcome, FOUNDER_USERNAME, FounderStore, claim_founder};
pub use form::{ClaimEffect, ClaimForm, ClaimInput, MAX_ATTEMPTS_PER_SESSION, strings};
pub use gate::FounderGate;
pub use token::{SetupToken, TOKEN_BYTES, TOKEN_TEXT_MAX_CHARS, TokenHash};

#[cfg(test)]
mod tests;
