//! The read service: `audit.view`, checked in core.

use super::store::{AuditQuery, AuditStore, LogEntry};
use crate::access::{Denied, Permission, Snapshot};
use crate::store::StoreError;

/// Why a read of the log failed.
#[derive(Debug, thiserror::Error)]
pub enum AuditViewError {
    /// The caller does not hold `audit.view`. Nothing was read.
    #[error(transparent)]
    Denied(#[from] Denied),
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// One page of the log for a caller whose permissions are `snapshot`. Needs `audit.view`
/// (globally); anything else is [`AuditViewError::Denied`] and the store is not touched.
pub async fn view_log<S: AuditStore>(
    store: &S,
    snapshot: &Snapshot,
    query: &AuditQuery,
) -> Result<Vec<LogEntry>, AuditViewError> {
    snapshot.require(Permission::AuditView, None)?;
    Ok(store.page(query).await?)
}
