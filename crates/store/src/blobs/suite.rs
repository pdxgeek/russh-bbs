//! The blob contract, written once and run against every backend (D30, D32): the local
//! directory always, and an S3-compatible server (MinIO) in the ignored run.
//!
//! Each case takes a store and a fresh namespace, so cases never see each other's objects even
//! on a shared bucket. A backend's own checks (tampering, path layout, TLS, credentials) live in
//! `tests.rs`.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use russh_bbs_core::blobs::{
    BlobError, BlobHash, BlobKey, BlobPrefix, BlobStore, Namespace, PutOptions, Shard,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};

/// A namespace no other case uses.
pub(crate) fn fresh_namespace() -> Namespace {
    let id = russh_bbs_core::blobs::BlobId::generate();
    Namespace::new(&format!("t{}", &id.as_str()[..12])).unwrap()
}

fn key() -> BlobKey {
    BlobKey::generate(fresh_namespace())
}

/// Deterministic bytes of any length, produced on demand so big objects never sit in memory.
pub(crate) struct Pattern {
    left: u64,
    state: u64,
}

impl Pattern {
    pub(crate) const fn new(len: u64, seed: u64) -> Self {
        Self {
            left: len,
            state: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
        }
    }

    /// The SHA-256 of the same bytes, computed by running a second generator.
    pub(crate) fn hash(len: u64, seed: u64) -> BlobHash {
        let mut p = Self::new(len, seed);
        let mut h = Sha256::new();
        let mut buf = [0_u8; 8192];
        loop {
            let n = p.fill(&mut buf);
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        BlobHash::from_bytes(h.finalize().into())
    }

    fn fill(&mut self, out: &mut [u8]) -> usize {
        let n = out.len().min(usize::try_from(self.left).unwrap_or(usize::MAX));
        for b in &mut out[..n] {
            self.state ^= self.state << 13;
            self.state ^= self.state >> 7;
            self.state ^= self.state << 17;
            *b = self.state.to_le_bytes()[3];
        }
        self.left -= n as u64;
        n
    }
}

impl AsyncRead for Pattern {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let room = buf.remaining().min(7000);
        let mut tmp = vec![0_u8; room];
        let n = self.fill(&mut tmp);
        buf.put_slice(&tmp[..n]);
        Poll::Ready(Ok(()))
    }
}

/// Yields `good` bytes of a pattern, then fails like a dropped connection.
struct FailsAfter {
    inner: Pattern,
    good: u64,
}

impl AsyncRead for FailsAfter {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.good == 0 {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, "reset")));
        }
        let before = buf.filled().len();
        let r = Pin::new(&mut self.inner).poll_read(cx, buf);
        self.good = self.good.saturating_sub((buf.filled().len() - before) as u64);
        r
    }
}

/// Yields some bytes and then never answers, like a client that went silent mid-upload.
struct Stalls {
    inner: Pattern,
    sent: bool,
}

