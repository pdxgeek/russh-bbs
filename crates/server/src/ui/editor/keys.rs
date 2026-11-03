//! The editor's keys: one table from what the terminal parser delivers to what the editor does.
//!
//! The editor never sees raw bytes. The SSH layer's [`Key`] delivers the arrows, `Home`, `End`,
//! `Delete`, `PageUp` and `PageDown` (CSI and SS3 forms), and a line break inside a bracketed paste
//! as [`Key::PasteBreak`]; this table is the one place a paste break becomes a new line. The
//! Emacs-style Ctrl chords work on every terminal too. Non-ASCII text is still not delivered.
//!
//! **Universal keys (D54) inside the editor.** Esc is "back", but a stray Esc must not throw
//! away a half-written message, so with unsaved changes it asks first. Ctrl-C and Ctrl-D log
//! off from every page, here too, and the editor hands the host the draft to save first.
//! `?` is help in menus; in an editor it is a character, so help is Ctrl-G. The pause setting
//! (a pager) does not apply: an editor scrolls with the cursor instead of stopping at pages.

use crate::ssh::terminal::Key;

/// What a key does in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorKey {
    /// A printable character.
    Char(char),
    /// A new line.
    Enter,
    /// Spaces up to the next tab stop.
    Tab,
    /// Delete the character before the cursor.
    Backspace,
    /// Delete the character under the cursor.
    Delete,
    /// Cursor left.
    Left,
    /// Cursor right.
    Right,
    /// Cursor up one row.
    Up,
    /// Cursor down one row.
    Down,
    /// Start of the line.
    Home,
    /// End of the line.
    End,
    /// Up one screen.
    PageUp,
    /// Down one screen.
    PageDown,
    /// Delete the word before the cursor.
    DeleteWord,
    /// Delete to the end of the line, keeping it for [`EditorKey::Yank`].
    KillLine,
    /// Put back what [`EditorKey::KillLine`] kept.
    Yank,
    /// Undo the last edit.
    Undo,
    /// Redo.
    Redo,
    /// Send the message.
    Send,
    /// Leave (asks first when there are unsaved changes).
    Leave,
    /// Show the key list.
    Help,
    /// Log off (Ctrl-C, Ctrl-D).
    LogOff,
}

impl EditorKey {
    /// The editor key a terminal key means, or `None` for a key the editor ignores.
    #[must_use]
    pub fn from_terminal(key: Key) -> Option<Self> {
        Some(match key {
            Key::Char(c) => Self::Char(c),
            // A pasted line break is a new line only here; every other screen drops it.
            Key::Enter | Key::PasteBreak => Self::Enter,
            Key::Delete => Self::Delete,
            Key::Backspace => Self::Backspace,
            Key::Escape => Self::Leave,
            // Tab arrives as Ctrl-I.
            Key::Ctrl('i') => Self::Tab,
            Key::Ctrl('c' | 'd') => Self::LogOff,
            Key::Ctrl('x') => Self::Send,
            Key::Ctrl('g') => Self::Help,
            Key::Left | Key::Ctrl('b') => Self::Left,
            Key::Right | Key::Ctrl('f') => Self::Right,
            // The arrow keys the parser delivers since M3-A, and Ctrl-P and Ctrl-N.
            Key::Up | Key::Ctrl('p') => Self::Up,
            Key::Down | Key::Ctrl('n') => Self::Down,
            Key::Home | Key::Ctrl('a') => Self::Home,
            Key::End | Key::Ctrl('e') => Self::End,
            Key::PageUp | Key::Ctrl('t') => Self::PageUp,
            Key::PageDown | Key::Ctrl('v') => Self::PageDown,
            Key::Ctrl('w') => Self::DeleteWord,
            Key::Ctrl('k') => Self::KillLine,
            Key::Ctrl('y') => Self::Yank,
            Key::Ctrl('z') => Self::Undo,
            Key::Ctrl('r') => Self::Redo,
            Key::Ctrl(_) | Key::Other => return None,
        })
    }
}
