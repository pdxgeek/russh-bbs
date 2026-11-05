//! The art that ships with the server: an original login banner and main-menu header for each of
//! the six styles, drawn for this project. Nothing here is copied from another board or art pack.

/// The embedded `.art.toml` files, one per style.
pub(super) const FILES: [&str; 6] = [
    include_str!("bundled/phosphor.art.toml"),
    include_str!("bundled/neon.art.toml"),
    include_str!("bundled/paper.art.toml"),
    include_str!("bundled/amber.art.toml"),
    include_str!("bundled/deepspace.art.toml"),
    include_str!("bundled/ansi.art.toml"),
];
