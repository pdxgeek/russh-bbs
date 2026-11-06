//! What a signed-in session can do with file areas. Every function takes the session's
//! [`Snapshot`] (loaded for this action) and checks it in this order: the area is reachable
//! (otherwise [`FileError::NotFound`], the same answer as for an area or file that does not
//! exist, so it reveals nothing), the permission is held (otherwise
//! [`FileError::NotPermitted`]), then the limit, then the write.
//!
//! A file in quarantine is visible to its uploader and to those who hold `files.approve` on its
//! area, and to nobody else; a rejected file is visible to nobody.

use crate::access::{
    AccessError, AccessStore, Granting, NewGrant, Permission, Scope, Snapshot, Subject,
};
use crate::audit::{Action, Actor, events};
use crate::blobs::{BlobError, BlobHash, BlobKey, BlobStore, Namespace, PutOptions};
use crate::limits::LimitRefusal;
use crate::sanitize::{Sanitized, SingleLine};
use crate::store::{AccountId, StoreError, UnixSeconds};

use super::ids::{AreaId, FileId};
use super::path;
use super::rules;
use super::store::{
    AreaRow, Decision, FileRow, FileState, FileStore, ListOutcome, NewArea, ScanState, TagOutcome,
};
use super::text::{
    MAX_TAGS_PER_FILE, TextError, prepare_area_description, prepare_area_name,
    prepare_file_description, prepare_tag,
};

/// Files on a page of a listing, a search or the queue.
pub const PAGE_SIZE: u32 = 12;
/// Most files on a download list.
pub const MAX_DOWNLOAD_LIST: u32 = 100;
/// Most entries an SFTP directory listing returns.
pub const LISTING_MAX: u32 = 500;
/// The blob namespace of files waiting for a decision.
pub const QUARANTINE: &str = "quarantine";
/// The blob namespace of live files.
pub const LIVE: &str = "files";

/// Why a file action did not happen.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    /// No such area or file, **or** the session may not see it: one answer for both.
    #[error("not found")]
    NotFound,
    /// The session can see the thing but may not do this to it.
    #[error("you may not do that")]
    NotPermitted(Permission),
    /// The session is not a signed-in account.
    #[error("not signed in")]
    NotSignedIn,
    /// A limit is used up.
    #[error("{0}")]
    Limit(LimitRefusal),
    /// The typed text was refused.
    #[error(transparent)]
    Text(#[from] TextError),
    /// An area with that name exists.
    #[error("an area with that name already exists")]
    NameTaken,
    /// No active member has that handle.
    #[error("no such member")]
    UnknownMember,
    /// The file has the most tags allowed.
    #[error("a file may have at most {MAX_TAGS_PER_FILE} tags")]
    TooManyTags,
    /// The download list is full.
    #[error("your download list holds at most {MAX_DOWNLOAD_LIST} files")]
    ListFull,
    /// The file was already decided.
    #[error("that file has already been decided")]
    NotPending,
    /// The blob store failed or refused.
    #[error("the file store failed: {0}")]
    Blob(#[from] BlobError),
    /// Storage failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

fn clean(text: &str, max: usize) -> String {
    SingleLine::new(max).sanitize(text).into_string()
}

/// An area in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaSummary {
    /// The area.
    pub id: AreaId,
    /// Its name.
    pub name: String,
    /// What it is for.
    pub description: String,
    /// Whether it needs a grant scoped to it.
    pub restricted: bool,
    /// Whether clean uploads skip approval.
    pub auto_approve: bool,
    /// Approved files in it.
    pub files: u64,
    /// The directory it appears under over SFTP.
    pub dir: String,
}

impl AreaSummary {
    fn of(row: &AreaRow) -> Self {
        let name = clean(&row.name, super::text::AREA_NAME_MAX_CHARS);
        Self {
            id: row.id,
            dir: path::area_dir_name(row.id, &name),
            name,
            description: clean(&row.description, super::text::AREA_DESCRIPTION_MAX_CHARS),
            restricted: row.restricted,
            auto_approve: row.auto_approve,
            files: row.files,
        }
    }
}

/// A file in a list: everything shown on a row, cleaned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSummary {
    /// The file.
    pub id: FileId,
    /// Its area.
    pub area: AreaId,
    /// The area's name.
    pub area_name: String,
    /// The display name.
    pub name: String,
    /// Size in bytes.
    pub size: u64,
    /// The uploader's handle.
    pub uploader: Option<String>,
    /// When it was uploaded.
    pub at: UnixSeconds,
    /// Where it is in its life.
    pub state: FileState,
    /// Tags.
    pub tags: Vec<String>,
    /// Downloads opened.
    pub downloads: u64,
}

