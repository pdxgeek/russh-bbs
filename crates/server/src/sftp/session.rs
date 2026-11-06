//! One SFTP session: the protocol handler. See the module docs of [`super`].
#![allow(
    clippy::unused_async_trait_impl,
    reason = "the Handler trait's methods are all async; some of ours need no await"
)]

use std::collections::HashMap;
use std::sync::Arc;

use russh_bbs_core::access::Snapshot;
use russh_bbs_core::blobs::{BlobError, BlobKey, BlobMeta, BlobStore};
use russh_bbs_core::files::path::{self, Entry, VPath};
use russh_bbs_core::files::{
    self, AreaId, FileError, FileId, FileState, FileSummary, UploadMeter, UploadPlan,
};
use russh_bbs_core::limits::{Clock, SystemClock};
use russh_bbs_core::store::{AccountId, UnixSeconds};
use russh_bbs_store::blobs::BlobsReader;
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, FileMode, Handle, Name, OpenFlags, Status, StatusCode,
    Version,
};
use russh_sftp::server::{Handler, StatusReply};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use super::FileServices;
use super::upload::{Chunk, ChunkReader};
use crate::ssh::Backend;

/// Open handles one session may hold.
const MAX_HANDLES: usize = 16;
/// Most bytes one read reply carries.
const READ_CHUNK: usize = 128 * 1024;
/// Directory entries per `readdir` reply.
const DIR_BATCH: usize = 64;

/// A failure sent to the client as a status. Messages are fixed words: never a path, a name or
/// anything that says whether a hidden thing exists.
#[derive(Debug)]
pub(super) struct SftpError(StatusReply);

impl SftpError {
    fn new(code: StatusCode, message: &str) -> Self {
        Self(code.with_message(message))
    }

    fn missing() -> Self {
        Self::new(StatusCode::NoSuchFile, "no such file")
    }

    fn denied(message: &str) -> Self {
        Self::new(StatusCode::PermissionDenied, message)
    }

    fn failure(message: &str) -> Self {
        Self::new(StatusCode::Failure, message)
    }

    fn unsupported(message: &str) -> Self {
        Self::new(StatusCode::OpUnsupported, message)
    }
}

impl From<SftpError> for StatusReply {
    fn from(error: SftpError) -> Self {
        error.0
    }
}

impl From<FileError> for SftpError {
    fn from(error: FileError) -> Self {
        match error {
            FileError::NotFound => Self::missing(),
            FileError::NotPermitted(_) | FileError::NotSignedIn => {
                Self::denied("you may not do that")
            }
            FileError::Limit(refusal) => Self::denied(&refusal.to_string()),
            FileError::Text(text) => {
                Self::failure(&format!("that file name is not accepted: {text}"))
            }
            FileError::NameTaken
            | FileError::UnknownMember
            | FileError::TooManyTags
            | FileError::ListFull
            | FileError::NotPending => Self::failure("that cannot be done"),
            FileError::Blob(error) => {
                warn!(%error, "the blob store failed during an sftp operation");
                Self::failure("the file store failed")
            }
            FileError::Store(error) => {
                warn!(%error, "the database failed during an sftp operation");
                Self::failure("the board could not do that")
            }
        }
    }
}

fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".into(),
        language_tag: "en-US".into(),
    }
}

fn mtime(at: UnixSeconds) -> u32 {
    u32::try_from(at.as_raw().max(0)).unwrap_or(u32::MAX)
}

fn dir_attrs() -> FileAttributes {
    FileAttributes {
        size: Some(0),
        uid: Some(0),
        gid: Some(0),
        permissions: Some(0o555 | FileMode::DIR.bits()),
        atime: Some(0),
        mtime: Some(0),
        ..FileAttributes::default()
    }
}

fn file_attrs(file: &FileSummary) -> FileAttributes {
    FileAttributes {
        size: Some(file.size),
        uid: Some(0),
        gid: Some(0),
        user: file.uploader.clone(),
        permissions: Some(0o444 | FileMode::REG.bits()),
        atime: Some(mtime(file.at)),
        mtime: Some(mtime(file.at)),
        ..FileAttributes::default()
    }
}

struct Listed {
    name: String,
    attrs: FileAttributes,
}

struct DirState {
    entries: Vec<Listed>,
    at: usize,
}

