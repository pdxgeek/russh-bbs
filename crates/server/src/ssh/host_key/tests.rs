#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::{HostKey, HostKeyError};

#[test]
fn first_start_generates_and_restart_reuses_the_same_fingerprint() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keys").join("host_ed25519_key");

    let first = HostKey::load_or_create(&path).unwrap();
    assert!(path.exists(), "key file is written on first start");
    let second = HostKey::load_or_create(&path).unwrap();

    assert_eq!(first.fingerprint(), second.fingerprint());
}

#[test]
fn different_paths_get_different_keys() {
    let dir = tempfile::tempdir().unwrap();
    let a = HostKey::load_or_create(&dir.path().join("a")).unwrap();
    let b = HostKey::load_or_create(&dir.path().join("b")).unwrap();
    assert_ne!(a.fingerprint(), b.fingerprint());
}

#[cfg(unix)]
#[test]
fn key_file_is_0600_and_its_directory_0700() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private").join("host_key");
    HostKey::load_or_create(&path).unwrap();

    let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let dir_mode = std::fs::metadata(path.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(file_mode, 0o600);
    assert_eq!(dir_mode, 0o700);
}

#[cfg(unix)]
#[test]
fn a_group_readable_key_file_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("host_key");
    HostKey::load_or_create(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

    let err = HostKey::load_or_create(&path).unwrap_err();
    assert!(matches!(
        err,
        HostKeyError::TooPermissive { mode: 0o640, .. }
    ));
}

#[test]
fn a_garbage_key_file_is_refused_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("host_key");
    std::fs::write(&path, "not a key").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let err = HostKey::load_or_create(&path).unwrap_err();
    assert!(matches!(err, HostKeyError::Invalid { .. }));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "not a key");
}

#[test]
fn debug_output_never_contains_key_material() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("host_key");
    let key = HostKey::load_or_create(&path).unwrap();

    let debug = format!("{key:?}");
    let pem = std::fs::read_to_string(&path).unwrap();
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    assert!(debug.contains("SHA256:"));
    assert!(!debug.contains("PRIVATE"));
    assert!(!debug.contains(&body[..40]));
}
