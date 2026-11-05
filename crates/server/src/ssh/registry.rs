//! A live view of connected sessions: who is connected, at what access, with what window size.
//!
//! It exists so the server can be observed (tests, and later the sysop console) without
//! reaching into per-connection handlers. The lock is a `std` mutex taken only inside
//! non-async methods, so it is never held across an `.await`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::Notify;

use russh_bbs_core::access::{Access, Fingerprint};
use russh_bbs_core::limits::SessionCounter;
use russh_bbs_core::queue::RequestGate;
use russh_bbs_core::registration::OpenGate;
use russh_bbs_core::store::AccountId;

use super::terminal::PtySize;

/// Identifies one SSH connection for the life of the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(u64);

impl SessionId {
    /// The number, for logs and audit notes.
    #[must_use]
    pub fn as_raw(self) -> u64 {
        self.0
    }
}

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
    /// The account this session is signed in as, once it is (FIX-M3-14).
    pub account: Option<AccountId>,
    /// Whether the session proved itself with the account's password (a password login, a join,
    /// a key linked by password, a recovery code's new password). The key it signed in with is
    /// `fingerprint`. Together they say which credential revoking ends this session (D99).
    pub by_password: bool,
}

/// What was revoked, for [`SessionRef::revoke_others`] (FIX-M3-14, D99).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revocation {
    /// The account removed this key: ends the sessions that signed in with it.
    Key(Fingerprint),
    /// The password was set, changed or cleared: ends the sessions that used the password.
    Password,
    /// A recovery code was redeemed and bound this key and a new password: ends the sessions
    /// that used the password and every session on any other key.
    Recovery(Fingerprint),
}

impl Revocation {
    fn hits(&self, session: &SessionSnapshot) -> bool {
        match self {
            Self::Key(key) => session.fingerprint.as_ref() == Some(key),
            Self::Password => session.by_password,
            Self::Recovery(new_key) => {
                session.by_password
                    || session
                        .fingerprint
                        .as_ref()
                        .is_some_and(|key| key != new_key)
            }
        }
    }

    /// The short tag the audit row carries.
    #[must_use]
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Key(_) => "key removed",
            Self::Password => "password changed",
            Self::Recovery(_) => "recovery code",
        }
    }
}

/// The signal that ends a session whose credential was revoked. Remembered if it fires before
/// the session waits on it.
#[derive(Debug, Default)]
pub struct RevokeSignal {
    revoked: AtomicBool,
    wake: Notify,
}

impl RevokeSignal {
    fn fire(&self) {
        self.revoked.store(true, Ordering::Release);
        self.wake.notify_one();
    }

    /// Whether the session was revoked.
    #[must_use]
    pub fn is_revoked(&self) -> bool {
        self.revoked.load(Ordering::Acquire)
    }

    /// Resolves once the session is revoked.
    pub async fn revoked(&self) {
        loop {
            if self.is_revoked() {
                return;
            }
            self.wake.notified().await;
        }
    }
}

#[derive(Debug)]
struct Entry {
    snapshot: SessionSnapshot,
    signal: Arc<RevokeSignal>,
}

/// Shared registry of live sessions.
#[derive(Debug, Default)]
pub struct SessionRegistry {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<SessionId, Entry>>,
    /// Signed-in sessions per account, for the concurrent-session limit (M2-H). In this
    /// process only: a second process has its own count.
    member_sessions: SessionCounter,
    /// The per-key and per-network limits on submitting an invite request (M2-E, D72), shared
    /// by every session of this server and kept in memory only.
    request_gate: RequestGate,
    /// The per-key and per-network limits on creating an account in `open` mode (M2-G, D74),
    /// separate from the request gate and kept in memory only.
    open_gate: OpenGate,
    /// The chat hub (M3-F), shared by every transport.
    chat: crate::chat::SharedHub,
    /// When this registry (the server) started, for the status widget's uptime (UI-10).
    started: StartedAt,
}

/// The time the registry was made.
#[derive(Debug, Clone, Copy)]
struct StartedAt(std::time::Instant);

impl Default for StartedAt {
    fn default() -> Self {
        Self(std::time::Instant::now())
    }
}

impl SessionRegistry {
    /// The chat hub every session of this server shares (M3-F).
    pub(crate) fn chat(&self) -> &Arc<crate::chat::Hub> {
        self.chat.hub()
    }

