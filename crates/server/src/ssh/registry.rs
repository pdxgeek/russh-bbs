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
}

/// Shared registry of live sessions.
#[derive(Debug, Default)]
pub struct SessionRegistry {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<SessionId, SessionSnapshot>>,
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
            },
        );
        SessionGuard {
            id,
            registry: Arc::clone(self),
        }
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

/// Removes its session from the registry on drop and is the handle for updating it.
#[derive(Debug)]
pub struct SessionGuard {
    id: SessionId,
    registry: Arc<SessionRegistry>,
}

impl SessionGuard {
    /// This session's id.
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.id
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
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.registry.lock().remove(&self.id);
    }
}

#[cfg(test)]
mod tests;
