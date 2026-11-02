//! A live view of connected sessions: who is connected, at what access, with what window size.
//!
//! It exists so the server can be observed (tests, and later the sysop console) without
//! reaching into per-connection handlers. The lock is a `std` mutex taken only inside
//! non-async methods, so it is never held across an `.await`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use russh_bbs_core::access::{Access, Fingerprint};
use russh_bbs_core::limits::SessionCounter;

use super::terminal::PtySize;

/// Identifies one SSH connection for the life of the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(u64);

/// A point-in-time copy of one session's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    /// The session's id.
    pub id: SessionId,
    /// Remote address, if known.
    pub peer: Option<SocketAddr>,
    /// The authenticated key, once auth succeeded.
    pub fingerprint: Option<Fingerprint>,
    /// The access level decided at authentication.
    pub access: Option<Access>,
    /// The current terminal size, once a PTY was granted.
    pub pty_size: Option<PtySize>,
    /// Whether the interactive shell screen is up.
    pub shell_open: bool,
    /// Frames sent to the client so far (a frame is one redraw: at most one per
    /// `Limits::redraw_interval`). Lets tests and the sysop console see how much work a session
    /// caused.
    pub frames_sent: u64,
    /// Bytes in those frames. A peer that never reads makes the server drop the connection
    /// once its output has been stuck for `Limits::write_stall`, so this stays bounded.
    pub bytes_sent: u64,
    /// Input bytes dropped because the session was over its input budget.
    pub input_dropped: u64,
}

/// Shared registry of live sessions.
#[derive(Debug, Default)]
pub struct SessionRegistry {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<SessionId, SessionSnapshot>>,
    /// Signed-in sessions per account, for the concurrent-session limit (M2-H). In this
    /// process only: a second process has its own count.
    member_sessions: SessionCounter,
}

impl SessionRegistry {
    /// Register a new connection. The returned guard removes it when dropped.
    pub fn register(self: &Arc<Self>, peer: Option<SocketAddr>) -> SessionGuard {
        let id = SessionId(self.next_id.fetch_add(1, Ordering::Relaxed));
        self.lock().insert(
            id,
            SessionSnapshot {
                id,
                peer,
                fingerprint: None,
                access: None,
                pty_size: None,
                shell_open: false,
                frames_sent: 0,
                bytes_sent: 0,
                input_dropped: 0,
            },
        );
        SessionGuard(SessionRef {
            id,
            registry: Arc::clone(self),
        })
    }

    /// Copies of every live session, ordered by id.
    #[must_use]
    pub fn snapshots(&self) -> Vec<SessionSnapshot> {
        let mut all: Vec<_> = self.lock().values().cloned().collect();
        all.sort_by_key(|s| s.id);
        all
    }

    fn update(&self, id: SessionId, change: impl FnOnce(&mut SessionSnapshot)) {
        if let Some(entry) = self.lock().get_mut(&id) {
            change(entry);
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<SessionId, SessionSnapshot>> {
        // A poisoned lock only means another thread panicked mid-update of plain data; the map
        // is still usable and refusing to serve would turn one bug into an outage.
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A cloneable reference to one session's registry entry, for the tasks of that session. It can
/// update the entry but does not remove it: only the [`SessionGuard`] does.
#[derive(Debug, Clone)]
pub struct SessionRef {
    id: SessionId,
    registry: Arc<SessionRegistry>,
}

impl SessionRef {
    /// This session's id.
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// The count of signed-in sessions per account, shared by every session of this server.
    #[must_use]
    pub fn member_sessions(&self) -> SessionCounter {
        self.registry.member_sessions.clone()
    }

    /// Record the authenticated key and the access decided for it.
    pub fn set_authenticated(&self, fingerprint: Fingerprint, access: Access) {
        self.registry.update(self.id, |s| {
            s.fingerprint = Some(fingerprint);
            s.access = Some(access);
        });
    }

    /// Record the terminal size (initial or after a window change).
    pub fn set_size(&self, size: PtySize) {
        self.registry.update(self.id, |s| s.pty_size = Some(size));
    }

    /// Record that the shell screen is up or gone.
    pub fn set_shell_open(&self, open: bool) {
        self.registry.update(self.id, |s| s.shell_open = open);
    }

    /// Count one frame of `bytes` bytes handed to the client's output queue.
    pub fn add_frame(&self, bytes: usize) {
        self.registry.update(self.id, |s| {
            s.frames_sent = s.frames_sent.saturating_add(1);
            s.bytes_sent = s
                .bytes_sent
                .saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
        });
    }

    /// Record the running total of input bytes dropped for being over budget.
    pub fn set_input_dropped(&self, total: u64) {
        self.registry.update(self.id, |s| s.input_dropped = total);
    }
}

/// Removes its session from the registry on drop. It owns the entry; [`SessionRef`]s borrow it.
#[derive(Debug)]
pub struct SessionGuard(SessionRef);

impl SessionGuard {
    /// A reference the session's other tasks can hold.
    #[must_use]
    pub fn reference(&self) -> SessionRef {
        self.0.clone()
    }
}

impl std::ops::Deref for SessionGuard {
    type Target = SessionRef;

    fn deref(&self) -> &SessionRef {
        &self.0
    }
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.0.registry.lock().remove(&self.0.id);
    }
}

#[cfg(test)]
mod tests;
