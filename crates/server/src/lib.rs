//! The russh-bbs server library. The binary in `main.rs` only parses flags and calls in here, so
//! everything is testable. SFTP and the full TUI arrive in later slices.

pub mod config;
pub mod ssh;
pub mod waiting_room;
