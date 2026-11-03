//! The SQLite-backed store: opening, migrating, and the storage trait implementations (D6, D24).
//!
//! SQLite-only SQL: `?` placeholders, `unixepoch()`, `INTEGER PRIMARY KEY AUTOINCREMENT`. The
//! migration set is `migrations/sqlite/`; the Postgres backend has its own.

mod access;
mod audit;
mod chat;
mod credits;
mod drafts;
mod feeds;
mod forums;
mod founder;
mod invites;
mod limits;
mod passwords;
mod profiles;
mod queue;
mod registration;
mod scheduler;
mod state;
mod terminal_settings;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use russh_bbs_core::access::{Fingerprint, Slug};
use russh_bbs_core::audit::{SystemWallClock, WallClock, events};
use russh_bbs_core::store::{
    Account, AccountId, AccountStore, NewAccount, SettingKey, SettingsStore, StoreError,
};
use sqlx::error::ErrorKind;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Connection as _, SqlitePool};

use crate::error::{OpenError, backend, kind};
use crate::rows::AccountRow;

/// The migrations embedded at build time, in version order.
pub(crate) static MIGRATOR: Migrator = sqlx::migrate!("./migrations/sqlite");

/// How long a writer waits for the lock before failing with "busy".
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Pool size. WAL allows many readers beside one writer; the board is small, so a handful of
/// connections is plenty and bounds file handles.
const MAX_CONNECTIONS: u32 = 5;

/// Mode of a newly created database file: owner only. It holds the member list.
#[cfg(unix)]
const DB_FILE_MODE: u32 = 0o600;

/// Mode of a newly created data directory.
#[cfg(unix)]
const DB_DIR_MODE: u32 = 0o700;

/// A SQLite database implementing the core storage traits. Cheap to clone; clones share a pool.
#[derive(Debug, Clone)]
pub struct SqliteStore {
    pool: SqlitePool,
    /// Where audit timestamps come from (injected so tests control it).
    clock: Arc<dyn WallClock>,
}

impl SqliteStore {
    /// Open the database at `path`, creating the file (mode 0600) and its directory (mode 0700)
    /// on first run, then apply pending migrations. WAL and foreign keys are always on.
    pub async fn open(path: &Path) -> Result<Self, OpenError> {
        prepare_path(path).await.map_err(|source| OpenError::Path {
            path: path.to_owned(),
            source,
        })?;
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(BUSY_TIMEOUT);
        migrate(&options).await?;
        let pool = SqlitePoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .connect_with(options)
            .await
            .map_err(OpenError::Connect)?;
        Ok(Self {
            pool,
            clock: Arc::new(SystemWallClock),
        })
    }

    /// Use `clock` for audit timestamps instead of the system clock.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn WallClock>) -> Self {
        self.clock = clock;
        self
    }

    /// The audit clock.
    pub(crate) fn clock(&self) -> &dyn WallClock {
        &*self.clock
    }

    /// The pool, for sibling modules and tests in this crate.
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Checkpoint the WAL into the main file (truncating it), then close the pool. The
    /// checkpoint is best effort: a failure is only reported, because SQLite recovers an
    /// un-checkpointed WAL on the next open. The connection used for it is closed explicitly,
    /// not returned to the pool, so that nothing is still settling when this returns.
    pub async fn close(&self) {
        match self.pool.acquire().await {
            Ok(connection) => {
                let mut connection = connection.detach();
                if let Err(error) = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                    .execute(&mut connection)
                    .await
                {
                    tracing::warn!(%error, "WAL checkpoint failed at close");
                }
                if let Err(error) = connection.close().await {
                    tracing::warn!(%error, "closing the checkpoint connection failed");
                }
            }
            Err(error) => tracing::warn!(%error, "no connection for the WAL checkpoint at close"),
        }
        self.pool.close().await;
    }
}

/// Apply pending migrations on one dedicated connection that has **foreign keys off**, then
/// verify no foreign key dangles, then close it. Only after that does the pool (foreign keys on)
/// exist.
///
/// Why: SQLite's table rebuild (the 12-step procedure, used by `0007_invites.sql` to drop a
/// column) has to run with foreign keys off, or `DROP TABLE accounts` deletes the table's rows
/// first and the `ON DELETE CASCADE` of every table that references it deletes theirs. The
/// `PRAGMA foreign_keys` switch is a no-op inside a transaction and sqlx wraps each migration in
/// one, so it cannot be done from the migration file; it has to be the connection's setting. The
/// price is that no migration is checked row by row as it runs, so the check at the end is not
/// optional: `PRAGMA foreign_key_check` must come back empty or the database is refused.
async fn migrate(options: &SqliteConnectOptions) -> Result<(), OpenError> {
    let mut connection = sqlx::SqliteConnection::connect_with(&options.clone().foreign_keys(false))
        .await
        .map_err(OpenError::Connect)?;
    let result = migrate_on(&mut connection).await;
    if let Err(error) = connection.close().await {
        tracing::warn!(%error, "closing the migration connection failed");
    }
    result
}

