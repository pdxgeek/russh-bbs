//! Rendering to the PTY: a ratatui terminal whose backend writes into an in-memory buffer that
//! the SSH handler then sends down the channel (decision D19, resolving Q5).
//!
//! The terminal uses a **fixed viewport** whose size we set from the client's `pty-req` and
//! `window-change`. ratatui therefore never asks the host's own tty for its size, and a session
//! cannot affect another or the server's console.
//!
//! **What is drawn.** The waiting room (`russh_bbs_core::waiting_room`, M1-C): the core's
//! [`WaitingRoom`] decides the page and its contents, `crate::waiting_room` draws them.
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
use russh_bbs_core::waiting_room::{RoomEffect, RoomInput, WaitingRoom};

use super::terminal::{Key, PtySize};
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
}

/// One session's screen.
pub(super) struct Screen {
    terminal: Terminal<CrosstermBackend<OutputBuffer>>,
    output: OutputBuffer,
    room: WaitingRoom,
}

impl Screen {
    /// Create a screen of `size` showing `room`. Nothing is drawn until [`Screen::enter`].
    pub(super) fn new(size: PtySize, room: WaitingRoom) -> io::Result<Self> {
        let output = OutputBuffer::default();
        Ok(Self {
            terminal: new_terminal(&output, size)?,
            output,
            room,
        })
    }

    /// Feed keypresses to the room and redraw once. Stops at the first key that ends the
    /// session; keys after it are ignored.
    pub(super) fn keys(&mut self, keys: &[Key]) -> io::Result<ScreenUpdate> {
        let mut disconnect = false;
        for input in keys.iter().copied().filter_map(room_input) {
            if self.room.input(input) == RoomEffect::Disconnect {
                disconnect = true;
                break;
            }
        }
        Ok(ScreenUpdate {
            bytes: self.draw()?,
            disconnect,
        })
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
        queue!(self.terminal.backend_mut(), Clear(ClearType::All))?;
        self.terminal = new_terminal(&self.output, size)?;
        self.draw()
    }

    /// Restore the client's terminal (cursor, main screen). Returns the bytes to send.
    pub(super) fn leave(&mut self) -> io::Result<Vec<u8>> {
        queue!(self.terminal.backend_mut(), Show, LeaveAlternateScreen)?;
        Ok(self.take_output())
    }

    fn draw(&mut self) -> io::Result<Vec<u8>> {
        let view = self.room.view();
        self.terminal.draw(|frame| render(frame, &view))?;
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

#[cfg(test)]
mod tests;
