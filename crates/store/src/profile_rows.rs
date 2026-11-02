//! Row types for profiles and the member list, shared by both backends, and their one-time
//! mapping to core's domain types. Text read back is sanitized again by core's constructors:
//! a database is never trusted to hold clean text.

use russh_bbs_core::profiles::{MemberRecord, MemberSummary, Profile, RawProfile};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountId, StoreError};

/// A member-list row: an active account and the display name of its profile ('' if none).
pub(crate) struct SummaryRow {
    pub(crate) id: i64,
    pub(crate) username: String,
    pub(crate) display_name: String,
}

impl SummaryRow {
    pub(crate) fn into_summary(self) -> Result<MemberSummary, StoreError> {
        let handle = sanitize_username(&self.username)
            .map_err(|_| StoreError::Corrupt("stored username is not valid".into()))?;
        Ok(MemberSummary::new(
            AccountId::from_raw(self.id),
            handle,
            &self.display_name,
        ))
    }
}

/// An active account with every profile column ('' where no profile row exists).
pub(crate) struct RecordRow {
    pub(crate) id: i64,
    pub(crate) username: String,
    pub(crate) display_name: String,
    pub(crate) pronouns: String,
    pub(crate) location: String,
    pub(crate) about: String,
    pub(crate) website: String,
}

impl RecordRow {
    pub(crate) fn into_record(self) -> Result<MemberRecord, StoreError> {
        let handle = sanitize_username(&self.username)
            .map_err(|_| StoreError::Corrupt("stored username is not valid".into()))?;
        Ok(MemberRecord {
            id: AccountId::from_raw(self.id),
            handle,
            profile: Profile::from_stored(&RawProfile {
                display_name: self.display_name,
                pronouns: self.pronouns,
                location: self.location,
                about: self.about,
                website: self.website,
            }),
        })
    }
}
