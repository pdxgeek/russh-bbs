//! The stored form of an object and the two stream helpers both backends share, so the rules
//! live in one place (D30: the local backend gets no shortcuts).
//!
//! An object is its content followed by a fixed 56-byte trailer:
//!
//! ```text
//! magic "RBBSBLB1" (8) | created, Unix seconds, u64 BE (8) | size, u64 BE (8) | SHA-256 (32)
//! ```
//!
//! The trailer is written last, so it is part of an object only once the whole stream has been
//! read and checked. Putting the hash at the end means a backend never needs to know it before
//! it starts (an S3 multipart upload fixes its metadata when it begins), and a reader can
//! verify as it streams.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, ReadBuf};

use super::{BlobError, BlobHash, BlobMeta, PutOptions};
use crate::store::UnixSeconds;

/// Marks the trailer and its layout version.
const MAGIC: &[u8; 8] = b"RBBSBLB1";

/// Bytes the trailer adds to every stored object.
pub const TRAILER_LEN: usize = 56;

/// The decoded trailer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trailer {
    /// When the object was written.
    pub created: UnixSeconds,
    /// Content length.
    pub size: u64,
    /// SHA-256 of the content.
    pub sha256: BlobHash,
}

impl Trailer {
    /// The 56 bytes to append after the content.
    #[must_use]
    pub fn encode(&self) -> [u8; TRAILER_LEN] {
        let mut out = [0_u8; TRAILER_LEN];
        out[..8].copy_from_slice(MAGIC);
        out[8..16].copy_from_slice(&self.created.as_raw().max(0).cast_unsigned().to_be_bytes());
        out[16..24].copy_from_slice(&self.size.to_be_bytes());
        out[24..].copy_from_slice(self.sha256.as_bytes());
        out
    }

    /// Read a trailer; anything that is not exactly a trailer is [`BlobError::Corrupt`].
    pub fn decode(bytes: &[u8]) -> Result<Self, BlobError> {
        let bytes: &[u8; TRAILER_LEN] = bytes
            .try_into()
            .map_err(|_| BlobError::Corrupt("trailer has the wrong length"))?;
        if &bytes[..8] != MAGIC {
            return Err(BlobError::Corrupt("trailer marker is missing"));
        }
        let word = |range: std::ops::Range<usize>| {
            let mut w = [0_u8; 8];
            w.copy_from_slice(&bytes[range]);
            u64::from_be_bytes(w)
        };
        let mut hash = [0_u8; 32];
        hash.copy_from_slice(&bytes[24..]);
        Ok(Self {
            created: UnixSeconds::from_raw(i64::try_from(word(8..16)).unwrap_or(i64::MAX)),
            size: word(16..24),
            sha256: BlobHash::from_bytes(hash),
        })
    }

    /// Check a decoded trailer against the length of the stored object it came from.
    pub fn check_total(&self, total_len: u64) -> Result<BlobMeta, BlobError> {
        let expect = self
            .size
            .checked_add(TRAILER_LEN as u64)
            .ok_or(BlobError::Corrupt("trailer size is impossible"))?;
        if expect != total_len {
            return Err(BlobError::Corrupt("stored length does not match its trailer"));
        }
        Ok(BlobMeta {
            size: self.size,
            sha256: self.sha256,
            created: self.created,
        })
    }
}

/// Counts, hashes and limits the bytes of one put, so every backend enforces the same rules in
/// the same order. Feed it each chunk with [`accept`](Self::accept), then call
/// [`finish`](Self::finish).
#[derive(Debug)]
pub struct PutAccounting {
    hasher: Sha256,
    seen: u64,
    cap: u64,
    declared: Option<u64>,
    expect: Option<BlobHash>,
}

impl PutAccounting {
    /// Start a put. `backend_cap` is the backend's configured maximum object size; the lower of
    /// it and `options.max_size` applies. A declared size above the cap is refused here, before
    /// a byte is read.
    pub fn new(options: &PutOptions, backend_cap: u64) -> Result<Self, BlobError> {
        let cap = options.max_size.min(backend_cap);
        if options.declared_size.is_some_and(|d| d > cap) {
            return Err(BlobError::TooLarge { max: cap });
        }
        Ok(Self {
            hasher: Sha256::new(),
            seen: 0,
            cap,
            declared: options.declared_size,
            expect: options.expect_sha256,
        })
    }

