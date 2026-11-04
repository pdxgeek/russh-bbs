//! Save versions: a character saved by H1 (version 1) still loads, and gains an empty school life.
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
use russh_bbs_door_hallpass::save::{decode_player, encode_player};
use russh_bbs_door_hallpass::state::{Player, SchoolLife};
use russh_bbs_door_hallpass::wire::encode;
use russh_bbs_door_hallpass::{Hallpass, SAVE_VERSION, WORLD_VERSION};

/// What version 1 wrote for `Player::new("ann", 1, 20_000, "dorm")`, byte for byte: H1's field
/// order with nothing after `history` (H2 only appends). The bytes are kept as a file so a change
/// to the encoder cannot silently change what a version 1 save is taken to be.
const V1_NEW_PLAYER: &str = include_str!("fixtures/v1_new_player.hex");

fn unhex(text: &str) -> Vec<u8> {
    let clean: String = text.chars().filter(char::is_ascii_hexdigit).collect();
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).unwrap())
        .collect()
}

/// The version 1 form of any current character: everything but the appended school life.
fn v1_of(player: &Player) -> Vec<u8> {
    let mut bytes = encode_player(player);
    let tail = encode(&SchoolLife::default());
    assert!(
        bytes.ends_with(&tail),
        "only a character with an empty school life has a v1 form"
    );
    bytes.truncate(bytes.len() - tail.len());
    bytes
}

#[test]
fn the_versions_are_what_the_door_says() {
    assert_eq!((SAVE_VERSION, WORLD_VERSION), (2, 1));
    let door = Hallpass::new().unwrap();
    assert_eq!(door.info().save_version, 2);
}

#[test]
fn a_version_1_character_migrates_to_the_same_character_with_an_empty_school_life() {
    let door = Hallpass::new().unwrap();
    // The pinned bytes from H1.
    let pinned = unhex(V1_NEW_PLAYER);
    let up = door.migrate(BlobKind::Save, 1, &pinned).unwrap();
    let player = decode_player(&up).unwrap();
    assert_eq!(player, Player::new("ann", 1, 20_000, "dorm"));
    assert_eq!(player.life, SchoolLife::default());
    // A character with a history, rebuilt as version 1 bytes.
    let mut sb = Sandbox::new("smooth_talker", 4);
    sb.pass(2);
    sb.game.player.say("a line of news".into());
    let v1 = v1_of(&sb.game.player);
    let up = door.migrate(BlobKind::Save, 1, &v1).unwrap();
    assert_eq!(decode_player(&up).unwrap(), sb.game.player);
    assert_eq!(
        up,
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
    // There is nothing older than 1, the world did not change, and the current version is not migrated.
    assert!(door.migrate(BlobKind::Save, 0, &v1).is_err());
    assert!(door.migrate(BlobKind::Save, 2, &v1).is_err());
    assert!(door.migrate(BlobKind::World, 1, &[]).is_err());
}

#[tokio::test]
async fn a_version_1_save_in_the_store_is_migrated_when_the_door_opens_and_saved_as_version_2() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Hallpass::shared().unwrap();
    let mut sb = Sandbox::new("tinkerer", 4);
    sb.pass(1);
    let day = sb.game.player.day;
    let before = sb.game.player.clone();
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
    assert_eq!(stored.version, 2);
    let after = decode_player(&stored.bytes).unwrap();
    assert_eq!(after.traits, before.traits);
    assert_eq!(after.slot, before.slot + 1);
    assert_eq!(after.life, SchoolLife::default());
    let _ = monday();
}