impl FileSummary {
    /// Clean a stored row.
    #[must_use]
    pub fn of(row: &FileRow) -> Self {
        Self {
            id: row.id,
            area: row.area,
            area_name: clean(&row.area_name, super::text::AREA_NAME_MAX_CHARS),
            name: clean(&row.name, super::text::FILE_NAME_MAX_CHARS),
            size: row.size,
            uploader: row.uploader.as_deref().map(|u| clean(u, 64)),
            at: row.created_at,
            state: row.state,
            tags: row.tags.iter().map(|t| clean(t, 24)).collect(),
            downloads: row.downloads,
        }
    }
}

/// One file in full.
// The flags are independent facts the screen reads (what the session may do here).
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileView {
    /// The row summary.
    pub file: FileSummary,
    /// The description, cleaned.
    pub description: Sanitized,
    /// SHA-256 of the content (64 hex characters).
    pub sha256: String,
    /// What the scanner said.
    pub scan: ScanState,
    /// The scanner's note, cleaned.
    pub scan_note: String,
    /// The SFTP path to fetch it by, when the session may download it.
    pub download_path: Option<String>,
    /// Whether the session may tag it.
    pub can_tag: bool,
    /// Whether the session may change its description (its uploader, or an approver).
    pub can_edit: bool,
    /// Whether the session may approve, reject or remove it.
    pub can_decide: bool,
    /// Whether it is on the session's download list.
    pub on_list: bool,
}

/// One page of files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePage {
    /// The area, when the page is one area's.
    pub area: Option<AreaSummary>,
    /// The files.
    pub files: Vec<FileSummary>,
    /// This page, from zero.
    pub page: u32,
    /// Whether a later page exists.
    pub more: bool,
    /// Whether the session may upload here.
    pub can_upload: bool,
    /// Whether the session may create areas.
    pub can_manage: bool,
}

fn account_of(snapshot: &Snapshot) -> Result<AccountId, FileError> {
    snapshot.account().ok_or(FileError::NotSignedIn)
}

async fn load_area<S: FileStore>(store: &S, id: AreaId) -> Result<AreaRow, FileError> {
    store.area(id).await?.ok_or(FileError::NotFound)
}

/// The area, if the session may list it: the one gate for looking inside an area.
async fn listable_area<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    id: AreaId,
) -> Result<AreaRow, FileError> {
    let area = load_area(store, id).await?;
    if rules::can(snapshot, Permission::FilesList, &area) {
        Ok(area)
    } else {
        Err(FileError::NotFound)
    }
}

/// The areas the session may list.
///
/// # Errors
/// [`FileError::Store`] when storage fails.
pub async fn areas<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
) -> Result<Vec<AreaSummary>, FileError> {
    let all = store.areas().await?;
    Ok(rules::reachable(snapshot, Permission::FilesList, &all)
        .into_iter()
        .map(AreaSummary::of)
        .collect())
}

/// A page of an area's approved files.
///
/// # Errors
/// [`FileError::NotFound`] for an area that is absent or not listable by the session.
pub async fn area_page<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    id: AreaId,
    page: u32,
) -> Result<FilePage, FileError> {
    let area = listable_area(store, snapshot, id).await?;
    let offset = page.saturating_mul(PAGE_SIZE);
    let mut rows = store
        .files(id, FileState::Approved, PAGE_SIZE + 1, offset)
        .await?;
    let more = rows.len() > PAGE_SIZE as usize;
    rows.truncate(PAGE_SIZE as usize);
    Ok(FilePage {
        area: Some(AreaSummary::of(&area)),
        files: rows.iter().map(FileSummary::of).collect(),
        page,
        more,
        can_upload: rules::can(snapshot, Permission::FilesUpload, &area),
        can_manage: snapshot
            .resolve(Permission::FilesManageAreas, None)
            .is_granted(),
    })
}

