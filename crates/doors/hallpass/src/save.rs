//! The save and world formats.
//!
//! Both are the [`crate::wire`] encodings of [`Player`] and [`World`]. The framework stores the
//! format version beside the bytes (`SAVE_VERSION`, `WORLD_VERSION` in the crate root) and calls
//! `Door::migrate` one step at a time, so a later slice that changes a struct raises the version
//! and adds a step in [`migrate`]; a blob newer than this code is never loaded or overwritten.
//!
//! Version 3 (M4-H4) appends [`Social`] to the character and world version 2 appends [`Commons`]
//! to the school; each older blob gets the empty value.
//!
//! Version 2 (M4-H2) appends [`SchoolLife`] (clubs, friendships, storylines, a lesson or a
//! conversation under way) to the character. A version 1 save gets the empty value; the world
//! format did not change.

use russh_bbs_door::{BlobKind, MigrateError};

use crate::commons::{Commons, Social};
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
            // Check it against the current layout (the version 2 form plus the empty social
            // state version 3 appends), since the current decoder is the one that reads it.
            let mut as_current = upgraded.clone();
            as_current.extend(encode(&Social::default()));
            decode_player(&as_current)
                .map(|_| upgraded)
                .ok_or(MigrateError::Corrupt(from))
        }
        (BlobKind::Save, 2) => {
            // Version 3 appends the social state: a version 2 character gains the empty one.
            let mut upgraded = bytes.to_vec();
            upgraded.extend(encode(&Social::default()));
            decode_player(&upgraded)
                .map(|_| upgraded)
                .ok_or(MigrateError::Corrupt(from))
        }
        (BlobKind::World, 1) => {
            // World version 2 appends the commons: a version 1 school gains an empty one.
            let mut upgraded = bytes.to_vec();
            upgraded.extend(encode(&Commons::default()));
            decode_world(&upgraded)
                .map(|_| upgraded)
                .ok_or(MigrateError::Corrupt(from))
        }
        _ => Err(MigrateError::Unsupported(from)),
    }
}
