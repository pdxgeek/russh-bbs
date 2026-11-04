//! The door framework (M4-D0, D98): the plumbing every door game builds on, and nothing else.
//!
//! A *door* is a game that runs inside a member's session. Each game has its own engine and
//! systems; this crate shares only what the rules in `docs/doors/README.md` ask every door to
//! have, and makes the right thing the easy thing:
//!
//! | Rule | Where it lives |
//! |---|---|
//! | The menu hands a session to the door and gets it back | [`Door`], [`DoorSession`], [`Flow`] |
//! | A pure engine, tested by replay | [`DoorRng`], [`replay`] |
//! | Saves through the store only, versioned, forward-only, capped | [`Save`], [`host`], core's `doors` |
//! | An injected clock | [`Ctx::now`], [`Ctx::today`] |
//! | A daily tick that is idempotent and catches up with a cap | [`tick`] |
//! | No filesystem or network | [`sandbox`] and the shape of every API here |
//! | Text is sanitized, names follow the reserved-name rule | [`Canvas`], [`Ctx::player_name`], [`Ctx::player_text`] |
//! | Content is data, validated by a schema test | [`content`] |
//! | A balance harness | [`sim`] |
//! | The same behavior over ssh and telnet | [`DoorEnv::transport`] is information only |
//!
//! **What a door is given.** Keys ([`DoorKey`]), a [`Canvas`] to draw on, a [`Ctx`] with the
//! injected clock, a seeded RNG and a way to ask for a save, and a [`DoorEnv`] describing the
//! member's settings. It is given no path, socket, thread, task, store handle or environment, and
//! nothing in this crate hands one out. A door is a *library called in-process*; see
//! [`sandbox`] for what that does and does not guarantee.

#![forbid(unsafe_code)]

mod ascii;
mod canvas;
mod color;
pub mod content;
mod ctx;
mod door;
pub mod host;
mod key;
pub mod replay;
mod rng;
pub mod sandbox;
pub mod sim;
mod surface;
pub mod tick;

pub use canvas::{Canvas, DoorFrame, DoorPalette, DoorStyle};
pub use color::ColorDepth;
pub use surface::{CleanBuffer, Composite, DoorSurface, Drew};
pub use ctx::{
    Ctx, Effects, PLAYER_TEXT_MAX_CHARS, PLAYER_TEXT_PER_WINDOW, PLAYER_TEXT_WINDOW_SECS,
    TextRefused, WorldEdit,
};
pub use door::{
    BlobKind, Door, DoorEnv, DoorInfo, DoorSession, Flow, MigrateError, Save, Start, TransportKind,
};
pub use key::DoorKey;
pub use rng::DoorRng;

// The ratatui a door draws with, so a door crate needs no dependency but this one (the sandbox
// audit allows no other). Only the drawing modules are re-exported: there is no backend,
// terminal or crossterm here for a door to reach.
pub use ratatui::{buffer, layout, style, symbols, text, widgets};

// The types a door names in its signatures, so a door crate needs no dependency but this one.
pub use russh_bbs_core::doors::{DoorId, InvalidDoorId};
pub use russh_bbs_core::icons::Tier;
pub use russh_bbs_core::limits::Day;
