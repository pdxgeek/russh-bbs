//! Keys, device linking and staff recovery (FIX-M3-11, D94 (8), D97).
//!
//! An account is a handle and a password; SSH keys are the optional extra (D94). This module
//! holds the rules for the extra:
//!
//! * [`list_keys`], [`add_key`], [`remove_key`]: My keys. A pasted key must be an Ed25519 public
//!   key ([`parse_ed25519`]); a key that belongs to an account already is refused with one
//!   generic message; the last way in (no password and no other key) is never removed.
//! * [`make_link_code`]: a signed-in member makes a short one-time code (10 minutes, five an
//!   hour) that a new machine enters in the waiting room ([`redeem_link_code`]) to bind its key.
//! * [`issue_recovery_code`]: staff with `keys.recover` make a one-time code (24 hours) for a
//!   member strictly below their rank (D69); the member enters it in the waiting room with a
//!   new password ([`redeem_recovery_code`]). Staff are told to confirm identity outside the
//!   board first.
//! * Every key change and every redemption is audited with ids only and shown to the member at
//!   the next sign-in ([`notices_for_sign_in`]).
//!
//! A code is stored only as a hash, shown once, used once, and a refusal reveals nothing.

mod code;
mod exec;
mod notices;
mod pubkey;
mod screen;
mod service;
mod store;
pub mod strings;

pub use code::{
    CODE_TEXT_MAX_CHARS, CodeKind, LINK_CHARS, LINK_TTL_SECS, LoginCode, LoginCodeHash,
    RECOVERY_CHARS, RECOVERY_TTL_SECS,
};
pub use exec::{Change, Refusal, Request, Response, ShownCode, execute};
pub use notices::notice_lines;
pub use pubkey::{PASTE_MAX_CHARS, PubKeyError, parse_ed25519};
pub use screen::{KeysKey, KeysScreen, RecoveryScreen, Step};
pub use service::{
    KeyError, LINK_CODES_PER_HOUR, RECOVERY_CODES_PER_HOUR, Refused, add_key, issue_recovery_code,
    list_keys, make_link_code, notices_for_sign_in, random_bytes, redeem_link_code,
    redeem_recovery_code, remove_key,
};
pub use store::{
    IssueError, KeyNotice, KeyRecord, KeyStore, NewCode, NoticeKind, RedeemError, RedeemRefusal,
    RemoveOutcome,
};

#[cfg(test)]
mod tests;
