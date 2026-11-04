//! Harnesses for the shared blob suite, plus the checks that belong to one backend.
//!
//! The local backend runs everywhere. The S3 tests need an S3-compatible server and are
//! `#[ignore]`d so a plain `cargo test --workspace` needs none; run them with
//! `cargo test -p russh-bbs-store blobs -- --ignored`. They **fail** (they do not skip) when
//! the variables are missing:
//!
//! - `RUSSH_BBS_TEST_S3_ENDPOINT`: for example `http://127.0.0.1:19000`
//! - `RUSSH_BBS_TEST_S3_BUCKET`: a bucket that already exists
//! - `RUSSH_BBS_TEST_S3_ACCESS_KEY` and `RUSSH_BBS_TEST_S3_SECRET_KEY`
//!
//! The recipe for a MinIO container is in `docs/deploy.md` ("Testing the S3 blob backend").
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;

use russh_bbs_core::blobs::{BlobError, BlobHash, BlobKey, BlobStore, Namespace, PutOptions, TRAILER_LEN};
use object_store::ObjectStoreExt;
use tokio::io::AsyncReadExt;

use super::suite::{Pattern, blob_suite, fresh_namespace};
use super::{LocalBlobStore, MIN_PART_BYTES, S3BlobStore, S3Credentials, S3Settings};

// ---- local ---------------------------------------------------------------------------------

async fn local() -> (LocalBlobStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalBlobStore::open(dir.path().join("blobs"), 1 << 30).await.unwrap();
    (store, dir)
}

blob_suite!(local_suite, crate::blobs::tests::local, crate::blobs::MIN_PART_BYTES);

fn files_under(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}

async fn drain<S: BlobStore>(store: &S, key: &BlobKey) -> Result<usize, BlobError> {
    let mut r = store.get(key).await?;
    let mut buf = Vec::new();
    r.reader.read_to_end(&mut buf).await.map_err(|e| BlobError::from_io(&e))?;
    Ok(buf.len())
}

#[tokio::test]
async fn local_objects_land_in_a_sharded_private_layout_with_no_temp_left() {
    let (store, dir) = local().await;
    let root = dir.path().join("blobs");
    let key = BlobKey::generate(Namespace::new("files").unwrap());
    store.put(&key, Pattern::new(1000, 1), &PutOptions::up_to(5000)).await.unwrap();

    let want = root.join("files").join(key.id().shard().as_str()).join(key.id().as_str());
    assert_eq!(files_under(&root), vec![want.clone()], "one object file and no temp file");
    assert_eq!(std::fs::metadata(&want).unwrap().len(), 1000 + TRAILER_LEN as u64);

    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&want).unwrap().permissions().mode() & 0o777, 0o600);
    for d in [&root, &root.join("files"), want.parent().unwrap()] {
        assert_eq!(std::fs::metadata(d).unwrap().permissions().mode() & 0o777, 0o700, "{d:?}");
    }
}

#[tokio::test]
async fn local_failed_puts_leave_no_file_behind() {
    let (store, dir) = local().await;
    let key = BlobKey::generate(fresh_namespace());
    let r = store.put(&key, Pattern::new(9000, 1), &PutOptions::exactly(9001)).await;
    assert!(matches!(r, Err(BlobError::SizeMismatch { .. })));
    let r = store.put(&key, Pattern::new(9000, 1), &PutOptions::up_to(10)).await;
    assert!(matches!(r, Err(BlobError::TooLarge { .. })));
    assert_eq!(files_under(&dir.path().join("blobs")), Vec::<std::path::PathBuf>::new());
}

#[tokio::test]
async fn local_tampering_with_stored_bytes_is_caught_on_read() {
    let (store, dir) = local().await;
    let ns = Namespace::new("files").unwrap();
    let path_of = |k: &BlobKey| {
        dir.path().join("blobs").join("files").join(k.id().shard().as_str()).join(k.id().as_str())
    };
    let put = |seed| {
        let store = store.clone();
        let ns = ns.clone();
        async move {
            let k = BlobKey::generate(ns);
            store.put(&k, Pattern::new(200_000, seed), &PutOptions::up_to(1 << 20)).await.unwrap();
            k
        }
    };

    // A flipped content byte: head (trailer only) still works, reading fails at the end.
    let k = put(1).await;
    let mut bytes = std::fs::read(path_of(&k)).unwrap();
    bytes[150_000] ^= 0x01;
    std::fs::write(path_of(&k), &bytes).unwrap();
    assert!(store.head(&k).await.is_ok());
    assert!(matches!(drain(&store, &k).await, Err(BlobError::Corrupt(_))));

    // Truncated: both head and get notice.
    let k = put(2).await;
    let bytes = std::fs::read(path_of(&k)).unwrap();
    std::fs::write(path_of(&k), &bytes[..bytes.len() - 100]).unwrap();
    assert!(matches!(store.head(&k).await, Err(BlobError::Corrupt(_))));
    assert!(matches!(drain(&store, &k).await, Err(BlobError::Corrupt(_))));

    // Extra bytes appended.
    let k = put(3).await;
    let mut bytes = std::fs::read(path_of(&k)).unwrap();
    bytes.extend_from_slice(b"junk");
    std::fs::write(path_of(&k), &bytes).unwrap();
    assert!(matches!(store.head(&k).await, Err(BlobError::Corrupt(_))));

    // A file that is not an object at all.
    let k = put(4).await;
    std::fs::write(path_of(&k), b"x").unwrap();
    assert!(matches!(store.head(&k).await, Err(BlobError::Corrupt(_))));
}

