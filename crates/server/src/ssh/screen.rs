//! Rendering to the PTY: a ratatui terminal whose backend writes into an in-memory buffer that
//! the SSH handler then sends down the channel (decision D19, resolving Q5).
//!
//! The terminal uses a **fixed viewport** whose size we set from the client's `pty-req` and
//! `window-change`. ratatui therefore never asks the host's own tty for its size, and a session
//! cannot affect another or the server's console.
//!
//! **What is drawn.** One of three [`Content`]s, each a core state machine that produces a
//! [`View`]: the waiting room (M1-C, drawn by `crate::waiting_room`), the founder token prompt
//! (`ssh claim@host`, M1-D), or a member's signed-in session (M1-E, drawn by
//! `crate::settings` according to the member's terminal settings).
//!
//! **Output safety.** Every string is fixed UI chrome or has passed the core sanitizer (the
//! sysop's banner and rules, and what the visitor typed). The SSH username is never rendered.

use std::io::{self, Write};
use std::sync::{Arc, Mutex, PoisonError};

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::queue;
use ratatui::crossterm::terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};
use russh_bbs_core::founder::{ClaimEffect, ClaimForm, ClaimInput};
use russh_bbs_core::terminal_settings::{MemberEffect, MemberFlow, MemberInput, TerminalSettings};
use russh_bbs_core::waiting_room::{RoomEffect, RoomInput, View, WaitingRoom};

use super::terminal::{Key, PtySize};
use crate::settings::{self, Look};
use crate::waiting_room::render;

/// Bytes the backend has written and the handler has not yet sent. The backend owns one handle
/// and the screen keeps another to drain it, because ratatui does not expose its writer.
/// The lock is held only for a copy, never across an `.await`.
#[derive(Debug, Clone, Default)]
struct OutputBuffer(Arc<Mutex<Vec<u8>>>);

impl OutputBuffer {
    fn take(&self) -> Vec<u8> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Write for OutputBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// What a batch of keypresses did.
#[derive(Debug)]
pub(super) struct ScreenUpdate {
    /// Bytes to send to the client (the redraw).
    pub(super) bytes: Vec<u8>,
    /// Whether the session should end after sending them.
    pub(super) disconnect: bool,
    /// A token the person submitted at the claim prompt. The handler checks it (it needs the
    /// store) and reports back through [`Screen::claim_rejected`] or [`Screen::claim_accepted`].
    pub(super) claim_token: Option<String>,
    /// Terminal settings the member changed. The handler stores them.
    pub(super) save_settings: Option<TerminalSettings>,
}

/// What a session is looking at.
pub(super) enum Content {
    /// The waiting room, for a key that is not a member.
    Room(WaitingRoom),
    /// The founder token prompt.
    Claim(ClaimForm),
    /// A member's signed-in session: login, glyph test, settings, home, logoff.
    Member(Box<MemberFlow>),
}

impl Content {
    fn view(&self) -> View {
        match self {
            Self::Room(room) => room.view(),
            Self::Claim(form) => form.view(),
            Self::Member(flow) => flow.view(),
        }
    }

    /// How to draw: a member's settings choose color and borders; everyone else gets the
    /// plain layout (the waiting room and claim prompt predate settings and use no color).
    fn look(&self) -> Option<Look> {
        match self {
            Self::Member(flow) => Some(Look::of(flow.settings())),
            Self::Room(_) | Self::Claim(_) => None,
        }
    }
}

/// One session's screen.
pub(super) struct Screen {
    terminal: Terminal<CrosstermBackend<OutputBuffer>>,
    output: OutputBuffer,
    content: Content,
    /// What the client reported. The drawn size is this with the member's overrides applied.
    pty: PtySize,
}

impl Screen {
    /// Create a screen of `size` showing `content`. Nothing is drawn until [`Screen::enter`].
    pub(super) fn new(size: PtySize, content: Content) -> io::Result<Self> {
        let output = OutputBuffer::default();
        let drawn = drawn_size(size, &content);
        Ok(Self {
            terminal: new_terminal(&output, drawn)?,
            output,
            content,
            pty: size,
        })
    }

    /// Feed keypresses to the current content and redraw once. Stops at the first key that
    /// ends the session or submits a token; keys after it are ignored.
    pub(super) fn keys(&mut self, keys: &[Key]) -> io::Result<ScreenUpdate> {
        let mut disconnect = false;
        let mut claim_token = None;
        let mut save_settings = None;
        match &mut self.content {
            Content::Room(room) => {
                for input in keys.iter().copied().filter_map(room_input) {
                    if room.input(input) == RoomEffect::Disconnect {
                        disconnect = true;
                        break;
                    }
                }
            }
            Content::Claim(form) => {
                for input in keys.iter().copied().filter_map(claim_input) {
                    match form.input(input) {
                        ClaimEffect::Stay => {}
                        ClaimEffect::Disconnect => {
                            disconnect = true;
                            break;
                        }
                        ClaimEffect::Submit(token) => {
                            claim_token = Some(token);
                            break;
                        }
                    }
                }
            }
            Content::Member(flow) => {
                for input in keys.iter().copied().filter_map(member_input) {
                    match flow.input(input) {
                        MemberEffect::Stay => {}
                        MemberEffect::Save(settings) => save_settings = Some(settings),
                        MemberEffect::Disconnect => {
                            disconnect = true;
                            break;
                        }
                    }
                }
            }
        }
        let bytes = if save_settings.is_some() {
            // A size override changes the drawn size; rebuild so the old cells are cleared.
            self.rebuild()?
        } else {
            self.draw()?
        };
        Ok(ScreenUpdate {
            bytes,
            disconnect,
            claim_token,
            save_settings,
        })
    }

