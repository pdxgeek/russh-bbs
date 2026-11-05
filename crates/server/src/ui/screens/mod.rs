//! The screens that moved onto the new frame, panes and widgets (UI-4, D-TBD).
//!
//! Shared pieces live here; each screen's own drawing stays next to the service that owns it
//! (`forums::draw`, `mail::draw`, `chat::render`, `members`), and calls these:
//!
//! - [`nick`]: a colour for a handle from a hash, never the reserved style;
//! - [`motion`]: the short wipe between screens and the blinking cursor, gated and capped;
//! - [`area`]: the frame for an area screen, with the board's `area` art above it;
//! - [`reader`]: the message reader (header block, quote blocks, scrollbar, tabs);
//! - [`chat`]: the three-pane chat layout;
//! - [`mailbox`], [`community`]: tables and panes for mail and the community screens.

pub(crate) mod nick;

pub(crate) mod motion;

pub(crate) mod reader;

#[cfg(test)]
mod tests;
