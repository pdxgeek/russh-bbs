//! SQLite and Postgres implementations of the `russh-bbs-core` storage traits (decisions D6, D24).
//!
//! One module per database ([`sqlite`], [`postgres`]), each with its own SQL, its own
//! forward-only migration set under `migrations/<backend>/`, and its own pool. There is no
//! `AnyPool` and no dialect-neutral SQL: the two dialects differ and each is tested for real
//! (D24). Rules that do not depend on SQL live once, in `directory`; row mapping in `rows`.
//!
//! Queries are compile-time checked by `sqlx`; the checked query data is committed in
//! `.sqlx/` so builds need no live database (`SQLX_OFFLINE=true`). See `regenerate-sqlx.sh`.
//!
//! The behaviour suite in `suite` is generic over the core traits and runs against every
//! backend (D32).

#[cfg(any(feature = "sqlite", feature = "postgres"))]
mod access_rows;
#[cfg(any(feature = "sqlite", feature = "postgres"))]
mod audit_rows;
mod directory;
mod draft_rows;
mod error;
#[cfg(any(feature = "sqlite", feature = "postgres"))]
mod invite_rows;
mod limit_rows;
mod profile_rows;
#[cfg(any(feature = "sqlite", feature = "postgres"))]
mod queue_rows;
#[cfg(any(feature = "sqlite", feature = "postgres"))]
mod rows;

#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "sqlite")]
mod sqlite;

#[cfg(test)]
mod migration_fixture;
#[cfg(test)]
mod suite;

pub use error::OpenError;
#[cfg(feature = "postgres")]
pub use postgres::{PostgresSettings, PostgresStore, TlsMode};
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStore;

/// Release a store's resources at shutdown: SQLite checkpoints its WAL and closes the pool,
/// Postgres closes the pool. After this, calls on the store fail (and callers fail closed).
pub trait Close: Send + Sync {
    /// Close the store.
    fn close(&self) -> impl std::future::Future<Output = ()> + Send;
}

#[cfg(feature = "sqlite")]
impl Close for SqliteStore {
    async fn close(&self) {
        SqliteStore::close(self).await;
    }
}

#[cfg(feature = "postgres")]
impl Close for PostgresStore {
    async fn close(&self) {
        PostgresStore::close(self).await;
    }
}
