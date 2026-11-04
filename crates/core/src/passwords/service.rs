//! Password login and the set/clear actions (D28, D81). Front ends call these; none of the
//! rules live in a front end.

use tracing::warn;
use zeroize::Zeroizing;

use super::hash::{self, DUMMY_HASH, needs_rehash};
use super::offload::{Offload, Unavailable};
use super::secret::{MAX_ATTEMPT_BYTES, PolicyError, Secret, check_new};
use super::store::PasswordStore;
use crate::access::{Denied, Fingerprint, Permission, Snapshot};
use crate::audit::events;
use crate::store::{Account, StoreError};

/// How the session in question signed in. A password session must prove the current password
/// again before it sets or clears one (D81); a key session already proved possession of its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginMethod {
    /// Public key.
    Key,
    /// Password (opt-in, D28).
    Password,
}

impl LoginMethod {
    /// The note an audit row carries.
    const fn note(self) -> &'static str {
        match self {
            Self::Key => "key session",
            Self::Password => "password session",
        }
    }
}

/// Longest handle a login looks up.
const MAX_HANDLE_CHARS: usize = 64;

/// The handle as the store compares it: ASCII-lowercased. `None` for text that cannot be a
/// handle (empty, too long, a control character), which then looks up nothing.
#[must_use]
pub fn login_handle(user: &str) -> Option<String> {
    let user = user.trim();
    let valid = !user.is_empty()
        && user.chars().count() <= MAX_HANDLE_CHARS
        && !user.chars().any(char::is_control);
    valid.then(|| user.to_ascii_lowercase())
}

/// The result of a password login.
#[derive(Debug, Clone)]
pub enum Login {
    /// The password matched an active account.
    Accepted(Account),
    /// Everything else. One answer for an unknown name, a name with no password, a suspended or
    /// not-yet-active account and a wrong password.
    Denied,
}

/// Check a password login.
///
/// Every path does one argon2id verification of the current cost: against the account's hash
/// when it is active and has one, against a dummy hash otherwise, so the answer and the time
/// do not tell an unknown name from a wrong password (the no-leak rule). On a match with an
/// older hash, the hash is upgraded to the current parameters.
///
/// # Errors
/// [`StoreError`] if the lookup fails (the caller refuses the login), [`Unavailable`] mapped to
/// [`Login::Denied`].
pub async fn authenticate<S: PasswordStore, O: Offload>(
    store: &S,
    offload: &O,
    user: &str,
    password: &str,
) -> Result<Login, StoreError> {
    let record = match login_handle(user) {
        Some(handle) => store.login_record(&handle).await?,
        None => None,
    };
    let (target, real) = match &record {
        Some(found) if found.account.state.is_member_access() => match &found.hash {
            Some(hash) => (hash.clone(), true),
            None => (DUMMY_HASH.to_owned(), false),
        },
        _ => (DUMMY_HASH.to_owned(), false),
    };
    let attempt = attempt_copy(password);
    let checked = {
        let (target, attempt) = (target.clone(), attempt.clone());
        offload.run(move || hash::verify(&target, &attempt)).await
    };
    let matched = checked.unwrap_or(false);
    let Some(found) = record.filter(|_| matched && real) else {
        return Ok(Login::Denied);
    };
    if needs_rehash(&target) {
        upgrade(store, offload, &found.account, &target, &attempt).await;
    }
    Ok(Login::Accepted(found.account))
}

/// "I already have an account" (D94 (8)): check `user` and `password` exactly as a login does
/// ([`authenticate`]: same dummy-hash path, same single refusal) and, if they match an active
/// member, bind `fingerprint` to that account, audited. Every failure (unknown handle, wrong
/// password, no password, suspended, a key that already has an account, a store error) is
/// [`Login::Denied`]. The caller applies the per-handle and per-network password limits first.
///
/// # Errors
/// [`StoreError`] only if the first lookup fails; the caller refuses the attempt.
pub async fn link_key_by_password<S: PasswordStore, O: Offload>(
    store: &S,
    offload: &O,
    user: &str,
    password: &str,
    fingerprint: &Fingerprint,
) -> Result<Login, StoreError> {
    let Login::Accepted(account) = authenticate(store, offload, user, password).await? else {
        return Ok(Login::Denied);
    };
    match store
        .link_key(
            account.id,
            fingerprint,
            &events::key_linked(account.id, fingerprint),
        )
        .await
    {
        Ok(()) => {
            tracing::info!(account = %account.id, fingerprint = %fingerprint, "key linked by password");
            Ok(Login::Accepted(account))
        }
        Err(error) => {
            warn!(account = %account.id, %error, "key could not be linked");
            Ok(Login::Denied)
        }
    }
}

/// A wipe-on-drop copy of a typed login attempt for a hashing job; over-long text becomes empty
/// (it cannot match, and is never hashed at full length).
fn attempt_copy(text: &str) -> Zeroizing<String> {
    Zeroizing::new(if text.len() > MAX_ATTEMPT_BYTES {
        String::new()
    } else {
        text.to_owned()
    })
}

