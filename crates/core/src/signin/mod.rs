//! Which way a connection goes, decided by the name the person gave (D94 (10), D96).
//!
//! The same rule serves every transport: over ssh the name is the SSH user name, over telnet it
//! is the `login:` answer. `new` is the waiting room (join, or add this key to an account you
//! already have), `claim` is the founder claim (ssh only, because the setup token makes a
//! sysop), and any other name is a sign-in as that handle. The transports differ only in what
//! they add around it: ssh may accept a key bound to the handle in place of the password, telnet
//! draws its `UNENCRYPTED` marker and drops staff roles. The password itself is always checked by
//! [`crate::passwords::authenticate`].
//!
//! The route is decided from the name alone and never from the board, so nothing about it can
//! tell a stranger whether a handle exists.

/// The name that opens the waiting room. Reserved as a handle (`sanitize`).
pub const JOIN_NAME: &str = "new";
/// The name that opens the founder claim on ssh. Reserved as a handle.
pub const CLAIM_NAME: &str = "claim";

/// Where a connection with a given name goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The waiting room: join, request, apply, or link this key to an existing account.
    Join,
    /// The founder claim prompt (ssh; over telnet it is just an unknown name).
    Claim,
    /// A sign-in as the named handle, by password, or on ssh by a key bound to it.
    SignIn,
}

/// The route for `name`. Case does not matter and surrounding space is ignored, like a login.
#[must_use]
pub fn route(name: &str) -> Route {
    let name = name.trim();
    if name.eq_ignore_ascii_case(JOIN_NAME) {
        Route::Join
    } else if name.eq_ignore_ascii_case(CLAIM_NAME) {
        Route::Claim
    } else {
        Route::SignIn
    }
}

/// Whether a key bound to the account named `handle` may stand in for its password when the
/// connection asked to sign in as `name`: the names must be the same handle. Compared the way
/// logins are (ASCII case folded); a name that is not a handle never matches.
#[must_use]
pub fn names_the_handle(name: &str, handle: &str) -> bool {
    crate::passwords::login_handle(name).is_some_and(|name| name == handle.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::{Route, names_the_handle, route};

    #[test]
    fn the_name_alone_picks_the_route() {
        assert_eq!(route("new"), Route::Join);
        assert_eq!(route(" NEW "), Route::Join);
        assert_eq!(route("claim"), Route::Claim);
        assert_eq!(route("Claim"), Route::Claim);
        for other in ["alice", "", "news", "claim-me", "anyone"] {
            assert_eq!(route(other), Route::SignIn, "{other:?}");
        }
    }

    #[test]
    fn a_key_stands_in_only_for_the_handle_it_is_bound_to() {
        assert!(names_the_handle("Alice", "alice"));
        assert!(names_the_handle("alice", "Alice"));
        assert!(!names_the_handle("bob", "alice"));
        assert!(!names_the_handle("", "alice"));
        assert!(!names_the_handle("new", "alice"));
    }
}
