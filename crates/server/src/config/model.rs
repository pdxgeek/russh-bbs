//! The typed configuration. Every section and key exists with its default even where the
//! feature behind it is not built yet, so later slices add behavior, not plumbing.
//! `docs/config.md` documents each key; a test keeps that file and the key registry in step.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use russh_bbs_core::store::RegistrationMode;
use russh_bbs_core::waiting_room::RatePolicy;
use serde::{Deserialize, Deserializer, de};

use super::SecretUrl;
use crate::ssh::{Limits, RenderCap};

/// The whole configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Who may register (D2, D23). See `docs/decisions.md` for how it meets the stored setting.
    #[serde(deserialize_with = "de_registration_mode")]
    pub registration_mode: RegistrationMode,
    /// SSH listener, host key, timeouts and limits.
    pub ssh: SshConfig,
    /// Where the board's data lives.
    pub datasource: DatasourceConfig,
    /// Where member-uploaded bytes live (D30). Parsed only.
    pub blobs: BlobsConfig,
    /// The optional telnet transport (D27). Parsed only.
    pub telnet: TelnetConfig,
    /// RSS in (feeds as boards) and RSS out (M3-G, D89).
    pub feeds: FeedsConfig,
    /// Waiting-room rate limits (D37).
    pub waiting_room: WaitingRoomConfig,
    /// Internal mail (M3-I, D90): where the key that seals stored mail lives.
    pub mail: MailConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            registration_mode: RegistrationMode::Invite,
            ssh: SshConfig::default(),
            datasource: DatasourceConfig::default(),
            blobs: BlobsConfig::default(),
            telnet: TelnetConfig::default(),
            feeds: FeedsConfig::default(),
            waiting_room: WaitingRoomConfig::default(),
            mail: MailConfig::default(),
        }
    }
}

/// `[mail]` (M3-I, D90).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct MailConfig {
    /// The mail key file; `None` means `mail.key` in the data directory, beside the host key.
    /// Generated with mode 0600 on the first start of a board that has no mail. **Back it up
    /// separately from the database: without it stored mail cannot be read.**
    pub key_file: Option<PathBuf>,
}

/// `[ssh]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SshConfig {
    /// Address and port to listen on.
    pub listen: SocketAddr,
    /// Host key path; `None` means the default data directory (D20).
    pub host_key: Option<PathBuf>,
    /// Seconds from accept to authenticated.
    pub auth_timeout_secs: u64,
    /// Seconds without traffic before a connection is closed.
    pub idle_timeout_secs: u64,
    /// Seconds between keepalives.
    pub keepalive_interval_secs: u64,
    /// Concurrent connections.
    pub max_connections: usize,
    /// Widest area one session draws into, in cells (D53).
    pub render_cols: u16,
    /// Tallest area one session draws into, in cells (D53).
    pub render_rows: u16,
    /// Authentication attempts per connection.
    pub max_auth_attempts: u32,
    /// Milliseconds a failed authentication is delayed.
    pub auth_rejection_millis: u64,
    /// Opt-in password login over SSH (D28, D81). Off by default: the server then offers public
    /// key only.
    pub allow_password_login: bool,
    /// Milliseconds between two redraws of one session at the fastest (D49).
    pub redraw_interval_millis: u64,
    /// Requests per second one session may send after login (D49).
    pub requests_per_second: u32,
    /// Input bytes per second one session may send (D49).
    pub input_bytes_per_second: u32,
    /// Over-budget events before a session is disconnected (D49).
    pub flood_strikes: u32,
    /// Seconds a peer may leave the server unable to send it output before it is dropped (D49).
    pub write_stall_secs: u64,
}

