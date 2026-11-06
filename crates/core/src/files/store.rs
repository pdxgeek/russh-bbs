//! The storage trait for files: persistence only. Rules (access, text, limits, quarantine,
//! scanning) are in [`super::service`], [`super::transfer`] and [`super::rules`]; the backends
//! only read and write rows. Blob bytes are not here: a row names a blob in the blob store.

use std::future::Future;

use crate::audit::{Actor, AuditEvent};
use crate::sanitize::Sanitized;
use crate::store::{AccountId, StoreError, UnixSeconds};

use super::ids::{AreaId, FileId};

/// A file area as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaRow {
    /// The area.
    pub id: AreaId,
    /// Its name.
    pub name: String,
    /// What it is for; may be empty.
    pub description: String,
    /// Whether a global holding of `files.*` is not enough to reach it.
    pub restricted: bool,
    /// Whether a clean upload goes live without staff approval.
    pub auto_approve: bool,
    /// Approved files in it.
    pub files: u64,
}

/// An area to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewArea {
    /// Its name; unique ignoring case.
    pub name: Sanitized,
    /// What it is for.
    pub description: Sanitized,
    /// Whether it needs a grant scoped to it.
    pub restricted: bool,
    /// Whether clean uploads skip the approval queue.
    pub auto_approve: bool,
}

/// Where a file is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::IntoStaticStr)]
pub enum FileState {
    /// Uploaded, not yet visible to others.
    #[strum(serialize = "quarantine")]
    Quarantine,
    /// Live: listed and downloadable by those who may.
    #[strum(serialize = "approved")]
    Approved,
    /// Refused by staff or the scanner; the bytes are gone.
    #[strum(serialize = "rejected")]
    Rejected,
}

impl FileState {
    /// The stored name.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.into()
    }
}

/// What the scanner concluded about a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::IntoStaticStr)]
pub enum ScanState {
    /// No scanner ran. The file was not scanned.
    #[strum(serialize = "unscanned")]
    Unscanned,
    /// Scanned, nothing found.
    #[strum(serialize = "clean")]
    Clean,
    /// Scanned, something found.
    #[strum(serialize = "infected")]
    Infected,
    /// The scanner could not answer.
    #[strum(serialize = "failed")]
    Failed,
}

impl ScanState {
    /// The stored name.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.into()
    }
}

/// A file as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    /// The file.
    pub id: FileId,
    /// Its area.
    pub area: AreaId,
    /// The area's name, for headings.
    pub area_name: String,
    /// Who uploaded it, or `None` when the account is gone.
    pub uploader_id: Option<AccountId>,
    /// Their handle.
    pub uploader: Option<String>,
    /// The display name (sanitized when stored; core cleans it again on read).
    pub name: String,
    /// The name as the client sent it. Data only.
    pub original_name: String,
    /// The description.
    pub description: String,
    /// Content length.
    pub size: u64,
    /// SHA-256 of the content, 64 lowercase hex characters.
    pub sha256: String,
    /// The blob key (`<namespace>/<id>`).
    pub blob_key: String,
    /// Where it is in its life.
    pub state: FileState,
    /// What the scanner said.
    pub scan: ScanState,
    /// The scanner's note (a signature name or a reason), cleaned on read.
    pub scan_note: String,
    /// When it was uploaded.
    pub created_at: UnixSeconds,
    /// When staff or the scanner decided it.
    pub decided_at: Option<UnixSeconds>,
    /// Times a download of it was opened.
    pub downloads: u64,
    /// Its tags, sorted.
    pub tags: Vec<String>,
}

/// An upload that has been stored in quarantine, to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFile {
    /// The area.
    pub area: AreaId,
    /// Who uploaded it.
    pub uploader: AccountId,
    /// The display name.
    pub name: Sanitized,
    /// The name as sent.
    pub original_name: String,
    /// Content length.
    pub size: u64,
    /// SHA-256, 64 lowercase hex characters.
    pub sha256: String,
    /// The quarantine blob key.
    pub blob_key: String,
    /// When.
    pub now: UnixSeconds,
}

