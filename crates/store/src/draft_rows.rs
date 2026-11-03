//! Backend-independent mapping for the `draft` table: what a stored row means, written once
//! (D24). Each backend runs its own SQL and hands the values here.

use russh_bbs_core::drafts::{ContextKey, DraftSummary, StoredDraft};
use russh_bbs_core::store::{StoreError, UnixSeconds};

fn context(text: &str) -> Result<ContextKey, StoreError> {
    ContextKey::parse(text)
        .map_err(|_| StoreError::Corrupt("a stored draft context key is not valid".into()))
}

/// A `draft` row as the store hands it over.
pub(crate) fn stored(
    context_key: &str,
    body: String,
    updated_at: i64,
) -> Result<StoredDraft, StoreError> {
    Ok(StoredDraft {
        context: context(context_key)?,
        body,
        updated_at: UnixSeconds::from_raw(updated_at),
    })
}

/// A row of the draft list.
pub(crate) fn summary(
    context_key: &str,
    updated_at: i64,
    chars: i64,
) -> Result<DraftSummary, StoreError> {
    Ok(DraftSummary {
        context: context(context_key)?,
        updated_at: UnixSeconds::from_raw(updated_at),
        chars: usize::try_from(chars)
            .map_err(|_| StoreError::Corrupt("a stored draft has a negative length".into()))?,
    })
}

/// The cap as a number the database can compare with.
pub(crate) fn cap(max_drafts: usize) -> i64 {
    i64::try_from(max_drafts).unwrap_or(i64::MAX)
}
