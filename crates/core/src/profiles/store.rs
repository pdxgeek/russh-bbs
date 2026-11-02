//! The storage trait for profiles and the member list: persistence only. Who may call it and
//! what a result means is in [`super::service`]; the traits take no permission because only
//! the service functions (which take a `Granted`) are meant to be called by a front end.

use std::future::Future;

use crate::audit::AuditEvent;
use crate::sanitize::{SingleLine, USERNAME_MAX_CHARS, Username};
use crate::store::{AccountId, StoreError};

use super::fields::Profile;

/// Members per page of the list. Nine keeps the list and its footer on an 80x24 screen and
/// lets the digits 1-9 pick a row.
pub const PAGE_SIZE: usize = 9;

/// The start of a handle, typed into the search. Only the characters a handle can hold
/// survive (ASCII letters, digits, `_` and `-`), lowercased, so it is safe to bind and to
/// compare without any pattern characters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HandlePrefix(String);

impl HandlePrefix {
    /// Keep what a handle can hold of `typed`, lowercased and capped at the handle length.
    #[must_use]
    pub fn parse(typed: &str) -> Self {
        Self(
            typed
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .map(|c| c.to_ascii_lowercase())
                .take(USERNAME_MAX_CHARS)
                .collect(),
        )
    }

    /// The prefix, possibly empty (which matches every handle).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether nothing was typed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Where the next page starts: after this lowercased handle. Handles are unique ignoring case,
/// so paging by the last one seen never skips or repeats a member, even if members join while
/// someone is reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor(String);

impl Cursor {
    /// The cursor that follows `member`.
    #[must_use]
    pub fn after(member: &MemberSummary) -> Self {
        Self(member.handle.as_str().to_ascii_lowercase())
    }

    /// The lowercased handle the page starts after.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One page of the member list asked for: handles starting with `prefix`, after `after`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberQuery {
    /// Handle prefix (empty: everyone).
    pub prefix: HandlePrefix,
    /// Where the page starts (none: the first page).
    pub after: Option<Cursor>,
}

/// A row of the member list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberSummary {
    /// The account.
    pub id: AccountId,
    /// The handle.
    pub handle: Username,
    /// The display name, sanitized when the row is built; empty if none is set.
    pub display_name: String,
}

impl MemberSummary {
    /// Build a row from stored text. The display name is sanitized here, never trusted.
    #[must_use]
    pub fn new(id: AccountId, handle: Username, display_name: &str) -> Self {
        Self {
            id,
            handle,
            display_name: SingleLine::new(super::fields::DISPLAY_NAME_MAX)
                .sanitize(display_name)
                .into_string(),
        }
    }
}

/// An active member and their profile (empty fields if they never wrote one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRecord {
    /// The account.
    pub id: AccountId,
    /// The handle.
    pub handle: Username,
    /// The profile.
    pub profile: Profile,
}

/// Profiles and the member list.
///
/// Only **active** accounts are members: pending, approved and suspended accounts are never
/// returned by any method, as if they did not exist.
pub trait ProfileStore: Send + Sync {
    /// Up to `limit` active members in handle order (ignoring case) that match `query`.
    fn list_members(
        &self,
        query: &MemberQuery,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<MemberSummary>, StoreError>> + Send;

    /// The active member with this id, or `None` if there is none or the account is not
    /// active. Absent and not-active are the same answer.
    fn member(
        &self,
        id: AccountId,
    ) -> impl Future<Output = Result<Option<MemberRecord>, StoreError>> + Send;

    /// The rank of the highest-ranked role `id` holds globally, as [`Snapshot::top_rank`] reads
    /// it, or `None` when it holds none (or is not active, which holds only `waiting`). The
    /// rank rule for staff edits (D60 (6), D77) compares it with the editor's.
    ///
    /// [`Snapshot::top_rank`]: crate::access::Snapshot::top_rank
    fn standing(
        &self,
        id: AccountId,
    ) -> impl Future<Output = Result<Option<u8>, StoreError>> + Send;

    /// Replace the account's profile. [`StoreError::AccountNotFound`] for an id with no
    /// account. `audit`, when given, is written in the same transaction (M2-C): the profile and
    /// its audit row commit or roll back together.
    fn save_profile(
        &self,
        id: AccountId,
        profile: &Profile,
        audit: Option<&AuditEvent>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

/// The empty directory has no members (used by tests and before a store exists).
impl ProfileStore for crate::access::NoMembers {
    fn list_members(
        &self,
        _query: &MemberQuery,
        _limit: usize,
    ) -> impl Future<Output = Result<Vec<MemberSummary>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn member(
        &self,
        _id: AccountId,
    ) -> impl Future<Output = Result<Option<MemberRecord>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn standing(
        &self,
        _id: AccountId,
    ) -> impl Future<Output = Result<Option<u8>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn save_profile(
        &self,
        _id: AccountId,
        _profile: &Profile,
        _audit: Option<&AuditEvent>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }
}
