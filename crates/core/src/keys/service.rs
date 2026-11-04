//! The key rules (FIX-M3-11, D97): list, add and remove a key, make a device-link code, issue a
//! staff recovery code, and redeem either in the waiting room. Front ends call these; none of
//! the rules live in a front end, and the permission is checked here, in core, against the
//! snapshot loaded for the action.
//!
//! **No leak.** A redemption refuses with [`Refused`] whatever was wrong (an unknown, used,
//! expired or voided code, a key that already has an account, an inactive account, a store
//! failure): the person reads one sentence and learns nothing about the board. The recovery
//! redemption hashes the new password before it asks the store, so a wrong code costs what a
//! right one does.

use rand::TryRng as _;
use rand::rngs::SysRng;
use tracing::{info, warn};

use super::code::{CodeKind, LoginCode, LoginCodeHash};
use super::pubkey::{PubKeyError, parse_ed25519};
use super::store::{
    IssueError, KeyNotice, KeyRecord, KeyStore, NewCode, RedeemError, RedeemRefusal, RemoveOutcome,
};
use crate::access::{Denied, Fingerprint, Permission, Snapshot};
use crate::audit::events;
use crate::passwords::{Offload, PasswordError, Secret, hash_new};
use crate::store::{AccountId, StoreError, UnixSeconds};

/// Most device-link codes one account may make in an hour (D97).
pub const LINK_CODES_PER_HOUR: u32 = 5;
/// Most recovery codes staff may make for one account in an hour. Staff are slow and a new code
/// voids the last, so the cap only stops a loop.
pub const RECOVERY_CODES_PER_HOUR: u32 = 5;