struct ReadState {
    key: BlobKey,
    size: u64,
    pos: u64,
    /// A mutex only to make the handle `Sync` (the S3 reader is `Send`, not `Sync`, and a
    /// handler future that borrows the session needs both); it is never locked, `get_mut` is
    /// enough because every use has `&mut`.
    reader: tokio::sync::Mutex<BlobsReader>,
}

struct WriteState {
    plan: UploadPlan,
    meter: UploadMeter,
    tx: mpsc::Sender<Chunk>,
    task: JoinHandle<Result<BlobMeta, BlobError>>,
    pos: u64,
    /// Set once a write was refused: the transfer is dead and its close reports this.
    failed: Option<StatusReply>,
}

enum Open {
    Dir(DirState),
    Read(Box<ReadState>),
    Write(Box<WriteState>),
}

/// The handler for one `sftp` channel.
pub(super) struct SftpSession<D> {
    directory: Arc<D>,
    files: Arc<FileServices>,
    account: AccountId,
    open: HashMap<String, Open>,
    next: u64,
    clock: SystemClock,
}

/// Serve SFTP on `stream` for `account`. The task ends when the stream does.
pub(crate) async fn serve<D, S>(
    stream: S,
    directory: Arc<D>,
    files: Arc<FileServices>,
    account: AccountId,
) where
    D: Backend,
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let session = SftpSession {
        directory,
        files,
        account,
        open: HashMap::new(),
        next: 0,
        clock: SystemClock,
    };
    russh_sftp::server::run(stream, session).await;
}

impl<D: Backend> SftpSession<D> {
    /// What the member holds right now. An account that is no longer an active member holds
    /// nothing, and every operation then ends as not found or denied.
    async fn access(&self) -> Result<Snapshot, SftpError> {
        match self.directory.access_for(Some(self.account)).await {
            Ok(snapshot) if snapshot.account() == Some(self.account) => Ok(snapshot),
            Ok(_) => Err(SftpError::denied("you may not do that")),
            Err(error) => {
                warn!(%error, "access could not be loaded for an sftp operation");
                Err(SftpError::failure("the board could not do that"))
            }
        }
    }

    fn handle_name(&mut self) -> Result<String, SftpError> {
        if self.open.len() >= MAX_HANDLES {
            return Err(SftpError::failure("too many files are open"));
        }
        self.next += 1;
        Ok(format!("h{}", self.next))
    }

    async fn area_dir(&self, snapshot: &Snapshot, area: AreaId) -> Result<(), SftpError> {
        let areas = files::areas(&*self.directory, snapshot).await?;
        if areas.iter().any(|a| a.id == area) {
            Ok(())
        } else {
            Err(SftpError::missing())
        }
    }

    /// The summary of file `id` if it is the kind of file `state` names, in `area`, and visible.
    async fn file_in(
        &self,
        snapshot: &Snapshot,
        area: AreaId,
        id: FileId,
        state: FileState,
    ) -> Result<FileSummary, SftpError> {
        let view = files::file_view(&*self.directory, snapshot, id).await?;
        if view.file.area == area && view.file.state == state {
            Ok(view.file)
        } else {
            Err(SftpError::missing())
        }
    }

    async fn on_download_list(
        &self,
        snapshot: &Snapshot,
        id: FileId,
    ) -> Result<FileSummary, SftpError> {
        let list = files::list_files(&*self.directory, snapshot).await?;
        let row = list
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(SftpError::missing)?;
        Ok(FileSummary::of(&row))
    }

    async fn attrs_of(&self, raw: &str) -> Result<FileAttributes, SftpError> {
        let parsed = path::parse(raw).map_err(|_| SftpError::missing())?;
        let snapshot = self.access().await?;
        match parsed {
            VPath::Root | VPath::Downloads => Ok(dir_attrs()),
            VPath::Area(area) | VPath::Pending(area) => {
                self.area_dir(&snapshot, area).await?;
                Ok(dir_attrs())
            }
            VPath::AreaEntry(area, Entry::Id(id)) => Ok(file_attrs(
                &self
                    .file_in(&snapshot, area, id, FileState::Approved)
                    .await?,
            )),
            VPath::PendingEntry(area, Entry::Id(id)) => Ok(file_attrs(
                &self
                    .file_in(&snapshot, area, id, FileState::Quarantine)
                    .await?,
            )),
            VPath::DownloadEntry(Entry::Id(id)) => {
                Ok(file_attrs(&self.on_download_list(&snapshot, id).await?))
            }
            VPath::AreaEntry(_, Entry::Name(_))
            | VPath::PendingEntry(_, Entry::Name(_))
            | VPath::DownloadEntry(Entry::Name(_)) => Err(SftpError::missing()),
        }
    }

