//! The forum screens (M3-E, D83): board list, thread list, reader, search, and posting and
//! replying through the M3-B editor.
//!
//! The rules are `russh_bbs_core::forums`; this module is the screen state machine
//! ([`ForumsScreen`], pure and unit-tested), its drawing ([`draw`]) and the executor that runs a
//! request against the store with a time limit ([`run`]). Like the other member screens, the
//! state machine never touches the store: a key that needs data produces a [`Request`], the
//! session runs it with the snapshot loaded for that action, and the answer comes back through
//! [`ForumsScreen::deliver`].
//!
//! * **Where content goes.** Subjects, handles, bodies and snippets are sanitized by core and
//!   cleaned again at the point they are drawn. They are drawn into the content box only; the
//!   status row with the security marker is outside it (`ui::chrome`), and no style but the
//!   plain ones here is applied to member text.
//! * **The reader pages long text** with `ui::pager` and the member's pause setting (D65).
//! * **No privacy notice** on any board screen and nothing is called private (D78).

mod draw;
mod exec;
mod flow;
mod strings;

pub(crate) use draw::{DrawCtx, render};
pub(crate) use exec::{Request, Response, run};

/// Longest address the feed prompt accepts while typing.
pub(crate) const fn feeds_url_max() -> usize {
    russh_bbs_core::feeds::MAX_URL_BYTES
}
pub(crate) use flow::{ForumEffect, ForumsScreen};

#[cfg(test)]
mod tests;
