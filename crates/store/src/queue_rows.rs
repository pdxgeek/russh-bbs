//! Row types for the queue and their one-time mapping to core's domain types. Both backends
//! read the same joined shape, so the grouping lives once.

use russh_bbs_core::queue::{Message, PendingRequest, RequestId, Voucher};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

/// A request joined to one of its vouches (or to none), as read by the checked queries.
pub(crate) struct QueueRow {
    pub(crate) id: i64,
    pub(crate) handle: String,
    pub(crate) message: String,
    pub(crate) created_at: i64,
    pub(crate) expires_at: i64,
    pub(crate) voucher_id: Option<i64>,
    /// `None` when the voucher is not an active member any more; such a vouch is not shown.
    pub(crate) voucher_handle: Option<String>,
}

fn corrupt() -> StoreError {
    StoreError::Corrupt("stored username is not valid".into())
}

/// Fold rows (ordered by request, then vouch) into requests with their vouchers. Handles and
/// messages are sanitized again on the way out, whatever was stored.
pub(crate) fn group(rows: Vec<QueueRow>) -> Result<Vec<PendingRequest>, StoreError> {
    let mut out: Vec<PendingRequest> = Vec::new();
    for row in rows {
        if out.last().is_none_or(|last| last.id.as_raw() != row.id) {
            out.push(PendingRequest {
                id: RequestId::from_raw(row.id),
                handle: sanitize_username(&row.handle).map_err(|_| corrupt())?,
                message: Message::new(&row.message),
                created_at: UnixSeconds::from_raw(row.created_at),
                expires_at: UnixSeconds::from_raw(row.expires_at),
                vouchers: Vec::new(),
            });
        }
        if let (Some(account), Some(name), Some(last)) =
            (row.voucher_id, row.voucher_handle, out.last_mut())
        {
            last.vouchers.push(Voucher {
                account: AccountId::from_raw(account),
                handle: sanitize_username(&name).map_err(|_| corrupt())?,
            });
        }
    }
    Ok(out)
}
