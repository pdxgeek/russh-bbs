//! The S3-compatible backend (D30), over the `object_store` crate: AWS S3, MinIO, R2, B2 and
//! anything else that speaks the S3 API.
//!
//! It stores the same bytes the local backend does (content then the 56-byte trailer from
//! `russh_bbs_core::blobs::format`) at the key `<namespace>/<xx>/<id>`, so a board can move
//! between a directory and a bucket by copying objects.
//!
//! - **Small objects** (at most the multipart threshold) are one conditional `PUT`
//!   (`If-None-Match: *`), so two concurrent puts of one key cannot both win.
//! - **Larger objects** are a multipart upload with parts of the threshold size. Memory is the
//!   threshold buffer plus at most two parts in flight, whatever the object size. A multipart
//!   upload cannot be made conditional, so a concurrent put of the same key can overwrite; keys
//!   are random ids minted by core, so that is a bug elsewhere, and the object is still whole
//!   (it carries its own trailer). Uploads abandoned by a crash are not visible as objects; the
//!   operator should set a bucket lifecycle rule to abort incomplete multipart uploads.
//! - **Credentials** come from the environment (never the config file or the database), are held
//!   only inside the HTTP client, and are scrubbed from every error text this module returns.
//! - **TLS** uses rustls with the `ring` provider. By default the platform's CA store is
//!   trusted; `ca_file` replaces it with only the PEM certificates in that file, as for
//!   `feeds.ca_file` and `datasource.tls_root_cert` (D92).

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::stream::{BoxStream, StreamExt, TryStreamExt};
use object_store::aws::AmazonS3Builder;
use object_store::path::Path;
use object_store::{
    ClientOptions, GetOptions, GetRange, ObjectStore, ObjectStoreExt, PutMode,
    PutOptions as StorePutOptions, PutPayload, WriteMultipart,
};
use russh_bbs_core::blobs::{
    BlobError, BlobId, BlobKey, BlobMeta, BlobPrefix, BlobRead, BlobStore, ListPage, MAX_LIST_LIMIT,
    Namespace, PutAccounting, PutOptions, TRAILER_LEN, Trailer, VerifyingReader,
};
use russh_bbs_core::store::UnixSeconds;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::io::StreamReader;

/// Bytes read from the caller's stream at a time.
const CHUNK: usize = 64 * 1024;

/// S3 refuses multipart parts under 5 MiB (except the last).
pub const MIN_PART_BYTES: u64 = 5 * 1024 * 1024;

/// S3 allows at most 10,000 parts in one upload.
const MAX_PARTS: u64 = 10_000;

/// Parts uploading at the same time; with the one being filled, the most memory is
/// `(IN_FLIGHT + 1)` parts.
const IN_FLIGHT: usize = 2;

/// Total time allowed for one request, including streaming a download to the end.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// Time allowed to open a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Access keys for the bucket. `Debug` never prints them.
#[derive(Clone)]
pub struct S3Credentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
}

impl S3Credentials {
    /// Wrap keys read from the environment.
    #[must_use]
    pub const fn new(
        access_key_id: String,
        secret_access_key: String,
        session_token: Option<String>,
    ) -> Self {
        Self {
            access_key_id,
            secret_access_key,
            session_token,
        }
    }

    /// Read `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` and `AWS_SESSION_TOKEN`. `None` unless
    /// both keys are set and non-empty; the backend then falls back to the platform's own
    /// credential sources (instance or container role, web identity).
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        Some(Self::new(
            var("AWS_ACCESS_KEY_ID")?,
            var("AWS_SECRET_ACCESS_KEY")?,
            var("AWS_SESSION_TOKEN"),
        ))
    }
}

impl std::fmt::Debug for S3Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("S3Credentials(***)")
    }
}