    async fn listing(&self, parsed: VPath) -> Result<Vec<Listed>, SftpError> {
        let snapshot = self.access().await?;
        let dir = || dir_attrs();
        let mut out = Vec::new();
        match parsed {
            VPath::Root => {
                for area in files::areas(&*self.directory, &snapshot).await? {
                    out.push(Listed {
                        name: area.dir,
                        attrs: dir(),
                    });
                }
                out.push(Listed {
                    name: path::DOWNLOADS_DIR.to_owned(),
                    attrs: dir(),
                });
            }
            VPath::Area(area) => {
                self.area_dir(&snapshot, area).await?;
                out.push(Listed {
                    name: path::PENDING_DIR.to_owned(),
                    attrs: dir(),
                });
                for row in files::listing(&*self.directory, &snapshot, area).await? {
                    let summary = FileSummary::of(&row);
                    out.push(Listed {
                        name: path::file_entry_name(row.id, &summary.name),
                        attrs: file_attrs(&summary),
                    });
                }
            }
            VPath::Pending(area) => {
                self.area_dir(&snapshot, area).await?;
                for row in files::pending_listing(&*self.directory, &snapshot, area).await? {
                    let summary = FileSummary::of(&row);
                    out.push(Listed {
                        name: path::file_entry_name(row.id, &summary.name),
                        attrs: file_attrs(&summary),
                    });
                }
            }
            VPath::Downloads => {
                for row in files::list_files(&*self.directory, &snapshot).await? {
                    let summary = FileSummary::of(&row);
                    out.push(Listed {
                        name: path::file_entry_name(row.id, &summary.name),
                        attrs: file_attrs(&summary),
                    });
                }
            }
            VPath::AreaEntry(..) | VPath::PendingEntry(..) | VPath::DownloadEntry(_) => {
                return Err(SftpError::missing());
            }
        }
        Ok(out)
    }

    async fn open_read(&mut self, parsed: VPath) -> Result<Open, SftpError> {
        let snapshot = self.access().await?;
        let id = match parsed {
            VPath::AreaEntry(area, Entry::Id(id)) => {
                self.file_in(&snapshot, area, id, FileState::Approved)
                    .await?;
                id
            }
            VPath::PendingEntry(area, Entry::Id(id)) => {
                self.file_in(&snapshot, area, id, FileState::Quarantine)
                    .await?;
                id
            }
            VPath::DownloadEntry(Entry::Id(id)) => {
                self.on_download_list(&snapshot, id).await?;
                id
            }
            _ => return Err(SftpError::missing()),
        };
        let target = files::open_download(&*self.directory, &snapshot, id).await?;
        let read = self.files.blobs.get(&target.key).await.map_err(|error| {
            warn!(%error, "a file's bytes could not be opened");
            SftpError::failure("the file could not be read")
        })?;
        Ok(Open::Read(Box::new(ReadState {
            key: target.key,
            size: read.size,
            pos: 0,
            reader: tokio::sync::Mutex::new(read.reader),
        })))
    }

    async fn open_write(&mut self, parsed: VPath, raw: &str) -> Result<Open, SftpError> {
        let (VPath::AreaEntry(area, entry) | VPath::PendingEntry(area, entry)) = parsed else {
            return Err(SftpError::denied("files can only be uploaded into an area"));
        };
        // A name that starts like an id (`2026-notes.txt`) is still a new file's name when it is
        // written: an upload never replaces anything, so the last component is taken as sent.
        let name = match entry {
            Entry::Name(name) => name,
            Entry::Id(_) => raw
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned(),
        };
        let snapshot = self.access().await?;
        let plan =
            files::plan_upload(&*self.directory, &snapshot, area, &name, self.clock.now()).await?;
        let (tx, reader) = ChunkReader::channel();
        let blobs = self.files.blobs.clone();
        let key = plan.key.clone();
        let options = plan.put_options();
        let task = tokio::spawn(async move { blobs.put(&key, reader, &options).await });
        Ok(Open::Write(Box::new(WriteState {
            meter: plan.meter(),
            plan,
            tx,
            task,
            pos: 0,
            failed: None,
        })))
    }