impl AsyncRead for Stalls {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.sent {
            return Poll::Pending;
        }
        self.sent = true;
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

async fn read_back<S: BlobStore>(store: &S, key: &BlobKey) -> Result<Vec<u8>, BlobError> {
    let mut r = store.get(key).await?;
    let mut out = Vec::new();
    r.reader
        .read_to_end(&mut out)
        .await
        .map_err(|e| BlobError::from_io(&e))?;
    assert_eq!(r.size, out.len() as u64);
    Ok(out)
}

async fn gone<S: BlobStore>(store: &S, key: &BlobKey) {
    assert!(matches!(store.head(key).await, Err(BlobError::NotFound)));
    assert!(matches!(store.get(key).await, Err(BlobError::NotFound)));
    let page = store
        .list(&BlobPrefix::namespace(key.namespace().clone()), None, 10)
        .await
        .unwrap();
    assert!(page.keys.is_empty(), "a partial object is listed: {page:?}");
}

pub(crate) async fn round_trips_content_of_every_shape<S: BlobStore>(store: &S) {
    for (len, seed) in [(0_u64, 1_u64), (1, 2), (65_535, 3), (65_536, 4), (65_537, 5), (1 << 20, 6)] {
        let key = key();
        let want = Pattern::hash(len, seed);
        let meta = store
            .put(&key, Pattern::new(len, seed), &PutOptions::up_to(1 << 30))
            .await
            .unwrap();
        assert_eq!((meta.size, meta.sha256), (len, want), "len {len}");
        assert_eq!(store.head(&key).await.unwrap(), meta, "len {len}");
        let bytes = read_back(store, &key).await.unwrap();
        assert_eq!(bytes.len() as u64, len);
        assert_eq!(BlobHash::of(&bytes), want, "len {len}");
    }
}

pub(crate) async fn large_object_streams_in_parts<S: BlobStore>(store: &S, len: u64) {
    let key = key();
    let want = Pattern::hash(len, 99);
    let meta = store
        .put(&key, Pattern::new(len, 99), &PutOptions::exactly(len))
        .await
        .unwrap();
    assert_eq!((meta.size, meta.sha256), (len, want));
    assert_eq!(store.head(&key).await.unwrap().size, len);
    // Read it back through a hashing sink so the test itself holds no whole object either.
    let mut r = store.get(&key).await.unwrap();
    assert_eq!(r.size, len);
    let mut h = Sha256::new();
    let mut buf = vec![0_u8; 40_000];
    let mut total = 0_u64;
    loop {
        let n = r.reader.read(&mut buf).await.unwrap();
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        total += n as u64;
    }
    assert_eq!(total, len);
    assert_eq!(BlobHash::from_bytes(h.finalize().into()), want);
}

pub(crate) async fn missing_object<S: BlobStore>(store: &S) {
    let key = key();
    gone(store, &key).await;
    store.delete(&key).await.unwrap();
}

pub(crate) async fn delete_removes_and_is_repeatable<S: BlobStore>(store: &S) {
    let key = key();
    store.put(&key, Pattern::new(10, 1), &PutOptions::up_to(100)).await.unwrap();
    store.delete(&key).await.unwrap();
    gone(store, &key).await;
    store.delete(&key).await.unwrap();
}

pub(crate) async fn an_existing_key_is_never_overwritten<S: BlobStore>(store: &S) {
    let key = key();
    let first = store
        .put(&key, Pattern::new(100, 1), &PutOptions::up_to(1000))
        .await
        .unwrap();
    let second = store
        .put(&key, Pattern::new(200, 2), &PutOptions::up_to(1000))
        .await;
    assert_eq!(second, Err(BlobError::AlreadyExists));
    assert_eq!(store.head(&key).await.unwrap(), first);
    assert_eq!(BlobHash::of(&read_back(store, &key).await.unwrap()), first.sha256);
}

pub(crate) async fn size_limits<S: BlobStore>(store: &S) {
    // Over the caller's max, found while streaming.
    let k = key();
    let r = store.put(&k, Pattern::new(5000, 1), &PutOptions::up_to(4999)).await;
    assert_eq!(r, Err(BlobError::TooLarge { max: 4999 }));
    gone(store, &k).await;
    // Declared above the max: refused before reading.
    let k = key();
    let mut o = PutOptions::exactly(10_000);
    o.max_size = 5000;
    assert_eq!(
        store.put(&k, Pattern::new(10_000, 1), &o).await,
        Err(BlobError::TooLarge { max: 5000 })
    );
    gone(store, &k).await;
    // Declared larger than the stream (a truncated upload).
    let k = key();
    let r = store.put(&k, Pattern::new(90, 1), &PutOptions::exactly(100)).await;
    assert_eq!(r, Err(BlobError::SizeMismatch { declared: 100, actual: 90 }));
    gone(store, &k).await;
    // Declared smaller than the stream.
    let k = key();
    let r = store.put(&k, Pattern::new(300_000, 1), &PutOptions::exactly(100)).await;
    assert!(matches!(r, Err(BlobError::SizeMismatch { declared: 100, .. })), "{r:?}");
    gone(store, &k).await;
    // Exactly the declared size is fine.
    let k = key();
    store.put(&k, Pattern::new(100, 1), &PutOptions::exactly(100)).await.unwrap();
}

pub(crate) async fn expected_hash_is_checked<S: BlobStore>(store: &S) {
    let k = key();
    let wrong = PutOptions::up_to(1000).with_sha256(Pattern::hash(100, 2));
    assert_eq!(
        store.put(&k, Pattern::new(100, 1), &wrong).await,
        Err(BlobError::HashMismatch)
    );
    gone(store, &k).await;
    let right = PutOptions::up_to(1000).with_sha256(Pattern::hash(100, 1));
    let meta = store.put(&k, Pattern::new(100, 1), &right).await.unwrap();
    assert_eq!(meta.sha256, Pattern::hash(100, 1));
}

pub(crate) async fn a_failed_source_leaves_nothing<S: BlobStore>(store: &S, len: u64, good: u64) {
    let k = key();
    let src = FailsAfter { inner: Pattern::new(len, 7), good };
    let r = store.put(&k, src, &PutOptions::up_to(len + 1)).await;
    assert!(matches!(r, Err(BlobError::Source(_))), "{r:?}");
    gone(store, &k).await;
}

pub(crate) async fn a_put_cancelled_mid_write_leaves_nothing<S: BlobStore>(store: &S, first: u64) {
    let k = key();
    let src = Stalls { inner: Pattern::new(first, 3), sent: false };
    // The "crash": the put future is dropped while the stream is stalled.
    let r = tokio::time::timeout(
        Duration::from_millis(400),
        store.put(&k, src, &PutOptions::up_to(first + 1000)),
    )
    .await;
    assert!(r.is_err(), "the stalled put should still be running");
    gone(store, &k).await;
    // The key is usable afterwards.
    store.put(&k, Pattern::new(10, 1), &PutOptions::up_to(100)).await.unwrap();
}

pub(crate) async fn concurrent_puts_of_one_key_have_one_winner<S: BlobStore + Clone + 'static>(
    store: &S,
) {
    let k = key();
    let mut tasks = Vec::new();
    for seed in 1..=8_u64 {
        let (s, k) = (store.clone(), k.clone());
        tasks.push(tokio::spawn(async move {
            let len = 50_000 + seed;
            let r = s.put(&k, Pattern::new(len, seed), &PutOptions::up_to(1 << 20)).await;
            (seed, len, r)
        }));
    }
    let mut winners = Vec::new();
    for t in tasks {
        let (seed, len, r) = t.await.unwrap();
        match r {
            Ok(meta) => winners.push((seed, len, meta)),
            Err(e) => assert_eq!(e, BlobError::AlreadyExists, "seed {seed}"),
        }
    }
    assert_eq!(winners.len(), 1, "exactly one put may win: {winners:?}");
    let (seed, len, meta) = winners.remove(0);
    assert_eq!(store.head(&k).await.unwrap(), meta);
    let bytes = read_back(store, &k).await.unwrap();
    assert_eq!(bytes.len() as u64, len);
    assert_eq!(BlobHash::of(&bytes), Pattern::hash(len, seed));
}

