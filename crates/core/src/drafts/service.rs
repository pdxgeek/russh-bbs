//! The draft rules: what is stored, when, and what comes back.

use crate::store::{AccountId, StoreError, UnixSeconds};

use super::body::{Body, BodyError, BodyLimits};
use super::context::ContextKey;
use super::store::{DraftPut, DraftStore, DraftSummary, MAX_DRAFTS_PER_MEMBER};

/// A draft read back: its text has passed the sanitizer again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// What the draft is for.
    pub context: ContextKey,
    /// The text.
    pub body: Body,
    /// When it was last saved.
    pub updated_at: UnixSeconds,
}

/// What [`save_draft`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    /// A new draft was stored.
    Created,
    /// An earlier draft for the context was replaced.
    Replaced,
    /// The text was blank, so the context's draft (if any) was removed instead. Clearing the
    /// editor and saving is how a member deletes a draft.
    Removed,
}

/// Why a draft could not be saved or read.
#[derive(Debug, thiserror::Error)]
pub enum DraftError {
    /// The text is over its size or line limit.
    #[error(transparent)]
    Body(#[from] BodyError),
    /// The member already keeps the most drafts allowed.
    #[error("you already keep {MAX_DRAFTS_PER_MEMBER} drafts; discard one first")]
    TooMany,
    /// The storage backend failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Save `text` as the member's draft for `context`.
///
/// The text is sanitized with the shared sanitizer and checked against `limits` first
/// ([`Body::prepare`]); over-long text is refused, not cut. Blank text removes the draft
/// instead of storing it.
///
/// # Errors
/// [`DraftError::Body`] for text over its limits, [`DraftError::TooMany`] when the member is at
/// the cap and `context` has no draft yet, [`DraftError::Store`] when storage fails.
pub async fn save_draft<S: DraftStore>(
    store: &S,
    account: AccountId,
    context: &ContextKey,
    text: &str,
    limits: BodyLimits,
    now: UnixSeconds,
) -> Result<SaveOutcome, DraftError> {
    let body = Body::prepare(limits, text)?;
    if body.is_blank() {
        store.delete_draft(account, context).await?;
        return Ok(SaveOutcome::Removed);
    }
    match store
        .put_draft(account, context, &body, now, MAX_DRAFTS_PER_MEMBER)
        .await?
    {
        DraftPut::Created => Ok(SaveOutcome::Created),
        DraftPut::Replaced => Ok(SaveOutcome::Replaced),
        DraftPut::Full => Err(DraftError::TooMany),
    }
}

/// The member's draft for `context`, if any. The stored text is sanitized again and checked
/// against the board-wide ceiling; a row that fails is [`StoreError::Corrupt`], never shown.
///
/// # Errors
/// [`DraftError::Store`] when storage fails or the row is not one the code could have written.
pub async fn load_draft<S: DraftStore>(
    store: &S,
    account: AccountId,
    context: &ContextKey,
) -> Result<Option<Draft>, DraftError> {
    let Some(stored) = store.get_draft(account, context).await? else {
        return Ok(None);
    };
    let body = Body::prepare(BodyLimits::MAX, &stored.body)
        .map_err(|_| StoreError::Corrupt("a stored draft is over the size limit".into()))?;
    Ok(Some(Draft {
        context: stored.context,
        body,
        updated_at: stored.updated_at,
    }))
}

/// Delete the member's draft for `context`. `true` when there was one. Call this after the
/// write the draft was for (the post, the mail) succeeded.
///
/// # Errors
/// [`DraftError::Store`] when storage fails.
pub async fn discard_draft<S: DraftStore>(
    store: &S,
    account: AccountId,
    context: &ContextKey,
) -> Result<bool, DraftError> {
    Ok(store.delete_draft(account, context).await?)
}

/// The member's drafts, newest first, without their text.
///
/// # Errors
/// [`DraftError::Store`] when storage fails.
pub async fn list_drafts<S: DraftStore>(
    store: &S,
    account: AccountId,
) -> Result<Vec<DraftSummary>, DraftError> {
    Ok(store.list_drafts(account).await?)
}
