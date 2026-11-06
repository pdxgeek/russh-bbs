//! Save and world versions: a character saved by H1 (version 1) or H2 (version 2) still loads, and
//! a school stored before the social layer (world version 1) still loads, each gaining the empty
//! new part. The old layouts are pinned as files so a change to the encoder cannot move them.
#![allow(
    clippy::cast_precision_loss,
    clippy::needless_pass_by_value,
    clippy::doc_markdown,
    clippy::similar_names,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::type_complexity,
    clippy::collapsible_if,
    clippy::cast_possible_wrap,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::print_stderr,
    clippy::panic
)]
mod common;

use std::sync::Arc;

use common::{Sandbox, monday};
use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::{BlobKind, Door, DoorEnv, DoorKey, MigrateError};
use russh_bbs_door_hallpass::save::{decode_player, decode_world, encode_player};
use russh_bbs_door_hallpass::commons::{Commons, Social};
use russh_bbs_door_hallpass::state::{Player, SchoolLife, World};
use russh_bbs_door_hallpass::wire::encode;
use russh_bbs_door_hallpass::{Hallpass, SAVE_VERSION, WORLD_VERSION};

/// What version 1 wrote for `Player::new("ann", 1, 20_000, "dorm")`, byte for byte: H1's field
/// order with nothing after `history`.
const V1_NEW_PLAYER: &str = include_str!("fixtures/v1_new_player.hex");
/// A character as version 2 (H2) wrote it, with a bond and a flag: `SchoolLife` last.
const V2_BUSY_PLAYER: &str = include_str!("fixtures/v2_busy_player.hex");
/// A school as world version 1 wrote it: a hall entry, a headline and one effect in force.
const V1_BUSY_WORLD: &str = include_str!("fixtures/v1_busy_world.hex");

fn unhex(text: &str) -> Vec<u8> {
    let clean: String = text.chars().filter(char::is_ascii_hexdigit).collect();
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).unwrap())
        .collect()
}

/// The version 2 form of any current character with no social state: everything but `Social`.
fn v2_of(player: &Player) -> Vec<u8> {
    let mut bytes = encode_player(player);
    let tail = encode(&Social::default());
    assert!(
        bytes.ends_with(&tail),
        "only a character with an empty social state has a v2 form"
    );
    bytes.truncate(bytes.len() - tail.len());
    bytes
}

/// The version 1 form: also without the school life.
fn v1_of(player: &Player) -> Vec<u8> {
    let mut bytes = v2_of(player);
    let tail = encode(&SchoolLife::default());
    assert!(
        bytes.ends_with(&tail),
        "only a character with an empty school life has a v1 form"
    );
    bytes.truncate(bytes.len() - tail.len());
    bytes
}

/// Walk a blob forward the way the framework does: one `migrate` step at a time.
fn up(door: &Hallpass, kind: BlobKind, mut from: u32, mut bytes: Vec<u8>, to: u32) -> Vec<u8> {
    while from < to {
        bytes = door.migrate(kind, from, &bytes).unwrap();
        from += 1;
    }
    bytes
}

#[test]
fn the_versions_are_what_the_door_says() {
    assert_eq!((SAVE_VERSION, WORLD_VERSION), (3, 2));
    let door = Hallpass::new().unwrap();
    assert_eq!(door.info().save_version, 3);
    assert_eq!(door.info().world_version, 2);
}

#[test]
fn a_version_1_character_walks_forward_to_the_same_character_with_empty_new_parts() {
    let door = Hallpass::new().unwrap();
    // The pinned bytes from H1.
    let pinned = unhex(V1_NEW_PLAYER);
    let one = door.migrate(BlobKind::Save, 1, &pinned).unwrap();
    assert_ne!(one, pinned, "version 2 appended the school life");
    let player = decode_player(&up(&door, BlobKind::Save, 1, pinned, 3)).unwrap();
    assert_eq!(player, Player::new("ann", 1, 20_000, "dorm"));
    assert_eq!(player.life, SchoolLife::default());
    assert_eq!(player.social, Social::default());
    // A character with a history, rebuilt as version 1 bytes.
    let mut sb = Sandbox::new("smooth_talker", 4);
    sb.pass(2);
    sb.game.player.say("a line of news".into());
    let v1 = v1_of(&sb.game.player);
    let now = up(&door, BlobKind::Save, 1, v1.clone(), 3);
    assert_eq!(decode_player(&now).unwrap(), sb.game.player);
    assert_eq!(
        now,
        encode_player(&sb.game.player),
        "the migrated bytes are the current bytes"
    );
    // Bytes that were never a version 1 character are refused, not guessed at.
    assert_eq!(
        door.migrate(BlobKind::Save, 1, &v1[..v1.len() - 3]),
        Err(MigrateError::Corrupt(1))
    );
    assert_eq!(
        door.migrate(BlobKind::Save, 1, &[1, 2, 3]),
        Err(MigrateError::Corrupt(1))
    );
    // There is nothing older than 1, and the current version is not migrated.
    assert!(door.migrate(BlobKind::Save, 0, &v1).is_err());
    assert!(door.migrate(BlobKind::Save, 3, &v1).is_err());
}