pub(crate) async fn listing_pages_in_order_without_gaps<S: BlobStore>(store: &S) {
    let ns = fresh_namespace();
    let other = fresh_namespace();
    let mut want = Vec::new();
    for i in 0..23_u64 {
        let k = BlobKey::generate(ns.clone());
        store.put(&k, Pattern::new(i, i), &PutOptions::up_to(100)).await.unwrap();
        want.push(k);
    }
    want.sort();
    for i in 0..3 {
        let k = BlobKey::generate(other.clone());
        store.put(&k, Pattern::new(i, i), &PutOptions::up_to(100)).await.unwrap();
    }
    let prefix = BlobPrefix::namespace(ns.clone());

    // Pages of 5 cover everything once, in order, then stop.
    let mut got = Vec::new();
    let mut after = None;
    let mut pages = 0;
    loop {
        let page = store.list(&prefix, after.as_ref(), 5).await.unwrap();
        assert!(page.keys.len() <= 5);
        got.extend(page.keys.clone());
        pages += 1;
        match page.next {
            Some(n) => {
                assert_eq!(Some(&n), page.keys.last());
                after = Some(n);
            }
            None => break,
        }
        assert!(pages < 20, "pagination does not end");
    }
    assert_eq!(got, want);
    assert_eq!(pages, 5);

    // One big page needs no continuation; limit 0 still returns a key.
    let all = store.list(&prefix, None, 1000).await.unwrap();
    assert_eq!((all.keys.as_slice(), all.next), (want.as_slice(), None));
    assert_eq!(store.list(&prefix, None, 0).await.unwrap().keys.len(), 1);

    // A shard prefix lists exactly that shard's keys.
    let shard = Shard::new(want[0].id().shard().as_str()).unwrap();
    let in_shard: Vec<_> = want.iter().filter(|k| k.id().shard() == shard).cloned().collect();
    let by_shard = BlobPrefix { namespace: ns.clone(), shard: Some(shard) };
    assert_eq!(store.list(&by_shard, None, 100).await.unwrap().keys, in_shard);

    // Another namespace and an empty one do not leak in.
    assert_eq!(store.list(&BlobPrefix::namespace(other), None, 100).await.unwrap().keys.len(), 3);
    let empty = BlobPrefix::namespace(fresh_namespace());
    assert_eq!(store.list(&empty, None, 10).await.unwrap(), russh_bbs_core::blobs::ListPage { keys: vec![], next: None });

    // Deleted keys leave the listing.
    store.delete(&want[3]).await.unwrap();
    let after_delete = store.list(&prefix, None, 100).await.unwrap().keys;
    assert_eq!(after_delete.len(), 22);
    assert!(!after_delete.contains(&want[3]));
}

