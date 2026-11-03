//! The chat hub and the chat screen's drawing (M3-F, D84).
//!
//! [`Hub`] is the one place live chat state lives: `russh_bbs_core::chat::Chat` (the rules and
//! the bounded state) behind a `std::sync::Mutex`, plus one [`Notify`] per session. **The lock
//! is never held across an `.await`** and every operation is a few pushes onto bounded queues,
//! so a slow or stalled client cannot hold up a room: its queue drops its oldest events and the
//! room carries on. A client that has stopped reading altogether is cut by the shell's write
//! timeout (D49), which ends the shell and drops its [`ChatSession`], which leaves the room.
//!
//! **No persistence (D78).** Nothing here touches a store except to read room definitions and
//! to let staff open and close rooms (audited by the store). Lines and whispers are held in
//! memory only, are not logged (no `tracing` call in this module formats a line, a whisper or
//! a handle's speech), and are gone on restart.
//!
//! **Push path.** New lines must reach a screen without a keypress. The shell task (`ssh`)
//! waits on its inbox *and* on [`ChatSession::woken`]; either wakes it, it takes what arrived,
//! applies it to the screen and draws once, inside the same `redraw_interval` rate cap as keys
//! (D49), so a busy room costs a session at most 20 redraws a second.

mod render;
mod run;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use russh_bbs_core::access::Granted;
use russh_bbs_core::chat::{
    Chat, ChatLimits, Entered, Event, JoinError, RoomId, SayError, SessionId, Touched, WhisperError,
};
use russh_bbs_core::sanitize::{Sanitized, Username};
use russh_bbs_core::store::AccountId;
use russh_bbs_core::waiting_room::Clock;
use tokio::sync::Notify;

pub(crate) use render::lines;

/// Rows of text that fit in the content box of a frame `frame_height` rows tall: one row goes to
/// the reserved status row, a roomy frame (over 24 rows of content) keeps a row of margin above
/// and below the box (`settings::boxed`), and the border takes two. The chat page fills exactly
/// this, so the input line is always the last row of the box.
pub(crate) fn box_height(frame_height: u16) -> usize {
    let content = frame_height.saturating_sub(1);
    let content = if content > 24 {
        content.saturating_sub(2)
    } else {
        content
    };
    usize::from(content.saturating_sub(2))
}

/// The monotonic clock the send limit reads, shared so tests can move it by hand.
struct SharedClock(Arc<dyn Clock>);

impl Clock for SharedClock {
    fn now(&self) -> Duration {
        self.0.now()
    }
}

struct Inner {
    chat: Chat<SharedClock>,
    wakers: HashMap<SessionId, Arc<Notify>>,
}

/// All live chat state for the process.
pub(crate) struct Hub {
    inner: Mutex<Inner>,
}

impl Hub {
    /// An empty hub with the given bounds, reading `clock` for the send limit.
    pub(crate) fn new(limits: ChatLimits, clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                chat: Chat::new(limits, SharedClock(clock)),
                wakers: HashMap::new(),
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // The state is plain data and every operation leaves it consistent; a panic elsewhere
        // must not turn into a chat outage.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wake the sessions whose queues changed. Called with the lock held; `notify_one` stores a
    /// permit, so a session that is not waiting yet is not missed.
    fn wake(inner: &Inner, touched: &Touched) {
        for id in touched {
            if let Some(waker) = inner.wakers.get(id) {
                waker.notify_one();
            }
        }
    }

    /// Put a session in chat. It is in no room until it joins one.
    pub(crate) fn connect(
        self: &Arc<Self>,
        account: AccountId,
        handle: Username,
    ) -> Result<ChatSession, JoinError> {
        let mut inner = self.lock();
        let id = inner.chat.connect(account, handle)?;
        let wake = Arc::new(Notify::new());
        inner.wakers.insert(id, Arc::clone(&wake));
        drop(inner);
        Ok(ChatSession {
            hub: Arc::clone(self),
            id,
            wake,
        })
    }

    /// Staff closed `room`: put its members out and wake them.
    pub(crate) fn room_closed(&self, room: RoomId) {
        let mut inner = self.lock();
        let touched = inner.chat.close_room(room);
        Self::wake(&inner, &touched);
    }

    /// Sessions in chat (tests and metrics).
    #[cfg(test)]
    pub(crate) fn session_count(&self) -> usize {
        self.lock().chat.session_count()
    }
}

/// One session's place in chat. Dropping it takes the session out of its room and out of the
/// hub, whatever ended the session.
pub(crate) struct ChatSession {
    hub: Arc<Hub>,
    id: SessionId,
    wake: Arc<Notify>,
}

impl ChatSession {
    /// Completes when something was queued for this session (or already was).
    pub(crate) async fn woken(&self) {
        self.wake.notified().await;
    }

    /// Take everything queued, oldest first.
    pub(crate) fn drain(&self) -> Vec<Event> {
        self.hub.lock().chat.drain(self.id)
    }

    /// The room the session is in, if any.
    pub(crate) fn room(&self) -> Option<RoomId> {
        self.hub.lock().chat.room_of(self.id)
    }

    pub(crate) fn join(&self, proof: &Granted) -> Result<Entered, JoinError> {
        let mut inner = self.hub.lock();
        let (entered, touched) = inner.chat.join(proof, self.id)?;
        Hub::wake(&inner, &touched);
        Ok(entered)
    }

    pub(crate) fn leave(&self) {
        let mut inner = self.hub.lock();
        let touched = inner.chat.leave(self.id);
        Hub::wake(&inner, &touched);
    }

    pub(crate) fn say(&self, proof: &Granted, text: Sanitized) -> Result<(), SayError> {
        let mut inner = self.hub.lock();
        let touched = inner.chat.say(proof, self.id, text)?;
        Hub::wake(&inner, &touched);
        Ok(())
    }

    pub(crate) fn whisper(
        &self,
        proof: &Granted,
        to: &str,
        text: Sanitized,
    ) -> Result<(), WhisperError> {
        let mut inner = self.hub.lock();
        let touched = inner.chat.whisper(proof, self.id, to, text)?;
        Hub::wake(&inner, &touched);
        Ok(())
    }

    pub(crate) fn roster(&self) -> Vec<Username> {
        self.hub.lock().chat.roster(self.id)
    }
}

impl Drop for ChatSession {
    fn drop(&mut self) {
        let mut inner = self.hub.lock();
        let touched = inner.chat.disconnect(self.id);
        inner.wakers.remove(&self.id);
        Hub::wake(&inner, &touched);
    }
}
