//! The Postgres-backed store: connecting (TLS always on, D31), migrating, and the storage trait
//! implementations (D6, D24).
//!
//! Postgres-native SQL: `$n` placeholders, identity columns, `extract(epoch FROM now())` for
//! integer Unix seconds (consistent with D35's `UnixSeconds`). The migration set is
//! `migrations/postgres/`. This is also the backend that runs against managed Postgres (RDS and
//! similar) and against `PGlite` through its wire-protocol socket server: both are deployments of
//! this code, not other backends.

mod access;
mod audit;
mod credits;
mod drafts;
mod founder;
mod invites;
mod limits;
mod profiles;
mod queue;
mod registration;
mod scheduler;
mod state;
mod terminal_settings;

#[cfg(test)]
mod tests;

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr as _;
use std::sync::Arc;
use std::time::Duration;

use russh_bbs_core::access::{Fingerprint, Slug};
use russh_bbs_core::audit::{SystemWallClock, WallClock, events};
use russh_bbs_core::store::{
    Account, AccountId, AccountStore, NewAccount, SettingKey, SettingsStore, StoreError,
};
use sqlx::error::ErrorKind;
use sqlx::migrate::Migrator;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use sqlx::{Connection as _, PgConnection, PgPool};

use crate::error::{OpenError, backend, kind};
use crate::rows::AccountRow;

/// The migrations embedded at build time, in version order.
pub(crate) static MIGRATOR: Migrator = sqlx::migrate!("./migrations/postgres");

/// Pool size. Same as SQLite (D35): the board is small and this bounds server connections.
const MAX_CONNECTIONS: u32 = 5;

/// How long to wait for a connection, so an unreachable server fails boot promptly instead of
/// after sqlx's 30 s default.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);

/// How strictly the server's certificate is checked. There is deliberately no way to turn TLS
/// off (D31).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsMode {
    /// Encrypt the connection. The certificate is **not** checked unless a root certificate is
    /// supplied, so this stops eavesdropping but not an active attacker.
    Require,
    /// Encrypt and check that the certificate chains to a trusted root.
    VerifyCa,
    /// Encrypt, check the chain and check that the certificate is for the host in the URL.
    VerifyFull,
}

impl TlsMode {
    const fn ssl_mode(self) -> PgSslMode {
        match self {
            Self::Require => PgSslMode::Require,
            Self::VerifyCa => PgSslMode::VerifyCa,
            Self::VerifyFull => PgSslMode::VerifyFull,
        }
    }
}

/// Where and how to reach Postgres. The URL may carry a password, so `Debug` never prints it.
#[derive(Clone)]
pub struct PostgresSettings {
    url: String,
    tls: TlsMode,
    root_cert: Option<PathBuf>,
}

impl fmt::Debug for PostgresSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresSettings")
            .field("url", &"<redacted>")
            .field("tls", &self.tls)
            .field("root_cert", &self.root_cert)
            .finish()
    }
}

impl PostgresSettings {
    /// A `postgres://` or `postgresql://` URL, the TLS mode, and an optional PEM file of root
    /// certificates to trust (for a private CA or the RDS bundle).
    #[must_use]
    pub fn new(url: impl Into<String>, tls: TlsMode, root_cert: Option<PathBuf>) -> Self {
        Self {
            url: url.into(),
            tls,
            root_cert,
        }
    }

    /// The URL's secrets (password, `password=` parameter), for scrubbing error text.
    fn secrets(&self) -> Vec<String> {
        let mut found = Vec::new();
        if let Some(rest) = self.url.split_once("://").map(|(_, rest)| rest) {
            let authority = rest.split(['/', '?']).next().unwrap_or_default();
            if let Some((userinfo, _)) = authority.rsplit_once('@')
                && let Some((_, password)) = userinfo.split_once(':')
            {
                found.push(password.to_owned());
            }
        }
        if let Some((_, query)) = self.url.split_once('?') {
            for pair in query.split('&') {
                if let Some(("password", value)) = pair.split_once('=') {
                    found.push(value.to_owned());
                }
            }
        }
        found.retain(|secret| !secret.is_empty());
        found
    }

    /// Replace every secret in `text` so an error message cannot leak it.
    fn scrub(&self, text: &str) -> String {
        // Longest first, so a secret that contains another is removed whole.
        let mut secrets = self.secrets();
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        secrets
            .iter()
            .fold(text.to_owned(), |text, secret| text.replace(secret, "***"))
    }

