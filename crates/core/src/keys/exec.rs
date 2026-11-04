//! Running one keys request for a signed-in session: the request/answer pair the screens and the
//! front end exchange, and the function that carries a request out.

use tracing::warn;

use super::code::LoginCode;
use super::service::{
    KeyError, add_key, issue_recovery_code, list_keys, make_link_code, random_bytes, remove_key,
};
use super::store::{KeyRecord, KeyStore};
use crate::access::{Fingerprint, Snapshot};
use crate::store::UnixSeconds;

/// What a keys screen needs done.
#[derive(Debug)]
pub enum Request {
    /// The account's keys.
    List,
    /// Add the pasted public key.
    Add(String),
    /// Remove a key.
    Remove(Fingerprint),
    /// Make a device-link code.
    LinkCode,
    /// Staff: make a recovery code for the member with this handle.
    Recovery(String),
}

/// A code the screen shows once.
pub struct ShownCode {
    /// The code.
    pub code: LoginCode,
    /// When it stops working.
    pub expires_at: UnixSeconds,
}

impl std::fmt::Debug for ShownCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShownCode")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// Why nothing changed. The words are the screen's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The session does not hold the permission.
    Denied,
    /// The paste is not an Ed25519 public key.
    NotAKey,
    /// The key cannot be added (one message whoever holds it).
    CannotAdd,
    /// The account has no such key.
    NoSuchKey,
    /// It is the last way in.
    LastWayIn,
    /// Too many codes this hour.
    RateLimited,
    /// Staff cannot help this member.
    NotYours,
    /// The store failed, or took too long.
    Failed,
}

/// The answer to a [`Request`].
#[derive(Debug)]
pub enum Response {
    /// The account's keys, after a list or a change.
    Keys {
        /// The keys.
        keys: Vec<KeyRecord>,
        /// What just happened, if a change did it.
        changed: Option<Change>,
    },
    /// A code to show once.
    Code(ShownCode),
    /// It did not happen.
    Refused(Refusal),
}

/// What a change did, for the line under the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// A key was added.
    Added,
    /// A key was removed.
    Removed,
}

/// Carry out `request` with the session's current `access`. The permission is checked in core,
/// whatever the screen offered.
pub async fn execute<S: KeyStore>(store: &S, access: &Snapshot, request: Request) -> Response {
    let outcome = run(store, access, request).await;
    match outcome {
        Ok(response) => response,
        Err(error) => Response::Refused(refusal(&error)),
    }
}

async fn run<S: KeyStore>(
    store: &S,
    access: &Snapshot,
    request: Request,
) -> Result<Response, KeyError> {
    match request {
        Request::List => Ok(Response::Keys {
            keys: list_keys(store, access).await?,
            changed: None,
        }),
        Request::Add(text) => {
            add_key(store, access, &text).await?;
            Ok(Response::Keys {
                keys: list_keys(store, access).await?,
                changed: Some(Change::Added),
            })
        }
        Request::Remove(fingerprint) => {
            remove_key(store, access, &fingerprint).await?;
            Ok(Response::Keys {
                keys: list_keys(store, access).await?,
                changed: Some(Change::Removed),
            })
        }
        Request::LinkCode => {
            let issued =
                make_link_code(store, access, &random_bytes(super::code::CodeKind::Link)?).await?;
            Ok(Response::Code(ShownCode {
                code: issued.code,
                expires_at: issued.expires_at,
            }))
        }
        Request::Recovery(handle) => {
            let issued = issue_recovery_code(
                store,
                access,
                &handle,
                &random_bytes(super::code::CodeKind::Recovery)?,
            )
            .await?;
            Ok(Response::Code(ShownCode {
                code: issued.code,
                expires_at: issued.expires_at,
            }))
        }
    }
}

fn refusal(error: &KeyError) -> Refusal {
    match error {
        KeyError::Denied(_) => Refusal::Denied,
        KeyError::NotAKey(_) => Refusal::NotAKey,
        KeyError::CannotAdd => Refusal::CannotAdd,
        KeyError::NoSuchKey => Refusal::NoSuchKey,
        KeyError::LastWayIn => Refusal::LastWayIn,
        KeyError::RateLimited { .. } => Refusal::RateLimited,
        KeyError::NotYours => Refusal::NotYours,
        KeyError::Random | KeyError::Store(_) => {
            warn!(%error, "keys request failed");
            Refusal::Failed
        }
    }
}