#[tokio::test]
async fn local_start_up_removes_old_temp_files_and_never_shows_any() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("blobs");
    let store = LocalBlobStore::open(&root, 1 << 20).await.unwrap();
    let old = root.join("tmp").join("old.part");
    let fresh = root.join("tmp").join("fresh.part");
    std::fs::write(&old, b"half a write").unwrap();
    std::fs::write(&fresh, b"a write in progress").unwrap();
    let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
    std::fs::File::options().write(true).open(&old).unwrap().set_modified(two_hours_ago).unwrap();
    drop(store);

    let store = LocalBlobStore::open(&root, 1 << 20).await.unwrap();
    assert!(!old.exists(), "a stale temp file is swept");
    assert!(fresh.exists(), "a recent temp file may belong to a live write");
    let ns = fresh_namespace();
    let page = store
        .list(&russh_bbs_core::blobs::BlobPrefix::namespace(ns), None, 10)
        .await
        .unwrap();
    assert!(page.keys.is_empty());
}

#[tokio::test]
async fn local_listing_ignores_files_that_are_not_objects() {
    let (store, dir) = local().await;
    let ns = Namespace::new("files").unwrap();
    let key = BlobKey::generate(ns.clone());
    store.put(&key, Pattern::new(5, 1), &PutOptions::up_to(10)).await.unwrap();
    let shard = dir.path().join("blobs").join("files").join(key.id().shard().as_str());
    std::fs::write(shard.join("not-an-id"), b"x").unwrap();
    std::fs::write(shard.join("..hidden"), b"x").unwrap();
    std::fs::create_dir_all(dir.path().join("blobs").join("files").join("zz")).unwrap();
    let keys = store
        .list(&russh_bbs_core::blobs::BlobPrefix::namespace(ns), None, 100)
        .await
        .unwrap()
        .keys;
    assert_eq!(keys, vec![key]);
}

#[tokio::test]
async fn local_backend_cap_applies_even_when_the_caller_asks_for_more() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalBlobStore::open(dir.path().join("b"), 100).await.unwrap();
    let key = BlobKey::generate(fresh_namespace());
    let r = store.put(&key, Pattern::new(101, 1), &PutOptions::up_to(u64::MAX)).await;
    assert_eq!(r, Err(BlobError::TooLarge { max: 100 }));
    store.put(&key, Pattern::new(100, 1), &PutOptions::up_to(u64::MAX)).await.unwrap();
}

#[tokio::test]
async fn local_hash_in_head_is_the_content_hash() {
    let (store, _dir) = local().await;
    let key = BlobKey::generate(fresh_namespace());
    store.put(&key, Pattern::new(4321, 8), &PutOptions::up_to(1 << 20)).await.unwrap();
    assert_eq!(store.head(&key).await.unwrap().sha256, Pattern::hash(4321, 8));
    assert_ne!(store.head(&key).await.unwrap().sha256, BlobHash::of(b""));
}

// ---- S3 (MinIO) ----------------------------------------------------------------------------

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name} to run the S3 blob tests (docs/deploy.md)"))
}

fn s3_settings() -> S3Settings {
    S3Settings {
        endpoint: Some(env("RUSSH_BBS_TEST_S3_ENDPOINT")),
        region: None,
        bucket: env("RUSSH_BBS_TEST_S3_BUCKET"),
        path_style: true,
        ca_file: None,
        credentials: Some(S3Credentials::new(
            env("RUSSH_BBS_TEST_S3_ACCESS_KEY"),
            env("RUSSH_BBS_TEST_S3_SECRET_KEY"),
            None,
        )),
        multipart_threshold: MIN_PART_BYTES,
        max_object_bytes: 1 << 30,
    }
}