/// What a decision does to a quarantined file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Make it live; the bytes now sit under this key.
    Approve {
        /// The new blob key (in the `files` namespace).
        blob_key: String,
    },
    /// Refuse it; the bytes are deleted by the caller.
    Reject,
}

/// What adding a tag did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagOutcome {
    /// Added.
    Added,
    /// The file already had it.
    Already,
    /// The file has the most tags allowed.
    Full,
}

/// What adding to a download list did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOutcome {
    /// Added.
    Added,
    /// Already on the list.
    Already,
    /// The list has the most files allowed.
    Full,
}

/// The files a search, a queue or a listing returns, newest first.
pub trait FileStore: Send + Sync {
    /// Every area, with its approved-file count.
    fn areas(&self) -> impl Future<Output = Result<Vec<AreaRow>, StoreError>> + Send;

    /// One area.
    fn area(&self, id: AreaId) -> impl Future<Output = Result<Option<AreaRow>, StoreError>> + Send;

    /// Create an area and write the audit row in the same transaction. `None` when the name
    /// is taken (ignoring case).
    fn create_area(
        &self,
        new: &NewArea,
        actor: AccountId,
        now: UnixSeconds,
    ) -> impl Future<Output = Result<Option<AreaId>, StoreError>> + Send;

    /// Record a stored upload as a quarantined, unscanned file.
    fn insert_file(&self, new: &NewFile)
    -> impl Future<Output = Result<FileId, StoreError>> + Send;

    /// One file with its tags.
    fn file(&self, id: FileId) -> impl Future<Output = Result<Option<FileRow>, StoreError>> + Send;

    /// A page of an area's files in `state`, newest first.
    fn files(
        &self,
        area: AreaId,
        state: FileState,
        limit: u32,
        offset: u32,
    ) -> impl Future<Output = Result<Vec<FileRow>, StoreError>> + Send;

    /// A page of quarantined files in any of `areas`, oldest first (the approval queue).
    fn pending_in(
        &self,
        areas: &[AreaId],
        limit: u32,
        offset: u32,
    ) -> impl Future<Output = Result<Vec<FileRow>, StoreError>> + Send;

