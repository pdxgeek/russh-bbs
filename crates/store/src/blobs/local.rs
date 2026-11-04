//! The local directory backend (D30). It implements the same [`BlobStore`] contract as the
//! S3-compatible one and gets no shortcuts: it hands no path to any other module, and nothing
//! outside this file touches a path under the blob directory.
//!
//! Layout under the root (mode 0700):
//!
//! ```text
//! <root>/tmp/<random>.part        writes in progress; never read
//! <root>/<namespace>/<xx>/<id>    a finished object (xx = the id's first two hex characters)
//! ```
//!
//! Every component of a path comes from a validated [`BlobKey`] (a lowercase word and lowercase
//! hex), so no key can name a parent directory, an absolute path or a hidden file.
//!
//! A put streams into a temp file, appends the trailer, fsyncs, then **hard-links** the temp
//! file to its final name and removes the temp name. The link fails if the name exists, so two
//! concurrent puts of one key cannot both win and an object is never overwritten, and the
//! object appears whole or not at all. The directory is fsynced so the link survives a crash.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use russh_bbs_core::blobs::{
    BlobError, BlobId, BlobKey, BlobMeta, BlobPrefix, BlobRead, BlobStore, ListPage, PutAccounting, PutOptions, TRAILER_LEN, Trailer, VerifyingReader, MAX_LIST_LIMIT,
};
use russh_bbs_core::store::UnixSeconds;
use tokio::fs::{self, File};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

/// Bytes read from the caller's stream at a time; the whole of a put's memory.
const CHUNK: usize = 64 * 1024;

/// Temp files older than this at start-up are leftovers of a crash and are removed.
const STALE_TEMP: Duration = Duration::from_secs(3600);

/// A directory of blobs.
#[derive(Debug, Clone)]
pub struct LocalBlobStore {
    root: PathBuf,
    max_object_bytes: u64,
}

fn backend(context: &str, e: &io::Error) -> BlobError {
    BlobError::Backend(format!("{context}: {:?}", e.kind()))
}

fn now() -> UnixSeconds {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    UnixSeconds::from_raw(i64::try_from(secs).unwrap_or(i64::MAX))
}

async fn mkdir(path: &Path) -> io::Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    b.mode(0o700);
    b.create(path).await
}

/// Make a durable record that `dir` now holds a new entry.
async fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir).await?.sync_all().await
}

impl LocalBlobStore {
    /// Open (creating if needed) the blob directory at `root`, refuse objects over
    /// `max_object_bytes`, and sweep temp files a crash left behind.
    pub async fn open(root: impl Into<PathBuf>, max_object_bytes: u64) -> Result<Self, BlobError> {
        let store = Self {
            root: root.into(),
            max_object_bytes,
        };
        mkdir(&store.tmp_dir())
            .await
            .map_err(|e| backend("creating the blob directory", &e))?;
        store.sweep_stale_temps().await;
        Ok(store)
    }

    fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    fn object_path(&self, key: &BlobKey) -> PathBuf {
        self.root
            .join(key.namespace().as_str())
            .join(key.id().shard().as_str())
            .join(key.id().as_str())
    }

    async fn sweep_stale_temps(&self) {
        let Ok(mut entries) = fs::read_dir(self.tmp_dir()).await else {
            return;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let old = entry
                .metadata()
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age >= STALE_TEMP);
            if old {
                let _ = fs::remove_file(entry.path()).await;
            }
        }
    }
}

/// Removes the temp file if the put does not finish (error, or the future being dropped).
struct TempGuard(Option<PathBuf>);

