//! The russh-bbs server binary: parse flags, load the host key, start the SSH server.
//! Everything testable lives in the library; this file only wires it together.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use clap::Parser;
use russh_bbs_core::access::NoMembers;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use tracing::info;
use tracing_subscriber::EnvFilter;

/// Name of the host key file inside the default data directory.
const HOST_KEY_FILE: &str = "host_ed25519_key";

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

    /// Seconds a connection may take to authenticate before it is dropped.
    #[arg(long, env = "RUSSH_BBS_AUTH_TIMEOUT_SECS", default_value_t = 30)]
    auth_timeout_secs: u64,

    /// Seconds without traffic before a connection is closed.
    #[arg(long, env = "RUSSH_BBS_IDLE_TIMEOUT_SECS", default_value_t = 600)]
    idle_timeout_secs: u64,

    /// Maximum concurrent connections.
    #[arg(long, env = "RUSSH_BBS_MAX_CONNECTIONS", default_value_t = 256)]
    max_connections: usize,
}

impl Cli {
    fn limits(&self) -> Limits {
        Limits {
            auth_timeout: Duration::from_secs(self.auth_timeout_secs),
            idle_timeout: Duration::from_secs(self.idle_timeout_secs),
            max_connections: self.max_connections,
            ..Limits::default()
        }
    }

    fn host_key_path(&self) -> anyhow::Result<PathBuf> {
        if let Some(path) = &self.host_key {
            return Ok(path.clone());
        }
        default_host_key_path(
            std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            std::env::var_os("HOME").map(PathBuf::from),
        )
        .context("no --host-key given and neither XDG_DATA_HOME nor HOME is set")
    }
}

/// The default key location: under the XDG data dir, else `~/.local/share`.
fn default_host_key_path(xdg_data_home: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    let base = xdg_data_home
        .filter(|p| p.is_absolute())
        .or_else(|| home.map(|h| h.join(".local").join("share")))?;
    Some(base.join("russh-bbs").join(HOST_KEY_FILE))
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

    let config = ServerConfig {
        listen: cli.listen,
        limits: cli.limits(),
    };
    // No member store yet (M1-B): `NoMembers` makes every key Restricted.
    let server = SshServer::start(&config, &host_key, Arc::new(NoMembers)).await?;

    tokio::signal::ctrl_c()
        .await
        .context("waiting for shutdown signal")?;
    info!("shutting down");
    server.shutdown().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::default_host_key_path;

    #[test]
    fn xdg_data_home_wins_when_absolute() {
        let path = default_host_key_path(Some("/xdg".into()), Some("/home/u".into()));
        assert_eq!(path, Some(PathBuf::from("/xdg/russh-bbs/host_ed25519_key")));
    }

    #[test]
    fn falls_back_to_home_local_share() {
        let path = default_host_key_path(None, Some("/home/u".into()));
        assert_eq!(
            path,
            Some(PathBuf::from(
                "/home/u/.local/share/russh-bbs/host_ed25519_key"
            ))
        );
    }

    #[test]
    fn relative_xdg_is_ignored() {
        let path = default_host_key_path(Some("rel".into()), Some("/home/u".into()));
        assert_eq!(
            path,
            Some(PathBuf::from(
                "/home/u/.local/share/russh-bbs/host_ed25519_key"
            ))
        );
    }

    #[test]
    fn no_env_means_no_default() {
        assert_eq!(default_host_key_path(None, None), None);
    }
}
