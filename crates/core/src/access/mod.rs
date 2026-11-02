//! The access decision: what an authenticated SSH key is allowed to do.
//!
//! Authentication proves a client holds a key. It says nothing about membership. This module
//! turns "which key is this" into an [`Access`] level, and the rule lives here, not in a front
//! end: **a key that is not a known member is [`Access::Restricted`]**. A restricted session
//! may see the waiting room and nothing else (D2, threat model "Waiting room").
//!
//! The member lookup is a trait ([`MemberDirectory`]) so the SQLite store (M1-B) can plug in
//! without this crate knowing about it. Until then [`NoMembers`] knows nobody.
//!
//! **Permissions (M2-B, D33).** Beyond "is this a member", every action has a named
//! [`Permission`]. What an account holds is a [`Snapshot`] the store loads at the start of each
//! action; [`resolve`] (pure, additive) answers a check and [`reach`] answers "which resources",
//! from the same facts. [`Snapshot::require`] returns the [`Granted`] proof that action
//! functions take. Group nesting rules are in [`GroupGraph`].

mod edit;
mod groups;
mod permission;
mod slug;
mod snapshot;
mod store;

use std::fmt;
use std::future::Future;

pub use edit::{
    EditError, MIN_CUSTOM_RANK, NewRole, create_role, delete_role, grant_permission, grant_role,
    revoke_permission, revoke_role, set_role_permissions,
};
pub use groups::{
    Child, GroupChange, GroupError, GroupGraph, GroupId, GroupInfo, Kind, MAX_GROUP_DEPTH,
};
pub use permission::Permission;
pub use slug::{InvalidSlug, SLUG_MAX_CHARS, Slug};
pub use snapshot::{
    Decision, Denied, Granted, Held, Reach, Reason, ResourceId, ResourceKind, ResourceRef, Scope,
    Snapshot, Source, Via, reach, resolve,
};
pub use store::{AccessError, AccessStore, Granting, NewGrant, RoleInfo, Subject};

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

/// The single access hook for menus: may this session use something that needs `permission`?
///
/// Menu code asks nothing else. Visibility and activation use the same answer, so an item a
/// session cannot see is also not reachable by its hotkey. [`Snapshot`] is the production
/// implementation; it answers with [`resolve`].
pub trait PermissionCheck {
    /// Whether the session holds `permission` globally.
    fn allows(&self, permission: Permission) -> bool;
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
mod edit_tests;
#[cfg(test)]
mod tests;