impl Default for SshConfig {
    fn default() -> Self {
        let limits = Limits::default();
        Self {
            listen: SocketAddr::from(([0, 0, 0, 0], 2222)),
            host_key: None,
            auth_timeout_secs: limits.auth_timeout.as_secs(),
            idle_timeout_secs: limits.idle_timeout.as_secs(),
            keepalive_interval_secs: limits.keepalive_interval.as_secs(),
            max_connections: limits.max_connections,
            render_cols: limits.render_cap.cols(),
            render_rows: limits.render_cap.rows(),
            max_auth_attempts: limits.max_auth_attempts,
            auth_rejection_millis: u64::try_from(limits.auth_rejection_time.as_millis())
                .unwrap_or(u64::MAX),
            allow_password_login: false,
            redraw_interval_millis: u64::try_from(limits.redraw_interval.as_millis())
                .unwrap_or(u64::MAX),
            requests_per_second: limits.requests_per_second,
            input_bytes_per_second: limits.input_bytes_per_second,
            flood_strikes: limits.flood_strikes,
            write_stall_secs: limits.write_stall.as_secs(),
        }
    }
}

/// Which database backend to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatasourceKind {
    /// A SQLite file (the default, and the only backend built so far).
    Sqlite,
    /// PostgreSQL (M1-G).
    Postgres,
}

/// TLS for the Postgres connection (D31). There is deliberately no "disable".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TlsMode {
    /// Encrypt, without checking the server certificate.
    Require,
    /// Encrypt and check the certificate chain.
    VerifyCa,
    /// Encrypt, check the chain and the host name.
    VerifyFull,
}

/// `[datasource]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DatasourceConfig {
    /// `sqlite` or `postgres`.
    pub kind: DatasourceKind,
    /// SQLite file; `None` means the default data directory. Only for `sqlite`.
    pub path: Option<PathBuf>,
    /// Postgres URL, redacted when printed. Only for `postgres`.
    pub url: Option<SecretUrl>,
    /// Postgres TLS mode. Only for `postgres`.
    pub tls_mode: TlsMode,
    /// PEM file of extra root certificates. Only for `postgres`.
    pub tls_root_cert: Option<PathBuf>,
    /// Use IAM token authentication instead of a password. Only for `postgres`.
    pub iam_auth: bool,
}

impl Default for DatasourceConfig {
    fn default() -> Self {
        Self {
            kind: DatasourceKind::Sqlite,
            path: None,
            url: None,
            tls_mode: TlsMode::Require,
            tls_root_cert: None,
            iam_auth: false,
        }
    }
}

/// Which blob backend to use (D30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlobBackend {
    /// A local directory.
    Local,
    /// An S3-compatible bucket. Credentials come from the environment, never from this file.
    S3,
}

/// `[blobs]`. Parsed only: no blob store exists yet.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BlobsConfig {
    /// `local` or `s3`.
    pub backend: BlobBackend,
    /// Directory for `local`; `None` means the default data directory.
    pub dir: Option<PathBuf>,
    /// Endpoint URL for `s3` (leave unset for AWS itself).
    pub endpoint: Option<String>,
    /// Bucket for `s3`.
    pub bucket: Option<String>,
    /// Region for `s3`.
    pub region: Option<String>,
}

impl Default for BlobsConfig {
    fn default() -> Self {
        Self {
            backend: BlobBackend::Local,
            dir: None,
            endpoint: None,
            bucket: None,
            region: None,
        }
    }
}

/// `[telnet]` (D27, M3-H): the optional unencrypted transport, off by default.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TelnetConfig {
    /// Turn the telnet transport on, which puts the board in low-security mode (D27).
    pub enabled: bool,
    /// Second opt-in: allow member login over telnet.
    pub allow_login: bool,
    /// Address and port for telnet, separate from SSH.
    pub listen: SocketAddr,
}

impl Default for TelnetConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allow_login: false,
            listen: SocketAddr::from(([0, 0, 0, 0], 2323)),
        }
    }
}

