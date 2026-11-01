//! Rendering to the PTY: a ratatui terminal whose backend writes into an in-memory buffer that
//! the SSH handler then sends down the channel (decision D19, resolving Q5).
//!
//! The terminal uses a **fixed viewport** whose size we set from the client's `pty-req` and
//! `window-change`. ratatui therefore never asks the host's own tty for its size, and a session
//! cannot affect another or the server's console.
//!
//! **Output safety.** The only text drawn here is fixed UI chrome. Nothing from the client is
//! rendered, not even the SSH username. Any later screen that shows user-supplied text must
//! pass it through `russh_bbs_core::sanitize` first.

use std::io::{self, Write};
use std::sync::{Arc, Mutex, PoisonError};

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::queue;
use ratatui::crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};

use super::terminal::PtySize;

/// The placeholder waiting-room text. Fixed chrome: it names no member, board or statistic.
/// The real waiting room is M1-C.
const BANNER: &str = "russh-bbs";
const SUBTITLE: &str = "Waiting room (placeholder)";
const NOTICE: &str = "This key is not recognised as a member.";
const HINT: &str = "Press q to disconnect.";

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

/// One session's screen.
pub(super) struct Screen {
    terminal: Terminal<CrosstermBackend<OutputBuffer>>,
    output: OutputBuffer,
}

impl Screen {
    /// Create a screen of `size`. Nothing is drawn until [`Screen::enter`].
    pub(super) fn new(size: PtySize) -> io::Result<Self> {
        let output = OutputBuffer::default();
        let backend = CrosstermBackend::new(output.clone());
        let options = TerminalOptions {
            viewport: Viewport::Fixed(rect(size)),
        };
        Ok(Self {
            terminal: Terminal::with_options(backend, options)?,
            output,
        })
    }

    /// Switch the client to the alternate screen, hide the cursor and draw. Returns the bytes
    /// to send.
    pub(super) fn enter(&mut self) -> io::Result<Vec<u8>> {
        queue!(self.terminal.backend_mut(), EnterAlternateScreen, Hide)?;
        self.draw()
    }

    /// Apply a new window size and redraw. Returns the bytes to send.
    pub(super) fn resize(&mut self, size: PtySize) -> io::Result<Vec<u8>> {
        self.terminal.resize(rect(size))?;
        self.draw()
    }

    /// Restore the client's terminal (cursor, main screen). Returns the bytes to send.
    pub(super) fn leave(&mut self) -> io::Result<Vec<u8>> {
        queue!(self.terminal.backend_mut(), Show, LeaveAlternateScreen)?;
        Ok(self.take_output())
    }

    fn draw(&mut self) -> io::Result<Vec<u8>> {
        self.terminal.draw(render_waiting_room)?;
        Ok(self.take_output())
    }

    fn take_output(&mut self) -> Vec<u8> {
        self.output.take()
    }
}

fn rect(size: PtySize) -> Rect {
    Rect::new(0, 0, size.cols(), size.rows())
}

/// Draw the placeholder: a bordered box centered in the window.
fn render_waiting_room(frame: &mut Frame<'_>) {
    let area = frame.area();
    let lines = vec![
        Line::styled(BANNER, Style::new().add_modifier(Modifier::BOLD)),
        Line::raw(SUBTITLE),
        Line::raw(""),
        Line::raw(NOTICE),
        Line::raw(HINT),
    ];
    // The block is 4 border/padding rows taller than the text, clamped to the window.
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height.min(area.height)),
        Constraint::Fill(1),
    ])
    .areas(area);
    let paragraph = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(paragraph, middle);
}

#[cfg(test)]
mod tests;
