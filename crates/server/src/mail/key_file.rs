//! The mail key file (D78, D90): 32 bytes in a small checked format (`core::mail::MailKey`),
//! kept beside the host key with mode 0600 and never in the database or a log.
//!
//! * **Generated only on a fresh install.** A missing file is created only when the database
//!   holds no mail. If mail exists and the file is gone, start-up stops with an error that says
//!   to restore the file; it never makes a new key, because a new key would turn every stored
//!   message into an unreadable entry.
//! * **Never overwritten.** An existing file that is damaged, the wrong size, not a mail key
//!   file or group/world accessible is refused with a clear error; the file is left as it is.
//! * **Not logged.** Errors name the path and the reason, never bytes. The key lives in a
//!   [`MailKey`] (zeroed on drop, `Debug` shows nothing), and the buffers used to read and write
//!   the file are zeroing buffers.
//!
//! Blocking file I/O: call it before serving or from `spawn_blocking`.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use russh_bbs_core::mail::{FILE_BYTES, KeyId, MailError, MailKey, MailKeys};
use zeroize::Zeroizing;

/// Why the mail key could not be loaded or created.
#[derive(Debug, thiserror::Error)]
pub enum KeyFileError {
    /// A filesystem operation failed.
    #[error("mail key file {path}: {source}")]
    Io {
        /// The key path.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// The file exists but is not a usable mail key file. It is left untouched.
    #[error(
        "mail key file {path} is not usable ({why}); it was not changed. Restore it from your backup"
    )]
    Invalid {
        /// The key path.
        path: PathBuf,
        /// Why, without any key bytes.
        why: &'static str,
    },
    /// The file is readable by other users.
    #[error(
        "mail key file {path} has mode {mode:o}; it must not be accessible to group or others (chmod 600)"
    )]
    TooPermissive {
        /// The key path.
        path: PathBuf,
        /// The Unix mode bits found.
        mode: u32,
    },
    /// The key file is missing but the database holds mail sealed with it.
    #[error(
        "the database holds mail but the mail key file {path} is missing. A new key is never made \
         for an existing board, because it could not open that mail. Restore the key file from \
         your backup"
    )]
    MissingWithMail {
        /// The key path.
        path: PathBuf,
    },
    /// Generating the key failed.
    #[error("could not generate the mail key: {0}")]
    Generate(#[from] MailError),
}

