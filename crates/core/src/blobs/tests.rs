//! Unit tests for keys, the trailer, put accounting and the verifying reader.
#![allow(clippy::unwrap_used, clippy::panic)]

use tokio::io::AsyncReadExt;

use super::*;
use crate::store::UnixSeconds;

fn stored(content: &[u8]) -> Vec<u8> {
    let mut acct = PutAccounting::new(&PutOptions::up_to(1 << 20), 1 << 20).unwrap();
    acct.accept(content).unwrap();
    let trailer = acct.finish(UnixSeconds::from_raw(1_700_000_000)).unwrap();
    let mut out = content.to_vec();
    out.extend_from_slice(&trailer.encode());
    out
}

async fn read_all(bytes: Vec<u8>) -> Result<Vec<u8>, BlobError> {
    let total = bytes.len() as u64;
    let mut reader = VerifyingReader::new(std::io::Cursor::new(bytes), total).unwrap();
    let mut out = Vec::new();
    reader
        .read_to_end(&mut out)
        .await
        .map(|_| out)
        .map_err(|e| BlobError::from_io(&e))
}

#[test]
fn ids_are_32_lowercase_hex_and_nothing_else_parses() {
    let id = BlobId::generate();
    assert_eq!(id.as_str().len(), 32);
    assert_eq!(BlobId::parse(id.as_str()).unwrap(), id);
    assert_ne!(BlobId::generate(), id);
    for bad in [
        "",
        "../../etc/passwd",
        "ABCDEF0123456789ABCDEF0123456789",
        "0123456789abcdef0123456789abcde",
        "0123456789abcdef0123456789abcdef0",
        "0123456789abcdef0123456789abcde/",
        "0123456789abcdef0123456789abcdeg",
    ] {
        assert_eq!(BlobId::parse(bad), Err(KeyError::Id), "{bad}");
    }
}

#[test]
fn namespaces_are_short_lowercase_words() {
    assert!(Namespace::new("files").is_ok());
    assert!(Namespace::new("quarantine").is_ok());
    for bad in ["", "Files", "1files", "a/b", "a.b", "..", "waytoolongnamespace", "a b"] {
        assert_eq!(Namespace::new(bad), Err(KeyError::Namespace), "{bad}");
    }
}

#[test]
fn keys_round_trip_and_reject_paths() {
    let key = BlobKey::generate(Namespace::new("files").unwrap());
    assert_eq!(BlobKey::parse(&key.to_string()).unwrap(), key);
    for bad in ["files", "files/", "/abc", "files/../x", "files/ab/cd", "Files/0123456789abcdef0123456789abcdef"] {
        assert_eq!(BlobKey::parse(bad), Err(KeyError::Key), "{bad}");
    }
    let moved = key.in_namespace(Namespace::new("quarantine").unwrap());
    assert_eq!(moved.id(), key.id());
}

#[test]
fn prefix_matches_namespace_and_shard() {
    let ns = Namespace::new("files").unwrap();
    let key = BlobKey::new(ns.clone(), BlobId::parse("ab0123456789abcdef0123456789abcd").unwrap());
    assert!(BlobPrefix::namespace(ns.clone()).matches(&key));
    let hit = BlobPrefix { namespace: ns.clone(), shard: Some(Shard::new("ab").unwrap()) };
    let miss = BlobPrefix { namespace: ns, shard: Some(Shard::new("cd").unwrap()) };
    assert!(hit.matches(&key));
    assert!(!miss.matches(&key));
    assert!(!BlobPrefix::namespace(Namespace::new("other").unwrap()).matches(&key));
    assert_eq!(Shard::new("a"), Err(KeyError::Shard));
}

