//! Saltmarch cannot reach the filesystem or the network: the framework's audit passes, and the
//! manifest names only the framework and the pure data crates.

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
fn the_audit_would_catch_what_a_game_must_never_do() {
    // The audit's own tests live in the framework; this pins that the patterns a tide or a
    // map generator might be tempted by are on its list.
    for needle in [
        "SystemTime",
        "thread_rng",
        "std::fs",
        "std::net",
        "Instant::now",
    ] {
        assert!(
            sandbox::FORBIDDEN.iter().any(|(p, _)| p.contains(needle)),
            "{needle}"
        );
    }
}

#[test]
fn the_door_id_is_valid_and_stable() {
    assert!(russh_bbs_door::DoorId::parse(russh_bbs_door_saltmarch::DOOR_ID).is_ok());
    assert_eq!(russh_bbs_door_saltmarch::DOOR_ID, "saltmarch");
}