/// Load the key at `path`, or create it if the file does not exist **and** `mail_exists` is
/// false. The key's id is [`KeyId::FIRST`] on a new file.
///
/// # Errors
/// [`KeyFileError`]. [`KeyFileError::MissingWithMail`] when the file is missing and mail exists.
pub fn load_or_create(path: &Path, mail_exists: bool) -> Result<MailKeys, KeyFileError> {
    match fs::symlink_metadata(path) {
        Ok(_) => load(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if mail_exists {
                return Err(KeyFileError::MissingWithMail {
                    path: path.to_path_buf(),
                });
            }
            create(path)
        }
        Err(source) => Err(KeyFileError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn io_err(path: &Path) -> impl Fn(io::Error) -> KeyFileError + '_ {
    move |source| KeyFileError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn load(path: &Path) -> Result<MailKeys, KeyFileError> {
    check_permissions(path)?;
    let file = fs::File::open(path).map_err(io_err(path))?;
    // One byte past the size, so a file that is too long is seen as too long and a file like
    // /dev/zero is not read to the end.
    let mut bytes = Zeroizing::new(Vec::with_capacity(FILE_BYTES + 1));
    file.take(FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_err(path))?;
    let (id, key) = MailKey::from_file_bytes(&bytes).map_err(|error| KeyFileError::Invalid {
        path: path.to_path_buf(),
        why: match error {
            MailError::KeyFile(why) => why,
            _ => "unreadable",
        },
    })?;
    Ok(MailKeys::single(id, key))
}

fn create(path: &Path) -> Result<MailKeys, KeyFileError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        create_private_dir(parent).map_err(io_err(path))?;
    }
    let key = MailKey::generate()?;
    let bytes = key.to_file_bytes(KeyId::FIRST);
    // `create_new`: two racing first starts cannot overwrite each other's key, and an existing
    // file is never replaced.
    let mut file = private_file_options()
        .create_new(true)
        .open(path)
        .map_err(io_err(path))?;
    file.write_all(&bytes).map_err(io_err(path))?;
    file.sync_all().map_err(io_err(path))?;
    Ok(MailKeys::single(KeyId::FIRST, key))
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
fn check_permissions(path: &Path) -> Result<(), KeyFileError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .map_err(io_err(path))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(KeyFileError::TooPermissive {
            path: path.to_path_buf(),
            mode: mode & 0o777,
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_permissions(_path: &Path) -> Result<(), KeyFileError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn a_fresh_install_makes_a_0600_key_and_the_next_start_loads_the_same_one() {
        let dir = dir();
        let path = dir.path().join("sub").join("mail.key");
        let first = load_or_create(&path, false).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(bytes.len(), FILE_BYTES);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        let second = load_or_create(&path, true).unwrap();
        assert_eq!(first.active(), second.active());
        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "the file was not rewritten"
        );
    }

    #[test]
    fn a_missing_file_with_mail_is_an_error_and_makes_no_file() {
        let dir = dir();
        let path = dir.path().join("mail.key");
        let error = load_or_create(&path, true).unwrap_err();
        assert!(
            matches!(error, KeyFileError::MissingWithMail { .. }),
            "{error}"
        );
        assert!(error.to_string().contains("Restore"));
        assert!(!path.exists(), "no key was made");
    }

    #[test]
    fn a_damaged_file_is_refused_and_left_alone_even_with_no_mail() {
        let dir = dir();
        let path = dir.path().join("mail.key");
        load_or_create(&path, false).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        bytes[20] ^= 1;
        fs::write(&path, &bytes).unwrap();
        let error = load_or_create(&path, false).unwrap_err();
        assert!(matches!(error, KeyFileError::Invalid { .. }), "{error}");
        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "never replaced with a new key"
        );
        for bad in [
            Vec::new(),
            vec![0_u8; FILE_BYTES],
            b"not a key".to_vec(),
            vec![1; 4096],
        ] {
            fs::write(&path, &bad).unwrap();
            assert!(matches!(
                load_or_create(&path, false).unwrap_err(),
                KeyFileError::Invalid { .. }
            ));
            assert_eq!(fs::read(&path).unwrap(), bad);
        }
    }

    #[test]
    fn an_error_never_shows_key_bytes() {
        let dir = dir();
        let path = dir.path().join("mail.key");
        load_or_create(&path, false).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        let key_hex = bytes[14..46].iter().fold(String::new(), |mut hex, b| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{b:02x}");
            hex
        });
        bytes[FILE_BYTES - 1] ^= 1;
        fs::write(&path, &bytes).unwrap();
        let text = format!(
            "{:?} {}",
            load_or_create(&path, false),
            load_or_create(&path, false).unwrap_err()
        );
        assert!(!text.contains(&key_hex));
    }

    #[cfg(unix)]
    #[test]
    fn a_group_or_world_readable_file_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = dir();
        let path = dir.path().join("mail.key");
        load_or_create(&path, false).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(matches!(
            load_or_create(&path, false).unwrap_err(),
            KeyFileError::TooPermissive { mode: 0o640, .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn an_endless_file_is_not_read_to_the_end() {
        use std::os::unix::fs::PermissionsExt;
        let dir = dir();
        let path = dir.path().join("mail.key");
        std::os::unix::fs::symlink("/dev/zero", &path).unwrap();
        // The link's target is /dev/zero (mode 0666 is not our business; it is refused either
        // way, quickly, without reading it all).
        let _ = fs::Permissions::from_mode(0o600);
        assert!(load_or_create(&path, false).is_err());
    }
}