/// Why a file may be seen by the session, if it may.
struct Access {
    area: AreaRow,
    uploader: bool,
    approver: bool,
}

/// Load a file and decide whether the session may see it. Everything else answers
/// [`FileError::NotFound`].
async fn visible<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    id: FileId,
) -> Result<(FileRow, Access), FileError> {
    let row = store.file(id).await?.ok_or(FileError::NotFound)?;
    let area = load_area(store, row.area).await?;
    let account = snapshot.account();
    let uploader = account.is_some() && row.uploader_id == account;
    let approver = rules::can(snapshot, Permission::FilesApprove, &area);
    let listable = rules::can(snapshot, Permission::FilesList, &area);
    let seen = match row.state {
        FileState::Approved => listable,
        FileState::Quarantine => approver || (uploader && listable),
        FileState::Rejected => false,
    };
    if seen {
        Ok((
            row,
            Access {
                area,
                uploader,
                approver,
            },
        ))
    } else {
        Err(FileError::NotFound)
    }
}

/// One file in full.
///
/// # Errors
/// [`FileError::NotFound`] for a file that is absent or not visible to the session.
pub async fn file_view<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    id: FileId,
) -> Result<FileView, FileError> {
    let (row, access) = visible(store, snapshot, id).await?;
    let on_list = match snapshot.account() {
        Some(account) => store
            .list_files(account)
            .await?
            .iter()
            .any(|f| f.id == row.id),
        None => false,
    };
    let approved = row.state == FileState::Approved;
    let summary = FileSummary::of(&row);
    let download_path = (approved && rules::can(snapshot, Permission::FilesDownload, &access.area))
        .then(|| {
            path::download_path(
                row.area,
                &clean(&access.area.name, super::text::AREA_NAME_MAX_CHARS),
                row.id,
                &summary.name,
            )
        });
    Ok(FileView {
        description: crate::sanitize::SingleLine::new(super::text::FILE_DESCRIPTION_MAX_CHARS)
            .sanitize(&row.description),
        sha256: row.sha256.clone(),
        scan: row.scan,
        scan_note: clean(&row.scan_note, 120),
        download_path,
        can_tag: approved && rules::can(snapshot, Permission::FilesTag, &access.area),
        can_edit: access.uploader || access.approver,
        can_decide: access.approver,
        on_list,
        file: summary,
    })
}

/// Search the approved files in the areas the session may list.
///
/// # Errors
/// [`FileError::Store`] when storage fails.
pub async fn search<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    raw: &str,
    page: u32,
) -> Result<FilePage, FileError> {
    let Some(query) = crate::forums::SearchQuery::parse(raw) else {
        return Ok(FilePage {
            area: None,
            files: Vec::new(),
            page,
            more: false,
            can_upload: false,
            can_manage: false,
        });
    };
    let all = store.areas().await?;
    let ids = rules::reachable_ids(snapshot, Permission::FilesList, &all);
    if ids.is_empty() {
        return Ok(FilePage {
            area: None,
            files: Vec::new(),
            page,
            more: false,
            can_upload: false,
            can_manage: false,
        });
    }
    let offset = page.saturating_mul(PAGE_SIZE);
    let mut rows = store
        .search(&ids, query.terms(), PAGE_SIZE + 1, offset)
        .await?;
    let more = rows.len() > PAGE_SIZE as usize;
    rows.truncate(PAGE_SIZE as usize);
    Ok(FilePage {
        area: None,
        files: rows.iter().map(FileSummary::of).collect(),
        page,
        more,
        can_upload: false,
        can_manage: false,
    })
}

