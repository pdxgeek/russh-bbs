//! The widgets' live pieces (UI-10): who is online, the chat rooms, the doors and the status.
//!
//! They come from the running server rather than the store, so `russh_bbs_core::widgets::fetch`
//! cannot fill them. Each is gated by the same [`WidgetId::reachable_by`] the store-backed
//! widgets use, with the access the session holds right now, and member text is sanitized before
//! it is put in the data (the widgets sanitize it again when they draw).

use russh_bbs_core::access::Snapshot;
use russh_bbs_core::chat::joinable_rooms;
use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::profiles::ProfileStore;
use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::widgets::{DoorLine, RoomCount, Status, WidgetData, WidgetId};

use super::Backend;
use super::registry::SessionRef;
use crate::chat::Hub;
use crate::doors::Doors;

/// Most names the who's-online widget keeps.
const ONLINE_KEPT: usize = 24;

/// What the live widgets read: the running server's own state.
pub(super) struct Sources<'a, D> {
    pub(super) directory: &'a D,
    pub(super) session: &'a SessionRef,
    pub(super) hub: &'a Hub,
    pub(super) doors: &'a Doors,
    /// The most connections the board takes (the status gauge's top).
    pub(super) capacity: usize,
}

/// Fill the live widgets in `wanted` that `access` may see.
pub(super) async fn fill<D: Backend>(
    data: &mut WidgetData,
    wanted: &[WidgetId],
    access: &Snapshot,
    sources: &Sources<'_, D>,
) {
    let Sources {
        directory,
        session,
        hub,
        doors,
        capacity,
    } = *sources;
    let want = |id: WidgetId| wanted.contains(&id) && id.reachable_by(access);
    if want(WidgetId::Online) {
        data.online = online(directory, session).await;
    }
    if want(WidgetId::Chat) {
        data.chat = chat(directory, access, hub).await;
    }
    if want(WidgetId::Doors) {
        data.doors = door_lines(directory, doors).await;
    }
    if want(WidgetId::Status) {
        data.status = Some(status(session, capacity));
    }
}

/// Handles of the members with a shell open, once each, in alphabetical order.
async fn online<D: ProfileStore>(directory: &D, session: &SessionRef) -> Option<Vec<String>> {
    let mut accounts: Vec<_> = session
        .live_sessions()
        .into_iter()
        .filter(|s| s.shell_open)
        .filter_map(|s| s.account)
        .collect();
    accounts.sort();
    accounts.dedup();
    let mut names = Vec::new();
    for account in accounts.into_iter().take(ONLINE_KEPT) {
        // An account that is not active has no member row: it is not listed.
        if let Ok(Some(member)) = directory.member(account).await {
            names.push(
                SingleLine::new(40)
                    .sanitize(member.handle.as_str())
                    .into_string(),
            );
        }
    }
    names.sort_by_key(|name| name.to_lowercase());
    (!names.is_empty()).then_some(names)
}

/// The rooms the member may join and how many are in each.
async fn chat<D: Backend>(directory: &D, access: &Snapshot, hub: &Hub) -> Option<Vec<RoomCount>> {
    let rooms = joinable_rooms(directory, access).await.ok()?;
    let mut counts: Vec<RoomCount> = rooms
        .iter()
        .map(|room| RoomCount {
            name: room.name.as_str().to_owned(),
            members: u32::try_from(hub.members_in(room.id)).unwrap_or(u32::MAX),
        })
        .collect();
    counts.sort_by(|a, b| b.members.cmp(&a.members).then_with(|| a.name.cmp(&b.name)));
    counts.truncate(8);
    (!counts.is_empty()).then_some(counts)
}

/// The doors this board runs with their seasons.
async fn door_lines<D: DoorStore>(directory: &D, doors: &Doors) -> Option<Vec<DoorLine>> {
    let mut lines = Vec::new();
    for door in doors.all() {
        let info = door.info();
        let season = directory.season(&info.id).await.unwrap_or(1);
        lines.push(DoorLine {
            name: info.title.to_owned(),
            season,
        });
    }
    (!lines.is_empty()).then_some(lines)
}

fn status(session: &SessionRef, capacity: usize) -> Status {
    let all = session.live_sessions();
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    Status {
        sessions: count(all.len()),
        members: count(all.iter().filter(|s| s.account.is_some()).count()),
        capacity: count(capacity),
        frames: all.iter().map(|s| s.frames_sent).sum(),
        bytes: all.iter().map(|s| s.bytes_sent).sum(),
        uptime: session.uptime_secs(),
    }
}
