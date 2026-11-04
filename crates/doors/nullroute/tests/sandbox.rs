//! Nullroute cannot reach the filesystem or the network, and a whole session changes only the
//! save and the world.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::sandbox;
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::{DoorEnv, DoorKey};
use russh_bbs_door_nullroute::Nullroute;

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
fn the_forbidden_list_names_what_a_game_might_reach_for() {
    // The audit is the framework's own (its tests prove it flags these); here we only prove the
    // forbidden list names the things a game might reach for, and that this crate's text has none.
    for needle in [
        "std::fs",
        "std::net",
        "SystemTime",
        "thread_rng",
        "println!",
    ] {
        assert!(
            sandbox::FORBIDDEN.iter().any(|(p, _)| *p == needle),
            "{needle}"
        );
    }
}

#[tokio::test]
async fn a_whole_session_changes_only_the_save_and_the_world() {
    let store = MemoryDoorStore::new();
    let door = Nullroute::shared().unwrap();
    let clock: Arc<ManualClock> = Arc::new(ManualClock::new(Day::from_raw(20_000).start()));
    let env = DoorEnv::for_tests("ann");
    let ann = AccountId::from_raw(1);
    let mut run = Running::open(
        &store,
        door.clone(),
        ann,
        &env,
        clock.clone() as Arc<dyn Clock>,
        9,
    )
    .await
    .unwrap();
    // Every key the door understands in every view, hostile text, and keys it does not.
    let keys: Vec<DoorKey> = "cdsrhl1234567890abcdefnpxjwtbo\u{1b}\u{7}\r%../..//etc/passwd"
        .chars()
        .map(DoorKey::Char)
        .chain([
            DoorKey::Up,
            DoorKey::Down,
            DoorKey::Left,
            DoorKey::Right,
            DoorKey::Enter,
            DoorKey::Backspace,
            DoorKey::Escape,
        ])
        .collect();
    for key in keys.iter().chain(keys.iter()) {
        run.key(*key);
        run.flush(&store).await.unwrap();
    }
    let id = door.info().id.clone();
    assert!(store.load_save(&id, ann).await.unwrap().is_some());
    assert!(
        store
            .load_save(&id, AccountId::from_raw(2))
            .await
            .unwrap()
            .is_none()
    );
}