    /// The submitted token failed. Redraws with the one generic message; the bool is whether
    /// the session should now end (attempts used up).
    pub(super) fn claim_rejected(&mut self) -> io::Result<(Vec<u8>, bool)> {
        let disconnect = match &mut self.content {
            Content::Claim(form) => form.rejected() == ClaimEffect::Disconnect,
            Content::Room(_) | Content::Member(_) => false,
        };
        Ok((self.draw()?, disconnect))
    }

    /// The claim succeeded: the new sysop's session starts at the login screen.
    pub(super) fn claim_accepted(&mut self, flow: MemberFlow) -> io::Result<Vec<u8>> {
        if let Content::Claim(form) = &mut self.content {
            form.accepted();
        }
        self.content = Content::Member(Box::new(flow));
        self.rebuild()
    }

    /// Switch the client to the alternate screen, hide the cursor and draw. Returns the bytes
    /// to send.
    pub(super) fn enter(&mut self) -> io::Result<Vec<u8>> {
        queue!(self.terminal.backend_mut(), EnterAlternateScreen, Hide)?;
        self.draw()
    }

    /// Apply a new window size and redraw. Returns the bytes to send.
    ///
    /// ratatui's own `Terminal::resize` asks the backend for its size, and the crossterm
    /// backend answers by querying the *server process's* terminal, which is wrong here and
    /// fails without a tty. So we clear the client's screen and build a fresh fixed-viewport
    /// terminal over the same output buffer; nothing ever consults the host tty.
    pub(super) fn resize(&mut self, size: PtySize) -> io::Result<Vec<u8>> {
        self.pty = size;
        self.rebuild()
    }

    /// Clear the client's screen, size a fresh terminal from the client's size and the member's
    /// overrides, and draw.
    fn rebuild(&mut self) -> io::Result<Vec<u8>> {
        queue!(self.terminal.backend_mut(), Clear(ClearType::All))?;
        self.terminal = new_terminal(&self.output, drawn_size(self.pty, &self.content))?;
        self.draw()
    }

    /// Restore the client's terminal (cursor, main screen). After a logoff the goodbye is
    /// printed on the main screen, so it stays in the scrollback. Returns the bytes to send.
    pub(super) fn leave(&mut self) -> io::Result<Vec<u8>> {
        queue!(self.terminal.backend_mut(), Show, LeaveAlternateScreen)?;
        let mut bytes = self.take_output();
        if let Content::Member(flow) = &self.content
            && let Some(lines) = flow.farewell()
        {
            for line in lines {
                bytes.extend_from_slice(line.as_bytes());
                bytes.extend_from_slice(b"\r\n");
            }
        }
        Ok(bytes)
    }

    fn draw(&mut self) -> io::Result<Vec<u8>> {
        let view = self.content.view();
        let look = self.content.look();
        self.terminal.draw(|frame| match look {
            Some(look) => settings::render(frame, &view, look),
            None => render(frame, &view),
        })?;
        Ok(self.take_output())
    }

    fn take_output(&mut self) -> Vec<u8> {
        self.output.take()
    }
}

fn new_terminal(
    output: &OutputBuffer,
    size: PtySize,
) -> io::Result<Terminal<CrosstermBackend<OutputBuffer>>> {
    let options = TerminalOptions {
        viewport: Viewport::Fixed(rect(size)),
    };
    Terminal::with_options(CrosstermBackend::new(output.clone()), options)
}

/// The size to draw at: the client's, with a member's width and height overrides applied.
fn drawn_size(pty: PtySize, content: &Content) -> PtySize {
    match content {
        Content::Member(flow) => {
            let (cols, rows) = flow.settings().size(pty.cols(), pty.rows());
            PtySize::from_request(u32::from(cols), u32::from(rows))
        }
        Content::Room(_) | Content::Claim(_) => pty,
    }
}

fn rect(size: PtySize) -> Rect {
    Rect::new(0, 0, size.cols(), size.rows())
}

/// Translate a keypress for the room. Keys the room has no use for are dropped.
fn room_input(key: Key) -> Option<RoomInput> {
    match key {
        Key::Char(c) => Some(RoomInput::Char(c)),
        Key::Enter => Some(RoomInput::Enter),
        Key::Backspace => Some(RoomInput::Backspace),
        Key::Escape => Some(RoomInput::Escape),
        Key::Ctrl('c' | 'd') => Some(RoomInput::Quit),
        Key::Ctrl(_) | Key::Other => None,
    }
}

/// Translate a keypress for the claim prompt.
fn claim_input(key: Key) -> Option<ClaimInput> {
    match key {
        Key::Char(c) => Some(ClaimInput::Char(c)),
        Key::Enter => Some(ClaimInput::Enter),
        Key::Backspace => Some(ClaimInput::Backspace),
        Key::Escape => Some(ClaimInput::Escape),
        Key::Ctrl('c' | 'd') => Some(ClaimInput::Quit),
        Key::Ctrl(_) | Key::Other => None,
    }
}

/// Translate a keypress for a member's session.
fn member_input(key: Key) -> Option<MemberInput> {
    match key {
        Key::Char(c) => Some(MemberInput::Char(c)),
        Key::Enter => Some(MemberInput::Enter),
        Key::Backspace => Some(MemberInput::Backspace),
        Key::Escape => Some(MemberInput::Escape),
        Key::Ctrl('c' | 'd') => Some(MemberInput::Quit),
        Key::Ctrl(_) | Key::Other => None,
    }
}

#[cfg(test)]
mod tests;