impl TempGuard {
    fn disarm(&mut self) -> Option<PathBuf> {
        self.0.take()
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// A file read as exactly its length, so a file that grows while open cannot add content.
type LocalReader = VerifyingReader<tokio::io::Take<File>>;

impl BlobStore for LocalBlobStore {
    type Reader = LocalReader;

    async fn put<R>(
        &self,
        key: &BlobKey,
        mut body: R,
        options: &PutOptions,
    ) -> Result<BlobMeta, BlobError>
    where
        R: AsyncRead + Send + Unpin,
    {
        let mut acct = PutAccounting::new(options, self.max_object_bytes)?;
        let final_path = self.object_path(key);
        if fs::try_exists(&final_path).await.unwrap_or(false) {
            return Err(BlobError::AlreadyExists);
        }

        let tmp_path = self
            .tmp_dir()
            .join(format!("{}.part", BlobId::generate().as_str()));
        let mut open = fs::OpenOptions::new();
        open.write(true).create_new(true);
        #[cfg(unix)]
        open.mode(0o600);
        let mut file = open
            .open(&tmp_path)
            .await
            .map_err(|e| backend("creating a temp file", &e))?;
        let mut guard = TempGuard(Some(tmp_path.clone()));

        let mut buf = vec![0_u8; CHUNK];
        loop {
            let n = body
                .read(&mut buf)
                .await
                .map_err(|e| BlobError::Source(format!("{:?}", e.kind())))?;
            if n == 0 {
                break;
            }
            acct.accept(&buf[..n])?;
            file.write_all(&buf[..n])
                .await
                .map_err(|e| backend("writing", &e))?;
        }
        let trailer = acct.finish(now())?;
        file.write_all(&trailer.encode())
            .await
            .map_err(|e| backend("writing", &e))?;
        file.flush().await.map_err(|e| backend("writing", &e))?;
        file.sync_all().await.map_err(|e| backend("syncing", &e))?;
        drop(file);

        let parent = final_path
            .parent()
            .ok_or_else(|| BlobError::Backend("object path has no parent".to_owned()))?;
        mkdir(parent)
            .await
            .map_err(|e| backend("creating a shard directory", &e))?;
        match fs::hard_link(&tmp_path, &final_path).await {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                return Err(BlobError::AlreadyExists);
            }
            Err(e) => return Err(backend("publishing the object", &e)),
        }
        // The object is visible. Drop the temp name and make the directory entry durable.
        if let Some(p) = guard.disarm() {
            let _ = fs::remove_file(p).await;
        }
        sync_dir(parent)
            .await
            .map_err(|e| backend("syncing the shard directory", &e))?;
        Ok(BlobMeta {
            size: trailer.size,
            sha256: trailer.sha256,
            created: trailer.created,
        })
    }

    async fn get(&self, key: &BlobKey) -> Result<BlobRead<LocalReader>, BlobError> {
        let file = open_object(&self.object_path(key)).await?;
        let total = file
            .metadata()
            .await
            .map_err(|e| backend("reading metadata", &e))?
            .len();
        let reader = VerifyingReader::new(file.take(total), total)?;
        Ok(BlobRead {
            size: reader.content_len(),
            reader,
        })
    }

    async fn head(&self, key: &BlobKey) -> Result<BlobMeta, BlobError> {
        let mut file = open_object(&self.object_path(key)).await?;
        let total = file
            .metadata()
            .await
            .map_err(|e| backend("reading metadata", &e))?
            .len();
        let tail = total
            .checked_sub(TRAILER_LEN as u64)
            .ok_or(BlobError::Corrupt("stored object is shorter than its trailer"))?;
        file.seek(io::SeekFrom::Start(tail))
            .await
            .map_err(|e| backend("seeking", &e))?;
        let mut bytes = [0_u8; TRAILER_LEN];
        file.read_exact(&mut bytes)
            .await
            .map_err(|e| backend("reading the trailer", &e))?;
        Trailer::decode(&bytes)?.check_total(total)
    }

    async fn delete(&self, key: &BlobKey) -> Result<(), BlobError> {
        match fs::remove_file(self.object_path(key)).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(backend("deleting", &e)),
        }
    }

    async fn list(
        &self,
        prefix: &BlobPrefix,
        after: Option<&BlobKey>,
        limit: usize,
    ) -> Result<ListPage, BlobError> {
        let limit = limit.clamp(1, MAX_LIST_LIMIT);
        let ns_dir = self.root.join(prefix.namespace.as_str());
        let mut shards = sorted_names(&ns_dir, |n| {
            n.len() == 2 && n.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .await?;
        if let Some(only) = &prefix.shard {
            shards.retain(|s| s == only.as_str());
        }
        let mut keys = Vec::new();
        for shard in shards {
            // Everything in an earlier shard sorts before `after`.
            if after.is_some_and(|a| a.id().shard().as_str() > shard.as_str()) {
                continue;
            }
            let ids = sorted_names(&ns_dir.join(&shard), |n| BlobId::parse(n).is_ok()).await?;
            for id in ids {
                let Ok(id) = BlobId::parse(&id) else { continue };
                let key = BlobKey::new(prefix.namespace.clone(), id);
                if after.is_some_and(|a| &key <= a) {
                    continue;
                }
                keys.push(key);
                if keys.len() > limit {
                    break;
                }
            }
            if keys.len() > limit {
                break;
            }
        }
        let next = if keys.len() > limit {
            keys.truncate(limit);
            keys.last().cloned()
        } else {
            None
        };
        Ok(ListPage { keys, next })
    }
}

async fn open_object(path: &Path) -> Result<File, BlobError> {
    match File::open(path).await {
        Ok(f) => Ok(f),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(BlobError::NotFound),
        Err(e) => Err(backend("opening", &e)),
    }
}

/// The names in `dir` accepted by `keep`, sorted. A missing directory is an empty one.
async fn sorted_names(dir: &Path, keep: impl Fn(&str) -> bool) -> Result<Vec<String>, BlobError> {
    let mut entries = match fs::read_dir(dir).await {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(backend("listing", &e)),
    };
    let mut names = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| backend("listing", &e))?
    {
        if let Some(name) = entry.file_name().to_str().filter(|n| keep(n)) {
            names.push(name.to_owned());
        }
    }
    names.sort();
    Ok(names)
}