/// A page of the approval queue: quarantined files in areas the session may approve in.
///
/// # Errors
/// [`FileError::Store`] when storage fails. A session that approves nowhere gets an empty page.
pub async fn queue<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    page: u32,
) -> Result<FilePage, FileError> {
    let all = store.areas().await?;
    let ids = rules::reachable_ids(snapshot, Permission::FilesApprove, &all);
    let offset = page.saturating_mul(PAGE_SIZE);
    let mut rows = if ids.is_empty() {
        Vec::new()
    } else {
        store.pending_in(&ids, PAGE_SIZE + 1, offset).await?
    };
    let more = rows.len() > PAGE_SIZE as usize;
    rows.truncate(PAGE_SIZE as usize);
    Ok(FilePage {
        area: None,
        files: rows.iter().map(FileSummary::of).collect(),
        page,
        more,
        can_upload: false,
        can_manage: false,
    })
}

/// Whether the session approves in at least one area (the queue is worth showing).
#[must_use]
pub fn can_approve_anywhere(snapshot: &Snapshot) -> bool {
    snapshot.reach(
        Permission::FilesApprove,
        crate::access::ResourceKind::FileArea,
    ) != crate::access::Reach::None
}

/// Create an area (`files.manage_areas`).
///
/// # Errors
/// [`FileError`]: not permitted, bad text, name taken, or storage.
pub async fn create_area<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    now: UnixSeconds,
    name: &str,
    description: &str,
    restricted: bool,
    auto_approve: bool,
) -> Result<AreaId, FileError> {
    let actor = account_of(snapshot)?;
    snapshot
        .require(Permission::FilesManageAreas, None)
        .map_err(|_| FileError::NotPermitted(Permission::FilesManageAreas))?;
    let new = NewArea {
        name: prepare_area_name(name)?,
        description: prepare_area_description(description)?,
        restricted,
        auto_approve,
    };
    store
        .create_area(&new, actor, now)
        .await?
        .ok_or(FileError::NameTaken)
}

/// The permissions an allow entry on an area gives: to list it, fetch from it, add to it.
const ALLOWED: [Permission; 3] = [
    Permission::FilesList,
    Permission::FilesDownload,
    Permission::FilesUpload,
];

/// Let the member with `handle` into `area` (`files.manage_areas`): three allow entries scoped
/// to the area, each audited as `permission.granted`. Already allowed is not an error.
///
/// # Errors
/// [`FileError`]: not found, not permitted, no such member, or storage.
pub async fn allow_member<S: FileStore + AccessStore>(
    store: &S,
    snapshot: &Snapshot,
    area: AreaId,
    handle: &str,
) -> Result<(), FileError> {
    let actor = account_of(snapshot)?;
    snapshot
        .require(Permission::FilesManageAreas, None)
        .map_err(|_| FileError::NotPermitted(Permission::FilesManageAreas))?;
    let row = load_area(store, area).await?;
    let resource = row.id.resource().ok_or(FileError::NotFound)?;
    for permission in ALLOWED {
        if !snapshot.resolve(permission, Some(resource)).is_granted() {
            return Err(FileError::NotPermitted(permission));
        }
    }
    let member = store
        .active_account_by_handle(handle.trim())
        .await?
        .ok_or(FileError::UnknownMember)?;
    for permission in ALLOWED {
        let grant = NewGrant {
            subject: Subject::Account(member),
            granting: Granting::Permission(permission),
            scope: Scope::Resource(resource),
        };
        store
            .add_grant(&grant, Actor::Account(actor))
            .await
            .map_err(|error| match error {
                AccessError::Store(error) => FileError::Store(error),
                AccessError::UnknownAccount => FileError::UnknownMember,
                _ => FileError::Store(StoreError::Corrupt("a grant was refused".into())),
            })?;
    }
    Ok(())
}