/// Everything the S3 backend needs, resolved from config and the environment.
#[derive(Debug, Clone)]
pub struct S3Settings {
    /// Endpoint URL, or `None` for AWS itself.
    pub endpoint: Option<String>,
    /// Region. Defaults to `us-east-1` when unset (S3-compatible servers mostly ignore it).
    pub region: Option<String>,
    /// Bucket name.
    pub bucket: String,
    /// Address the bucket in the path (`https://host/bucket/key`) instead of the host name.
    /// Needed for MinIO and most self-hosted servers.
    pub path_style: bool,
    /// PEM certificates that replace the platform CA store.
    pub ca_file: Option<PathBuf>,
    /// Access keys, or `None` to use the platform's credential sources.
    pub credentials: Option<S3Credentials>,
    /// Objects larger than this are uploaded in parts of this size. At least [`MIN_PART_BYTES`].
    pub multipart_threshold: u64,
    /// The most bytes any object may hold.
    pub max_object_bytes: u64,
}

/// The shared secrets to cut out of any text that leaves this module.
#[derive(Clone)]
struct Scrub(Arc<Vec<String>>);

impl Scrub {
    fn new(credentials: Option<&S3Credentials>) -> Self {
        let mut secrets = Vec::new();
        if let Some(c) = credentials {
            secrets.push(c.access_key_id.clone());
            secrets.push(c.secret_access_key.clone());
            secrets.extend(c.session_token.clone());
        }
        secrets.retain(|s| !s.is_empty());
        Self(Arc::new(secrets))
    }

    fn text(&self, text: impl Into<String>) -> String {
        let mut out: String = text.into();
        for secret in self.0.iter() {
            out = out.replace(secret.as_str(), "[redacted]");
        }
        out
    }
}

/// A bucket of blobs.
#[derive(Debug, Clone)]
pub struct S3BlobStore {
    store: Arc<dyn ObjectStore>,
    scrub: Scrub,
    multipart_threshold: usize,
    max_object_bytes: u64,
}

impl std::fmt::Debug for Scrub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Scrub")
    }
}

/// The verifying stream of a downloaded object.
pub type S3Reader = VerifyingReader<StreamReader<BoxStream<'static, io::Result<bytes::Bytes>>, bytes::Bytes>>;

fn now() -> UnixSeconds {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    UnixSeconds::from_raw(i64::try_from(secs).unwrap_or(i64::MAX))
}

fn object_path(key: &BlobKey) -> Path {
    Path::from(format!(
        "{}/{}/{}",
        key.namespace(),
        key.id().shard().as_str(),
        key.id()
    ))
}

fn key_of(path: &Path) -> Option<BlobKey> {
    let mut parts = path.parts();
    let ns = Namespace::new(parts.next()?.as_ref()).ok()?;
    let shard = parts.next()?;
    let id = BlobId::parse(parts.next()?.as_ref()).ok()?;
    if parts.next().is_some() || id.shard().as_str() != shard.as_ref() {
        return None;
    }
    Some(BlobKey::new(ns, id))
}

