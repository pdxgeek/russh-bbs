//! Forward migrations to version 3 from bytes built by hand to the version 2 layout (not made
//! by the current encoder), and the refusals.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::many_single_char_names,
    clippy::assert_is_empty,
    clippy::expect_used
)]

mod common;

use common::*;
use russh_bbs_door::{BlobKind, Door, MigrateError};
use russh_bbs_door_nullroute::Nullroute;
use russh_bbs_door_nullroute::player::Ties;
use russh_bbs_door_nullroute::save::{decode, decode_world};

/// A version 2 save, laid out by hand: Brindle, day 20000, with a deck, two fragments and one
/// coded script.
fn v2_save() -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    b.extend(0x0102_0304_0506_0708u64.to_le_bytes()); // seed
    b.extend(3u32.to_le_bytes()); // season
    b.push(7);
    b.extend(b"Brindle"); // name
    b.extend(20_000i64.to_le_bytes()); // day
    b.extend(31u32.to_le_bytes()); // cycles
    b.extend(1_234u32.to_le_bytes()); // marks
    b.extend(50u32.to_le_bytes()); // stash
    b.push(17); // heat
    b.extend(9u32.to_le_bytes()); // rep
    b.extend([1, 0, 2, 0, 3]); // rig
    b.push(0b100); // fried
    b.extend([5, 1, 5, 8, 11, 23]); // owned: count, then scripts
    b.extend([4, 1, 5, 8, 23]); // deck
    for xp in [1u32, 2, 3, 4, 5] {
        b.extend(xp.to_le_bytes());
    }
    b.extend([2, 1, 1]); // bots, workbench, fence
    b.extend([1u8, 0xfe, 3, 0, 0, 0, 0, 0xfb]); // standing 1, -2, 3, 0, 0, 0, 0, -5
    b.push(2); // done
    b.extend(320_016u32.to_le_bytes());
    b.extend(5u32.to_le_bytes());
    b.push(1); // pending
    b.extend(77u32.to_le_bytes());
    b.push(1); // proxies
    b.extend(4u32.to_le_bytes()); // runs
    b.extend(3u32.to_le_bytes()); // jobs
    b.extend(1u32.to_le_bytes()); // burns
    b.extend(99_999u64.to_le_bytes()); // earned
    b.extend([2, 14, 3]); // fragments
    b.extend([1, 14, 23]); // coded slot 0
    b.extend([0; 15 * 3]); // the other 15 slots
    b
}

/// A version 2 world, laid out by hand (the layout the N2 tests pinned).
fn v2_world() -> Vec<u8> {
    vec![
        0x21, 0x4e, 0, 0, 0, 0, 0, 0,   // day
        110, // market
        7, 0, 0, 0, // breaches
        1, // nets
        2, 3, 1, 1, 2, 1, // corp, tier, alert, backdoor, breaches
        2, 4, 9, // holes
        1, // board
        0x10, 0xe2, 0x04, 0, 5, 1, 2, 2, 150, 0, 0, 0, 0x23, 0x4e, 0, 0, 0, 0, 0, 0, //
        3, 0, 0, 0, // season
        1, 2, 0x07, 0x04, 9, // learned
    ]
}

#[test]
fn a_hand_built_version_2_save_moves_to_version_3_and_is_the_same_character() {
    let door = Nullroute::new().unwrap();
    let moved = door.migrate(BlobKind::Save, 2, &v2_save()).unwrap();
    let p = decode(&moved).unwrap();
    assert_eq!(
        (
            p.name.as_str(),
            p.day,
            p.marks,
            p.heat,
            p.rep,
            p.runs,
            p.jobs,
            p.burns
        ),
        ("Brindle", 20_000, 1_234, 17, 9, 4, 3, 1)
    );
    assert_eq!(p.standing, [1, -2, 3, 0, 0, 0, 0, -5]);
    assert_eq!(p.fragments, vec![14, 3]);
    assert_eq!(p.pending, Some(77));
    assert!(p.coded[0].is_some());
    // The social layer starts empty: on the rivals board, a crew-taker, no honeypot, no story
    // yet, and not new (the protection week is behind them).
    assert_eq!(p.ties, Ties::grandfathered(20_000));
    assert!(p.ties.pvp && p.ties.crews_open && p.ties.honeypot == 0);
    assert!(
        p.ties
            .arcs
            .iter()
            .all(|a| a.step == 0 && !a.awaiting && a.ending == 0)
    );
    assert!(20_000 - p.ties.born >= 7);
    // Moved bytes are exactly what the encoder writes for that character.
    assert_eq!(moved, russh_bbs_door_nullroute::save::encode(&p));
    // Bytes that are not a version 2 save are an error, never guessed at.
    for cut in [0, 5, v2_save().len() - 1] {
        assert_eq!(
            door.migrate(BlobKind::Save, 2, &v2_save()[..cut]),
            Err(MigrateError::Corrupt(2))
        );
    }
    let mut longer = v2_save();
    longer.push(0);
    assert_eq!(
        door.migrate(BlobKind::Save, 2, &longer),
        Err(MigrateError::Corrupt(2))
    );
    // And a save from the future is not something this code has an arm for.
    assert_eq!(
        door.migrate(BlobKind::Save, 9, &moved),
        Err(MigrateError::Unsupported(9))
    );
}

#[test]
fn a_hand_built_version_2_world_moves_to_version_3_with_nothing_social_in_it() {
    let door = Nullroute::new().unwrap();
    let moved = door.migrate(BlobKind::World, 2, &v2_world()).unwrap();
    let w = decode_world(&moved).unwrap();
    assert_eq!(
        (w.day, w.market_pct, w.breaches, w.season),
        (20_001, 110, 7, 3)
    );
    assert_eq!(w.nets[0].holes, vec![4, 9]);
    assert_eq!(w.board.len(), 1);
    assert_eq!(w.learned.len(), 1);
    assert!(w.hist.is_empty());
    assert!(w.social.posted.is_empty() && w.social.crews.is_empty() && w.social.ops.is_empty());
    assert_eq!(w.social.next_id, 0);
    assert_eq!(moved, russh_bbs_door_nullroute::save::encode_world(&w));
    assert_eq!(
        door.migrate(BlobKind::World, 2, &v2_world()[..20]),
        Err(MigrateError::Corrupt(2))
    );
}

#[test]
fn a_version_1_save_walks_the_whole_way_one_step_at_a_time() {
    // The framework migrates one version at a time; version 1 passes through 2 and arrives at 3.
    let door = Nullroute::new().unwrap();
    let v2 = v2_save();
    let v1 = v2[..v2.len() - (1 + 2 + 16 * 3)].to_vec();
    let a = door.migrate(BlobKind::Save, 1, &v1).unwrap();
    assert_eq!(a, v2_save_with_empty_tail(&v1));
    let b = door.migrate(BlobKind::Save, 2, &a).unwrap();
    assert!(decode(&b).is_some());
    let _ = content();
}

fn v2_save_with_empty_tail(v1: &[u8]) -> Vec<u8> {
    let mut out = v1.to_vec();
    out.push(0);
    out.extend([0; 16 * 3]);
    out
}
