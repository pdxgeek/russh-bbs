//! Command-line flags. They are the top layer: each one overrides the same setting from the
//! environment, the file and the default. Environment variables are read by this crate (not by
//! clap) so that an error can say whether a value came from a flag or from the environment.

use std::path::PathBuf;

use clap::Parser;

use super::keys;
use super::layers::CliValue;

/// russh-bbs: an invite-only, SSH-first bulletin board.
///
/// Settings come from, lowest to highest: built-in defaults, the config file, `RUSSH_BBS_*`
/// environment variables, these flags. See docs/config.md for every key.
#[derive(Debug, Default, Parser)]
#[command(version, about)]
pub struct Cli {
    /// Path of the TOML config file (env: `RUSSH_BBS_CONFIG`). A missing file named here is an
    /// error; with no file named, the default locations are tried and "none" means defaults.
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Address and port to listen on for SSH (`ssh.listen`).
    #[arg(long)]
    listen: Option<String>,

    /// Path of the SSH host key (`ssh.host_key`). Generated with mode 0600 on first start.
    #[arg(long)]
    host_key: Option<String>,

    /// Path of the SQLite database (`datasource.path`).
    #[arg(long)]
    db: Option<String>,

    /// Seconds a connection may take to authenticate (`ssh.auth_timeout_secs`).
    #[arg(long)]
    auth_timeout_secs: Option<String>,

    /// Seconds without traffic before a connection is closed (`ssh.idle_timeout_secs`).
    #[arg(long)]
    idle_timeout_secs: Option<String>,

    /// Maximum concurrent connections (`ssh.max_connections`).
    #[arg(long)]
    max_connections: Option<String>,

    /// New connections allowed back to back per network (`waiting_room.ip_burst`).
    #[arg(long)]
    ip_burst: Option<String>,

    /// Seconds to earn back one connection per network (`waiting_room.ip_interval_secs`).
    #[arg(long)]
    ip_interval_secs: Option<String>,

    /// Authentication attempts allowed back to back per key (`waiting_room.key_burst`).
    #[arg(long)]
    key_burst: Option<String>,

    /// Seconds to earn back one attempt per key (`waiting_room.key_interval_secs`).
    #[arg(long)]
    key_interval_secs: Option<String>,

    /// Waiting-room form submissions per session (`waiting_room.max_form_submissions`).
    #[arg(long)]
    max_form_submissions: Option<String>,
}

impl Cli {
    /// The flags that were given, as overrides.
    #[must_use]
    pub fn overrides(&self) -> Vec<CliValue> {
        let given = [
            ("ssh.listen", &self.listen),
            ("ssh.host_key", &self.host_key),
            ("datasource.path", &self.db),
            ("ssh.auth_timeout_secs", &self.auth_timeout_secs),
            ("ssh.idle_timeout_secs", &self.idle_timeout_secs),
            ("ssh.max_connections", &self.max_connections),
            ("waiting_room.ip_burst", &self.ip_burst),
            ("waiting_room.ip_interval_secs", &self.ip_interval_secs),
            ("waiting_room.key_burst", &self.key_burst),
            ("waiting_room.key_interval_secs", &self.key_interval_secs),
            (
                "waiting_room.max_form_submissions",
                &self.max_form_submissions,
            ),
        ];
        given
            .into_iter()
            .filter_map(|(path, text)| {
                let def = keys::by_path(path)?;
                text.as_ref().map(|t| CliValue {
                    def,
                    text: t.clone(),
                })
            })
            .collect()
    }
}