    /// Count and hash one chunk. Fails as soon as the stream passes the cap or the declared
    /// size, so an endless stream is cut off.
    pub fn accept(&mut self, chunk: &[u8]) -> Result<(), BlobError> {
        let len = chunk.len() as u64;
        let total = self.seen.saturating_add(len);
        if let Some(declared) = self.declared {
            if total > declared {
                return Err(BlobError::SizeMismatch {
                    declared,
                    actual: total,
                });
            }
        }
        if total > self.cap {
            return Err(BlobError::TooLarge { max: self.cap });
        }
        self.hasher.update(chunk);
        self.seen = total;
        Ok(())
    }

    /// Bytes accepted so far.
    #[must_use]
    pub const fn seen(&self) -> u64 {
        self.seen
    }

    /// The stream ended: check the declared size and expected hash and produce the trailer to
    /// append. `now` is the creation time to record.
    pub fn finish(self, now: UnixSeconds) -> Result<Trailer, BlobError> {
        if let Some(declared) = self.declared {
            if self.seen != declared {
                return Err(BlobError::SizeMismatch {
                    declared,
                    actual: self.seen,
                });
            }
        }
        let sha256 = BlobHash::from_bytes(self.hasher.finalize().into());
        if self.expect.is_some_and(|expected| expected != sha256) {
            return Err(BlobError::HashMismatch);
        }
        Ok(Trailer {
            created: now,
            size: self.seen,
            sha256,
        })
    }
}

/// Turn a stored object (content then trailer, `total_len` bytes in all) into a stream of its
/// content that checks itself. At the end of the content it reads the trailer and compares the
/// hash and size it computed; a mismatch is an `InvalidData` error carrying
/// [`BlobError::Corrupt`] instead of a clean end of stream.
#[derive(Debug)]
pub struct VerifyingReader<R> {
    inner: R,
    hasher: Sha256,
    content_left: u64,
    content_len: u64,
    trailer: [u8; TRAILER_LEN],
    trailer_have: usize,
    done: bool,
}

impl<R> VerifyingReader<R> {
    /// Wrap `inner`, which must yield exactly `total_len` bytes. A `total_len` shorter than a
    /// trailer is [`BlobError::Corrupt`].
    pub fn new(inner: R, total_len: u64) -> Result<Self, BlobError> {
        let content_len = total_len
            .checked_sub(TRAILER_LEN as u64)
            .ok_or(BlobError::Corrupt("stored object is shorter than its trailer"))?;
        Ok(Self {
            inner,
            hasher: Sha256::new(),
            content_left: content_len,
            content_len,
            trailer: [0; TRAILER_LEN],
            trailer_have: 0,
            done: false,
        })
    }

    /// The content length this reader will yield.
    #[must_use]
    pub const fn content_len(&self) -> u64 {
        self.content_len
    }
}

fn corrupt(why: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, BlobError::Corrupt(why))
}

impl<R: AsyncRead + Unpin> AsyncRead for VerifyingReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if this.content_left > 0 {
            let room = buf
                .remaining()
                .min(usize::try_from(this.content_left).unwrap_or(usize::MAX));
            if room == 0 {
                return Poll::Ready(Ok(()));
            }
            let mut limited = ReadBuf::new(buf.initialize_unfilled_to(room));
            match Pin::new(&mut this.inner).poll_read(cx, &mut limited) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Ready(Ok(())) => {}
            }
            let n = limited.filled().len();
            if n == 0 {
                return Poll::Ready(Err(corrupt("stored object ended early")));
            }
            this.hasher.update(limited.filled());
            buf.advance(n);
            this.content_left -= n as u64;
            return Poll::Ready(Ok(()));
        }
        while this.trailer_have < TRAILER_LEN && !this.done {
            let mut tb = ReadBuf::new(&mut this.trailer[this.trailer_have..]);
            match Pin::new(&mut this.inner).poll_read(cx, &mut tb) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Ready(Ok(())) => {}
            }
            let n = tb.filled().len();
            if n == 0 {
                return Poll::Ready(Err(corrupt("stored object ended early")));
            }
            this.trailer_have += n;
        }
        if !this.done {
            this.done = true;
            let trailer = Trailer::decode(&this.trailer).map_err(|e| match e {
                BlobError::Corrupt(why) => corrupt(why),
                _ => corrupt("trailer unreadable"),
            })?;
            let actual = BlobHash::from_bytes(this.hasher.clone().finalize().into());
            if trailer.size != this.content_len {
                return Poll::Ready(Err(corrupt("size does not match its trailer")));
            }
            if trailer.sha256 != actual {
                return Poll::Ready(Err(corrupt("content does not match its hash")));
            }
        }
        Poll::Ready(Ok(()))
    }
}
