//! The mail screens (M3-I, D78, D90): inbox, reader, compose, reply with quote, delete and
//! report, composing through the M3-B editor.
//!
//! The rules and the sealing are `russh_bbs_core::mail`; this module is the screen state machine
//! ([`MailScreen`], pure and unit-tested), its drawing ([`draw`]), the executor that runs its
//! requests against the store with a time limit ([`run`]), and the mail key file ([`key_file`]).
//! Like the other member screens, the state machine never touches the store or the key: a key
//! that needs data produces a [`Request`], the session runs it with the snapshot loaded for
//! that action, and the answer comes back through [`MailScreen::deliver`].
//!
//! * **Where content goes.** Subjects, handles and bodies are opened and sanitized by core and
//!   cleaned again where they are drawn. They go into the content box only; the status row with
//!   the security marker is outside it, and no style but plain ones is applied to them.
//! * **No privacy notice** on any mail screen, and nothing here calls mail private, secure or
//!   encrypted (D78). The one explanation is in the welcome message and the help screen.
//! * **No staff screen** reads mail. (Reports, which are copies, get their staff screen with
//!   the M5 sysop console.)

mod boot;
mod draw;
mod exec;
mod flow;
pub mod key_file;
mod strings;

pub use boot::{BootError, load_keys};
pub(crate) use draw::{DrawCtx, render};
pub(crate) use exec::{Request, Response, run};
pub(crate) use flow::{MailEffect, MailScreen};

#[cfg(test)]
mod tests;
