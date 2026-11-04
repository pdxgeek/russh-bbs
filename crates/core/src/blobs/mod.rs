//! The blob store interface (D30, M4-F1): every byte a member uploads lives behind this trait,
//! with object-store semantics (put, get, head, delete, list by prefix), whether the bytes sit
//! in a local directory or in an S3-compatible bucket.
//!
//! Rules that hold for every backend:
//!
//! - **Keys are minted by core, never typed.** A [`BlobKey`] is a [`Namespace`] plus a random
//!   [`BlobId`] (128 bits, lowercase hex). User text, file names and paths never reach a
//!   backend, so a key cannot traverse a path or collide with a reserved name.
//! - **Objects are immutable.** A put of a key that exists fails with
//!   [`BlobError::AlreadyExists`]; there is no overwrite.
//! - **Content is hashed on put and verified on get.** The SHA-256 of the content, its size
//!   and its creation time travel with the object in a fixed trailer ([`format`]), so both
//!   backends store the same bytes and a read that does not match its trailer fails with
//!   [`BlobError::Corrupt`] instead of returning bad data.
//! - **Memory is bounded.** Put and get are streams; no backend buffers a whole object.
//! - **An object appears whole or not at all.** A truncated stream, a wrong hash, a size
//!   overrun or a crash mid-write leaves nothing visible.
//! - **Errors never carry secrets.** [`BlobError::Backend`] holds text a backend has already
//!   scrubbed of credentials.
//!
//! The backends live in `russh-bbs-store`; the suite that holds them to this contract is
//! `russh_bbs_store::blobs::suite`.

use std::fmt;
use std::future::Future;

use tokio::io::AsyncRead;

use crate::store::UnixSeconds;

pub mod format;

#[cfg(test)]
mod tests;

pub use format::{PutAccounting, TRAILER_LEN, Trailer, VerifyingReader};

/// Length of a [`BlobId`] in hex characters (128 bits).
pub const ID_HEX_LEN: usize = 32;

/// The most keys one [`BlobStore::list`] page returns, whatever the caller asks for.
pub const MAX_LIST_LIMIT: usize = 1000;

/// Why a blob operation failed. Display text is safe to log and to show staff: it never holds
/// credentials, object contents or member text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BlobError {
    /// No object has this key.
    #[error("blob not found")]
    NotFound,
    /// An object with this key exists; objects are never overwritten.
    #[error("blob already exists")]
    AlreadyExists,
    /// The content is larger than the limit allows. Nothing was stored.
    #[error("blob is larger than the {max} byte limit")]
    TooLarge {
        /// The limit that applied.
        max: u64,
    },
    /// The stream ended early or ran past the size the caller declared. Nothing was stored.
    #[error("blob stream was {actual} bytes but {declared} were declared")]
    SizeMismatch {
        /// The size the caller declared.
        declared: u64,
        /// What the stream actually held (at least this many; an overrun stops counting at the
        /// first excess byte).
        actual: u64,
    },
    /// The content's SHA-256 is not the one the caller expected. Nothing was stored.
    #[error("blob content does not match the expected hash")]
    HashMismatch,
    /// A stored object does not match its own trailer: it was damaged, truncated or tampered
    /// with outside the backend.
    #[error("stored blob failed verification: {0}")]
    Corrupt(&'static str),
    /// The caller's stream failed while the blob was being written. Nothing was stored.
    #[error("reading the blob source failed: {0}")]
    Source(String),
    /// The backend failed (disk, network, bucket). The text has been scrubbed of credentials.
    #[error("blob backend error: {0}")]
    Backend(String),
}

/// Why a key, prefix or id was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    /// A namespace is 1 to 16 lowercase ASCII letters or digits, starting with a letter.
    #[error("namespace must be 1 to 16 lowercase letters or digits, starting with a letter")]
    Namespace,
    /// An id is exactly 32 lowercase hex characters.
    #[error("blob id must be 32 lowercase hex characters")]
    Id,
    /// A shard is exactly 2 lowercase hex characters.
    #[error("shard must be 2 lowercase hex characters")]
    Shard,
    /// A key is `<namespace>/<id>`.
    #[error("blob key must look like <namespace>/<id>")]
    Key,
}

/// A group of keys that a prefix can name, such as `files` or `quarantine`. Chosen by code, not
/// by members.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Namespace(String);