impl S3BlobStore {
    /// Build the client and check the bucket answers with these credentials, so a wrong
    /// endpoint, bucket or key is a boot error and not a failure on the first upload.
    pub async fn connect(settings: S3Settings) -> Result<Self, BlobError> {
        let scrub = Scrub::new(settings.credentials.as_ref());
        if settings.multipart_threshold < MIN_PART_BYTES {
            return Err(BlobError::Backend(format!(
                "multipart threshold must be at least {MIN_PART_BYTES} bytes"
            )));
        }
        if settings.max_object_bytes / settings.multipart_threshold >= MAX_PARTS {
            return Err(BlobError::Backend(
                "max object size needs more than 10,000 parts at this multipart threshold"
                    .to_owned(),
            ));
        }
        // reqwest's rustls has no built-in provider here (D44: ring only, no aws-lc).
        let _ = rustls::crypto::ring::default_provider().install_default();

        let plain_http = settings
            .endpoint
            .as_deref()
            .is_some_and(|e| e.starts_with("http://"));
        let mut client = ClientOptions::new()
            .with_allow_http(plain_http)
            .with_timeout(REQUEST_TIMEOUT)
            .with_connect_timeout(CONNECT_TIMEOUT);
        if let Some(path) = &settings.ca_file {
            let pem = std::fs::read(path).map_err(|e| {
                BlobError::Backend(format!("reading blobs.ca_file: {:?}", e.kind()))
            })?;
            let cert = object_store::Certificate::from_pem(&pem)
                .map_err(|_| BlobError::Backend("blobs.ca_file is not a PEM certificate".to_owned()))?;
            client = client
                .with_root_certificate(cert)
                .with_no_system_certificates(true);
        }
        let mut builder = AmazonS3Builder::new()
            .with_bucket_name(&settings.bucket)
            .with_region(settings.region.as_deref().unwrap_or("us-east-1"))
            .with_virtual_hosted_style_request(!settings.path_style)
            .with_client_options(client);
        if let Some(endpoint) = &settings.endpoint {
            builder = builder.with_endpoint(endpoint);
        }
        if let Some(c) = &settings.credentials {
            builder = builder
                .with_access_key_id(&c.access_key_id)
                .with_secret_access_key(&c.secret_access_key);
            if let Some(token) = &c.session_token {
                builder = builder.with_token(token);
            }
        }
        let store = builder
            .build()
            .map_err(|e| BlobError::Backend(scrub.text(format!("configuring the client: {e}"))))?;
        let this = Self {
            store: Arc::new(store),
            scrub,
            multipart_threshold: usize::try_from(settings.multipart_threshold).unwrap_or(usize::MAX),
            max_object_bytes: settings.max_object_bytes,
        };
        let probe = Namespace::new("probe")
            .map_err(|_| BlobError::Backend("probe namespace".to_owned()))?;
        this.list(&BlobPrefix::namespace(probe), None, 1).await?;
        Ok(this)
    }

    /// The raw client, for tests that damage objects behind the backend's back.
    #[cfg(test)]
    pub(super) fn raw(&self) -> &Arc<dyn ObjectStore> {
        &self.store
    }

    fn fail(&self, op: &str, e: &object_store::Error) -> BlobError {
        match e {
            object_store::Error::NotFound { .. } => BlobError::NotFound,
            _ => BlobError::Backend(self.scrub.text(format!("{op}: {e}"))),
        }
    }

    /// Abandon a multipart upload and return the error that caused it.
    async fn abort(&self, upload: WriteMultipart, cause: BlobError) -> BlobError {
        let _ = upload.abort().await;
        cause
    }
}

impl BlobStore for S3BlobStore {
    type Reader = S3Reader;

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
        let path = object_path(key);

        // Fill a buffer with one byte more than the threshold. If the stream ends first, this
        // is a small object and goes up in one request.
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = vec![0_u8; CHUNK];
        let limit = self.multipart_threshold.saturating_add(1);
        let mut ended = false;
        while buf.len() < limit {
            let want = CHUNK.min(limit - buf.len());
            let n = body
                .read(&mut chunk[..want])
                .await
                .map_err(|e| BlobError::Source(format!("{:?}", e.kind())))?;
            if n == 0 {
                ended = true;
                break;
            }
            acct.accept(&chunk[..n])?;
            buf.extend_from_slice(&chunk[..n]);
        }

        if ended {
            let trailer = acct.finish(now())?;
            buf.extend_from_slice(&trailer.encode());
            let opts = StorePutOptions {
                mode: PutMode::Create,
                ..StorePutOptions::default()
            };
            return match self.store.put_opts(&path, PutPayload::from(buf), opts).await {
                Ok(_) => Ok(BlobMeta {
                    size: trailer.size,
                    sha256: trailer.sha256,
                    created: trailer.created,
                }),
                Err(
                    object_store::Error::AlreadyExists { .. }
                    | object_store::Error::Precondition { .. },
                ) => Err(BlobError::AlreadyExists),
                Err(e) => Err(self.fail("uploading", &e)),
            };
        }

