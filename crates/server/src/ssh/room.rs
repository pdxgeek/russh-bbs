//! Building a session's waiting room from the board's settings. Shared by the SSH handler and
//! the telnet front end so both transports show the same room (D94): same mode, same banner and
//! rules, same degrade-on-a-full-day rule, same `waiting.view` check.

use std::fmt::Debug;
use std::time::Duration;

use russh_bbs_core::access::Permission;
use russh_bbs_core::registration::open_cap_reached;
use russh_bbs_core::store::RegistrationMode;
use russh_bbs_core::waiting_room::{RoomConfig, WaitingRoom};
use tokio::time::timeout;
use tracing::warn;

use super::Backend;
use super::shell::MEMBER_START_TIMEOUT;

/// How long a new shell waits for the waiting-room settings before using the built-in
/// defaults. A slow store must not hold a stranger's screen hostage.
const ROOM_CONFIG_TIMEOUT: Duration = Duration::from_secs(2);

/// Build the room from the board's settings. Any failure falls back to the strictest built-in
/// configuration (`invite` mode, default text), never a looser one.
pub(crate) async fn build_room<D: Backend>(
    directory: &D,
    max_form_submissions: u32,
    session: &impl Debug,
) -> WaitingRoom {
    let config = match timeout(ROOM_CONFIG_TIMEOUT, directory.load_room_config()).await {
        Ok(Ok(config)) => config,
        Ok(Err(error)) => {
            warn!(?session, %error, "waiting-room settings unreadable; using defaults");
            RoomConfig::default()
        }
        Err(_) => {
            warn!(?session, "waiting-room settings timed out; using defaults");
            RoomConfig::default()
        }
    };
    let mut room = WaitingRoom::new(&config, max_form_submissions);
    if config.mode == RegistrationMode::Open {
        // A full day turns `open` into `apply` for every connection until 00:00 UTC (M2-G,
        // D2). If the count cannot be read, take the stricter path without claiming the cap is
        // reached.
        match timeout(ROOM_CONFIG_TIMEOUT, open_cap_reached(directory)).await {
            Ok(Ok(false)) => {}
            Ok(Ok(true)) => room.degrade_open(true),
            Ok(Err(error)) => {
                warn!(?session, %error, "daily cap unreadable; offering apply");
                room.degrade_open(false);
            }
            Err(_) => {
                warn!(?session, "daily cap check timed out; offering apply");
                room.degrade_open(false);
            }
        }
    }
    room
}

/// Whether the waiting room may be shown: the unauthenticated snapshot must hold
/// `waiting.view` (D33). Any failure answers no.
pub(crate) async fn room_allowed<D: Backend>(directory: &D, session: &impl Debug) -> bool {
    match timeout(MEMBER_START_TIMEOUT, directory.access_for(None)).await {
        Ok(Ok(access)) => access.require(Permission::WaitingView, None).is_ok(),
        Ok(Err(error)) => {
            warn!(?session, %error, "waiting-room access unreadable; refusing");
            false
        }
        Err(_) => {
            warn!(?session, "waiting-room access timed out; refusing");
            false
        }
    }
}
