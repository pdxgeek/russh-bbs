//! Member passwords and password login (M3-D, D28, D81).
//!
//! A password is optional, never required for key users, and used only where the sysop has
//! opted in: SSH password login (`ssh.allow_password_login`, off by default) and later telnet
//! (M3-H, D27). The rules are here; the SSH front end only receives the typed text and asks.
//!
//! * [`hash`]: argon2id in the PHC string format. The parameters are in the stored string, so
//!   they can be raised later and a login upgrades older hashes.
//! * [`authenticate`]: the login check. An unknown name, a name with no password, an account
//!   that is not active and a wrong password all cost the same one verification and give the
//!   same [`Login::Denied`], so a failed attempt reveals nothing about members.
//! * [`set_password`] / [`clear_password`]: need `account.set_password`, are audited without
//!   any secret, and a password session must give the current password again. A key session is
//!   the only one that can set a first password; the waiting room has no way to reach them.
//! * [`PasswordScreen`]: the settings sub-screen over them; typed text shows as `*`.
//!
//! Plain-text passwords exist only in [`Secret`]s (redacted `Debug`) and in the hasher call.
//! They are never logged, audited, stored or put in an error.

mod exec;
pub mod hash;
mod offload;
mod screen;
mod secret;
mod service;
mod store;
pub mod strings;

pub use exec::{Done, Kind, Refusal, Request, Response, execute};
pub use offload::{Inline, Offload, Unavailable};
pub use screen::{PasswordKey, PasswordScreen, Step};
pub use secret::{MAX_ATTEMPT_BYTES, MAX_CHARS, MIN_CHARS, PolicyError, Secret, check_new};
pub use service::{
    Login, LoginMethod, PasswordError, authenticate, clear_password, has_password, hash_new,
    login_handle, set_password,
};
pub use store::{LoginRecord, PasswordStore};

#[cfg(test)]
mod tests;