        // Large object: multipart. Refuse an existing key up front (best effort, see the module
        // comment).
        match self.store.head(&path).await {
            Ok(_) => return Err(BlobError::AlreadyExists),
            Err(object_store::Error::NotFound { .. }) => {}
            Err(e) => return Err(self.fail("checking the key", &e)),
        }
        let upload = self
            .store
            .put_multipart(&path)
            .await
            .map_err(|e| self.fail("starting the upload", &e))?;
        let mut writer = WriteMultipart::new_with_chunk_size(upload, self.multipart_threshold);
        writer.write(&buf);
        drop(buf);
        loop {
            if let Err(e) = writer.wait_for_capacity(IN_FLIGHT).await {
                let cause = self.fail("uploading a part", &e);
                return Err(self.abort(writer, cause).await);
            }
            let n = match body.read(&mut chunk).await {
                Ok(n) => n,
                Err(e) => {
                    let cause = BlobError::Source(format!("{:?}", e.kind()));
                    return Err(self.abort(writer, cause).await);
                }
            };
            if n == 0 {
                break;
            }
            if let Err(cause) = acct.accept(&chunk[..n]) {
                return Err(self.abort(writer, cause).await);
            }
            writer.write(&chunk[..n]);
        }
        let trailer = match acct.finish(now()) {
            Ok(t) => t,
            Err(cause) => return Err(self.abort(writer, cause).await),
        };
        writer.write(&trailer.encode());
        writer
            .finish()
            .await
            .map_err(|e| self.fail("completing the upload", &e))?;
        Ok(BlobMeta {
            size: trailer.size,
            sha256: trailer.sha256,
            created: trailer.created,
        })
    }

    async fn get(&self, key: &BlobKey) -> Result<BlobRead<S3Reader>, BlobError> {
        let result = self
            .store
            .get(&object_path(key))
            .await
            .map_err(|e| self.fail("downloading", &e))?;
        let total = result.meta.size;
        let scrub = self.scrub.clone();
        let stream: BoxStream<'static, io::Result<bytes::Bytes>> = result
            .into_stream()
            .map_err(move |e| io::Error::other(scrub.text(format!("download interrupted: {e}"))))
            .boxed();
        let reader = VerifyingReader::new(StreamReader::new(stream), total)?;
        Ok(BlobRead {
            size: reader.content_len(),
            reader,
        })
    }

    async fn head(&self, key: &BlobKey) -> Result<BlobMeta, BlobError> {
        let opts = GetOptions {
            range: Some(GetRange::Suffix(TRAILER_LEN as u64)),
            ..GetOptions::default()
        };
        let result = self
            .store
            .get_opts(&object_path(key), opts)
            .await
            .map_err(|e| self.fail("reading the object tail", &e))?;
        let total = result.meta.size;
        let tail = result
            .bytes()
            .await
            .map_err(|e| self.fail("reading the object tail", &e))?;
        Trailer::decode(&tail)?.check_total(total)
    }

    async fn delete(&self, key: &BlobKey) -> Result<(), BlobError> {
        match self.store.delete(&object_path(key)).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(e) => Err(self.fail("deleting", &e)),
        }
    }

    async fn list(
        &self,
        prefix: &BlobPrefix,
        after: Option<&BlobKey>,
        limit: usize,
    ) -> Result<ListPage, BlobError> {
        let limit = limit.clamp(1, MAX_LIST_LIMIT);
        let dir = prefix.shard.as_ref().map_or_else(
            || Path::from(prefix.namespace.as_str()),
            |s| Path::from(format!("{}/{}", prefix.namespace, s.as_str())),
        );
        let mut stream = match after {
            Some(a) => self.store.list_with_offset(Some(&dir), &object_path(a)),
            None => self.store.list(Some(&dir)),
        };
        let mut keys = Vec::new();
        while let Some(item) = stream.next().await {
            let meta = item.map_err(|e| self.fail("listing", &e))?;
            let Some(key) = key_of(&meta.location) else {
                continue;
            };
            if after.is_some_and(|a| &key <= a) {
                continue;
            }
            keys.push(key);
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