/// Run the migrations on `connection` and refuse a result with dangling foreign keys.
pub(crate) async fn migrate_on(connection: &mut sqlx::SqliteConnection) -> Result<(), OpenError> {
    MIGRATOR
        .run(&mut *connection)
        .await
        .map_err(OpenError::Migrate)?;
    let dangling = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut *connection)
        .await
        .map_err(OpenError::Connect)?;
    if dangling.is_empty() {
        Ok(())
    } else {
        Err(OpenError::ForeignKeys {
            rows: dangling.len(),
        })
    }
}

/// Create the directory and an empty owner-only file so SQLite does not create them with the
/// process umask. An existing file is left alone.
async fn prepare_path(path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
        && !tokio::fs::try_exists(dir).await?
    {
        tokio::fs::create_dir_all(dir).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            tokio::fs::set_permissions(dir, std::fs::Permissions::from_mode(DB_DIR_MODE)).await?;
        }
    }
    let mut file = tokio::fs::OpenOptions::new();
    file.create(true).append(true);
    #[cfg(unix)]
    file.mode(DB_FILE_MODE);
    file.open(path).await.map(drop)
}

impl AccountStore for SqliteStore {
    async fn create_account(&self, account: NewAccount) -> Result<Account, StoreError> {
        let skeleton = account.username.skeleton();
        let username = account.username.as_str();
        // The authority is the grant below (D33); `member` needs no grant: `everyone` carries it.
        let state = account.state.as_str();
        let input_role = account.role.clone();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let row = sqlx::query_as!(
            AccountRow,
            r#"INSERT INTO accounts (username, username_skeleton, state)
               VALUES (?, ?, ?)
               RETURNING id AS "id!", username, state, created_at"#,
            username,
            skeleton,
            state,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => StoreError::UsernameTaken,
            _ => backend(error),
        })?;
        if account.role != Slug::MEMBER {
            let role = account.role.as_str();
            sqlx::query!(
                "INSERT INTO access_grant (account_id, role) VALUES (?, ?)",
                row.id,
                role,
            )
            .execute(&mut *tx)
            .await
            .map_err(|error| match kind(&error) {
                Some(ErrorKind::ForeignKeyViolation) => {
                    StoreError::Corrupt("the role to grant does not exist".into())
                }
                _ => backend(error),
            })?;
        }
        let account = row.into_account()?;
        self.audit_in(
            &mut tx,
            &events::account_created(account.id, &input_role, account.state),
        )
        .await?;
        tx.commit().await.map_err(backend)?;
        Ok(account)
    }

    async fn attach_fingerprint(
        &self,
        account: AccountId,
        fingerprint: &Fingerprint,
    ) -> Result<(), StoreError> {
        let id = account.as_raw();
        let fingerprint = fingerprint.as_str();
        sqlx::query!(
            "INSERT INTO account_keys (fingerprint, account_id) VALUES (?, ?)",
            fingerprint,
            id,
        )
        .execute(self.pool())
        .await
        .map(drop)
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => StoreError::FingerprintTaken,
            Some(ErrorKind::ForeignKeyViolation) => StoreError::AccountNotFound,
            _ => backend(error),
        })
    }

    async fn account_by_fingerprint(
        &self,
        fingerprint: &Fingerprint,
    ) -> Result<Option<Account>, StoreError> {
        let fingerprint = fingerprint.as_str();
        let row = sqlx::query_as!(
            AccountRow,
            r#"SELECT a.id AS "id!", a.username, a.state, a.created_at
               FROM account_keys k JOIN accounts a ON a.id = k.account_id
               WHERE k.fingerprint = ?"#,
            fingerprint,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        row.map(AccountRow::into_account).transpose()
    }
}

impl SettingsStore for SqliteStore {
    async fn get_setting(&self, key: SettingKey) -> Result<Option<String>, StoreError> {
        let key = key.as_str();
        sqlx::query_scalar!("SELECT value FROM settings WHERE key = ?", key)
            .fetch_optional(self.pool())
            .await
            .map_err(backend)
    }

    async fn set_setting(&self, key: SettingKey, value: &str) -> Result<(), StoreError> {
        let key = key.as_str();
        sqlx::query!(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            key,
            value,
        )
        .execute(self.pool())
        .await
        .map(drop)
        .map_err(backend)
    }
}