/// Add a tag to an approved file (`files.tag` on its area).
///
/// # Errors
/// [`FileError`]: not found, not permitted, bad tag, too many tags, or storage.
pub async fn tag<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    id: FileId,
    raw: &str,
) -> Result<(), FileError> {
    account_of(snapshot)?;
    let (row, access) = visible(store, snapshot, id).await?;
    if row.state != FileState::Approved || !rules::can(snapshot, Permission::FilesTag, &access.area)
    {
        return Err(FileError::NotPermitted(Permission::FilesTag));
    }
    let tag = prepare_tag(raw)?;
    match store.add_tag(id, &tag, MAX_TAGS_PER_FILE).await? {
        TagOutcome::Added | TagOutcome::Already => Ok(()),
        TagOutcome::Full => Err(FileError::TooManyTags),
    }
}

/// Remove a tag (`files.tag` on its area).
///
/// # Errors
/// [`FileError`]: not found, not permitted, or storage.
pub async fn untag<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    id: FileId,
    raw: &str,
) -> Result<(), FileError> {
    account_of(snapshot)?;
    let (row, access) = visible(store, snapshot, id).await?;
    if row.state != FileState::Approved || !rules::can(snapshot, Permission::FilesTag, &access.area)
    {
        return Err(FileError::NotPermitted(Permission::FilesTag));
    }
    store.remove_tag(id, &prepare_tag(raw)?).await?;
    Ok(())
}

/// Change a file's description: its uploader, or an approver of its area.
///
/// # Errors
/// [`FileError`]: not found, not permitted, bad text, or storage.
pub async fn describe<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    id: FileId,
    raw: &str,
) -> Result<(), FileError> {
    account_of(snapshot)?;
    let (_row, access) = visible(store, snapshot, id).await?;
    if !(access.uploader || access.approver) {
        return Err(FileError::NotPermitted(Permission::FilesApprove));
    }
    store
        .set_description(id, &prepare_file_description(raw)?)
        .await?;
    Ok(())
}

fn blob_key(text: &str, namespace: &str) -> Result<BlobKey, FileError> {
    let key =
        BlobKey::parse(text).map_err(|_| StoreError::Corrupt("a blob key is invalid".into()))?;
    let namespace = Namespace::new(namespace)
        .map_err(|_| StoreError::Corrupt("a blob namespace is invalid".into()))?;
    Ok(key.in_namespace(namespace))
}

fn hash_of(hex: &str) -> Result<BlobHash, StoreError> {
    let bad = || StoreError::Corrupt("a stored hash is invalid".into());
    if hex.len() != 64 {
        return Err(bad());
    }
    let mut bytes = [0_u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2).ok_or_else(bad)?, 16)
            .map_err(|_| bad())?;
    }
    Ok(BlobHash::from_bytes(bytes))
}

/// Delete a blob; a failure is logged and leaves an orphan the sysop's blob sweep can find.
pub(crate) async fn forget_blob<B: BlobStore>(blobs: &B, key: &BlobKey) {
    if let Err(error) = blobs.delete(key).await {
        tracing::warn!(%error, "a blob could not be deleted");
    }
}

/// Move a quarantined file's bytes into the live namespace and decide it, atomically with the
/// audit row. The caller has checked who may do this.
pub(crate) async fn approve_row<S: FileStore, B: BlobStore>(
    store: &S,
    blobs: &B,
    row: &FileRow,
    by: Actor,
    now: UnixSeconds,
) -> Result<(), FileError> {
    let old = blob_key(&row.blob_key, QUARANTINE)?;
    let new = blob_key(&row.blob_key, LIVE)?;
    let read = blobs.get(&old).await?;
    let options = PutOptions::exactly(row.size).with_sha256(hash_of(&row.sha256)?);
    match blobs.put(&new, read.reader, &options).await {
        Ok(_) | Err(BlobError::AlreadyExists) => {}
        Err(error) => return Err(error.into()),
    }
    let area = row.area.resource();
    let audit = events::file_event(
        by,
        Action::FileApproved,
        area,
        row.uploader_id,
        row.id.as_raw(),
        "",
    );
    let decided = store
        .decide(
            row.id,
            &Decision::Approve {
                blob_key: new.to_string(),
            },
            by,
            now,
            &audit,
        )
        .await;
    match decided {
        Ok(true) => {
            forget_blob(blobs, &old).await;
            Ok(())
        }
        Ok(false) => {
            forget_blob(blobs, &new).await;
            Err(FileError::NotPending)
        }
        Err(error) => {
            forget_blob(blobs, &new).await;
            Err(error.into())
        }
    }
}

