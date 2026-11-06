//! The door list and a running door as a state machine: keys in, requests and effects out. No
//! store, no terminal; `draw` turns the state into lines and `exec` runs its requests.

use std::sync::Arc;

use russh_bbs_door::host::Running;
use russh_bbs_door::{Door, DoorEnv, DoorKey, Flow};

use super::exec::{Refusal, Request, Response};
use super::strings;
use crate::ssh::terminal::Key;

/// What the session does after a key.
pub(crate) enum DoorEffect {
    /// Redraw.
    Stay,
    /// Leave the doors for the main menu.
    Exit,
    /// Run this against the store, then give the answer to [`DoorsScreen::deliver`].
    Request(Request),
    /// The member logged off (Ctrl-C, Ctrl-D). Nothing is pending: every key's writes are
    /// flushed before the next key is read.
    LogOff,
}

pub(super) enum State {
    /// The list of doors.
    List,
    /// A request is running; keys are ignored (except logoff) until it answers.
    Busy,
    /// A door is open.
    Playing(Box<Running>),
}

/// The doors screens.
pub(crate) struct DoorsScreen {
    pub(super) doors: Vec<Arc<dyn Door>>,
    pub(super) state: State,
    pub(super) cursor: usize,
    /// What to say under the list: why the last door closed.
    pub(super) notice: Option<&'static str>,
    /// The door that is open or opening, for the title.
    pub(super) current: Option<usize>,
    pub(super) env: DoorEnv,
}

impl DoorsScreen {
    /// Open the door list. `env` describes the member's terminal and the transport.
    pub(crate) fn open(doors: &[Arc<dyn Door>], env: DoorEnv) -> Self {
        Self {
            doors: doors.to_vec(),
            state: State::List,
            cursor: 0,
            notice: None,
            current: None,
            env,
        }
    }

    /// Whether a door is open (not the list, not waiting for the store).
    #[cfg(test)]
    pub(crate) const fn is_playing(&self) -> bool {
        matches!(self.state, State::Playing(_))
    }

    /// The terminal the doors see changed (a window change, a theme or a layout switch).
    pub(crate) fn set_env(&mut self, env: DoorEnv) {
        self.env = env;
    }

    /// A key from the member.
    pub(crate) fn input(&mut self, key: Key) -> DoorEffect {
        if matches!(key, Key::Ctrl('c' | 'd')) {
            return DoorEffect::LogOff;
        }
        match std::mem::replace(&mut self.state, State::Busy) {
            State::Busy => DoorEffect::Stay,
            State::List => {
                self.state = State::List;
                self.list_key(key)
            }
            State::Playing(running) => self.play_key(key, running),
        }
    }

    fn list_key(&mut self, key: Key) -> DoorEffect {
        self.notice = None;
        let count = self.doors.len();
        let pick = match key {
            Key::Up => {
                self.cursor = self.cursor.saturating_sub(1);
                None
            }
            Key::Down => {
                self.cursor = (self.cursor + 1).min(count.saturating_sub(1));
                None
            }
            Key::Enter => Some(self.cursor),
            Key::Char(c) => c
                .to_digit(10)
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| n.checked_sub(1))
                .filter(|n| *n < count),
            Key::Escape => return DoorEffect::Exit,
            _ => None,
        };
        let Some(at) = pick else {
            return DoorEffect::Stay;
        };
        let Some(door) = self.doors.get(at) else {
            return DoorEffect::Stay;
        };
        self.cursor = at;
        self.current = Some(at);
        let request = Request::Open {
            door: Arc::clone(door),
            env: Box::new(self.env.clone()),
        };
        self.state = State::Busy;
        DoorEffect::Request(request)
    }

    fn play_key(&mut self, key: Key, mut running: Box<Running>) -> DoorEffect {
        let Some(door_key) = door_key(key) else {
            self.state = State::Playing(running);
            return DoorEffect::Stay;
        };
        if running.world_is_stale() {
            // Re-read the door's world before this key (one store read), so the door sees other
            // players' edits; the store answers like a flush.
            self.state = State::Busy;
            return DoorEffect::Request(Request::Key {
                running,
                key: door_key,
            });
        }
        let flow = running.key(door_key);
        if flow == Flow::Exit || running.has_pending() {
            // Write before the next key is read, so the writes keep their order and a crash
            // loses at most this key.
            self.state = State::Busy;
            return DoorEffect::Request(Request::Flush {
                running,
                leave: flow == Flow::Exit,
            });
        }
        self.state = State::Playing(running);
        DoorEffect::Stay
    }

    /// The answer to the request taken from the effects.
    pub(crate) fn deliver(&mut self, response: Response) {
        // The door stays open only for an opened door or a flushed one that was not leaving;
        // everything else (a refusal, a leave) is back to the list, with the reason if any.
        let (outcome, leave) = match response {
            Response::Opened(result) => (result, false),
            Response::Flushed { result, leave } => (result, leave),
        };
        match outcome {
            Ok(running) if !leave => self.state = State::Playing(running),
            Ok(_) => {
                self.state = State::List;
                self.current = None;
            }
            Err(refusal) => {
                self.notice = Some(refusal_text(refusal));
                self.state = State::List;
                self.current = None;
            }
        }
    }
}

fn refusal_text(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::Denied => strings::DENIED,
        Refusal::Barred => strings::BARRED,
        Refusal::TooNew => strings::TOO_NEW,
        Refusal::Conflict => strings::CONFLICT,
        Refusal::TooLarge => strings::TOO_LARGE,
        Refusal::Failed => strings::FAILED,
        Refusal::SaveFailed => strings::SAVE_FAILED,
    }
}

/// The key a door sees. Keys the framework has no name for are dropped.
fn door_key(key: Key) -> Option<DoorKey> {
    match key {
        Key::Char(c) => Some(DoorKey::Char(c)),
        Key::Enter => Some(DoorKey::Enter),
        Key::Escape => Some(DoorKey::Escape),
        Key::Backspace => Some(DoorKey::Backspace),
        Key::Up => Some(DoorKey::Up),
        Key::Down => Some(DoorKey::Down),
        Key::Left => Some(DoorKey::Left),
        Key::Right => Some(DoorKey::Right),
        Key::Ctrl(_)
        | Key::Home
        | Key::End
        | Key::Delete
        | Key::PageUp
        | Key::PageDown
        | Key::PasteBreak
        | Key::Other => None,
    }
}