/// Why a key action did not happen.
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    /// The session does not hold the permission. Nothing was read or written.
    #[error(transparent)]
    Denied(#[from] Denied),
    /// The paste is not an Ed25519 public key.
    #[error(transparent)]
    NotAKey(#[from] PubKeyError),
    /// The key could not be added: it already belongs to an account (this one or another). One
    /// message for both, so a paste cannot be used to find out who has a key.
    #[error("the key cannot be added")]
    CannotAdd,
    /// The account has no such key.
    #[error("no such key")]
    NoSuchKey,
    /// Removing it would leave no way in: no password and no other key.
    #[error("it is the last way in")]
    LastWayIn,
    /// The account has made as many codes this hour as it may.
    #[error("too many codes this hour")]
    RateLimited {
        /// When the oldest of them stops counting.
        retry_at: UnixSeconds,
    },
    /// Staff cannot act on this member: no such active member, or the member is not strictly
    /// below the staff member's rank (D69).
    #[error("no member this session may help")]
    NotYours,
    /// The operating system's random source failed.
    #[error("no random source")]
    Random,
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// A freshly made code and when it stops working. Show it once, then drop it.
#[derive(Debug)]
pub struct IssuedCode {
    /// The code. Never stored, logged or audited.
    pub code: LoginCode,
    /// When it expires.
    pub expires_at: UnixSeconds,
}

/// A redemption was refused. One value for every cause (no oracle).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("code refused")]
pub struct Refused;

/// A new code's random bytes from the operating system.
///
/// # Errors
/// [`KeyError::Random`] when the random source fails.
pub fn random_bytes(kind: CodeKind) -> Result<Vec<u8>, KeyError> {
    let mut bytes = vec![0_u8; kind.chars()];
    SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| KeyError::Random)?;
    Ok(bytes)
}

fn account_of(access: &Snapshot, permission: Permission) -> Result<AccountId, KeyError> {
    let granted = access.require(permission, None)?;
    granted
        .account()
        .ok_or(KeyError::Store(StoreError::AccountNotFound))
}

/// The account's keys. Needs `keys.manage`.
///
/// # Errors
/// [`KeyError::Denied`] without the permission, [`KeyError::Store`] otherwise.
pub async fn list_keys<S: KeyStore>(
    store: &S,
    access: &Snapshot,
) -> Result<Vec<KeyRecord>, KeyError> {
    let account = account_of(access, Permission::KeysManage)?;
    Ok(store.list_keys(account).await?)
}

/// Bind a pasted Ed25519 public key to the account. Needs `keys.manage`. Audited as `key.added`
/// and told to the member at the next sign-in.
///
/// # Errors
/// [`KeyError::NotAKey`] for a paste that is not an Ed25519 public key, [`KeyError::CannotAdd`]
/// for a key that belongs to an account already, [`KeyError::Denied`] and [`KeyError::Store`].
pub async fn add_key<S: KeyStore>(
    store: &S,
    access: &Snapshot,
    pasted: &str,
) -> Result<Fingerprint, KeyError> {
    let account = account_of(access, Permission::KeysManage)?;
    let fingerprint = parse_ed25519(pasted)?;
    match store
        .add_key(account, &fingerprint, &events::key_added(account, &fingerprint))
        .await
    {
        Ok(()) => {
            info!(%account, %fingerprint, "key added");
            Ok(fingerprint)
        }
        Err(StoreError::FingerprintTaken) => Err(KeyError::CannotAdd),
        Err(error) => Err(error.into()),
    }
}

/// Remove one of the account's keys, unless it is the last way in (no password, no other key).
/// Needs `keys.manage`. Audited as `key.removed` and told to the member at the next sign-in.
///
/// # Errors
/// [`KeyError::NoSuchKey`], [`KeyError::LastWayIn`], [`KeyError::Denied`], [`KeyError::Store`].
pub async fn remove_key<S: KeyStore>(
    store: &S,
    access: &Snapshot,
    fingerprint: &Fingerprint,
) -> Result<(), KeyError> {
    let account = account_of(access, Permission::KeysManage)?;
    match store
        .remove_key(account, fingerprint, &events::key_removed(account, fingerprint))
        .await?
    {
        RemoveOutcome::Removed => {
            info!(%account, %fingerprint, "key removed");
            Ok(())
        }
        RemoveOutcome::NotFound => Err(KeyError::NoSuchKey),
        RemoveOutcome::LastWayIn => Err(KeyError::LastWayIn),
    }
}

/// Make a device-link code for the signed-in account from `random`
/// ([`random_bytes`]`(`[`CodeKind::Link`]`)`). Needs `keys.manage`. At most
/// [`LINK_CODES_PER_HOUR`] an hour; a new code voids the last unused one. Audited as
/// `device_code.issued` (no code, no hash).
///
/// # Errors
/// [`KeyError::RateLimited`], [`KeyError::Denied`], [`KeyError::Store`].
pub async fn make_link_code<S: KeyStore>(
    store: &S,
    access: &Snapshot,
    random: &[u8],
) -> Result<IssuedCode, KeyError> {
    let account = account_of(access, Permission::KeysManage)?;
    let code = LoginCode::from_bytes(CodeKind::Link, random);
    let new = NewCode {
        kind: CodeKind::Link,
        target: account,
        issuer: account,
        hash: code.hash(),
        ttl_secs: CodeKind::Link.ttl_secs(),
        per_hour: Some(LINK_CODES_PER_HOUR),
    };
    let expires_at = recorded(store.issue_code(&new, &events::device_code_issued(account)).await)?;
    info!(%account, "device link code made");
    Ok(IssuedCode { code, expires_at })
}

fn recorded(result: Result<UnixSeconds, IssueError>) -> Result<UnixSeconds, KeyError> {
    match result {
        Ok(expires_at) => Ok(expires_at),
        Err(IssueError::RateLimited { retry_at }) => Err(KeyError::RateLimited { retry_at }),
        Err(IssueError::NoAccount) => Err(KeyError::NotYours),
        Err(IssueError::Store(error)) => Err(error.into()),
    }
}

/// Issue a one-time recovery code for the member with `handle`, from `random`
/// ([`random_bytes`]`(`[`CodeKind::Recovery`]`)`). Needs `keys.recover`, and the member must be
/// active and **strictly below the staff member's rank** (D69): a cosysop cannot help a sysop,
/// and nobody can issue one for themselves. Audited as `recovery_code.issued`.
///
/// # Errors
/// [`KeyError::NotYours`] for an unknown or inactive member or one not below the staff member
/// (staff can see both from the members list, so the screen may say which),
/// [`KeyError::RateLimited`], [`KeyError::Denied`], [`KeyError::Store`].
pub async fn issue_recovery_code<S: KeyStore>(
    store: &S,
    access: &Snapshot,
    handle: &str,
    random: &[u8],
) -> Result<IssuedCode, KeyError> {
    let staff = account_of(access, Permission::KeysRecover)?;
    let Some(handle) = crate::passwords::login_handle(handle) else {
        return Err(KeyError::NotYours);
    };
    let Some(target) = store.active_account(&handle).await? else {
        return Err(KeyError::NotYours);
    };
    let theirs = store.account_rank(target.id).await?;
    let ours = access.top_rank().unwrap_or(0);
    if target.id == staff || theirs >= ours {
        return Err(KeyError::NotYours);
    }
    let code = LoginCode::from_bytes(CodeKind::Recovery, random);
    let new = NewCode {
        kind: CodeKind::Recovery,
        target: target.id,
        issuer: staff,
        hash: code.hash(),
        ttl_secs: CodeKind::Recovery.ttl_secs(),
        per_hour: Some(RECOVERY_CODES_PER_HOUR),
    };
    let audit = events::recovery_code_issued(staff, target.id);
    let expires_at = recorded(store.issue_code(&new, &audit).await)?;
    info!(%staff, account = %target.id, "recovery code issued");
    Ok(IssuedCode { code, expires_at })
}

/// Redeem a device-link code: bind `fingerprint` (this connection's key) to the account the code
/// was made for. Needs `keys.redeem`, which the waiting room's snapshot holds. Every failure is
/// [`Refused`]. Returns the account's id.
///
/// # Errors
/// [`Refused`] for every cause.
pub async fn redeem_link_code<S: KeyStore>(
    store: &S,
    access: &Snapshot,
    typed: &str,
    fingerprint: &Fingerprint,
) -> Result<crate::store::Account, Refused> {
    if access.require(Permission::KeysRedeem, None).is_err() {
        return Err(Refused);
    }
    let hash = LoginCodeHash::of(CodeKind::Link, typed);
    settle(store.redeem_link_code(&hash, fingerprint).await, fingerprint)
}

/// Redeem a staff recovery code: bind `fingerprint` and set `new_password` (D81 rules) in one
/// step. Needs `keys.redeem`. The password is hashed first, whatever the code, so the time says
/// nothing about it. Every failure, a weak password included, is [`Refused`]; the waiting room
/// checks the password rules before it gets here so the person can fix a typing mistake.
///
/// # Errors
/// [`Refused`] for every cause.
pub async fn redeem_recovery_code<S: KeyStore, O: Offload>(
    store: &S,
    offload: &O,
    access: &Snapshot,
    typed: &str,
    fingerprint: &Fingerprint,
    new_password: &Secret,
) -> Result<crate::store::Account, Refused> {
    if access.require(Permission::KeysRedeem, None).is_err() {
        return Err(Refused);
    }
    let hashed = hash_new(offload, new_password).await;
    let hash = LoginCodeHash::of(CodeKind::Recovery, typed);
    let Ok(password_hash) = hashed else {
        if let Err(error) = &hashed {
            note_unusable(error);
        }
        return Err(Refused);
    };
    settle(
        store
            .redeem_recovery_code(&hash, fingerprint, &password_hash)
            .await,
        fingerprint,
    )
}

fn note_unusable(error: &PasswordError) {
    // The error names no secret.
    info!(%error, "recovery password not usable; code refused");
}

fn settle(
    result: Result<crate::store::Account, RedeemError>,
    fingerprint: &Fingerprint,
) -> Result<crate::store::Account, Refused> {
    match result {
        Ok(account) => {
            info!(account = %account.id, %fingerprint, "code redeemed");
            Ok(account)
        }
        Err(RedeemError::Refused(RedeemRefusal::Code | RedeemRefusal::Key)) => Err(Refused),
        Err(RedeemError::Store(error)) => {
            warn!(%error, "code redemption failed");
            Err(Refused)
        }
    }
}

/// What the member is told at sign-in: the unseen notices, marked as told. A failure is logged
/// and shows nothing (the notices stay for the next sign-in).
pub async fn notices_for_sign_in<S: KeyStore>(store: &S, account: AccountId) -> Vec<KeyNotice> {
    match store.take_notices(account).await {
        Ok(notices) => notices,
        Err(error) => {
            warn!(%account, %error, "key notices could not be read");
            Vec::new()
        }
    }
}
