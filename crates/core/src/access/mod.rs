//! The access decision: what an authenticated SSH key is allowed to do.
//!
//! Authentication proves a client holds a key. It says nothing about membership. This module
//! turns "which key is this" into an [`Access`] level, and the rule lives here, not in a front
//! end: **a key that is not a known member is [`Access::Restricted`]**. A restricted session
//! may see the waiting room and nothing else (D2, threat model "Waiting room").
//!
//! The member lookup is a trait ([`MemberDirectory`]) so the SQLite store (M1-B) can plug in
//! without this crate knowing about it. Until then [`NoMembers`] knows nobody.

use std::fmt;
use std::future::Future;

use crate::store::Role;

mod permission;

pub use permission::{Permission, PermissionCheck, RolePermissions};

/// Length of the base64 (unpadded) part of a SHA-256 fingerprint: 32 bytes encode to 43 chars.
const SHA256_BASE64_LEN: usize = 43;

/// Prefix OpenSSH puts on SHA-256 fingerprints.
const SHA256_PREFIX: &str = "SHA256:";

/// An SSH public key fingerprint in OpenSSH SHA-256 form, for example `SHA256:abc...`.
///
/// A fingerprint identifies a key and is not secret, so logging it is fine. Construct it only
/// at a boundary; the constructor checks the shape.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Fingerprint(String);

/// Why a string is not a [`Fingerprint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidFingerprint;

impl fmt::Display for InvalidFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a SHA256 key fingerprint")
    }
}

impl std::error::Error for InvalidFingerprint {}

impl Fingerprint {
    /// Parse an OpenSSH SHA-256 fingerprint (`SHA256:` plus 43 unpadded base64 characters).
    pub fn parse(text: &str) -> Result<Self, InvalidFingerprint> {
        let body = text.strip_prefix(SHA256_PREFIX).ok_or(InvalidFingerprint)?;
        let well_formed = body.len() == SHA256_BASE64_LEN
            && body
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/');
        if well_formed {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidFingerprint)
        }
    }

    /// The fingerprint as text, `SHA256:...`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Fingerprint").field(&self.0).finish()
    }
}

/// What an authenticated session may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Access {
    /// Not a known member. Sees the waiting room only: no member list, boards or stats.
    Restricted,
    /// A known member.
    Member,
}

/// Looks up whether a key belongs to a member.
///
/// Implementations **fail closed**: on any error (store down, corrupt row) answer `false`, so
/// the key falls back to [`Access::Restricted`].
pub trait MemberDirectory: Send + Sync {
    /// Whether `key` is the key of a known member.
    fn is_member(&self, key: &Fingerprint) -> impl Future<Output = bool> + Send;
}

/// The directory with no members. Every key is unknown. Used until the store exists (M1-B).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoMembers;

impl MemberDirectory for NoMembers {
    fn is_member(&self, _key: &Fingerprint) -> impl Future<Output = bool> + Send {
        std::future::ready(false)
    }
}

/// Looks up the role of a member's key, so a session can say what it is signed in as.
///
/// Like [`MemberDirectory`] it **fails closed**: an unknown key, an account that is not
/// active, or any error answers `None`. Roles are a stored value only in M1; the permission
/// model that gives them meaning is M2's (D33).
pub trait RoleDirectory: Send + Sync {
    /// The role of the active account `key` is bound to.
    fn role_of(&self, key: &Fingerprint) -> impl Future<Output = Option<Role>> + Send;
}

impl RoleDirectory for NoMembers {
    fn role_of(&self, _key: &Fingerprint) -> impl Future<Output = Option<Role>> + Send {
        std::future::ready(None)
    }
}

/// Decide the access level for an authenticated key. Unknown keys are [`Access::Restricted`].
pub async fn decide_access<D: MemberDirectory>(directory: &D, key: &Fingerprint) -> Access {
    if directory.is_member(key).await {
        Access::Member
    } else {
        Access::Restricted
    }
}

#[cfg(test)]
mod tests;
