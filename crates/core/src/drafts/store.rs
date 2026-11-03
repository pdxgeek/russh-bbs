//! The storage trait for drafts: persistence only. The rules (sanitizing, bounds, blank text
//! removes a draft) are in [`super::service`]; the backends only read and write rows.

use std::future::Future;

use crate::store::{AccountId, StoreError, UnixSeconds};

use super::body::Body;
use super::context::ContextKey;

/// Most drafts one member may keep, across every context. A save into a context that already
/// has a draft always succeeds (it replaces it); a save into a new context at the cap is
/// refused.
pub const MAX_DRAFTS_PER_MEMBER: usize = 20;

/// A draft as the store holds it. The body is text the store was given as a [`Body`]; the core
/// functions sanitize it again when they read it, so a row edited by hand cannot reach a
/// screen unchecked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDraft {
    /// What the draft is for.
    pub context: ContextKey,
    /// The text as stored.
    pub body: String,
    /// When it was last saved.
    pub updated_at: UnixSeconds,
}

/// A draft without its text, for a "your drafts" list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftSummary {
    /// What the draft is for.
    pub context: ContextKey,
    /// When it was last saved.
    pub updated_at: UnixSeconds,
    /// Characters in the stored text.
    pub chars: usize,
}

/// What [`DraftStore::put_draft`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftPut {
    /// A new draft was stored.
    Created,
    /// The member's draft for that context was replaced.
    Replaced,
    /// The member already holds the most drafts allowed and this context has none, so nothing
    /// was written.
    Full,
}

/// Drafts, per member and per context.
pub trait DraftStore: Send + Sync {
    /// Store `body` as the member's draft for `context`, replacing any earlier one. A new draft
    /// is refused with [`DraftPut::Full`] when the member already holds `max_drafts`; the count
    /// and the write are atomic, so two saves at once cannot pass the cap.
    ///
    /// Fails with [`StoreError::AccountNotFound`] for an id with no account.
    fn put_draft(
        &self,
        account: AccountId,
        context: &ContextKey,
        body: &Body,
        now: UnixSeconds,
        max_drafts: usize,
    ) -> impl Future<Output = Result<DraftPut, StoreError>> + Send;

    /// The member's draft for `context`, if they have one.
    fn get_draft(
        &self,
        account: AccountId,
        context: &ContextKey,
    ) -> impl Future<Output = Result<Option<StoredDraft>, StoreError>> + Send;

    /// Delete the member's draft for `context`. `true` when there was one.
    fn delete_draft(
        &self,
        account: AccountId,
        context: &ContextKey,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// The member's drafts, newest first, without their text.
    fn list_drafts(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<DraftSummary>, StoreError>> + Send;
}
