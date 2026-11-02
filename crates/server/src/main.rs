//! The russh-bbs server binary: parse flags, load the host key, start the SSH server.
//! Everything testable lives in the library; this file only wires it together.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use clap::Parser;
use russh_bbs_core::waiting_room::RatePolicy;
use russh_bbs_server::founder;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use tracing::info;
use tracing_subscriber::EnvFilter;

/// Name of the host key file inside the default data directory.
const HOST_KEY_FILE: &str = "host_ed25519_key";

/// Name of the database file inside the default data directory.
const DB_FILE: &str = "russh-bbs.db";

/// russh-bbs: an invite-only, SSH-first bulletin board.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    /// Address and port to listen on for SSH.
    #[arg(long, env = "RUSSH_BBS_LISTEN", default_value = "0.0.0.0:2222")]
    listen: SocketAddr,

    /// Path of the SSH host key. Generated with mode 0600 on first start if missing.
    /// Defaults to `$XDG_DATA_HOME/russh-bbs/host_ed25519_key`, or
    /// `$HOME/.local/share/russh-bbs/host_ed25519_key`. Keep it outside the repository.
    #[arg(long, env = "RUSSH_BBS_HOST_KEY")]
    host_key: Option<PathBuf>,

    /// Path of the SQLite database. Created (mode 0600) with its migrations on first start.
    /// Defaults to `$XDG_DATA_HOME/russh-bbs/russh-bbs.db`, or
    /// `$HOME/.local/share/russh-bbs/russh-bbs.db`. Keep it outside the repository.
    #[arg(long, env = "RUSSH_BBS_DB")]
    db: Option<PathBuf>,

    /// Seconds a connection may take to authenticate before it is dropped.
    #[arg(long, env = "RUSSH_BBS_AUTH_TIMEOUT_SECS", default_value_t = 30)]
    auth_timeout_secs: u64,

    /// Seconds without traffic before a connection is closed.
    #[arg(long, env = "RUSSH_BBS_IDLE_TIMEOUT_SECS", default_value_t = 600)]
    idle_timeout_secs: u64,

    /// Maximum concurrent connections.
    #[arg(long, env = "RUSSH_BBS_MAX_CONNECTIONS", default_value_t = 256)]
    max_connections: usize,

    /// New connections allowed back to back from one network (IPv4 address or IPv6 /64).
    #[arg(long, env = "RUSSH_BBS_IP_BURST", default_value_t = 10)]
    ip_burst: u32,

    /// Seconds to earn back one connection for a network.
    #[arg(long, env = "RUSSH_BBS_IP_INTERVAL_SECS", default_value_t = 2)]
    ip_interval_secs: u64,

    /// Authentication attempts allowed back to back for one key.
    #[arg(long, env = "RUSSH_BBS_KEY_BURST", default_value_t = 5)]
    key_burst: u32,

    /// Seconds to earn back one authentication attempt for a key.
    #[arg(long, env = "RUSSH_BBS_KEY_INTERVAL_SECS", default_value_t = 10)]
    key_interval_secs: u64,

    /// Waiting-room form submissions per session before it is disconnected.
    #[arg(long, env = "RUSSH_BBS_MAX_FORM_SUBMISSIONS", default_value_t = 5)]
    max_form_submissions: u32,
}

impl Cli {
    fn limits(&self) -> Limits {
        Limits {
            auth_timeout: Duration::from_secs(self.auth_timeout_secs),
            idle_timeout: Duration::from_secs(self.idle_timeout_secs),
            max_connections: self.max_connections,
            ip_rate: RatePolicy::new(self.ip_burst, Duration::from_secs(self.ip_interval_secs)),
            key_rate: RatePolicy::new(self.key_burst, Duration::from_secs(self.key_interval_secs)),
            max_form_submissions: self.max_form_submissions,
            ..Limits::default()
        }
    }

    fn host_key_path(&self) -> anyhow::Result<PathBuf> {
        if let Some(path) = &self.host_key {
            return Ok(path.clone());
        }
        default_data_path(HOST_KEY_FILE)
            .context("no --host-key given and neither XDG_DATA_HOME nor HOME is set")
    }

    fn db_path(&self) -> anyhow::Result<PathBuf> {
        if let Some(path) = &self.db {
            return Ok(path.clone());
        }
        default_data_path(DB_FILE)
            .context("no --db given and neither XDG_DATA_HOME nor HOME is set")
    }
}

/// `file` inside the default data directory, read from the environment.
fn default_data_path(file: &str) -> Option<PathBuf> {
    default_path(
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        file,
    )
}

/// The default location of `file`: under the XDG data dir, else `~/.local/share`.
fn default_path(
    xdg_data_home: Option<PathBuf>,
    home: Option<PathBuf>,
    file: &str,
) -> Option<PathBuf> {
    let base = xdg_data_home
        .filter(|p| p.is_absolute())
        .or_else(|| home.map(|h| h.join(".local").join("share")))?;
    Some(base.join("russh-bbs").join(file))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let key_path = cli.host_key_path()?;
    let load_path = key_path.clone();
    let host_key = tokio::task::spawn_blocking(move || HostKey::load_or_create(&load_path))
        .await
        .context("host key task panicked")??;
    // The fingerprint is public: the sysop publishes it so users can verify the first connect.
    info!(path = %key_path.display(), fingerprint = %host_key.fingerprint(), "host key ready");

    let db_path = cli.db_path()?;
    let store = SqliteStore::open(&db_path)
        .await
        .with_context(|| format!("opening database {}", db_path.display()))?;
    info!(path = %db_path.display(), "database ready");

    let config = ServerConfig {
        listen: cli.listen,
        limits: cli.limits(),
    };
    // The store decides who is a member; an unknown key or a store error is Restricted.
    // First run: print the one-time setup token to the console (D38).
    let founder = founder::start(&store, &mut std::io::stdout()).await?;
    let server = SshServer::start_with_founder(
        &config,
        &host_key,
        Arc::new(store.clone()),
        Arc::new(founder),
    )
    .await?;

    tokio::signal::ctrl_c()
        .await
        .context("waiting for shutdown signal")?;
    info!("shutting down");
    server.shutdown().await;
    store.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{DB_FILE, HOST_KEY_FILE, default_path};

    #[test]
    fn xdg_data_home_wins_when_absolute() {
        let path = default_path(Some("/xdg".into()), Some("/home/u".into()), HOST_KEY_FILE);
        assert_eq!(path, Some(PathBuf::from("/xdg/russh-bbs/host_ed25519_key")));
    }

    #[test]
    fn falls_back_to_home_local_share() {
        let path = default_path(None, Some("/home/u".into()), HOST_KEY_FILE);
        assert_eq!(
            path,
            Some(PathBuf::from(
                "/home/u/.local/share/russh-bbs/host_ed25519_key"
            ))
        );
    }

    #[test]
    fn relative_xdg_is_ignored() {
        let path = default_path(Some("rel".into()), Some("/home/u".into()), HOST_KEY_FILE);
        assert_eq!(
            path,
            Some(PathBuf::from(
                "/home/u/.local/share/russh-bbs/host_ed25519_key"
            ))
        );
    }

    #[test]
    fn the_database_lives_beside_the_host_key() {
        let path = default_path(Some("/xdg".into()), None, DB_FILE);
        assert_eq!(path, Some(PathBuf::from("/xdg/russh-bbs/russh-bbs.db")));
    }

    #[test]
    fn no_env_means_no_default() {
        assert_eq!(default_path(None, None, HOST_KEY_FILE), None);
    }
}
