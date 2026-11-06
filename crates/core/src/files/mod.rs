//! File areas: areas, files, quarantine, approval, tags, search and download lists (M4-F2,
//! D-TBD). The bytes live in the blob store (D30, D106); this module owns every rule about them.
//!
//! * [`FileStore`] is persistence only (both backends, one shared test suite). A front end
//!   (the SFTP subsystem, the TUI) calls [`service`] and [`transfer`] with the session's
//!   [`Snapshot`](crate::access::Snapshot), never the store directly.
//! * **Areas are access resources** (`ResourceKind::FileArea`). A listing asks
//!   [`Snapshot::reach`](crate::access::Snapshot::reach); by-id access answers
//!   [`FileError::NotFound`] the same for an area or file that does not exist and one the member
//!   may not see. A **restricted** area is not reached through a global holding of `files.*`;
//!   it needs a grant scoped to it or `files.manage_areas` ([`rules`]).
//! * **Quarantine.** An upload is stored under the `quarantine` blob namespace and is visible
//!   only to its uploader and to those who hold `files.approve` on the area until it is approved
//!   (staff, or the area's auto-approve after a scan that did not fail). Approving copies the
//!   bytes into the `files` namespace through the blob store (verifying the hash) and deletes the
//!   quarantine copy; rejecting deletes the bytes and keeps the row.
//! * **Scanning** is a hook ([`ScanHook`]) whose default does nothing; an unscanned file is
//!   labelled unscanned everywhere it is shown.
//! * **The virtual file system** SFTP serves is parsed by [`path`]: ids only, no real paths.
//! * **Text.** Names, descriptions and tags are member text: cleaned by the shared sanitizer in
//!   [`text`], again on read in [`service`]; the name as the client sent it is kept only as data.
//!
//! Nothing here is "private": the server can read every file unless a member encrypted it
//! themselves before uploading (D3).

mod ids;
pub mod keys;
pub mod path;
pub mod rules;
pub mod scan;
pub mod screen;
pub mod service;
mod store;
pub mod text;
pub mod transfer;

pub use ids::{AreaId, FileId};
pub use scan::{NoScan, ScanHook, ScanOutcome};
pub use screen::{FilesMode, FilesScreen, Note, Request, Response, execute};
pub use service::{
    AreaSummary, DownloadTarget, FileError, FilePage, FileSummary, FileView, LISTING_MAX, LIVE,
    MAX_DOWNLOAD_LIST, PAGE_SIZE, QUARANTINE, allow_member, approve, area_page, areas,
    can_approve_anywhere, create_area, describe, file_view, list_add, list_clear, list_files,
    list_remove, listing, open_download, pending_listing, queue, reject, remove, search, tag,
    untag,
};
pub use store::{
    AreaRow, Decision, FileRow, FileState, FileStore, ListOutcome, NewArea, NewFile, ScanState,
    TagOutcome,
};
pub use text::{
    AREA_DESCRIPTION_MAX_CHARS, AREA_NAME_MAX_CHARS, FILE_DESCRIPTION_MAX_CHARS,
    FILE_NAME_MAX_CHARS, FileName, MAX_TAGS_PER_FILE, TAG_MAX_CHARS, TextError,
    prepare_area_description, prepare_area_name, prepare_file_description, prepare_file_name,
    prepare_tag,
};
pub use transfer::{
    RESERVE_BLOCK, SCAN_TIMEOUT, UploadMeter, UploadPlan, Uploaded, plan_upload, record_upload,
};

#[cfg(test)]
mod tests;
