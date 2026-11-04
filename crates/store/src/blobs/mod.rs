//! Blob store backends (D30, M4-F1): a local directory and an S3-compatible bucket, both
//! implementing `russh_bbs_core::blobs::BlobStore` and held to the same contract by one shared
//! suite.

mod local;
mod s3;

#[cfg(test)]
mod suite;
#[cfg(test)]
mod tests;

pub use local::LocalBlobStore;
pub use s3::{MIN_PART_BYTES, S3BlobStore, S3Credentials, S3Reader, S3Settings};

use russh_bbs_core::blobs::{
    BlobError, BlobKey, BlobMeta, BlobPrefix, BlobRead, BlobStore, ListPage, PutOptions,
};
use tokio::io::{AsyncRead, ReadBuf};

/// The configured blob store: the server picks one at start from `[blobs]`.
#[derive(Debug, Clone)]
pub enum Blobs {
    /// A directory under the data directory.
    Local(LocalBlobStore),
    /// An S3-compatible bucket.
    S3(S3BlobStore),
}

impl Blobs {
    /// Which backend this is, for the start-up log line.
    #[must_use]
    pub const fn backend_name(&self) -> &'static str {
        match self {
            Self::Local(_) => "local",
            Self::S3(_) => "s3",
        }
    }
}

/// The verifying stream of either backend.
pub enum BlobsReader {
    /// From the local backend.
    Local(<LocalBlobStore as BlobStore>::Reader),
    /// From the S3 backend.
    S3(S3Reader),
}

impl AsyncRead for BlobsReader {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Local(r) => std::pin::Pin::new(r).poll_read(cx, buf),
            Self::S3(r) => std::pin::Pin::new(r).poll_read(cx, buf),
        }
    }
}

impl BlobStore for Blobs {
    type Reader = BlobsReader;

    async fn put<R>(
        &self,
        key: &BlobKey,
        body: R,
        options: &PutOptions,
    ) -> Result<BlobMeta, BlobError>
    where
        R: AsyncRead + Send + Unpin,
    {
        match self {
            Self::Local(s) => s.put(key, body, options).await,
            Self::S3(s) => s.put(key, body, options).await,
        }
    }

    async fn get(&self, key: &BlobKey) -> Result<BlobRead<BlobsReader>, BlobError> {
        Ok(match self {
            Self::Local(s) => {
                let r = s.get(key).await?;
                BlobRead {
                    size: r.size,
                    reader: BlobsReader::Local(r.reader),
                }
            }
            Self::S3(s) => {
                let r = s.get(key).await?;
                BlobRead {
                    size: r.size,
                    reader: BlobsReader::S3(r.reader),
                }
            }
        })
    }

    async fn head(&self, key: &BlobKey) -> Result<BlobMeta, BlobError> {
        match self {
            Self::Local(s) => s.head(key).await,
            Self::S3(s) => s.head(key).await,
        }
    }

    async fn delete(&self, key: &BlobKey) -> Result<(), BlobError> {
        match self {
            Self::Local(s) => s.delete(key).await,
            Self::S3(s) => s.delete(key).await,
        }
    }

    async fn list(
        &self,
        prefix: &BlobPrefix,
        after: Option<&BlobKey>,
        limit: usize,
    ) -> Result<ListPage, BlobError> {
        match self {
            Self::Local(s) => s.list(prefix, after, limit).await,
            Self::S3(s) => s.list(prefix, after, limit).await,
        }
    }
}