impl Namespace {
    /// Validate a namespace.
    pub fn new(name: &str) -> Result<Self, KeyError> {
        let mut chars = name.chars();
        let ok = name.len() <= 16
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        if ok {
            Ok(Self(name.to_owned()))
        } else {
            Err(KeyError::Namespace)
        }
    }

    /// The namespace text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_lower_hex(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A random 128-bit identifier, rendered as 32 lowercase hex characters. Validated hex is what
/// makes a path built from it safe: it has no separator, dot or other special character.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlobId(String);

impl BlobId {
    /// Mint a new random id from the operating system's random source.
    #[must_use]
    pub fn generate() -> Self {
        use rand::RngExt;
        let bytes: [u8; 16] = rand::rng().random();
        let mut hex = String::with_capacity(ID_HEX_LEN);
        for b in bytes {
            hex.push_str(&format!("{b:02x}"));
        }
        Self(hex)
    }

    /// Validate an id read back from storage or from a database row.
    pub fn parse(text: &str) -> Result<Self, KeyError> {
        if text.len() == ID_HEX_LEN && is_lower_hex(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(KeyError::Id)
        }
    }

    /// The 32 hex characters.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The first two hex characters, which backends use to shard.
    #[must_use]
    pub fn shard(&self) -> Shard {
        Shard(self.0[..2].to_owned())
    }
}

impl fmt::Display for BlobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The first two hex characters of an id (256 shards).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Shard(String);

impl Shard {
    /// Validate a shard.
    pub fn new(text: &str) -> Result<Self, KeyError> {
        if text.len() == 2 && is_lower_hex(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(KeyError::Shard)
        }
    }

    /// The two hex characters.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The address of one object: `<namespace>/<id>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlobKey {
    namespace: Namespace,
    id: BlobId,
}

impl BlobKey {
    /// Combine a namespace and an id.
    #[must_use]
    pub const fn new(namespace: Namespace, id: BlobId) -> Self {
        Self { namespace, id }
    }

    /// A key in `namespace` with a fresh random id.
    #[must_use]
    pub fn generate(namespace: Namespace) -> Self {
        Self::new(namespace, BlobId::generate())
    }

    /// Parse `<namespace>/<id>`, as stored in a database row.
    pub fn parse(text: &str) -> Result<Self, KeyError> {
        let (ns, id) = text.split_once('/').ok_or(KeyError::Key)?;
        Ok(Self::new(
            Namespace::new(ns).map_err(|_| KeyError::Key)?,
            BlobId::parse(id).map_err(|_| KeyError::Key)?,
        ))
    }

    /// The namespace.
    #[must_use]
    pub const fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    /// The id.
    #[must_use]
    pub const fn id(&self) -> &BlobId {
        &self.id
    }

    /// The same id in another namespace, for moving an object between `quarantine` and `files`.
    #[must_use]
    pub fn in_namespace(&self, namespace: Namespace) -> Self {
        Self::new(namespace, self.id.clone())
    }
}

impl fmt::Display for BlobKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.namespace, self.id)
    }
}

/// What [`BlobStore::list`] matches: every key in a namespace, or only one shard of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobPrefix {
    /// The namespace to list.
    pub namespace: Namespace,
    /// Only keys whose id starts with these two hex characters.
    pub shard: Option<Shard>,
}

impl BlobPrefix {
    /// Every key in `namespace`.
    #[must_use]
    pub const fn namespace(namespace: Namespace) -> Self {
        Self {
            namespace,
            shard: None,
        }
    }

    /// Whether `key` falls under this prefix.
    #[must_use]
    pub fn matches(&self, key: &BlobKey) -> bool {
        key.namespace == self.namespace
            && self
                .shard
                .as_ref()
                .is_none_or(|s| key.id.as_str().starts_with(s.as_str()))
    }
}

/// A SHA-256 digest.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlobHash([u8; 32]);

impl BlobHash {
    /// Wrap raw digest bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The digest of `data`.
    #[must_use]
    pub fn of(data: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        Self(Sha256::digest(data).into())
    }

    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex, 64 characters.
    #[must_use]
    pub fn to_hex(&self) -> String {
        let mut out = String::with_capacity(64);
        for b in self.0 {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }
}

impl fmt::Debug for BlobHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlobHash({})", self.to_hex())
    }
}

