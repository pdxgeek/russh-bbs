//! The save and world formats.
//!
//! Both are the [`crate::wire`] encodings of [`Player`] and [`World`]. The framework stores the
//! format version beside the bytes (`SAVE_VERSION`, `WORLD_VERSION` in the crate root) and calls
//! `Door::migrate` one step at a time, so a later slice that changes a struct raises the version
//! and adds a step in [`migrate`]; a blob newer than this code is never loaded or overwritten.
//!
//! Version 2 (M4-H2) appends [`SchoolLife`] (clubs, friendships, storylines, a lesson or a
//! conversation under way) to the character. A version 1 save gets the empty value; the world
//! format did not change.

use russh_bbs_door::{BlobKind, MigrateError};

use crate::state::{Player, SchoolLife, World};
use crate::wire::{decode, encode};

/// Encode a character.
#[must_use]
pub fn encode_player(player: &Player) -> Vec<u8> {
    encode(player)
}

/// Decode a character, or `None` for bytes that are not one.
#[must_use]
pub fn decode_player(bytes: &[u8]) -> Option<Player> {
    decode(bytes)
}

/// Decode a world.
#[must_use]
pub fn decode_world(bytes: &[u8]) -> Option<World> {
    decode(bytes)
}

/// Bring a stored blob forward one version: a version 1 character gains an empty school life.
pub fn migrate(kind: BlobKind, from: u32, bytes: &[u8]) -> Result<Vec<u8>, MigrateError> {
    match (kind, from) {
        (BlobKind::Save, 1) => {
            // Only bytes that were a version 1 character are carried forward; anything else is
            // refused here rather than guessed at when the new reader finds it damaged.
            let mut upgraded = bytes.to_vec();
            upgraded.extend(encode(&SchoolLife::default()));
            decode_player(&upgraded)
                .map(|_| upgraded)
                .ok_or(MigrateError::Corrupt(from))
        }
        _ => Err(MigrateError::Unsupported(from)),
    }
}