#[test]
fn trailer_round_trips_and_rejects_damage() {
    let t = Trailer {
        created: UnixSeconds::from_raw(42),
        size: 7,
        sha256: BlobHash::of(b"payload"),
    };
    let bytes = t.encode();
    assert_eq!(Trailer::decode(&bytes).unwrap(), t);
    assert!(t.check_total(7 + TRAILER_LEN as u64).is_ok());
    assert!(t.check_total(8 + TRAILER_LEN as u64).is_err());
    let mut bad = bytes;
    bad[0] ^= 1;
    assert!(matches!(Trailer::decode(&bad), Err(BlobError::Corrupt(_))));
    assert!(matches!(Trailer::decode(&bytes[1..]), Err(BlobError::Corrupt(_))));
}

#[test]
fn accounting_enforces_cap_declared_size_and_hash() {
    // Over the cap: the lower of the option and the backend cap wins.
    let mut a = PutAccounting::new(&PutOptions::up_to(100), 10).unwrap();
    assert_eq!(a.accept(&[0; 10]), Ok(()));
    assert_eq!(a.accept(&[0; 1]), Err(BlobError::TooLarge { max: 10 }));
    // Declared above the cap is refused before reading.
    assert_eq!(
        PutAccounting::new(&PutOptions::exactly(11), 10).unwrap_err(),
        BlobError::TooLarge { max: 10 }
    );
    // Short stream.
    let mut a = PutAccounting::new(&PutOptions::exactly(5), 100).unwrap();
    a.accept(b"abc").unwrap();
    assert_eq!(
        a.finish(UnixSeconds::from_raw(0)).unwrap_err(),
        BlobError::SizeMismatch { declared: 5, actual: 3 }
    );
    // Long stream stops at the first excess chunk.
    let mut a = PutAccounting::new(&PutOptions::exactly(2), 100).unwrap();
    assert!(matches!(a.accept(b"abc"), Err(BlobError::SizeMismatch { declared: 2, .. })));
    // Wrong hash.
    let opts = PutOptions::up_to(100).with_sha256(BlobHash::of(b"other"));
    let mut a = PutAccounting::new(&opts, 100).unwrap();
    a.accept(b"abc").unwrap();
    assert_eq!(a.finish(UnixSeconds::from_raw(0)).unwrap_err(), BlobError::HashMismatch);
    // Right hash.
    let opts = PutOptions::up_to(100).with_sha256(BlobHash::of(b"abc"));
    let mut a = PutAccounting::new(&opts, 100).unwrap();
    a.accept(b"abc").unwrap();
    assert_eq!(a.finish(UnixSeconds::from_raw(9)).unwrap().size, 3);
}

#[tokio::test]
async fn verifying_reader_returns_content_and_detects_every_kind_of_damage() {
    let bytes = stored(b"hello blob");
    assert_eq!(read_all(bytes.clone()).await.unwrap(), b"hello blob");
    assert_eq!(read_all(stored(b"")).await.unwrap(), b"");

    // A flipped content byte.
    let mut flipped = bytes.clone();
    flipped[2] ^= 0x40;
    assert!(matches!(read_all(flipped).await, Err(BlobError::Corrupt(_))));
    // Truncated inside the content and inside the trailer.
    for cut in [3, bytes.len() - 20, bytes.len() - 1] {
        let short = bytes[..cut].to_vec();
        let total = bytes.len() as u64;
        let mut reader = VerifyingReader::new(std::io::Cursor::new(short), total).unwrap();
        let err = reader.read_to_end(&mut Vec::new()).await.unwrap_err();
        assert!(matches!(BlobError::from_io(&err), BlobError::Corrupt(_)), "cut {cut}");
    }
    // A trailer whose hash was swapped.
    let mut swapped = bytes.clone();
    let n = swapped.len();
    swapped[n - 1] ^= 1;
    assert!(matches!(read_all(swapped).await, Err(BlobError::Corrupt(_))));
    // Shorter than a trailer at all.
    assert!(VerifyingReader::new(std::io::Cursor::new(vec![0_u8; 3]), 3).is_err());
}