#[test]
fn a_version_2_character_from_h2_migrates_to_the_same_character_with_an_empty_social_state() {
    let door = Hallpass::new().unwrap();
    let pinned = unhex(V2_BUSY_PLAYER);
    let up3 = door.migrate(BlobKind::Save, 2, &pinned).unwrap();
    let player = decode_player(&up3).unwrap();
    // What the pinned character had is still there.
    assert_eq!(player.name, "ann");
    assert!(player.knows("back_stairs"));
    assert_eq!(player.cool, 9);
    assert_eq!(player.life.bond_with("sol"), 7);
    assert_eq!(player.social, Social::default());
    assert!(!player.social.opt_out, "a migrated character is open, not opted out");
    // The migrated bytes are the pinned bytes with exactly the empty social state appended.
    let mut expected = pinned.clone();
    expected.extend(encode(&Social::default()));
    assert_eq!(up3, expected);
    // A truncated blob is refused.
    assert_eq!(
        door.migrate(BlobKind::Save, 2, &pinned[..pinned.len() - 2]),
        Err(MigrateError::Corrupt(2))
    );
}

#[test]
fn a_version_1_school_migrates_to_world_version_2_with_an_empty_commons() {
    let door = Hallpass::new().unwrap();
    let pinned = unhex(V1_BUSY_WORLD);
    let up2 = door.migrate(BlobKind::World, 1, &pinned).unwrap();
    let world = decode_world(&up2).unwrap();
    assert_eq!(world.term, 1);
    assert_eq!(world.hall.first().map(|h| h.name.as_str()), Some("ann"));
    assert_eq!(world.paper.len(), 1);
    assert!(world.has("tuck_shop_closed"));
    assert_eq!(world.commons, Commons::default());
    assert_eq!(World::from_bytes(Some(&up2), 0), world);
    // Damaged bytes are refused; nothing older than 1 exists; the current version is not migrated.
    assert_eq!(
        door.migrate(BlobKind::World, 1, &pinned[..pinned.len() - 1]),
        Err(MigrateError::Corrupt(1))
    );
    assert!(door.migrate(BlobKind::World, 0, &pinned).is_err());
    assert!(door.migrate(BlobKind::World, 2, &up2).is_err());
}

#[tokio::test]
async fn an_old_save_and_an_old_world_in_the_store_are_migrated_when_the_door_opens() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Hallpass::shared().unwrap();
    let mut sb = Sandbox::new("tinkerer", 4);
    sb.pass(1);
    let day = sb.game.player.day;
    let before = sb.game.player.clone();
    // A version 1 character and a version 1 school, as the board would have stored them.
    store
        .write_save(
            &door.info().id,
            AccountId::from_raw(1),
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: 1,
                bytes: &v1_of(&before),
            },
        )
        .await
        .unwrap();
    store
        .write_world(
            &door.info().id,
            &russh_bbs_core::doors::WorldWrite {
                expected: Revision::NONE,
                version: 1,
                bytes: &unhex(V1_BUSY_WORLD),
                tick: None,
            },
        )
        .await
        .unwrap();
    store.set_now(Day::from_raw(day).start());
    let clock = Arc::new(ManualClock::new(Day::from_raw(day).start()));
    let env = DoorEnv::for_tests("ann");
    let c: Arc<dyn Clock> = clock.clone();
    let mut run = Running::open(&store, door.clone(), AccountId::from_raw(1), &env, c, 1)
        .await
        .unwrap();
    let screen = run.look_around(&env).join("\n");
    assert!(
        screen.contains("Third period") || screen.contains("Second period"),
        "{screen}"
    );
    // The character is where they were, and the first thing they do is saved in the new version.
    run.key(DoorKey::Char('r'));
    run.flush(&store).await.unwrap();
    let stored = store
        .load_save(&door.info().id, AccountId::from_raw(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.version, 3);
    let after = decode_player(&stored.bytes).unwrap();
    assert_eq!(after.traits, before.traits);
    assert_eq!(after.slot, before.slot + 1);
    assert_eq!(after.life, SchoolLife::default());
    assert_eq!(after.social, Social::default());
    // The school is stored as version 2 once a key has changed it, with the pupil on the roster
    // and what was already there kept.
    let world = store.load_world(&door.info().id).await.unwrap().unwrap();
    assert_eq!(world.version, 2);
    let w = decode_world(&world.bytes).unwrap();
    assert!(w.has("tuck_shop_closed"));
    assert_eq!(w.commons.roster.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["ann"]);
    let _ = monday();
}
