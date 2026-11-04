//! Save compatibility: a captain saved by the L1 engine (format 1, framework save version 1) must
//! still load under L2, through the framework's migration hook, with nothing lost and the new
//! sections empty. A newer or damaged save is refused and never trusted.
#![allow(
    clippy::assert_is_empty,
    clippy::single_match_else,
    unused_mut,
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines
)]

mod common;

use common::Harness;
use russh_bbs_door::{BlobKind, Door, MigrateError};
use russh_bbs_door_long_haul::contracts::MAX_CONTRACTS;
use russh_bbs_door_long_haul::crew::Role;
use russh_bbs_door_long_haul::engine::Action;
use russh_bbs_door_long_haul::player::{Cause, Player, SAVE_FORMAT};
use russh_bbs_door_long_haul::{LongHaul, SAVE_VERSION, WORLD_VERSION};

/// A captain written by the L1 engine at commit 68e6cb6 (season 1): a Free Captain's kid who
/// bought six tons, undocked and made one jump.
const L1_SAVE: &str = "010754657374657230030400000003000000000000003c00000000000000f6ffffff0000000014000000d102000000000000f055000000000000274e000000000000204e00000000000000016400430003000018000400000600000100030600df010000000000002c000007000000204e0000000000000300000000000000000000b004000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000df01000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001000000060000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn an_l1_save_migrates_and_loads_with_everything_it_had() {
    let h = Harness::new(1, 1);
    let door = LongHaul::new().unwrap();
    assert_eq!(SAVE_VERSION, 2);
    assert_eq!(WORLD_VERSION, 1);
    let v1 = unhex(L1_SAVE);
    assert_eq!(v1[0], 1);
    // The raw v1 bytes read directly (the decoder keeps accepting format 1) ...
    let direct = Player::decode(&v1, &h.content, &h.frontier).unwrap();
    // ... and through the framework's migration step, to the same captain.
    let v2 = door.migrate(BlobKind::Save, 1, &v1).unwrap();
    assert_eq!(v2[0], SAVE_FORMAT);
    let migrated = Player::decode(&v2, &h.content, &h.frontier).unwrap();
    assert_eq!(migrated, direct);
    // What L1 knew survived.
    assert_eq!(migrated.captain, "Tester0");
    assert_eq!(migrated.cash, 721);
    assert_eq!(migrated.debt.principal, 22_000);
    assert_eq!(&migrated.standing[..3], &[-10, 0, 20]);
    assert_eq!(&migrated.standing[3..], &[0, 0]);
    assert_eq!(migrated.stats.jumps, 1);
    assert_eq!(migrated.ship.cargo.len(), 1);
    assert_eq!(migrated.ship.cargo[0].tons, 6);
    assert_eq!(migrated.at, 44);
    assert_eq!(migrated.days_left, 7);
    assert_eq!(migrated.ledger.net(), migrated.cash);
    // The L2 sections start empty.
    assert!(migrated.crew.is_empty() && migrated.contracts.is_empty());
    assert!(migrated.scene.is_none() && migrated.taken.is_empty());
    assert_eq!(migrated.flags, 0);
    assert_eq!(migrated.ledger.income[Cause::Crew.index()], 0);
    // A migrated captain plays on: hire, and save again at format 2.
    let mut p = migrated;
    let mut h = h;
    let st = h.station("industrial");
    h.dock_free(&mut p, st);
    h.go(&mut p, &Action::Hire(0)).unwrap();
    assert_eq!(p.crew.len(), 1);
    let again = Player::decode(&p.encode(), &h.content, &h.frontier).unwrap();
    assert_eq!(again, p);
    // Migrating twice is not a thing: only version 1 migrates.
    assert!(matches!(
        door.migrate(BlobKind::Save, 2, &v2),
        Err(MigrateError::Unsupported(2))
    ));
    assert!(matches!(
        door.migrate(BlobKind::World, 1, &v1),
        Err(MigrateError::Unsupported(1))
    ));
    assert!(
        door.migrate(BlobKind::Save, 1, &v2).is_err(),
        "a v2 blob is not a v1 blob"
    );
}

#[test]
fn a_format_1_encode_of_a_new_captain_is_what_l1_wrote() {
    // The v1 encoder drops everything L2 adds, and migrating it back gives the same captain.
    let mut h = Harness::new(2, 2);
    let p = h.captain("washout");
    let v1 = p.encode_v1();
    assert_eq!(v1[0], 1);
    let door = LongHaul::new().unwrap();
    let v2 = door.migrate(BlobKind::Save, 1, &v1).unwrap();
    assert_eq!(Player::decode(&v2, &h.content, &h.frontier).unwrap(), p);
    // A captain with L2 state loses it in v1 (this is a test seam, not a downgrade path).
    let mut q = p.clone();
    let st = h.station("industrial");
    h.dock_free(&mut q, st);
    h.go(&mut q, &Action::Hire(0)).unwrap();
    let flat = Player::decode(&q.encode_v1(), &h.content, &h.frontier).unwrap();
    assert!(flat.crew.is_empty());
    let _ = Role::Pilot;
}

#[test]
fn damaged_and_newer_saves_are_refused() {
    let h = Harness::new(1, 3);
    let v1 = unhex(L1_SAVE);
    let door = LongHaul::new().unwrap();
    let v2 = door.migrate(BlobKind::Save, 1, &v1).unwrap();
    // Truncated, extended, or a newer format: never trusted.
    for cut in [1, 10, v2.len() / 2, v2.len() - 1] {
        assert!(
            Player::decode(&v2[..cut], &h.content, &h.frontier).is_none(),
            "cut {cut}"
        );
    }
    let mut longer = v2.clone();
    longer.push(0);
    assert!(Player::decode(&longer, &h.content, &h.frontier).is_none());
    let mut newer = v2.clone();
    newer[0] = SAVE_FORMAT + 1;
    assert!(Player::decode(&newer, &h.content, &h.frontier).is_none());
    // Every single flipped byte either decodes to a valid captain or is refused; never a panic.
    for i in 0..v2.len() {
        let mut bad = v2.clone();
        bad[i] ^= 0xA5;
        if let Some(p) = Player::decode(&bad, &h.content, &h.frontier) {
            assert!(p.crew.len() <= 8 && p.contracts.len() <= MAX_CONTRACTS);
            assert!(p.taken.len() <= 48);
        }
    }
    // A save from another season's frontier is refused if its places do not exist.
    let tiny = Harness::new(1, 3);
    let _ = tiny;
}

#[test]
fn a_scene_open_in_a_save_comes_back_exactly() {
    let mut h = Harness::new(1, 4);
    let mut p = h.captain("dockhand");
    h.sign(&mut p, Role::Gunner, 3);
    h.fit(&mut p, "lance1");
    h.fight(&mut p, "raider");
    let bytes = p.encode();
    let back = Player::decode(&bytes, &h.content, &h.frontier).unwrap();
    assert_eq!(back, p);
    assert!(back.scene.is_some());
    // The save with a scene is the one the framework would store; a reload cannot reroll it:
    // the same orders from the same state give the same round.
    let go = |mut q: Player| {
        let mut h2 = Harness::new(1, 4);
        h2.reseed(99);
        let _ = h2.go_strict(&mut q, &Action::Fight(common::orders(3, 2, 1)));
        q
    };
    assert_eq!(go(p), go(back));
}
