//! Hallpass cannot reach the filesystem, the network, the clock or the OS randomness.
#![allow(clippy::unwrap_used)]

use russh_bbs_door::sandbox;

#[test]
fn the_source_and_manifest_pass_the_framework_audit() {
    assert_eq!(sandbox::audit_crate(env!("CARGO_MANIFEST_DIR")), Ok(()));
}

#[test]
fn the_manifest_depends_on_the_framework_and_pure_data_crates_only() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let deps: Vec<&str> = manifest
        .split("[dependencies]")
        .nth(1)
        .and_then(|rest| rest.split("\n[").next())
        .unwrap()
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name.trim()))
        .collect();
    assert_eq!(deps, ["russh-bbs-door", "serde", "toml"]);
}

#[test]
fn the_audit_catches_what_it_is_for_in_this_crates_shape() {
    // The audit is only worth anything if it would fire: check a copy of this crate's layout with
    // a forbidden call added.
    let dir = tempdir();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname='x'\n[lints]\nworkspace = true\n[dependencies]\nrussh-bbs-door = { workspace = true }\n").unwrap();
    std::fs::write(dir.join("src/lib.rs"), "#![forbid(unsafe_code)]\nfn f() { let _ = std::fs::read(\"/etc/passwd\"); let _ = std::time::SystemTime::now(); }\n").unwrap();
    let found = sandbox::audit_crate(dir.to_str().unwrap()).unwrap_err();
    assert!(found.iter().any(|f| f.what.contains("std::fs")));
    assert!(found.iter().any(|f| f.what.contains("SystemTime")));
    std::fs::remove_dir_all(dir).unwrap();
}

fn tempdir() -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("hallpass-audit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    path
}
