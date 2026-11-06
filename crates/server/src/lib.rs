//! The russh-bbs server library. The binary in `main.rs` only parses flags and calls in here, so
//! everything is testable.

pub mod blobs;
pub(crate) mod chat;
pub mod community;
pub mod config;
pub mod doors;
pub mod feeds;
pub mod forums;
pub mod founder;
pub mod mail;
pub mod members;
pub mod settings;
pub mod sftp;
pub mod shutdown;
pub mod ssh;
pub mod sysop;
pub mod telnet;
pub mod ui;
pub mod waiting_room;