    /// The account's own uploads that are still in quarantine, oldest first.
    fn own_pending(
        &self,
        account: AccountId,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<FileRow>, StoreError>> + Send;

    /// Record the scanner's answer for a file still in quarantine. `false` if it is no longer.
    fn set_scan(
        &self,
        id: FileId,
        scan: ScanState,
        note: &Sanitized,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// Decide a file that is still in quarantine and write `audit` in the same transaction.
    /// `false` (and nothing written) if it is not in quarantine, so two deciders cannot both win.
    fn decide(
        &self,
        id: FileId,
        decision: &Decision,
        by: Actor,
        now: UnixSeconds,
        audit: &AuditEvent,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// Delete a file's row (its tags and list entries go with it) and write `audit`. `false` if
    /// there was none.
    fn delete_file(
        &self,
        id: FileId,
        audit: &AuditEvent,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// Add a tag unless the file already has `max` of them.
    fn add_tag(
        &self,
        id: FileId,
        tag: &str,
        max: u32,
    ) -> impl Future<Output = Result<TagOutcome, StoreError>> + Send;

    /// Remove a tag. `false` if the file did not have it.
    fn remove_tag(
        &self,
        id: FileId,
        tag: &str,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// Replace a file's description. `false` if there is no such file.
    fn set_description(
        &self,
        id: FileId,
        description: &Sanitized,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// Approved files in any of `areas` where **every** term appears in the name, the
    /// description or a tag (ignoring case). Terms are plain words (core's `SearchQuery`); the
    /// areas are part of the query, so a file in another area is never read.
    fn search(
        &self,
        areas: &[AreaId],
        terms: &[String],
        limit: u32,
        offset: u32,
    ) -> impl Future<Output = Result<Vec<FileRow>, StoreError>> + Send;

    /// Count a download that was opened.
    fn note_download(&self, id: FileId) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Add a file to the account's download list unless it already holds `max`.
    fn list_add(
        &self,
        account: AccountId,
        id: FileId,
        now: UnixSeconds,
        max: u32,
    ) -> impl Future<Output = Result<ListOutcome, StoreError>> + Send;

    /// Take a file off the account's list. `false` if it was not on it.
    fn list_remove(
        &self,
        account: AccountId,
        id: FileId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send;

    /// Empty the account's list.
    fn list_clear(&self, account: AccountId)
    -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The approved files on the account's list, in the order they were added.
    fn list_files(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Vec<FileRow>, StoreError>> + Send;

    /// The active account whose handle is `handle`, ignoring case.
    fn active_account_by_handle(
        &self,
        handle: &str,
    ) -> impl Future<Output = Result<Option<AccountId>, StoreError>> + Send;
}

/// The empty directory has no areas: reads find nothing, writes are refused.
impl FileStore for crate::access::NoMembers {
    fn areas(&self) -> impl Future<Output = Result<Vec<AreaRow>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn area(
        &self,
        _id: AreaId,
    ) -> impl Future<Output = Result<Option<AreaRow>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn create_area(
        &self,
        _new: &NewArea,
        _actor: AccountId,
        _now: UnixSeconds,
    ) -> impl Future<Output = Result<Option<AreaId>, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn insert_file(
        &self,
        _new: &NewFile,
    ) -> impl Future<Output = Result<FileId, StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }

    fn file(
        &self,
        _id: FileId,
    ) -> impl Future<Output = Result<Option<FileRow>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }

    fn files(
        &self,
        _area: AreaId,
        _state: FileState,
        _limit: u32,
        _offset: u32,
    ) -> impl Future<Output = Result<Vec<FileRow>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn pending_in(
        &self,
        _areas: &[AreaId],
        _limit: u32,
        _offset: u32,
    ) -> impl Future<Output = Result<Vec<FileRow>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn own_pending(
        &self,
        _account: AccountId,
        _limit: u32,
    ) -> impl Future<Output = Result<Vec<FileRow>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn set_scan(
        &self,
        _id: FileId,
        _scan: ScanState,
        _note: &Sanitized,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(false))
    }

    fn decide(
        &self,
        _id: FileId,
        _decision: &Decision,
        _by: Actor,
        _now: UnixSeconds,
        _audit: &AuditEvent,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(false))
    }

    fn delete_file(
        &self,
        _id: FileId,
        _audit: &AuditEvent,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(false))
    }

    fn add_tag(
        &self,
        _id: FileId,
        _tag: &str,
        _max: u32,
    ) -> impl Future<Output = Result<TagOutcome, StoreError>> + Send {
        std::future::ready(Ok(TagOutcome::Full))
    }

    fn remove_tag(
        &self,
        _id: FileId,
        _tag: &str,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(false))
    }

    fn set_description(
        &self,
        _id: FileId,
        _description: &Sanitized,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(false))
    }

    fn search(
        &self,
        _areas: &[AreaId],
        _terms: &[String],
        _limit: u32,
        _offset: u32,
    ) -> impl Future<Output = Result<Vec<FileRow>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn note_download(&self, _id: FileId) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Ok(()))
    }

    fn list_add(
        &self,
        _account: AccountId,
        _id: FileId,
        _now: UnixSeconds,
        _max: u32,
    ) -> impl Future<Output = Result<ListOutcome, StoreError>> + Send {
        std::future::ready(Ok(ListOutcome::Full))
    }

    fn list_remove(
        &self,
        _account: AccountId,
        _id: FileId,
    ) -> impl Future<Output = Result<bool, StoreError>> + Send {
        std::future::ready(Ok(false))
    }

    fn list_clear(
        &self,
        _account: AccountId,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Ok(()))
    }

    fn list_files(
        &self,
        _account: AccountId,
    ) -> impl Future<Output = Result<Vec<FileRow>, StoreError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    fn active_account_by_handle(
        &self,
        _handle: &str,
    ) -> impl Future<Output = Result<Option<AccountId>, StoreError>> + Send {
        std::future::ready(Ok(None))
    }
}
