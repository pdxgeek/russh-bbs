//! The example door cannot reach the filesystem or the network.
//!
//! Three checks, each of a different thing:
//! 1. the crate's source and manifest pass the framework's audit (no `std::fs`, `std::net`,
//!    processes, threads, wall clock, OS randomness, printing, `unsafe`, or any dependency but
//!    the framework);
//! 2. the audit itself catches those things (its own tests in the framework crate, and one
//!    here against this crate's real manifest shape);
//! 3. the only way the door can change anything outside itself is the effects it returns, and a
//!    whole session of every key it understands leaves the store with exactly one save, the
//!    shared world, and nothing else.

use std::sync::Arc;

use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::sandbox;
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::{DoorEnv, DoorKey};
use russh_bbs_door_example::CoinToss;

#[test]
fn the_source_and_manifest_pass_the_framework_audit() {
    assert_eq!(sandbox::audit_crate(env!("CARGO_MANIFEST_DIR")), Ok(()));
}

#[test]
fn the_manifest_depends_on_the_framework_and_pure_data_crates_only() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
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
    let door = CoinToss::shared().unwrap();
    let clock: Arc<ManualClock> = Arc::new(ManualClock::new(Day::from_raw(20_000).start()));
    let env = DoorEnv::for_tests("ann");
    let ann = AccountId::from_raw(1);
    let mut run = Running::open(&store, door.clone(), ann, &env, clock.clone() as Arc<dyn Clock>, 9)
        .await
        .unwrap();
    // Every key the door understands, and several it does not (escape sequences, controls).
    for key in "htHTnx\u{1b}\u{7}\r%../..//etc/passwd".chars().map(DoorKey::Char).chain([
        DoorKey::Up,
        DoorKey::Down,
        DoorKey::Left,
        DoorKey::Right,
        DoorKey::Enter,
        DoorKey::Backspace,
    ]) {
        run.key(key);
        run.flush(&store).await.unwrap();
    }
    let id = door.info().id.clone();
    assert!(store.load_save(&id, ann).await.unwrap().is_some());
    assert!(store.load_world(&id).await.unwrap().is_some());
    assert!(store.load_save(&id, AccountId::from_raw(2)).await.unwrap().is_none());
    // Typing a path as a name is just text: it is refused by the name rules and stored nowhere.
    let screen = run.look_around(&env).join("\n");
    assert!(!screen.contains("passwd") || screen.contains("not allowed"), "{screen}");
}
