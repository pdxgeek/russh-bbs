//! Start-up wiring for the blob store (D30, M4-F1): build the backend `[blobs]` names. Nothing
//! uses it yet; M4-F2 (file areas) takes the store from here.

use anyhow::Context;
use russh_bbs_store::blobs::{Blobs, LocalBlobStore, S3BlobStore, S3Credentials, S3Settings};

use crate::config::{BlobBackend, BlobsConfig};

/// Open the configured store. A bucket that cannot be reached, or a directory that cannot be
/// created, is a boot error and never falls back to the other backend. Error text never holds
/// the access keys: they come from `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` /
/// `AWS_SESSION_TOKEN` (or the platform's role) and are scrubbed by the backend.
pub async fn open(
    config: &BlobsConfig,
    local_dir: Option<&std::path::Path>,
) -> anyhow::Result<Blobs> {
    match config.backend {
        BlobBackend::Local => {
            let dir = local_dir.context("the local blob directory was not resolved")?;
            let store = LocalBlobStore::open(dir, config.max_object_bytes)
                .await
                .with_context(|| format!("opening the blob directory {}", dir.display()))?;
            Ok(Blobs::Local(store))
        }
        BlobBackend::S3 => {
            let bucket = config
                .bucket
                .clone()
                .context("blobs.bucket is required with blobs.backend = \"s3\"")?;
            let credentials = S3Credentials::from_env();
            let store = S3BlobStore::connect(S3Settings {
                endpoint: config.endpoint.clone(),
                region: config.region.clone(),
                bucket,
                path_style: config.path_style,
                ca_file: config.ca_file.clone(),
                credentials,
                multipart_threshold: config.multipart_threshold_bytes,
                max_object_bytes: config.max_object_bytes,
            })
            .await
            .context("connecting to the blob bucket")?;
            Ok(Blobs::S3(store))
        }
    }
}