/// `[feeds]` (M3-G, D89): reading feeds into read-only boards, and serving public boards as RSS.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FeedsConfig {
    /// Read the feeds the sysop added (an outbound fetch job). Off by default: with it off the
    /// server makes no outbound request for feeds.
    pub enabled: bool,
    /// Serve public boards as RSS over a small HTTP listener. Off by default; when off no port
    /// is bound.
    pub rss_out_enabled: bool,
    /// Address and port of the RSS listener (plain HTTP: put a TLS proxy in front, or keep it
    /// on a private address).
    pub rss_out_listen: SocketAddr,
    /// The public address of the RSS listener as readers reach it, with no trailing slash;
    /// used in the documents' links. Required when `rss_out_enabled`.
    pub rss_out_base_url: Option<String>,
}

impl Default for FeedsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rss_out_enabled: false,
            rss_out_listen: SocketAddr::from(([127, 0, 0, 1], 8081)),
            rss_out_base_url: None,
        }
    }
}

/// `[waiting_room]`: the rate limits of D37.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WaitingRoomConfig {
    /// Connections allowed back to back per network.
    pub ip_burst: u32,
    /// Seconds to earn back one connection.
    pub ip_interval_secs: u64,
    /// Authentication attempts allowed back to back per key.
    pub key_burst: u32,
    /// Seconds to earn back one attempt.
    pub key_interval_secs: u64,
    /// Form submissions per session.
    pub max_form_submissions: u32,
    /// Sources each limiter remembers.
    pub max_tracked_sources: usize,
}

impl Default for WaitingRoomConfig {
    fn default() -> Self {
        let limits = Limits::default();
        Self {
            ip_burst: limits.ip_rate.burst,
            ip_interval_secs: limits.ip_rate.interval.as_secs(),
            key_burst: limits.key_rate.burst,
            key_interval_secs: limits.key_rate.interval.as_secs(),
            max_form_submissions: limits.max_form_submissions,
            max_tracked_sources: limits.max_tracked_sources,
        }
    }
}

impl Config {
    /// The SSH front end's limits, built from the validated settings.
    #[must_use]
    pub fn limits(&self) -> Limits {
        Limits {
            auth_timeout: Duration::from_secs(self.ssh.auth_timeout_secs),
            max_auth_attempts: self.ssh.max_auth_attempts,
            auth_rejection_time: Duration::from_millis(self.ssh.auth_rejection_millis),
            idle_timeout: Duration::from_secs(self.ssh.idle_timeout_secs),
            keepalive_interval: Duration::from_secs(self.ssh.keepalive_interval_secs),
            max_connections: self.ssh.max_connections,
            render_cap: RenderCap::new(self.ssh.render_cols, self.ssh.render_rows),
            ip_rate: RatePolicy::new(
                self.waiting_room.ip_burst,
                Duration::from_secs(self.waiting_room.ip_interval_secs),
            ),
            key_rate: RatePolicy::new(
                self.waiting_room.key_burst,
                Duration::from_secs(self.waiting_room.key_interval_secs),
            ),
            max_tracked_sources: self.waiting_room.max_tracked_sources,
            max_form_submissions: self.waiting_room.max_form_submissions,
            redraw_interval: Duration::from_millis(self.ssh.redraw_interval_millis),
            requests_per_second: self.ssh.requests_per_second,
            input_bytes_per_second: self.ssh.input_bytes_per_second,
            flood_strikes: self.ssh.flood_strikes,
            write_stall: Duration::from_secs(self.ssh.write_stall_secs),
            allow_password_login: self.ssh.allow_password_login,
            low_security: self.telnet.enabled,
            ..Limits::default()
        }
    }
}

fn de_registration_mode<'de, D: Deserializer<'de>>(d: D) -> Result<RegistrationMode, D::Error> {
    let text = String::deserialize(d)?;
    RegistrationMode::parse(&text)
        .ok_or_else(|| de::Error::custom("must be one of \"invite\", \"apply\", \"open\""))
}
