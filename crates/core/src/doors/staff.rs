//! The per-door staff tools: reset a player, end a season, bar and unbar a player.
//!
//! Each takes the staff member's [`Snapshot`], checks its own permission, and for the tools
//! that act on one player, checks that the player's rank is **strictly below** the staff
//! member's (D69; nobody acts on themselves, and a `cosysop` cannot act on a `sysop`). The
//! audit row is written by the store in the transaction that makes the change; a change that
//! changed nothing writes no row.

use tracing::info;

use super::store::{DoorId, DoorStore};
use crate::access::{Denied, Permission, Snapshot};
use crate::audit::{Actor, events};
use crate::store::{AccountId, StoreError};

/// Why a staff tool did nothing.
#[derive(Debug, thiserror::Error)]
pub enum DoorStaffError {
    /// The staff member does not hold the permission.
    #[error(transparent)]
    Denied(#[from] Denied),
    /// The player does not exist, or is not strictly below the staff member's rank.
    #[error("no such player, or not below your rank")]
    NotBelowYou,
    /// The storage backend failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

fn staff_of(access: &Snapshot, permission: Permission) -> Result<AccountId, DoorStaffError> {
    let granted = access.require(permission, None)?;
    granted
        .account()
        .ok_or(DoorStaffError::Store(StoreError::AccountNotFound))
}

async fn below<S: DoorStore>(
    store: &S,
    access: &Snapshot,
    staff: AccountId,
    target: AccountId,
) -> Result<(), DoorStaffError> {
    let Some(theirs) = store.account_rank(target).await? else {
        return Err(DoorStaffError::NotBelowYou);
    };
    if target == staff || theirs >= access.top_rank().unwrap_or(0) {
        return Err(DoorStaffError::NotBelowYou);
    }
    Ok(())
}

/// Delete `player`'s save in `door` (`doors.reset_player`). `true` if they had one.
///
/// # Errors
/// [`DoorStaffError::Denied`], [`DoorStaffError::NotBelowYou`], [`DoorStaffError::Store`].
pub async fn reset_player<S: DoorStore>(
    store: &S,
    access: &Snapshot,
    door: &DoorId,
    player: AccountId,
) -> Result<bool, DoorStaffError> {
    let staff = staff_of(access, Permission::DoorsResetPlayer)?;
    below(store, access, staff, player).await?;
    let audit = events::door_player_reset(Actor::Account(staff), player, door.as_str());
    let had = store.reset_player(door, player, &audit).await?;
    info!(%staff, %player, door = %door, had, "door player reset");
    Ok(had)
}

/// End the door's season (`doors.end_season`). Returns the new season number.
///
/// # Errors
/// [`DoorStaffError::Denied`], [`DoorStaffError::Store`].
pub async fn end_season<S: DoorStore>(
    store: &S,
    access: &Snapshot,
    door: &DoorId,
) -> Result<u32, DoorStaffError> {
    let staff = staff_of(access, Permission::DoorsEndSeason)?;
    let audit = events::door_season_ended(Actor::Account(staff), door.as_str());
    let season = store.end_season(door, &audit).await?;
    info!(%staff, door = %door, season, "door season ended");
    Ok(season)
}

/// Bar `player` from `door` (`doors.bar_player`). `true` if they were not barred already.
///
/// # Errors
/// [`DoorStaffError::Denied`], [`DoorStaffError::NotBelowYou`], [`DoorStaffError::Store`].
pub async fn bar_player<S: DoorStore>(
    store: &S,
    access: &Snapshot,
    door: &DoorId,
    player: AccountId,
) -> Result<bool, DoorStaffError> {
    let staff = staff_of(access, Permission::DoorsBarPlayer)?;
    below(store, access, staff, player).await?;
    let audit = events::door_player_barred(Actor::Account(staff), player, door.as_str());
    let changed = store.bar_player(door, player, staff, &audit).await?;
    info!(%staff, %player, door = %door, changed, "door player barred");
    Ok(changed)
}

/// Lift a bar (`doors.bar_player`). `true` if the player was barred.
///
/// # Errors
/// [`DoorStaffError::Denied`], [`DoorStaffError::NotBelowYou`], [`DoorStaffError::Store`].
pub async fn unbar_player<S: DoorStore>(
    store: &S,
    access: &Snapshot,
    door: &DoorId,
    player: AccountId,
) -> Result<bool, DoorStaffError> {
    let staff = staff_of(access, Permission::DoorsBarPlayer)?;
    below(store, access, staff, player).await?;
    let audit = events::door_player_unbarred(Actor::Account(staff), player, door.as_str());
    let changed = store.unbar_player(door, player, &audit).await?;
    info!(%staff, %player, door = %door, changed, "door player unbarred");
    Ok(changed)
}
