//! The hub's state: who is in which room, each room's short scrollback, each session's bounded
//! queue, and the rules that bound them. Pure and synchronous: the server wraps it in a lock
//! and wakes the sessions it names, so these rules are tested without a runtime or a sleep.
//!
//! **Nothing here is stored (D78).** Lines and whispers live in memory only: a room's last
//! [`ChatLimits::scrollback`] lines while the process runs, and each session's queue until its
//! shell takes them. A restart forgets all of it. The only thing that leaves this module is
//! what the sessions' own screens draw.
//!
//! **Bounds.**
//! - A line is cut to [`MAX_LINE_CHARS`] by the shared sanitizer before it gets here.
//! - Each member may send [`ChatLimits::rate`] lines and whispers (one allowance for both).
//! - Each room holds [`ChatLimits::scrollback`] lines and [`ChatLimits::members_per_room`]
//!   members; the hub holds [`ChatLimits::sessions`] sessions and [`MAX_ROOMS`] rooms.
//! - Each session has a queue of [`ChatLimits::queue`] events. **A full queue drops its oldest
//!   event** and counts it ([`Event::Missed`]); it never waits and never blocks the room. A
//!   session whose shell is itself stuck writing is disconnected by the shell's write timeout
//!   (D49), which ends the session and so removes it here.
//!
//! **Whispers** go to one session in the sender's room, by handle. A target who is not in the
//! room is [`WhisperError::NotHere`], whether or not the handle exists anywhere else, so the
//! answer reveals nothing the sender could not read off the room's list.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use super::event::{Event, Line};
use super::room::{MAX_ROOMS, RoomId};
use super::text::same_handle;
use crate::access::{Granted, Permission};
use crate::sanitize::{Sanitized, Username};
use crate::store::AccountId;
use crate::waiting_room::{Clock, KeyedLimiter, RatePolicy, Verdict};

/// Identifies one connected session in the hub. Never reused within a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(u64);

/// The bounds. [`ChatLimits::DEFAULT`] is what the server uses; tests build smaller ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatLimits {
    /// Lines and whispers one session may send: a burst, refilling one per interval.
    pub rate: RatePolicy,
    /// Lines a room remembers for people who come in later.
    pub scrollback: usize,
    /// Events a session's queue holds before it starts dropping the oldest.
    pub queue: usize,
    /// Sessions in one room.
    pub members_per_room: usize,
    /// Sessions in chat at all.
    pub sessions: usize,
}

impl ChatLimits {
    /// The server's bounds.
    pub const DEFAULT: Self = Self {
        rate: RatePolicy::new(5, Duration::from_secs(1)),
        scrollback: 100,
        queue: 128,
        members_per_room: 64,
        sessions: 512,
    };
}

/// Why a session could not enter chat or a room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum JoinError {
    /// The hub is at its session cap.
    #[error("chat is full")]
    Busy,
    /// The room is at its member cap.
    #[error("the room is full")]
    Full,
    /// The proof is not a join on this room for this account.
    #[error("no such room")]
    NotFound,
}

/// Why a line was not said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SayError {
    /// The session is not in the room the proof names (or in none).
    #[error("not in a room")]
    NotInRoom,
    /// Too fast. Nothing was sent.
    #[error("slow down")]
    RateLimited {
        /// How long until one more line would be allowed.
        retry_after: Duration,
    },
}

/// Why a whisper was not sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WhisperError {
    /// No such session in this room. One answer for a handle that does not exist, one that is
    /// elsewhere, one that is not valid, and the sender's own.
    #[error("not here")]
    NotHere,
    /// The session is not in the room the proof names.
    #[error("not in a room")]
    NotInRoom,
    /// Too fast.
    #[error("slow down")]
    RateLimited {
        /// How long until one more would be allowed.
        retry_after: Duration,
    },
}

/// What entering a room gives the session to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entered {
    /// Who is in the room now, including the session, by name.
    pub roster: Vec<Username>,
    /// The room's remembered lines, oldest first.
    pub history: Vec<Line>,
}

#[derive(Debug)]
struct Member {
    account: AccountId,
    handle: Username,
    room: Option<RoomId>,
    queue: VecDeque<Event>,
    missed: u32,
}

#[derive(Debug, Default)]
struct RoomRuntime {
    members: Vec<SessionId>,
    scrollback: VecDeque<Line>,
}

/// The sessions to wake because their queue changed.
pub type Touched = Vec<SessionId>;

/// All of chat's live state.
#[derive(Debug)]
pub struct Chat<C> {
    limits: ChatLimits,
    next: u64,
    sessions: HashMap<SessionId, Member>,
    rooms: HashMap<RoomId, RoomRuntime>,
    sends: KeyedLimiter<SessionId, C>,
}

impl<C: Clock> Chat<C> {
    /// An empty hub. `clock` is the monotonic clock the send limit reads.
    #[must_use]
    pub fn new(limits: ChatLimits, clock: C) -> Self {
        Self {
            limits,
            next: 0,
            sessions: HashMap::new(),
            rooms: HashMap::new(),
            sends: KeyedLimiter::new(limits.rate, limits.sessions.saturating_mul(4), clock),
        }
    }

