//! Uploading (M4-F2): the rules between "a client wants to write a file" and "the file waits in
//! quarantine". The front end owns the bytes' transport (SFTP) and hands core the stream; core
//! owns the permission, the limits, the quarantine key, the scan and the auto-approve rule.
//!
//! The order of an upload:
//!
//! 1. [`plan_upload`]: the area is reachable, `files.upload` is held, the name is cleaned, the
//!    per-file and per-day limits are resolved, and a day already used up is refused **before** a
//!    byte is sent.
//! 2. [`UploadMeter::accept`] for every chunk the client sends, so a transfer over the per-file
//!    limit stops at the first byte past it and the per-day count rises as bytes arrive.
//! 3. The front end streams the same chunks through `BlobStore::put` to the quarantine key
//!    ([`UploadPlan::key`]), capped at [`UploadPlan::put_options`].
//! 4. [`record_upload`] writes the row, scans the stored bytes, and either leaves the file in
//!    quarantine, rejects it (scanner found something), or approves it (the area auto-approves
//!    and the scan did not fail).
//!
//! **The day's count is not refunded.** `add_usage` only adds, so bytes a transfer sent before
//! it failed, was refused or was rejected still count for the day; the count is also rounded up
//! to [`RESERVE_BLOCK`] per transfer (the limit is reserved in blocks to avoid a database write
//! per packet). Both are said where the limit is documented.

use std::time::Duration;

use crate::access::{Permission, Snapshot};
use crate::audit::Actor;
use crate::blobs::{BlobKey, BlobMeta, BlobStore, Namespace, PutOptions};
use crate::limits::{
    Day, Limit, LimitKind, LimitRefusal, LimitStore, SECONDS_PER_DAY, UsageOutcome, load_effective,
};
use crate::sanitize::SingleLine;
use crate::store::{AccountId, UnixSeconds};

use super::ids::{AreaId, FileId};
use super::rules;
use super::scan::{NoScan, ScanHook, ScanOutcome};
use super::service::{FileError, QUARANTINE, approve_row, forget_blob, reject_row};
use super::store::{AreaRow, FileState, FileStore, NewFile, ScanState};
use super::text::{FileName, prepare_file_name};

/// The day's allowance is taken from the database in blocks of this many bytes.
pub const RESERVE_BLOCK: u64 = 1024 * 1024;
/// A scan that takes longer than this is a failed scan.
pub const SCAN_TIMEOUT: Duration = Duration::from_secs(60);

/// A permitted upload, before any byte is sent.
#[derive(Debug, Clone)]
pub struct UploadPlan {
    /// Who is uploading.
    pub account: AccountId,
    /// The area.
    pub area: AreaRow,
    /// The cleaned name and the name as sent.
    pub name: FileName,
    /// The quarantine key the bytes go to.
    pub key: BlobKey,
    /// The per-file limit.
    pub per_file: Limit,
    /// The per-day limit.
    pub per_day: Limit,
    /// The UTC day the upload counts against.
    pub day: Day,
}

impl UploadPlan {
    /// The put options: no more than the per-file limit.
    #[must_use]
    pub const fn put_options(&self) -> PutOptions {
        PutOptions::up_to(match self.per_file {
            Limit::AtMost(n) => n,
            Limit::Unlimited => u64::MAX,
        })
    }

    /// A meter for this upload's bytes.
    #[must_use]
    pub const fn meter(&self) -> UploadMeter {
        UploadMeter {
            account: self.account,
            per_file: self.per_file,
            per_day: self.per_day,
            day: self.day,
            received: 0,
            reserved: 0,
        }
    }
}

fn refusal(kind: LimitKind, limit: u64, now: UnixSeconds, day: Day) -> FileError {
    FileError::Limit(LimitRefusal {
        kind,
        limit,
        resets_in: (kind == LimitKind::UploadBytesPerDay).then(|| {
            u64::try_from((day.end().as_raw() - now.as_raw()).clamp(0, SECONDS_PER_DAY))
                .unwrap_or(0)
        }),
    })
}

/// Check that the session may upload `raw_name` into `area`, and what limits apply.
///
/// # Errors
/// [`FileError`]: not signed in, area not found or not listable, not permitted, bad name, a day
/// already used up, or storage.
pub async fn plan_upload<S: FileStore + LimitStore>(
    store: &S,
    snapshot: &Snapshot,
    area: AreaId,
    raw_name: &str,
    now: UnixSeconds,
) -> Result<UploadPlan, FileError> {
    let account = snapshot.account().ok_or(FileError::NotSignedIn)?;
    let row = store.area(area).await?.ok_or(FileError::NotFound)?;
    if !rules::can(snapshot, Permission::FilesList, &row) {
        return Err(FileError::NotFound);
    }
    if !rules::can(snapshot, Permission::FilesUpload, &row) {
        return Err(FileError::NotPermitted(Permission::FilesUpload));
    }
    let name = prepare_file_name(raw_name)?;
    let limits = load_effective(store, account, snapshot).await?;
    let per_file = limits.get(LimitKind::UploadBytesPerFile);
    let per_day = limits.get(LimitKind::UploadBytesPerDay);
    let day = Day::of(now);
    if let Limit::AtMost(cap) = per_day {
        let used = store
            .usage(account, LimitKind::UploadBytesPerDay, day)
            .await?;
        if used >= cap {
            return Err(refusal(LimitKind::UploadBytesPerDay, cap, now, day));
        }
    }
    let namespace = Namespace::new(QUARANTINE)
        .map_err(|_| crate::store::StoreError::Corrupt("a blob namespace is invalid".into()))?;
    Ok(UploadPlan {
        account,
        area: row,
        name,
        key: BlobKey::generate(namespace),
        per_file,
        per_day,
        day,
    })
}

