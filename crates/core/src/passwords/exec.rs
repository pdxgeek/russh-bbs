//! Running one password request for a signed-in session: the request/answer pair the screen and
//! the front end exchange, and the function that carries a request out.

use tracing::warn;

use super::offload::Offload;
use super::secret::{PolicyError, Secret};
use super::service::{LoginMethod, PasswordError, clear_password, has_password, set_password};
use super::store::PasswordStore;
use crate::access::Snapshot;

/// What the password screen needs done.
#[derive(Debug)]
pub struct Request {
    /// How the session signed in; decides whether the current password is required.
    pub method: LoginMethod,
    /// The action.
    pub kind: Kind,
}

/// The action of a [`Request`].
#[derive(Debug)]
pub enum Kind {
    /// Whether the account has a password.
    Status,
    /// Set or change it.
    Set {
        /// The current password (password sessions only).
        current: Option<Secret>,
        /// The new password.
        new: Secret,
    },
    /// Remove it.
    Clear {
        /// The current password (password sessions only).
        current: Option<Secret>,
    },
}

/// What a finished action did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Done {
    /// A password was stored.
    Set,
    /// The password was removed.
    Cleared,
}

/// Why nothing changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The session does not hold `account.set_password`.
    Denied,
    /// The current password was missing or wrong.
    WrongCurrent,
    /// The new password breaks the rules.
    Policy(PolicyError),
    /// The store or the hasher failed, or took too long.
    Failed,
}

/// The answer to a [`Request`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    /// Whether the account has a password.
    Status(bool),
    /// The action happened.
    Done(Done),
    /// It did not.
    Refused(Refusal),
}

/// Carry out `request` with the session's current `access`. The permission is checked here, in
/// core, whatever the screen offered.
pub async fn execute<S: PasswordStore, O: Offload>(
    store: &S,
    offload: &O,
    access: &Snapshot,
    request: Request,
) -> Response {
    let Request { method, kind } = request;
    let outcome = match kind {
        Kind::Status => {
            return match has_password(store, access).await {
                Ok(set) => Response::Status(set),
                Err(error) => Response::Refused(refusal(&error)),
            };
        }
        Kind::Set { current, new } => {
            set_password(store, offload, access, method, current.as_ref(), &new)
                .await
                .map(|()| Done::Set)
        }
        Kind::Clear { current } => clear_password(store, offload, access, method, current.as_ref())
            .await
            .map(|()| Done::Cleared),
    };
    match outcome {
        Ok(done) => Response::Done(done),
        Err(error) => Response::Refused(refusal(&error)),
    }
}

fn refusal(error: &PasswordError) -> Refusal {
    match error {
        PasswordError::Denied(_) => Refusal::Denied,
        PasswordError::Policy(policy) => Refusal::Policy(*policy),
        PasswordError::NeedsCurrent | PasswordError::WrongCurrent => Refusal::WrongCurrent,
        PasswordError::Unavailable(_) | PasswordError::Hash(_) | PasswordError::Store(_) => {
            // The error text names no secret; the cause of a store failure is logged here.
            warn!(%error, "password request failed");
            Refusal::Failed
        }
    }
}
