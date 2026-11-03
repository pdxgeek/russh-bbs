//! What a telnet session may hold: nothing a staff role grants (D27).
//!
//! Telnet is unencrypted, so a password typed over it can be read by anyone on the path. A
//! sysop or cosysop whose password leaks that way must not hand over the board. The session
//! therefore drops every permission that came from a role of rank 40 or more before anything
//! is resolved; the account keeps what its lower roles and groups give it, and nothing it holds
//! as a staff role is visible or usable for the whole session. SSH sessions are not touched.

use russh_bbs_core::access::{Held, Snapshot, Source};

/// The lowest role rank refused over telnet: `cosysop` (40) and `sysop` (100), and any custom
/// role placed at or above them (`docs/access.md`).
pub const STAFF_RANK: u8 = 40;

/// `snapshot` without any permission held through a role of rank [`STAFF_RANK`] or more.
///
/// Allow-list entries and group grants carry no rank and are kept: they are a deliberate,
/// narrow grant, not a staff role. Applied to the snapshot a telnet session starts with and to
/// every one it loads afterward.
#[must_use]
pub fn without_staff(snapshot: &Snapshot) -> Snapshot {
    let kept: Vec<Held> = snapshot
        .held()
        .iter()
        .filter(|held| !matches!(&held.source, Source::Role { rank, .. } if *rank >= STAFF_RANK))
        .cloned()
        .collect();
    Snapshot::new(snapshot.account(), kept)
}