    /// The bounds in force.
    #[must_use]
    pub const fn limits(&self) -> ChatLimits {
        self.limits
    }

    /// Sessions in chat right now.
    #[must_use]
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// The room the session is in, if any.
    #[must_use]
    pub fn room_of(&self, id: SessionId) -> Option<RoomId> {
        self.sessions.get(&id).and_then(|m| m.room)
    }

    /// Sessions in `room` right now.
    #[must_use]
    pub fn member_count(&self, room: RoomId) -> usize {
        self.rooms.get(&room).map_or(0, |r| r.members.len())
    }

    /// Rooms with runtime state (a room someone has been in and that is still open).
    #[must_use]
    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    /// Lines remembered for `room`.
    #[must_use]
    pub fn scrollback_len(&self, room: RoomId) -> usize {
        self.rooms.get(&room).map_or(0, |r| r.scrollback.len())
    }

    /// Put a session in chat (in no room yet).
    pub fn connect(
        &mut self,
        account: AccountId,
        handle: Username,
    ) -> Result<SessionId, JoinError> {
        if self.sessions.len() >= self.limits.sessions {
            return Err(JoinError::Busy);
        }
        self.next += 1;
        let id = SessionId(self.next);
        self.sessions.insert(
            id,
            Member {
                account,
                handle,
                room: None,
                queue: VecDeque::new(),
                missed: 0,
            },
        );
        Ok(id)
    }

    /// Take a session out of chat, however its connection ended. Others in its room are told
    /// it left. Safe to call twice.
    pub fn disconnect(&mut self, id: SessionId) -> Touched {
        let touched = self.leave(id);
        self.sessions.remove(&id);
        touched
    }

    /// Enter the room `proof` names (a `chat.join` on it, for this session's account),
    /// leaving any other first.
    pub fn join(
        &mut self,
        proof: &Granted,
        id: SessionId,
    ) -> Result<(Entered, Touched), JoinError> {
        let room = proof
            .resource()
            .filter(|_| proof.permission() == Permission::ChatJoin)
            .map(|r| RoomId::from_resource(r.id))
            .ok_or(JoinError::NotFound)?;
        let member = self.sessions.get(&id).ok_or(JoinError::NotFound)?;
        if proof.account() != Some(member.account) {
            return Err(JoinError::NotFound);
        }
        if member.room == Some(room) {
            return Ok((self.entered(room), Touched::new()));
        }
        // The caps are checked before the old room is left, so a refusal changes nothing.
        let members = self.rooms.get(&room).map_or(0, |r| r.members.len());
        if members >= self.limits.members_per_room {
            return Err(JoinError::Full);
        }
        if !self.rooms.contains_key(&room) && self.rooms.len() >= MAX_ROOMS {
            return Err(JoinError::Busy);
        }
        let mut touched = self.leave(id);
        let runtime = self.rooms.entry(room).or_default();
        runtime.members.push(id);
        let handle = match self.sessions.get_mut(&id) {
            Some(member) => {
                member.room = Some(room);
                member.handle.clone()
            }
            None => return Err(JoinError::NotFound),
        };
        touched.extend(self.deliver(room, Some(id), &Event::Joined(handle)));
        Ok((self.entered(room), touched))
    }

    /// Leave the current room, if in one. Others are told.
    pub fn leave(&mut self, id: SessionId) -> Touched {
        let Some(member) = self.sessions.get_mut(&id) else {
            return Touched::new();
        };
        let Some(room) = member.room.take() else {
            return Touched::new();
        };
        let handle = member.handle.clone();
        if let Some(runtime) = self.rooms.get_mut(&room) {
            runtime.members.retain(|m| *m != id);
        }
        self.deliver(room, None, &Event::Left(handle))
    }

    /// Say a line in the room `proof` names (`chat.speak` on it). Everyone in the room,
    /// the speaker included, gets it, and the room remembers it.
    pub fn say(
        &mut self,
        proof: &Granted,
        id: SessionId,
        text: Sanitized,
    ) -> Result<Touched, SayError> {
        let room = self
            .in_room(proof, Permission::ChatSpeak, id)
            .ok_or(SayError::NotInRoom)?;
        if let Verdict::Limited { retry_after } = self.sends.check(&id) {
            return Err(SayError::RateLimited { retry_after });
        }
        let from = self
            .sessions
            .get(&id)
            .map(|m| m.handle.clone())
            .ok_or(SayError::NotInRoom)?;
        let line = Line { from, text };
        if let Some(runtime) = self.rooms.get_mut(&room) {
            runtime.scrollback.push_back(line.clone());
            while runtime.scrollback.len() > self.limits.scrollback {
                runtime.scrollback.pop_front();
            }
        }
        Ok(self.deliver(room, None, &Event::Line(line)))
    }