async fn s3() -> (S3BlobStore, ()) {
    (S3BlobStore::connect(s3_settings()).await.unwrap(), ())
}

blob_suite!(
    s3_suite,
    crate::blobs::tests::s3,
    crate::blobs::MIN_PART_BYTES,
    ignore = "needs an S3-compatible server: set RUSSH_BBS_TEST_S3_ENDPOINT (see docs/deploy.md)"
);

#[tokio::test]
#[ignore = "needs an S3-compatible server: set RUSSH_BBS_TEST_S3_ENDPOINT (see docs/deploy.md)"]
async fn s3_objects_are_stored_with_the_shared_trailer_and_tampering_is_caught() {
    let (store, ()) = s3().await;
    let key = BlobKey::generate(fresh_namespace());
    let meta = store.put(&key, Pattern::new(3000, 4), &PutOptions::up_to(1 << 20)).await.unwrap();

    // The raw object is the content then the trailer, at `<ns>/<shard>/<id>`.
    let path = object_store::path::Path::from(format!(
        "{}/{}/{}",
        key.namespace(),
        key.id().shard().as_str(),
        key.id()
    ));
    let raw = store.raw().get(&path).await.unwrap().bytes().await.unwrap();
    assert_eq!(raw.len(), 3000 + TRAILER_LEN);
    assert_eq!(meta.sha256, Pattern::hash(3000, 4));

    // Overwrite it behind the backend's back with a flipped content byte.
    let mut bad = raw.to_vec();
    bad[10] ^= 0x01;
    store.raw().put(&path, bad.into()).await.unwrap();
    assert!(store.head(&key).await.is_ok());
    assert!(matches!(drain(&store, &key).await, Err(BlobError::Corrupt(_))));

    // And truncated.
    store.raw().put(&path, raw.slice(..raw.len() - 9).into()).await.unwrap();
    assert!(matches!(store.head(&key).await, Err(BlobError::Corrupt(_))));
    assert!(matches!(drain(&store, &key).await, Err(BlobError::Corrupt(_))));
    store.delete(&key).await.unwrap();
}

#[tokio::test]
#[ignore = "needs an S3-compatible server: set RUSSH_BBS_TEST_S3_ENDPOINT (see docs/deploy.md)"]
async fn s3_errors_never_contain_the_credentials() {
    let good = s3_settings();
    let secret = "super-secret-key-ABC123xyz";
    let access = "AKIAFAKEFAKEFAKE0000";

    // Wrong credentials: the connection check fails and says nothing about the keys.
    let mut s = good.clone();
    s.credentials = Some(S3Credentials::new(access.to_owned(), secret.to_owned(), Some("tok-ZZZ".to_owned())));
    let err = S3BlobStore::connect(s.clone()).await.unwrap_err().to_string();
    assert!(!err.contains(secret) && !err.contains(access) && !err.contains("tok-ZZZ"), "{err}");
    assert!(!format!("{s:?}").contains(secret), "Debug of the settings must hide the keys");

    // A bucket that does not exist.
    let mut s = good.clone();
    s.bucket = "russh-bbs-no-such-bucket".to_owned();
    let err = S3BlobStore::connect(s).await.unwrap_err().to_string();
    assert!(!err.contains(&env("RUSSH_BBS_TEST_S3_SECRET_KEY")), "{err}");

    // An unreachable endpoint.
    let mut s = good.clone();
    s.endpoint = Some("http://127.0.0.1:1".to_owned());
    assert!(matches!(S3BlobStore::connect(s).await, Err(BlobError::Backend(_))));
}

#[tokio::test]
async fn s3_settings_are_validated_before_any_connection() {
    let base = S3Settings {
        endpoint: Some("http://127.0.0.1:1".to_owned()),
        region: None,
        bucket: "b".to_owned(),
        path_style: true,
        ca_file: None,
        credentials: None,
        multipart_threshold: MIN_PART_BYTES,
        max_object_bytes: 1 << 30,
    };
    let mut s = base.clone();
    s.multipart_threshold = MIN_PART_BYTES - 1;
    assert!(matches!(S3BlobStore::connect(s).await, Err(BlobError::Backend(m)) if m.contains("multipart")));
    let mut s = base.clone();
    s.max_object_bytes = MIN_PART_BYTES * 10_000;
    assert!(matches!(S3BlobStore::connect(s).await, Err(BlobError::Backend(m)) if m.contains("10,000")));
    let mut s = base;
    s.ca_file = Some("/nonexistent/ca.pem".into());
    assert!(matches!(S3BlobStore::connect(s).await, Err(BlobError::Backend(m)) if m.contains("ca_file")));
}
