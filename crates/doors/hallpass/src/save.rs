//! The save and world formats.
//!
//! Both are the [`crate::wire`] encodings of [`Player`] and [`World`]. The framework stores the
//! format version beside the bytes (`SAVE_VERSION`, `WORLD_VERSION` in the crate root) and calls
//! `Door::migrate` one step at a time, so a later slice that changes a struct raises the version
//! and adds a step in [`migrate`]; a blob newer than this code is never loaded or overwritten.
//! There is no older version yet, so every step is unsupported today.

use russh_bbs_door::{BlobKind, MigrateError};

use crate::state::{Player, World};
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

/// Bring a stored blob forward one version. There is nothing older than version 1.
pub fn migrate(_kind: BlobKind, from: u32, _bytes: &[u8]) -> Result<Vec<u8>, MigrateError> {
    Err(MigrateError::Unsupported(from))
}