impl fmt::Display for BlobHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// What a backend knows about a stored object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobMeta {
    /// Content length in bytes (the trailer is not counted).
    pub size: u64,
    /// SHA-256 of the content.
    pub sha256: BlobHash,
    /// When the object was written.
    pub created: UnixSeconds,
}

/// Limits and expectations for one put.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PutOptions {
    /// The exact size the caller expects. A stream that ends early or runs past it fails with
    /// [`BlobError::SizeMismatch`]. Use it when the size is known up front (an SFTP write with
    /// a declared length).
    pub declared_size: Option<u64>,
    /// The most bytes this put may store. The backend also applies its configured cap and uses
    /// the lower of the two.
    pub max_size: u64,
    /// The SHA-256 the content must have, if the caller already knows it.
    pub expect_sha256: Option<BlobHash>,
}

impl PutOptions {
    /// Accept up to `max_size` bytes, with no declared size or expected hash.
    #[must_use]
    pub const fn up_to(max_size: u64) -> Self {
        Self {
            declared_size: None,
            max_size,
            expect_sha256: None,
        }
    }

    /// Require exactly `size` bytes.
    #[must_use]
    pub const fn exactly(size: u64) -> Self {
        Self {
            declared_size: Some(size),
            max_size: size,
            expect_sha256: None,
        }
    }

    /// Also require this SHA-256.
    #[must_use]
    pub const fn with_sha256(mut self, hash: BlobHash) -> Self {
        self.expect_sha256 = Some(hash);
        self
    }
}

/// One page of [`BlobStore::list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListPage {
    /// Keys in ascending order.
    pub keys: Vec<BlobKey>,
    /// Pass this as `after` to get the next page; `None` means the listing is complete.
    pub next: Option<BlobKey>,
}

/// An object being read: its content size and a verifying stream of exactly that many bytes.
/// Reading to the end checks the content against its stored hash and size; a mismatch is an
/// `InvalidData` I/O error whose source is [`BlobError::Corrupt`] (see
/// [`BlobError::from_io`]).
#[derive(Debug)]
pub struct BlobRead<R> {
    /// Content length in bytes.
    pub size: u64,
    /// The content.
    pub reader: R,
}

impl BlobError {
    /// Recover a [`BlobError`] from an error a [`VerifyingReader`] returned, or wrap any other
    /// I/O error as [`BlobError::Backend`].
    #[must_use]
    pub fn from_io(error: &std::io::Error) -> Self {
        error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<Self>())
            .cloned()
            .unwrap_or_else(|| Self::Backend(format!("read failed: {:?}", error.kind())))
    }
}

/// The object-store interface. Implemented by the local directory backend and the
/// S3-compatible backend in `russh-bbs-store`.
pub trait BlobStore: Send + Sync {
    /// The verifying stream [`get`](Self::get) returns.
    type Reader: AsyncRead + Send + Unpin + 'static;

    /// Store `body` under `key`. Reads the stream once, in chunks, hashing as it goes. The
    /// object becomes visible only when it is complete and verified; on any error nothing is
    /// left behind.
    fn put<R>(
        &self,
        key: &BlobKey,
        body: R,
        options: &PutOptions,
    ) -> impl Future<Output = Result<BlobMeta, BlobError>> + Send
    where
        R: AsyncRead + Send + Unpin;

    /// Open the object as a stream that verifies itself at the end.
    fn get(
        &self,
        key: &BlobKey,
    ) -> impl Future<Output = Result<BlobRead<Self::Reader>, BlobError>> + Send;

    /// Size, content hash and creation time, without reading the content.
    fn head(&self, key: &BlobKey) -> impl Future<Output = Result<BlobMeta, BlobError>> + Send;

    /// Remove the object. Removing one that is not there succeeds, so a retry is safe.
    fn delete(&self, key: &BlobKey) -> impl Future<Output = Result<(), BlobError>> + Send;

    /// Up to `limit` keys under `prefix`, in ascending order, starting after `after`. A
    /// `limit` of 0 or above [`MAX_LIST_LIMIT`] is clamped into `1..=MAX_LIST_LIMIT`.
    fn list(
        &self,
        prefix: &BlobPrefix,
        after: Option<&BlobKey>,
        limit: usize,
    ) -> impl Future<Output = Result<ListPage, BlobError>> + Send;
}
