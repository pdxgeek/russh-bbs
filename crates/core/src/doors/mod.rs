//! Door persistence and staff tools (M4-D0, D98): the part of the door framework that belongs
//! with the board's rules. The `Door` trait, the sandbox and the games themselves live in
//! `crates/doors/`; this module is what they all save through.
//!
//! - **One save per player per door**, a versioned opaque blob ([`StoredSave`]), and **one
//!   optional shared world blob per door** ([`StoredWorld`]). The store never looks inside a
//!   blob; a game owns its format and migrates it forward by version (the framework runs the
//!   migrations, this module only keeps the number).
//! - **Size caps** ([`MAX_SAVE_BYTES`], [`MAX_WORLD_BYTES`]) are checked in code and again by a
//!   `CHECK` in the database, so a bug cannot grow a row without bound.
//! - **No lost updates.** Every write names the [`Revision`] it read (zero for "nothing yet")
//!   and is refused with [`DoorStoreError::Conflict`] if the row moved: two sessions of one
//!   player, or a tick racing a session, cannot overwrite each other silently.
//! - **Seasons.** Each door has a season number (starts at 1). Ending a season is a staff
//!   action that bumps it; each save records the season it was written in, and the game decides
//!   what an older save means. The framework does not delete anything on a season change.
//! - **Staff tools** ([`staff`]) are permissioned (`doors.reset_player`, `doors.end_season`,
//!   `doors.bar_player`), rank-checked against the target (D69) and audited in the same
//!   transaction as the change. They have no screen yet.
//! - **A barred player** cannot open the door ([`DoorStore::is_barred`]); their save is kept.
//!
//! Door data is personal data the server can read. It is deleted with the account (foreign key
//! cascade), and nothing here is private from the sysop.

mod staff;
mod store;

pub use staff::{DoorStaffError, bar_player, end_season, reset_player, unbar_player};
pub use store::{
    DoorId, DoorStore, DoorStoreError, InvalidDoorId, MAX_DOOR_ID_CHARS, MAX_SAVE_BYTES,
    MAX_WORLD_BYTES, Revision, SaveWrite, StoredSave, StoredWorld, WorldWrite,
};

#[cfg(test)]
mod tests;
