//! Board art (UI-3): the `.ans` parser, the `.art.toml` format, art slots, the large-letter
//! fallback, and the sysop preview.
//!
//! Art is a sysop asset, not member content, but it is parsed as untrusted input: it becomes
//! [`Grid`] cells (a character and two optional palette colours, never an attribute) and is drawn
//! only inside its slot and never on the status bar's row. See `docs/ui.md`, "Board art".

mod ans;
mod bundled;
mod cp437;
pub(crate) mod draw;
mod file;
mod font;
mod grid;
mod piece;
pub(crate) mod preview;
mod set;

pub use ans::{MAX_BYTES as ANS_MAX_BYTES, Parsed, Sauce, parse as parse_ans};
pub(crate) use file::quote;
pub use file::{
    ArtFile, MAX_BYTES as ART_TOML_MAX_BYTES, parse as parse_art_toml, write as write_art_toml,
};
pub use font::{HEIGHT as LETTER_HEIGHT, render as big_letters, width_of as big_letters_width};
pub use grid::{ArtCell, ArtError, Grid, MAX_COLS, MAX_ROWS};
pub use piece::{ArtPiece, Slot};
pub use set::{ArtSet, MAX_FILES, Problem};
pub(crate) use set::{ans_piece, page_piece};

#[cfg(test)]
mod tests;
