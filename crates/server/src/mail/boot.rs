//! Start-up for mail: load (or, on a fresh install, create) the key and check it against the
//! mail the database already holds. Any failure stops the boot with a clear error; nothing
//! here falls back to a new key.

use std::path::Path;

use russh_bbs_core::mail::{KeyCheckError, MailKeys, MailStore, verify_keys};

use super::key_file::{self, KeyFileError};

/// Why mail could not start.
#[derive(Debug, thiserror::Error)]
pub enum BootError {
    /// The key file is missing (with mail present), damaged or too open.
    #[error(transparent)]
    KeyFile(#[from] KeyFileError),
    /// The key does not match the stored mail.
    #[error(transparent)]
    Check(#[from] KeyCheckError),
    /// Storage failed while checking.
    #[error("could not check stored mail: {0}")]
    Store(#[from] russh_bbs_core::store::StoreError),
    /// The key-loading task failed.
    #[error("the mail key task failed")]
    Task,
}

/// Load the mail key at `path` for `store`: generated only if the file is missing **and** the
/// store holds no mail; then checked against what is stored.
///
/// # Errors
/// [`BootError`].
pub async fn load_keys<S: MailStore>(store: &S, path: &Path) -> Result<MailKeys, BootError> {
    let mail_exists = !store.key_ids_in_use().await?.is_empty();
    let path = path.to_path_buf();
    let keys = tokio::task::spawn_blocking(move || key_file::load_or_create(&path, mail_exists))
        .await
        .map_err(|_| BootError::Task)??;
    verify_keys(store, &keys).await?;
    Ok(keys)
}