    /// Whisper `text` to the session in the same room whose handle is `to`
    /// (`chat.whisper` on the room). The sender gets an echo; the room does not see it and the
    /// scrollback does not keep it.
    pub fn whisper(
        &mut self,
        proof: &Granted,
        id: SessionId,
        to: &str,
        text: Sanitized,
    ) -> Result<Touched, WhisperError> {
        let room = self
            .in_room(proof, Permission::ChatWhisper, id)
            .ok_or(WhisperError::NotInRoom)?;
        let target = self
            .rooms
            .get(&room)
            .into_iter()
            .flat_map(|r| r.members.iter().copied())
            .find(|m| {
                *m != id
                    && self
                        .sessions
                        .get(m)
                        .is_some_and(|member| same_handle(to, &member.handle))
            })
            .ok_or(WhisperError::NotHere)?;
        if let Verdict::Limited { retry_after } = self.sends.check(&id) {
            return Err(WhisperError::RateLimited { retry_after });
        }
        let (Some(sender), Some(receiver)) = (
            self.sessions.get(&id).map(|m| m.handle.clone()),
            self.sessions.get(&target).map(|m| m.handle.clone()),
        ) else {
            return Err(WhisperError::NotHere);
        };
        self.push(
            target,
            Event::Whisper {
                from: sender,
                text: text.clone(),
            },
        );
        self.push(id, Event::WhisperSent { to: receiver, text });
        Ok(vec![target, id])
    }

    /// Who is in the session's room, one name per account, by name. Empty outside a room.
    #[must_use]
    pub fn roster(&self, id: SessionId) -> Vec<Username> {
        self.sessions
            .get(&id)
            .and_then(|m| m.room)
            .map(|room| self.names(room))
            .unwrap_or_default()
    }

    /// Take everything queued for the session, oldest first, with a count of what was dropped
    /// ahead of it.
    pub fn drain(&mut self, id: SessionId) -> Vec<Event> {
        let Some(member) = self.sessions.get_mut(&id) else {
            return Vec::new();
        };
        let mut events = Vec::with_capacity(member.queue.len() + 1);
        if member.missed > 0 {
            events.push(Event::Missed(member.missed));
            member.missed = 0;
        }
        events.extend(member.queue.drain(..));
        events
    }

    /// Whether anything is waiting for the session.
    #[must_use]
    pub fn has_events(&self, id: SessionId) -> bool {
        self.sessions
            .get(&id)
            .is_some_and(|m| !m.queue.is_empty() || m.missed > 0)
    }

    /// Staff closed `room`: everyone in it is put out and told, and its scrollback is
    /// forgotten.
    pub fn close_room(&mut self, room: RoomId) -> Touched {
        let Some(runtime) = self.rooms.remove(&room) else {
            return Touched::new();
        };
        for id in &runtime.members {
            if let Some(member) = self.sessions.get_mut(id) {
                member.room = None;
            }
            self.push(*id, Event::RoomClosed);
        }
        runtime.members
    }

    fn in_room(&self, proof: &Granted, permission: Permission, id: SessionId) -> Option<RoomId> {
        let room = proof
            .resource()
            .filter(|_| proof.permission() == permission)
            .map(|r| RoomId::from_resource(r.id))?;
        let member = self.sessions.get(&id)?;
        (member.room == Some(room) && proof.account() == Some(member.account)).then_some(room)
    }

    fn names(&self, room: RoomId) -> Vec<Username> {
        let mut seen = Vec::new();
        let mut names = Vec::new();
        for id in self.rooms.get(&room).into_iter().flat_map(|r| &r.members) {
            if let Some(member) = self.sessions.get(id)
                && !seen.contains(&member.account)
            {
                seen.push(member.account);
                names.push(member.handle.clone());
            }
        }
        names.sort_by_key(Username::skeleton);
        names
    }

    fn entered(&self, room: RoomId) -> Entered {
        Entered {
            roster: self.names(room),
            history: self
                .rooms
                .get(&room)
                .map(|r| r.scrollback.iter().cloned().collect())
                .unwrap_or_default(),
        }
    }

    /// Queue `event` for everyone in `room` except `skip`; return who to wake.
    fn deliver(&mut self, room: RoomId, skip: Option<SessionId>, event: &Event) -> Touched {
        let members: Vec<SessionId> = self
            .rooms
            .get(&room)
            .map(|r| {
                r.members
                    .iter()
                    .copied()
                    .filter(|m| Some(*m) != skip)
                    .collect()
            })
            .unwrap_or_default();
        for id in &members {
            self.push(*id, event.clone());
        }
        members
    }

    /// Queue one event, dropping the oldest when the queue is full. Never waits.
    fn push(&mut self, id: SessionId, event: Event) {
        let cap = self.limits.queue.max(1);
        if let Some(member) = self.sessions.get_mut(&id) {
            if member.queue.len() >= cap {
                member.queue.pop_front();
                member.missed = member.missed.saturating_add(1);
            }
            member.queue.push_back(event);
        }
    }
}