/// Counts an upload's bytes against the per-file and per-day limits as they arrive.
#[derive(Debug, Clone)]
pub struct UploadMeter {
    account: AccountId,
    per_file: Limit,
    per_day: Limit,
    day: Day,
    received: u64,
    reserved: u64,
}

impl UploadMeter {
    /// Bytes accepted so far.
    #[must_use]
    pub const fn received(&self) -> u64 {
        self.received
    }

    /// Account for `len` more bytes. On a refusal nothing is counted for them and the caller
    /// must fail the transfer.
    ///
    /// # Errors
    /// [`FileError::Limit`] naming the limit, or [`FileError::Store`].
    pub async fn accept<S: LimitStore>(
        &mut self,
        store: &S,
        now: UnixSeconds,
        len: usize,
    ) -> Result<(), FileError> {
        let len = u64::try_from(len).unwrap_or(u64::MAX);
        let total = self.received.saturating_add(len);
        if let Limit::AtMost(cap) = self.per_file
            && total > cap
        {
            return Err(refusal(LimitKind::UploadBytesPerFile, cap, now, self.day));
        }
        if total > self.reserved {
            let need = total - self.reserved;
            let cap = self.per_day.cap();
            let mut taken = need.max(RESERVE_BLOCK);
            let mut outcome = store
                .add_usage(
                    self.account,
                    LimitKind::UploadBytesPerDay,
                    self.day,
                    taken,
                    cap,
                )
                .await?;
            if matches!(outcome, UsageOutcome::Full(_)) && taken > need {
                taken = need;
                outcome = store
                    .add_usage(
                        self.account,
                        LimitKind::UploadBytesPerDay,
                        self.day,
                        taken,
                        cap,
                    )
                    .await?;
            }
            if matches!(outcome, UsageOutcome::Full(_)) {
                return Err(refusal(
                    LimitKind::UploadBytesPerDay,
                    cap.unwrap_or(0),
                    now,
                    self.day,
                ));
            }
            self.reserved += taken;
        }
        self.received = total;
        Ok(())
    }
}

/// What became of a finished upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uploaded {
    /// The new file.
    pub id: FileId,
    /// Where it ended up: quarantine, approved (auto-approve), or rejected (the scanner).
    pub state: FileState,
    /// What the scanner said.
    pub scan: ScanState,
    /// The scanner's note, cleaned.
    pub note: String,
}

/// Record a stored upload, scan it and apply the area's rule. `meta` is what `put` returned for
/// the bytes at `plan.key`. On any error before the row exists the quarantine blob is deleted.
///
/// # Errors
/// [`FileError`] when the row could not be written, or an approve or reject step failed (the
/// file then stays in quarantine for staff).
pub async fn record_upload<S: FileStore, B: BlobStore, H: ScanHook>(
    store: &S,
    blobs: &B,
    scanner: &H,
    plan: &UploadPlan,
    meta: &BlobMeta,
    now: UnixSeconds,
) -> Result<Uploaded, FileError> {
    let new = NewFile {
        area: plan.area.id,
        uploader: plan.account,
        name: plan.name.display.clone(),
        original_name: plan.name.original.clone(),
        size: meta.size,
        sha256: meta.sha256.to_hex(),
        blob_key: plan.key.to_string(),
        now,
    };
    let id = match store.insert_file(&new).await {
        Ok(id) => id,
        Err(error) => {
            forget_blob(blobs, &plan.key).await;
            return Err(error.into());
        }
    };
    let (scan, note) = run_scan(blobs, scanner, &plan.key, meta.size).await;
    let note_clean = SingleLine::new(120).sanitize(&note);
    store.set_scan(id, scan, &note_clean).await?;
    let mut row = store.file(id).await?.ok_or(FileError::NotFound)?;
    row.scan = scan;
    let system = Actor::System;
    let state = match scan {
        ScanState::Infected => {
            reject_row(store, blobs, &row, system, now, "infected").await?;
            FileState::Rejected
        }
        ScanState::Unscanned | ScanState::Clean if plan.area.auto_approve => {
            approve_row(store, blobs, &row, system, now).await?;
            FileState::Approved
        }
        ScanState::Failed | ScanState::Unscanned | ScanState::Clean => FileState::Quarantine,
    };
    Ok(Uploaded {
        id,
        state,
        scan,
        note: note_clean.into_string(),
    })
}

async fn run_scan<B: BlobStore, H: ScanHook>(
    blobs: &B,
    scanner: &H,
    key: &BlobKey,
    size: u64,
) -> (ScanState, String) {
    let read = match blobs.get(key).await {
        Ok(read) => read,
        Err(error) => {
            return (
                ScanState::Failed,
                format!("could not read the upload: {error}"),
            );
        }
    };
    match tokio::time::timeout(SCAN_TIMEOUT, scanner.scan(size, read.reader)).await {
        Ok(ScanOutcome::Skipped) => (ScanState::Unscanned, String::new()),
        Ok(ScanOutcome::Clean) => (ScanState::Clean, String::new()),
        Ok(ScanOutcome::Infected(name)) => (ScanState::Infected, name),
        Ok(ScanOutcome::Failed(why)) => (ScanState::Failed, why),
        Err(_) => (ScanState::Failed, "the scan timed out".to_owned()),
    }
}

/// A scanner that never runs, for callers that have none.
#[must_use]
pub const fn no_scan() -> NoScan {
    NoScan
}