/// Generates one test per case for a backend. `$make` is an async fn returning the store and a
/// guard that keeps its resources (a temp directory) alive for the test; `$part` is the backend's
/// multipart part size, so the large-object cases cross it. `ignore = "reason"` marks every case
/// `#[ignore]` (the MinIO run).
macro_rules! blob_suite {
    ($module:ident, $make:path, $part:expr $(, ignore = $reason:literal)?) => {
        $crate::blobs::suite::blob_suite!(@expand $module, $make, $part, [$(#[ignore = $reason])?]);
    };
    (@expand $module:ident, $make:path, $part:expr, $attrs:tt) => {
        mod $module {
            use $crate::blobs::suite as s;

            $crate::blobs::suite::blob_suite!(@case $make, $attrs, round_trips_content_of_every_shape, |st| s::round_trips_content_of_every_shape(st).await);
            $crate::blobs::suite::blob_suite!(@case $make, $attrs, large_object_streams_in_parts, |st| s::large_object_streams_in_parts(st, $part * 2 + 12_345).await);
            $crate::blobs::suite::blob_suite!(@case $make, $attrs, missing_object, |st| s::missing_object(st).await);
            $crate::blobs::suite::blob_suite!(@case $make, $attrs, delete_removes_and_is_repeatable, |st| s::delete_removes_and_is_repeatable(st).await);
            $crate::blobs::suite::blob_suite!(@case $make, $attrs, an_existing_key_is_never_overwritten, |st| s::an_existing_key_is_never_overwritten(st).await);
            $crate::blobs::suite::blob_suite!(@case $make, $attrs, size_limits, |st| s::size_limits(st).await);
            $crate::blobs::suite::blob_suite!(@case $make, $attrs, expected_hash_is_checked, |st| s::expected_hash_is_checked(st).await);
            $crate::blobs::suite::blob_suite!(@case $make, $attrs, a_failed_source_leaves_nothing, |st| s::a_failed_source_leaves_nothing(st, 100_000, 60_000).await);
            $crate::blobs::suite::blob_suite!(@case $make, $attrs, a_failed_source_leaves_nothing_in_a_multipart_upload, |st| s::a_failed_source_leaves_nothing(st, $part * 3, $part * 2 + 1).await);
            $crate::blobs::suite::blob_suite!(@case $make, $attrs, a_put_cancelled_mid_write_leaves_nothing, |st| s::a_put_cancelled_mid_write_leaves_nothing(st, 70_000).await);
            $crate::blobs::suite::blob_suite!(@case $make, $attrs, a_multipart_put_cancelled_mid_write_leaves_nothing, |st| s::a_put_cancelled_mid_write_leaves_nothing(st, $part + 70_000).await);
            $crate::blobs::suite::blob_suite!(@case $make, $attrs, concurrent_puts_of_one_key_have_one_winner, |st| s::concurrent_puts_of_one_key_have_one_winner(st).await);
            $crate::blobs::suite::blob_suite!(@case $make, $attrs, listing_pages_in_order_without_gaps, |st| s::listing_pages_in_order_without_gaps(st).await);
        }
    };
    (@case $make:path, [$($attr:tt)*], $name:ident, |$st:ident| $body:expr) => {
        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        $($attr)*
        async fn $name() {
            let (store, _guard) = $make().await;
            let $st = &store;
            $body;
        }
    };
}
pub(crate) use blob_suite;