    async fn finish_write(&self, mut state: Box<WriteState>) -> Result<(), SftpError> {
        if let Some(reply) = state.failed.take() {
            state.task.abort();
            return Err(SftpError(reply));
        }
        if state.tx.send(Chunk::Commit).await.is_err() {
            state.task.abort();
            return Err(SftpError::failure("the upload failed"));
        }
        let WriteState { plan, tx, task, .. } = *state;
        drop(tx);
        let meta = match task.await {
            Ok(Ok(meta)) => meta,
            Ok(Err(error)) => {
                debug!(%error, "an upload was not stored");
                return Err(match error {
                    BlobError::TooLarge { .. } => SftpError::denied("that file is too large"),
                    _ => SftpError::failure("the upload failed"),
                });
            }
            Err(_) => return Err(SftpError::failure("the upload failed")),
        };
        let uploaded = files::record_upload(
            &*self.directory,
            &self.files.blobs,
            &self.files.scanner,
            &plan,
            &meta,
            self.clock.now(),
        )
        .await?;
        match uploaded.state {
            FileState::Rejected => Err(SftpError::denied(
                "the virus scanner rejected that file; it was not stored",
            )),
            FileState::Quarantine | FileState::Approved => Ok(()),
        }
    }
}

impl<D: Backend> Handler for SftpSession<D> {
    type Error = SftpError;

    fn unimplemented(&self) -> Self::Error {
        SftpError::unsupported("that operation is not supported")
    }