/// Store a current-parameters hash in place of an older one. A failure is logged and ignored:
/// the login already succeeded and the old hash still works.
async fn upgrade<S: PasswordStore, O: Offload>(
    store: &S,
    offload: &O,
    account: &Account,
    old: &str,
    password: &Zeroizing<String>,
) {
    let password = password.clone();
    let made = offload.run(move || hash::hash(&password)).await;
    let Ok(Ok(new)) = made else {
        warn!(account = %account.id, "password hash could not be upgraded");
        return;
    };
    if let Err(error) = store.upgrade_hash(account.id, old, &new).await {
        warn!(account = %account.id, %error, "password hash could not be upgraded");
    }
}

/// Why a password was not set or cleared.
#[derive(Debug, thiserror::Error)]
pub enum PasswordError {
    /// The session does not hold `account.set_password`. Nothing was read or written.
    #[error(transparent)]
    Denied(#[from] Denied),
    /// The new password breaks the rules.
    #[error(transparent)]
    Policy(#[from] PolicyError),
    /// A password session did not give the current password.
    #[error("the current password is needed")]
    NeedsCurrent,
    /// The current password given is not the account's.
    #[error("the current password is wrong")]
    WrongCurrent,
    /// The account has no SSH key, so its password is its only way in and is not cleared (D94).
    #[error("the password is the account's only way in")]
    LastCredential,
    /// Hashing could not run.
    #[error(transparent)]
    Unavailable(#[from] Unavailable),
    /// Hashing failed.
    #[error(transparent)]
    Hash(#[from] hash::HashError),
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Whether the session's account has a password. Needs `account.set_password`.
///
/// # Errors
/// [`PasswordError::Denied`] without the permission, [`PasswordError::Store`] otherwise.
pub async fn has_password<S: PasswordStore>(
    store: &S,
    access: &Snapshot,
) -> Result<bool, PasswordError> {
    let granted = access.require(Permission::AccountSetPassword, None)?;
    let account = granted.account().ok_or(StoreError::AccountNotFound)?;
    Ok(store.stored_hash(account).await?.is_some())
}

/// Prove the current password for a password session.
async fn prove_current<S: PasswordStore, O: Offload>(
    store: &S,
    offload: &O,
    account: crate::store::AccountId,
    current: Option<&Secret>,
) -> Result<(), PasswordError> {
    let current = current.ok_or(PasswordError::NeedsCurrent)?;
    let stored = store.stored_hash(account).await?;
    let (target, real) = stored.map_or((DUMMY_HASH.to_owned(), false), |hash| (hash, true));
    let attempt = attempt_copy(current.expose());
    let matched = offload.run(move || hash::verify(&target, &attempt)).await?;
    if matched && real {
        Ok(())
    } else {
        Err(PasswordError::WrongCurrent)
    }
}

/// Set or change the session's own password. Needs `account.set_password`; a password session
/// must also give the current password (a key session never needs it). Audited as
/// `password.set` in the store's transaction, with no secret in the row.
///
/// # Errors
/// See [`PasswordError`].
pub async fn set_password<S: PasswordStore, O: Offload>(
    store: &S,
    offload: &O,
    access: &Snapshot,
    method: LoginMethod,
    current: Option<&Secret>,
    new: &Secret,
) -> Result<(), PasswordError> {
    let granted = access.require(Permission::AccountSetPassword, None)?;
    let account = granted.account().ok_or(StoreError::AccountNotFound)?;
    check_new(new.expose())?;
    if method == LoginMethod::Password {
        prove_current(store, offload, account, current).await?;
    }
    let text = new.copy();
    let made = offload.run(move || hash::hash(&text)).await??;
    store
        .set_hash(
            account,
            Some(&made),
            &events::password_set(account, method.note()),
        )
        .await?;
    Ok(())
}

/// Hash a new password for an account that does not exist yet (D94: sign-up over telnet). Checks
/// the rules for a new password ([`check_new`]) and hashes on `offload`, so the hashing never
/// runs on the session's own task. The result is the argon2id PHC string the account store
/// keeps; the plain text is not kept or logged.
///
/// # Errors
/// [`PasswordError::Policy`] when the password breaks the rules, [`PasswordError::Unavailable`]
/// or [`PasswordError::Hash`] when hashing could not run.
pub async fn hash_new<O: Offload>(offload: &O, password: &Secret) -> Result<String, PasswordError> {
    check_new(password.expose())?;
    let text = password.copy();
    Ok(offload.run(move || hash::hash(&text)).await??)
}

/// Remove the session's own password, so only keys log in. Same rules as [`set_password`];
/// audited as `password.cleared`.
///
/// # Errors
/// See [`PasswordError`].
pub async fn clear_password<S: PasswordStore, O: Offload>(
    store: &S,
    offload: &O,
    access: &Snapshot,
    method: LoginMethod,
    current: Option<&Secret>,
) -> Result<(), PasswordError> {
    let granted = access.require(Permission::AccountSetPassword, None)?;
    let account = granted.account().ok_or(StoreError::AccountNotFound)?;
    if method == LoginMethod::Password {
        prove_current(store, offload, account, current).await?;
    }
    // A password account has no key (D94): clearing its only credential would lock it out.
    if !store.has_key(account).await? {
        return Err(PasswordError::LastCredential);
    }
    store
        .set_hash(
            account,
            None,
            &events::password_cleared(account, method.note()),
        )
        .await?;
    Ok(())
}
