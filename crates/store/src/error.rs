//! Errors from opening the store and mapping database failures onto core's [`StoreError`].

use russh_bbs_core::store::StoreError;
use sqlx::error::ErrorKind;

/// Why the store could not be opened. All of these are startup errors.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// The database file or its directory could not be prepared.
    #[error("cannot prepare database path {path}")]
    Path {
        /// The path that failed.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// SQLite refused to open the database.
    #[error("cannot open database")]
    Connect(#[source] sqlx::Error),
    /// The Postgres URL or TLS settings are unusable. The message never contains the password.
    #[error("invalid postgres settings: {0}")]
    PostgresSettings(String),
    /// Postgres could not be reached or refused the connection: network, TLS or authentication.
    /// The message never contains the password.
    #[error("cannot connect to postgres: {0}")]
    PostgresConnect(String),
    /// A migration failed, or a landed migration was edited or is unknown to this binary.
    #[error("cannot apply migrations")]
    Migrate(#[source] sqlx::migrate::MigrateError),
}

/// Map an unexpected database error. Constraint violations the caller can name are handled
/// before this is reached.
pub(crate) fn backend(error: sqlx::Error) -> StoreError {
    StoreError::Backend(Box::new(error))
}

/// The constraint class of a database error, if it is one.
pub(crate) fn kind(error: &sqlx::Error) -> Option<ErrorKind> {
    error
        .as_database_error()
        .map(sqlx::error::DatabaseError::kind)
}