    /// Parse the URL and force TLS. The URL's own `sslmode` can never weaken it.
    fn connect_options(&self) -> Result<PgConnectOptions, OpenError> {
        let settings_error = |message: String| OpenError::PostgresSettings(self.scrub(&message));
        let mut options = PgConnectOptions::from_str(&self.url)
            .map_err(|error| settings_error(error.to_string()))?;
        if options.get_socket().is_some() {
            return Err(settings_error(
                "a unix socket path cannot be used because TLS is required; use a host name".into(),
            ));
        }
        let wanted = self.tls.ssl_mode();
        if self.url.contains("sslmode=") && !same_mode(options.get_ssl_mode(), wanted) {
            return Err(settings_error(
                "the URL's sslmode conflicts with datasource.tls_mode; remove sslmode from the \
                 URL (TLS is set by datasource.tls_mode and cannot be turned off)"
                    .into(),
            ));
        }
        options = options.ssl_mode(wanted);
        if let Some(path) = &self.root_cert {
            options = options.ssl_root_cert(path);
        }
        Ok(options)
    }
}

fn same_mode(a: PgSslMode, b: PgSslMode) -> bool {
    std::mem::discriminant(&a) == std::mem::discriminant(&b)
}

/// A Postgres database implementing the core storage traits. Cheap to clone; clones share a
/// pool.
#[derive(Debug, Clone)]
pub struct PostgresStore {
    pool: PgPool,
    /// Where audit timestamps come from (injected so tests control it).
    clock: Arc<dyn WallClock>,
}

impl PostgresStore {
    /// Connect with TLS, then apply pending migrations. Fails if the server does not offer TLS,
    /// if the certificate check fails, or if the credentials are refused. The error text never
    /// contains the password.
    pub async fn open(settings: &PostgresSettings) -> Result<Self, OpenError> {
        let options = settings.connect_options()?;
        Self::open_with(options, settings).await
    }

    pub(crate) async fn open_with(
        options: PgConnectOptions,
        settings: &PostgresSettings,
    ) -> Result<Self, OpenError> {
        // One direct connection first: a pool hides the real cause (refused, TLS, authentication)
        // behind "pool timed out", and a boot error should say what is wrong.
        let probe = PgConnection::connect_with(&options)
            .await
            .map_err(|error| OpenError::PostgresConnect(settings.scrub(&chain(&error))))?;
        drop(probe.close().await);
        let pool = PgPoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .acquire_timeout(ACQUIRE_TIMEOUT)
            .connect_with(options)
            .await
            .map_err(|error| OpenError::PostgresConnect(settings.scrub(&chain(&error))))?;
        MIGRATOR.run(&pool).await.map_err(OpenError::Migrate)?;
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
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Close the pool. There is no file to checkpoint, so this is all graceful shutdown needs.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

/// An error and its sources on one line, so a TLS failure names its cause.
fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        // sqlx's own Display often already ends with its cause; do not say it twice.
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

impl AccountStore for PostgresStore {
    async fn create_account(&self, account: NewAccount) -> Result<Account, StoreError> {
        let skeleton = account.username.skeleton();
        let username = account.username.as_str();
        // The authority is the grant below (D33); `member` needs no grant: `everyone` carries it.
        let state = account.state.as_str();
        let input_role = account.role.clone();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let row = sqlx::query_as!(
            AccountRow,
            "INSERT INTO accounts (username, username_skeleton, state)
             VALUES ($1, $2, $3)
             RETURNING id, username, state, created_at",
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
                "INSERT INTO access_grant (account_id, role) VALUES ($1, $2)",
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
            "INSERT INTO account_keys (fingerprint, account_id) VALUES ($1, $2)",
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
            "SELECT a.id, a.username, a.state, a.created_at
             FROM account_keys k JOIN accounts a ON a.id = k.account_id
             WHERE k.fingerprint = $1",
            fingerprint,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        row.map(AccountRow::into_account).transpose()
    }
}

impl SettingsStore for PostgresStore {
    async fn get_setting(&self, key: SettingKey) -> Result<Option<String>, StoreError> {
        let key = key.as_str();
        sqlx::query_scalar!("SELECT value FROM settings WHERE key = $1", key)
            .fetch_optional(self.pool())
            .await
            .map_err(backend)
    }

    async fn set_setting(&self, key: SettingKey, value: &str) -> Result<(), StoreError> {
        let key = key.as_str();
        sqlx::query!(
            "INSERT INTO settings (key, value) VALUES ($1, $2)
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
            key,
            value,
        )
        .execute(self.pool())
        .await
        .map(drop)
        .map_err(backend)
    }
}