    /// Register a new connection. The returned guard removes it when dropped.
    pub fn register(self: &Arc<Self>, peer: Option<SocketAddr>) -> SessionGuard {
        let id = SessionId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let signal = Arc::new(RevokeSignal::default());
        self.lock().insert(
            id,
            Entry {
                snapshot: SessionSnapshot {
                    id,
                    peer,
                    fingerprint: None,
                    access: None,
                    pty_size: None,
                    shell_open: false,
                    frames_sent: 0,
                    bytes_sent: 0,
                    input_dropped: 0,
                    account: None,
                    by_password: false,
                },
                signal: Arc::clone(&signal),
            },
        );
        SessionGuard(SessionRef {
            id,
            registry: Arc::clone(self),
            signal,
        })
    }

    /// Copies of every live session, ordered by id.
    #[must_use]
    pub fn snapshots(&self) -> Vec<SessionSnapshot> {
        let mut all: Vec<_> = self.lock().values().map(|e| e.snapshot.clone()).collect();
        all.sort_by_key(|s| s.id);
        all
    }

    fn update(&self, id: SessionId, change: impl FnOnce(&mut SessionSnapshot)) {
        if let Some(entry) = self.lock().get_mut(&id) {
            change(&mut entry.snapshot);
        }
    }

    /// Fire the revoke signal of every session of `account` that `what` hits, except `keep`.
    /// Returns the ids it ended, in order. This process's sessions only (see `docs/access.md`).
    fn revoke(&self, account: AccountId, what: &Revocation, keep: SessionId) -> Vec<SessionId> {
        let mut ended: Vec<SessionId> = self
            .lock()
            .values()
            .filter(|e| {
                e.snapshot.id != keep
                    && !e.signal.is_revoked()
                    && e.snapshot.account == Some(account)
                    && what.hits(&e.snapshot)
            })
            .map(|e| {
                e.signal.fire();
                e.snapshot.id
            })
            .collect();
        ended.sort();
        ended
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<SessionId, Entry>> {
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
    signal: Arc<RevokeSignal>,
}

impl SessionRef {
    /// This session's id.
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// Copies of every live session (the who's-online and status widgets, UI-10).
    #[must_use]
    pub(crate) fn live_sessions(&self) -> Vec<SessionSnapshot> {
        self.registry.snapshots()
    }

    /// Seconds since the server started.
    #[must_use]
    pub(crate) fn uptime_secs(&self) -> u64 {
        self.registry.started.0.elapsed().as_secs()
    }

    /// The count of signed-in sessions per account, shared by every session of this server.
    #[must_use]
    pub fn member_sessions(&self) -> SessionCounter {
        self.registry.member_sessions.clone()
    }

    /// The limits on submitting an invite request, shared by every session of this server.
    #[must_use]
    pub fn request_gate(&self) -> &RequestGate {
        &self.registry.request_gate
    }

    /// The limits on creating an `open` account, shared by every session of this server.
    #[must_use]
    pub fn open_gate(&self) -> &OpenGate {
        &self.registry.open_gate
    }

    /// Record the authenticated key and the access decided for it.
    pub fn set_authenticated(&self, fingerprint: Fingerprint, access: Access) {
        self.registry.update(self.id, |s| {
            s.fingerprint = Some(fingerprint);
            s.access = Some(access);
        });
    }

    /// Record a password login: an account with no key to name, and the access it was given.
    pub fn set_authenticated_by_password(&self, access: Access) {
        self.registry.update(self.id, |s| {
            s.access = Some(access);
            s.by_password = true;
        });
    }

    /// Record that this session also proved the account's password (a key linked by password,
    /// a recovery code's new password, a join that set one), so a later password change ends it.
    pub fn mark_password(&self) {
        self.registry.update(self.id, |s| s.by_password = true);
    }

    /// Record the account this session is signed in as (FIX-M3-14), so revoking one of its
    /// credentials can find the session.
    pub fn set_account(&self, account: AccountId) {
        self.registry.update(self.id, |s| s.account = Some(account));
    }

    /// End every other live session of `account` that `what` hits (FIX-M3-14, D99): each one
    /// shows the fixed revoked line and closes at its next checkpoint. This session stays open.
    /// Returns the sessions it ended.
    #[must_use]
    pub fn revoke_others(&self, account: AccountId, what: &Revocation) -> Vec<SessionId> {
        self.registry.revoke(account, what, self.id)
    }

    /// The signal that this session's credential was revoked.
    #[must_use]
    pub fn revoke_signal(&self) -> &RevokeSignal {
        &self.signal
    }

    /// Record a visitor who offered no key (the waiting room only).
    pub fn set_visitor(&self) {
        self.registry
            .update(self.id, |s| s.access = Some(Access::Restricted));
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
