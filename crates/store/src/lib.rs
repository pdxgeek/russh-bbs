//! SQLite implementation of the `russh-bbs-core` storage traits (decisions D6, Q4).
//!
//! One [`SqliteStore`] wraps a connection pool on a WAL-mode database file. Migrations are
//! embedded in the binary, forward-only, and applied by [`SqliteStore::open`]. Queries are
//! compile-time checked by `sqlx`; the checked query data is committed in `.sqlx/` so builds
//! need no live database (`SQLX_OFFLINE=true`).

mod directory;
mod error;
mod founder;
mod rows;
mod sqlite;
mod terminal_settings;

pub use error::OpenError;
pub use sqlite::SqliteStore;

#[cfg(test)]
mod founder_tests;
#[cfg(test)]
mod migrate_tests;
#[cfg(test)]
mod terminal_settings_tests;
#[cfg(test)]
mod tests;