    async fn init(
        &mut self,
        _version: u32,
        _extensions: HashMap<String, String>,
    ) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let canonical = path::canonical(&path).map_err(|_| SftpError::missing())?;
        Ok(Name {
            id,
            files: vec![File::dummy(canonical)],
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        Ok(Attrs {
            id,
            attrs: self.attrs_of(&path).await?,
        })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.stat(id, path).await
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        let attrs = match self.open.get(&handle) {
            Some(Open::Dir(_)) => dir_attrs(),
            Some(Open::Read(read)) => FileAttributes {
                size: Some(read.size),
                permissions: Some(0o444 | FileMode::REG.bits()),
                ..FileAttributes::default()
            },
            Some(Open::Write(write)) => FileAttributes {
                size: Some(write.pos),
                permissions: Some(0o644 | FileMode::REG.bits()),
                ..FileAttributes::default()
            },
            None => return Err(SftpError::missing()),
        };
        Ok(Attrs { id, attrs })
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let parsed = path::parse(&path).map_err(|_| SftpError::missing())?;
        let entries = self.listing(parsed).await?;
        let handle = self.handle_name()?;
        self.open
            .insert(handle.clone(), Open::Dir(DirState { entries, at: 0 }));
        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        let Some(Open::Dir(dir)) = self.open.get_mut(&handle) else {
            return Err(SftpError::missing());
        };
        if dir.at >= dir.entries.len() {
            return Err(SftpError::new(StatusCode::Eof, "end of directory"));
        }
        let end = (dir.at + DIR_BATCH).min(dir.entries.len());
        let files = dir.entries[dir.at..end]
            .iter()
            .map(|entry| File::new(entry.name.clone(), entry.attrs.clone()))
            .collect();
        dir.at = end;
        Ok(Name { id, files })
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let parsed = path::parse(&filename).map_err(|_| SftpError::missing())?;
        if self.open.len() >= MAX_HANDLES {
            return Err(SftpError::failure("too many files are open"));
        }
        let writing = pflags.contains(OpenFlags::WRITE) || pflags.contains(OpenFlags::CREATE);
        let state = if writing {
            if pflags.contains(OpenFlags::READ) || pflags.contains(OpenFlags::APPEND) {
                return Err(SftpError::denied(
                    "files are written in one pass, from the start",
                ));
            }
            self.open_write(parsed, &filename).await?
        } else if pflags.contains(OpenFlags::READ) {
            self.open_read(parsed).await?
        } else {
            return Err(SftpError::denied("open the file for reading or writing"));
        };
        let handle = self.handle_name()?;
        self.open.insert(handle.clone(), state);
        Ok(Handle { id, handle })
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let blobs = self.files.blobs.clone();
        let Some(Open::Read(state)) = self.open.get_mut(&handle) else {
            return Err(SftpError::missing());
        };
        if offset >= state.size {
            return Err(SftpError::new(StatusCode::Eof, "end of file"));
        }
        let failed = |error: &dyn std::fmt::Display| {
            warn!(%error, "a download could not be read to the end");
            SftpError::failure("the file could not be read")
        };
        if offset != state.pos {
            // A resume or a client that reads out of order: start over and skip. Bounded by the
            // file's size, and the hash is still verified when the stream reaches its end.
            if offset < state.pos {
                let read = blobs.get(&state.key).await.map_err(|e| failed(&e))?;
                *state.reader.get_mut() = read.reader;
                state.pos = 0;
            }
            let mut skip = state.reader.get_mut().take(offset - state.pos);
            tokio::io::copy(&mut skip, &mut tokio::io::sink())
                .await
                .map_err(|e| failed(&e))?;
            state.pos = offset;
        }
        let want = usize::try_from(len)
            .unwrap_or(READ_CHUNK)
            .min(READ_CHUNK)
            .min(usize::try_from(state.size - offset).unwrap_or(READ_CHUNK));
        let mut buf = vec![0_u8; want];
        let mut filled = 0;
        while filled < want {
            let n = state
                .reader
                .get_mut()
                .read(&mut buf[filled..])
                .await
                .map_err(|e| failed(&e))?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            return Err(failed(&"the object ended early"));
        }
        buf.truncate(filled);
        state.pos += filled as u64;
        Ok(Data { id, data: buf })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        let directory = Arc::clone(&self.directory);
        let now = self.clock.now();
        let Some(Open::Write(state)) = self.open.get_mut(&handle) else {
            return Err(SftpError::missing());
        };
        if let Some(reply) = &state.failed {
            return Err(SftpError(reply.clone()));
        }
        let refuse = |error: SftpError, state: &mut WriteState| {
            state.failed = Some(error.0.clone());
            error
        };
        if offset != state.pos {
            return Err(refuse(
                SftpError::failure("uploads must be written in order"),
                state,
            ));
        }
        if let Err(error) = state.meter.accept(&*directory, now, data.len()).await {
            return Err(refuse(error.into(), state));
        }
        let len = data.len() as u64;
        if state.tx.send(Chunk::Data(data)).await.is_err() {
            return Err(refuse(SftpError::failure("the upload failed"), state));
        }
        state.pos += len;
        Ok(ok(id))
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        match self.open.remove(&handle) {
            Some(Open::Write(state)) => self.finish_write(state).await?,
            Some(Open::Dir(_) | Open::Read(_)) => {}
            None => return Err(SftpError::missing()),
        }
        Ok(ok(id))
    }

    async fn fsetstat(
        &mut self,
        id: u32,
        handle: String,
        _attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        // Modes and times are never stored, so a client that sets them after an upload is told
        // it worked only for a file it is writing; everything else is unsupported.
        match self.open.get(&handle) {
            Some(Open::Write(_)) => Ok(ok(id)),
            _ => Err(SftpError::unsupported("attributes cannot be changed")),
        }
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        let parsed = path::parse(&filename).map_err(|_| SftpError::missing())?;
        let VPath::PendingEntry(area, Entry::Id(file)) = parsed else {
            return Err(SftpError::denied(
                "only an upload still waiting for approval can be removed here",
            ));
        };
        let snapshot = self.access().await?;
        self.file_in(&snapshot, area, file, FileState::Quarantine)
            .await?;
        files::remove(&*self.directory, &self.files.blobs, &snapshot, file).await?;
        Ok(ok(id))
    }

    async fn mkdir(&mut self, _: u32, _: String, _: FileAttributes) -> Result<Status, Self::Error> {
        Err(SftpError::denied("directories cannot be made"))
    }

    async fn rmdir(&mut self, _: u32, _: String) -> Result<Status, Self::Error> {
        Err(SftpError::denied("directories cannot be removed"))
    }

    async fn rename(&mut self, _: u32, _: String, _: String) -> Result<Status, Self::Error> {
        Err(SftpError::denied("files cannot be renamed"))
    }

    async fn symlink(&mut self, _: u32, _: String, _: String) -> Result<Status, Self::Error> {
        Err(SftpError::denied("there are no links here"))
    }

    async fn readlink(&mut self, _: u32, _: String) -> Result<Name, Self::Error> {
        Err(SftpError::missing())
    }

    async fn setstat(
        &mut self,
        _: u32,
        _: String,
        _: FileAttributes,
    ) -> Result<Status, Self::Error> {
        Err(SftpError::unsupported("attributes cannot be changed"))
    }
}
