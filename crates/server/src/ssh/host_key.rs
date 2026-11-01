//! The persistent SSH host key.
//!
//! The key is generated on first start and reused afterwards, so the fingerprint a sysop
//! publishes stays valid (threat model: "Host key is persistent"). It is Ed25519 (D20), stored
//! in OpenSSH format with mode 0600 outside the repository. The private key is never logged:
//! [`HostKey`] has a redacting `Debug` and exposes only the public fingerprint.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, HashAlg, PrivateKey, PublicKey};
use russh_bbs_core::access::{Fingerprint, InvalidFingerprint};

/// Why the host key could not be loaded or created.
#[derive(Debug, thiserror::Error)]
pub enum HostKeyError {
    /// A filesystem operation failed.
    #[error("host key file {path}: {source}")]
    Io {
        /// The key path.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// The file exists but is not a usable OpenSSH private key.
    #[error("host key file {path} is not a valid unencrypted Ed25519 OpenSSH key")]
    Invalid {
        /// The key path.
        path: PathBuf,
    },
    /// The file is readable by other users; refusing to use a key that may have leaked.
    #[error(
        "host key file {path} has mode {mode:o}; it must not be accessible to group or others (chmod 600)"
    )]
    TooPermissive {
        /// The key path.
        path: PathBuf,
        /// The Unix mode bits found.
        mode: u32,
    },
    /// Key generation or encoding failed.
    #[error("could not generate or encode the host key: {0}")]
    Generate(String),
}

/// The server's host key pair.
pub struct HostKey {
    key: PrivateKey,
    fingerprint: Fingerprint,
}

impl fmt::Debug for HostKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostKey")
            .field("fingerprint", &self.fingerprint.to_string())
            .finish_non_exhaustive()
    }
}

impl HostKey {
    /// Load the key at `path`, or generate and persist one if the file does not exist.
    ///
    /// The file is created with mode 0600 and its parent directory (mode 0700) if missing. An
    /// existing file that is group- or world-accessible is refused, as OpenSSH does.
    ///
    /// This does blocking file I/O; call it before serving or from `spawn_blocking`.
    pub fn load_or_create(path: &Path) -> Result<Self, HostKeyError> {
        let io_err = |source| HostKeyError::Io {
            path: path.to_path_buf(),
            source,
        };
        match fs::metadata(path) {
            Ok(_) => Self::load(path),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Self::create(path),
            Err(e) => Err(io_err(e)),
        }
    }

    fn load(path: &Path) -> Result<Self, HostKeyError> {
        check_permissions(path)?;
        let text = fs::read_to_string(path).map_err(|source| HostKeyError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let key = PrivateKey::from_openssh(text.as_bytes()).map_err(|_| HostKeyError::Invalid {
            path: path.to_path_buf(),
        })?;
        if key.is_encrypted() || key.algorithm() != Algorithm::Ed25519 {
            return Err(HostKeyError::Invalid {
                path: path.to_path_buf(),
            });
        }
        Self::from_key(key)
    }

    fn from_key(key: PrivateKey) -> Result<Self, HostKeyError> {
        let fingerprint =
            fingerprint_of(key.public_key()).map_err(|e| HostKeyError::Generate(e.to_string()))?;
        Ok(Self { key, fingerprint })
    }

    fn create(path: &Path) -> Result<Self, HostKeyError> {
        let io_err = |source| HostKeyError::Io {
            path: path.to_path_buf(),
            source,
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            create_private_dir(parent).map_err(io_err)?;
        }
        let key = generate()?;
        let pem = key
            .to_openssh(LineEnding::LF)
            .map_err(|e| HostKeyError::Generate(e.to_string()))?;
        // `create_new` so two racing first starts cannot overwrite each other's key.
        let mut file = private_file_options()
            .create_new(true)
            .open(path)
            .map_err(io_err)?;
        file.write_all(pem.as_bytes()).map_err(io_err)?;
        file.sync_all().map_err(io_err)?;
        Self::from_key(key)
    }

    /// The SHA-256 fingerprint of the public key, for the sysop to publish.
    #[must_use]
    pub fn fingerprint(&self) -> &Fingerprint {
        &self.fingerprint
    }

    pub(crate) fn private_key(&self) -> &PrivateKey {
        &self.key
    }
}

/// The OpenSSH SHA-256 fingerprint of a public key, as the core newtype.
pub(crate) fn fingerprint_of(key: &PublicKey) -> Result<Fingerprint, InvalidFingerprint> {
    Fingerprint::parse(&key.fingerprint(HashAlg::Sha256).to_string())
}

fn generate() -> Result<PrivateKey, HostKeyError> {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
        .map_err(|e| HostKeyError::Generate(e.to_string()))
}

#[cfg(unix)]
fn private_file_options() -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.write(true).mode(0o600);
    options
}

#[cfg(not(unix))]
fn private_file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true);
    options
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)
}

#[cfg(unix)]
fn check_permissions(path: &Path) -> Result<(), HostKeyError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .map_err(|source| HostKeyError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(HostKeyError::TooPermissive {
            path: path.to_path_buf(),
            mode: mode & 0o777,
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_permissions(_path: &Path) -> Result<(), HostKeyError> {
    Ok(())
}

#[cfg(test)]
mod tests;
