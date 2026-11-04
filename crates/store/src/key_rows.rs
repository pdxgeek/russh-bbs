//! Rows read for keys and key notices, mapped once to core's types (FIX-M3-11).

use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::keys::{KeyNotice, KeyRecord, NoticeKind};
use russh_bbs_core::store::{StoreError, UnixSeconds};

/// An `account_keys` row.
pub(crate) struct KeyRow {
    pub(crate) fingerprint: String,
    pub(crate) created_at: i64,
    pub(crate) last_used_at: Option<i64>,
}

impl KeyRow {
    /// Validate the stored text; anything the code could not have written is `Corrupt`.
    pub(crate) fn into_record(self) -> Result<KeyRecord, StoreError> {
        Ok(KeyRecord {
            fingerprint: Fingerprint::parse(&self.fingerprint)
                .map_err(|_| StoreError::Corrupt("stored fingerprint is not valid".into()))?,
            added_at: UnixSeconds::from_raw(self.created_at),
            last_used_at: self.last_used_at.map(UnixSeconds::from_raw),
        })
    }
}

/// A `key_notice` row.
pub(crate) struct NoticeRow {
    pub(crate) id: i64,
    pub(crate) kind: String,
    pub(crate) fingerprint: Option<String>,
    pub(crate) at: i64,
}

impl NoticeRow {
    pub(crate) fn into_notice(self) -> Result<KeyNotice, StoreError> {
        let kind = NoticeKind::parse(&self.kind)
            .ok_or_else(|| StoreError::Corrupt("stored notice kind is not known".into()))?;
        let fingerprint = self
            .fingerprint
            .map(|text| Fingerprint::parse(&text))
            .transpose()
            .map_err(|_| StoreError::Corrupt("stored fingerprint is not valid".into()))?;
        Ok(KeyNotice {
            kind,
            at: UnixSeconds::from_raw(self.at),
            fingerprint,
        })
    }
}

/// Notices in the order they happened.
pub(crate) fn notices_in_order(mut rows: Vec<NoticeRow>) -> Result<Vec<KeyNotice>, StoreError> {
    rows.sort_by_key(|row| row.id);
    rows.into_iter().map(NoticeRow::into_notice).collect()
}