/// Refuse a quarantined file and delete its bytes. The caller has checked who may do this.
pub(crate) async fn reject_row<S: FileStore, B: BlobStore>(
    store: &S,
    blobs: &B,
    row: &FileRow,
    by: Actor,
    now: UnixSeconds,
    note: &str,
) -> Result<(), FileError> {
    let old = blob_key(&row.blob_key, QUARANTINE)?;
    let audit = events::file_event(
        by,
        Action::FileRejected,
        row.area.resource(),
        row.uploader_id,
        row.id.as_raw(),
        note,
    );
    if !store
        .decide(row.id, &Decision::Reject, by, now, &audit)
        .await?
    {
        return Err(FileError::NotPending);
    }
    forget_blob(blobs, &old).await;
    Ok(())
}

async fn decider<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    id: FileId,
) -> Result<(AccountId, FileRow), FileError> {
    let actor = account_of(snapshot)?;
    let (row, access) = visible(store, snapshot, id).await?;
    if !access.approver {
        return Err(FileError::NotPermitted(Permission::FilesApprove));
    }
    if row.state != FileState::Quarantine {
        return Err(FileError::NotPending);
    }
    Ok((actor, row))
}

/// Approve a quarantined file (`files.approve` on its area).
///
/// # Errors
/// [`FileError`]: not found, not permitted, already decided, or the blob store or storage failed.
pub async fn approve<S: FileStore, B: BlobStore>(
    store: &S,
    blobs: &B,
    snapshot: &Snapshot,
    id: FileId,
    now: UnixSeconds,
) -> Result<(), FileError> {
    let (actor, row) = decider(store, snapshot, id).await?;
    approve_row(store, blobs, &row, Actor::Account(actor), now).await
}

/// Reject a quarantined file (`files.approve` on its area).
///
/// # Errors
/// As for [`approve`].
pub async fn reject<S: FileStore, B: BlobStore>(
    store: &S,
    blobs: &B,
    snapshot: &Snapshot,
    id: FileId,
    now: UnixSeconds,
) -> Result<(), FileError> {
    let (actor, row) = decider(store, snapshot, id).await?;
    reject_row(store, blobs, &row, Actor::Account(actor), now, "").await
}

/// Remove a file: an approver of its area may remove any visible file; an uploader may remove
/// their own while it is still in quarantine.
///
/// # Errors
/// [`FileError`]: not found, not permitted, or the blob store or storage failed.
pub async fn remove<S: FileStore, B: BlobStore>(
    store: &S,
    blobs: &B,
    snapshot: &Snapshot,
    id: FileId,
) -> Result<(), FileError> {
    let actor = account_of(snapshot)?;
    let (row, access) = visible(store, snapshot, id).await?;
    let own_pending = access.uploader && row.state == FileState::Quarantine;
    if !(access.approver || own_pending) {
        return Err(FileError::NotPermitted(Permission::FilesApprove));
    }
    let namespace = match row.state {
        FileState::Quarantine => QUARANTINE,
        FileState::Approved | FileState::Rejected => LIVE,
    };
    let key = blob_key(&row.blob_key, namespace)?;
    let audit = events::file_event(
        Actor::Account(actor),
        Action::FileRemoved,
        row.area.resource(),
        row.uploader_id,
        row.id.as_raw(),
        "",
    );
    if !store.delete_file(id, &audit).await? {
        return Err(FileError::NotFound);
    }
    forget_blob(blobs, &key).await;
    Ok(())
}

/// What an opened download is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadTarget {
    /// The file row.
    pub file: FileRow,
    /// Where its bytes are.
    pub key: BlobKey,
}

