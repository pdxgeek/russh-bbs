//! Emberdeep cannot reach the filesystem or the network: the framework's source audit passes, the
//! manifest names only what a door may, and a whole session leaves the store with exactly a save
//! and the shared world.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::sandbox;
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::{DoorEnv, DoorKey};
use russh_bbs_door_emberdeep::Emberdeep;

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

#[tokio::test]
async fn a_whole_session_changes_only_the_save_and_the_world() {
    let store = MemoryDoorStore::new();
    let door = Emberdeep::shared().unwrap();
    let clock: Arc<ManualClock> = Arc::new(ManualClock::new(Day::from_raw(20_000).start()));
    let clock_dyn: Arc<dyn Clock> = clock;
    let env = DoorEnv::for_tests("ann");
    let account = AccountId::from_raw(1);
    let mut running = Running::open(&store, Arc::clone(&door), account, &env, clock_dyn, 99)
        .await
        .unwrap();
    // Make a hero and wander: every key the door understands, and several it does not.
    let script = "1ab\n1123\ns1\x1bd\x1bnesw12345678>rumcqzxfgav\x1b";
    for c in script.chars() {
        let key = match c {
            '\n' => DoorKey::Enter,
            '\x1b' => DoorKey::Escape,
            c => DoorKey::Char(c),
        };
        let _ = running.key(key);
        running.flush(&store).await.unwrap();
    }
    let id = door.info().id.clone();
    assert!(store.load_save(&id, account).await.unwrap().is_some());
    assert!(
        store
            .load_save(&id, AccountId::from_raw(2))
            .await
            .unwrap()
            .is_none()
    );
}