/// Open a file for download. An approved file needs `files.download` on its area; a file in
/// quarantine is readable by its uploader and by approvers of its area (to review it), and by no
/// one else. Everything else is [`FileError::NotFound`], the same as a file that does not exist.
///
/// # Errors
/// [`FileError::NotFound`], or storage.
pub async fn open_download<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    id: FileId,
) -> Result<DownloadTarget, FileError> {
    account_of(snapshot)?;
    let (row, access) = visible(store, snapshot, id).await?;
    let allowed = match row.state {
        FileState::Approved => rules::can(snapshot, Permission::FilesDownload, &access.area),
        FileState::Quarantine => access.approver || access.uploader,
        FileState::Rejected => false,
    };
    if !allowed {
        return Err(FileError::NotFound);
    }
    let namespace = if row.state == FileState::Approved {
        LIVE
    } else {
        QUARANTINE
    };
    let key = blob_key(&row.blob_key, namespace)?;
    if row.state == FileState::Approved {
        store.note_download(id).await?;
    }
    Ok(DownloadTarget { file: row, key })
}

/// The approved files of an area for an SFTP directory listing (`files.list` on the area).
///
/// # Errors
/// [`FileError::NotFound`] for an area the session cannot list.
pub async fn listing<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    id: AreaId,
) -> Result<Vec<FileRow>, FileError> {
    listable_area(store, snapshot, id).await?;
    Ok(store.files(id, FileState::Approved, LISTING_MAX, 0).await?)
}

/// The quarantined files the session may see in an area, for the `pending` directory: its own,
/// or all of them for an approver.
///
/// # Errors
/// [`FileError::NotFound`] for an area the session cannot list.
pub async fn pending_listing<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    id: AreaId,
) -> Result<Vec<FileRow>, FileError> {
    let account = account_of(snapshot)?;
    let area = listable_area(store, snapshot, id).await?;
    if rules::can(snapshot, Permission::FilesApprove, &area) {
        return Ok(store.pending_in(&[id], LISTING_MAX, 0).await?);
    }
    Ok(store
        .own_pending(account, LISTING_MAX)
        .await?
        .into_iter()
        .filter(|f| f.area == id)
        .collect())
}

/// Add an approved file to the download list (`files.download` on its area).
///
/// # Errors
/// [`FileError`]: not found, list full, or storage.
pub async fn list_add<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    id: FileId,
    now: UnixSeconds,
) -> Result<(), FileError> {
    let account = account_of(snapshot)?;
    let (row, access) = visible(store, snapshot, id).await?;
    if row.state != FileState::Approved
        || !rules::can(snapshot, Permission::FilesDownload, &access.area)
    {
        return Err(FileError::NotFound);
    }
    match store.list_add(account, id, now, MAX_DOWNLOAD_LIST).await? {
        ListOutcome::Added | ListOutcome::Already => Ok(()),
        ListOutcome::Full => Err(FileError::ListFull),
    }
}

/// Take a file off the download list.
///
/// # Errors
/// [`FileError::Store`] when storage fails.
pub async fn list_remove<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
    id: FileId,
) -> Result<(), FileError> {
    let account = account_of(snapshot)?;
    store.list_remove(account, id).await?;
    Ok(())
}

/// Empty the download list.
///
/// # Errors
/// [`FileError::Store`] when storage fails.
pub async fn list_clear<S: FileStore>(store: &S, snapshot: &Snapshot) -> Result<(), FileError> {
    let account = account_of(snapshot)?;
    store.list_clear(account).await?;
    Ok(())
}

/// The download list, limited to files the session may **still** download: access is checked
/// again here, so a file in an area the member lost is not listed (and not served).
///
/// # Errors
/// [`FileError::Store`] when storage fails.
pub async fn list_files<S: FileStore>(
    store: &S,
    snapshot: &Snapshot,
) -> Result<Vec<FileRow>, FileError> {
    let account = account_of(snapshot)?;
    let rows = store.list_files(account).await?;
    let areas = store.areas().await?;
    let ids = rules::reachable_ids(snapshot, Permission::FilesDownload, &areas);
    Ok(rows.into_iter().filter(|f| ids.contains(&f.area)).collect())
}
