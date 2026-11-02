//! Range and consistency checks on the merged settings. Every failure names the key and where
//! its value came from, and none echoes a secret.

use std::net::SocketAddr;
use std::path::Path;

use super::error::{ConfigError, Source};
use super::layers::Sources;
use super::model::{BlobBackend, Config, DatasourceKind, SshConfig, TlsMode};

struct Check<'a> {
    sources: &'a Sources,
}

impl Check<'_> {
    fn fail(&self, key: &str, message: impl Into<String>) -> ConfigError {
        ConfigError::invalid(key, &self.sources.get(key), message)
    }

    fn range(&self, key: &str, value: u64, min: u64, max: u64) -> Result<(), ConfigError> {
        if (min..=max).contains(&value) {
            Ok(())
        } else {
            Err(self.fail(key, format!("must be between {min} and {max}, got {value}")))
        }
    }

    fn port(&self, key: &str, addr: SocketAddr) -> Result<(), ConfigError> {
        if addr.port() == 0 {
            Err(self.fail(key, "the port must not be 0"))
        } else {
            Ok(())
        }
    }

    /// A path that names a file (or a directory when `dir`): not empty, no NUL, and not already
    /// something of the other kind. Nothing is created.
    fn path(&self, key: &str, path: &Path, dir: bool) -> Result<(), ConfigError> {
        if path.as_os_str().is_empty() {
            return Err(self.fail(key, "the path is empty"));
        }
        if path.as_os_str().as_encoded_bytes().contains(&0) {
            return Err(self.fail(key, "the path contains a NUL byte"));
        }
        match std::fs::metadata(path) {
            Ok(meta) if dir && !meta.is_dir() => {
                Err(self.fail(key, "the path exists but is not a directory"))
            }
            Ok(meta) if !dir && meta.is_dir() => {
                Err(self.fail(key, "the path is a directory; name a file"))
            }
            _ => Ok(()),
        }
    }

    /// `key` is only meaningful for another setting's value; reject it when set explicitly.
    fn only_with(&self, key: &str, requirement: &str) -> Result<(), ConfigError> {
        if self.sources.is_set(key) {
            Err(self.fail(key, format!("is only used when {requirement}")))
        } else {
            Ok(())
        }
    }
}

/// The per-session limits of D46.
fn session_limits(c: &Check<'_>, ssh: &SshConfig) -> Result<(), ConfigError> {
    c.range(
        "ssh.redraw_interval_millis",
        ssh.redraw_interval_millis,
        10,
        5_000,
    )?;
    c.range(
        "ssh.requests_per_second",
        u64::from(ssh.requests_per_second),
        10,
        100_000,
    )?;
    c.range(
        "ssh.input_bytes_per_second",
        u64::from(ssh.input_bytes_per_second),
        64,
        10_000_000,
    )?;
    c.range(
        "ssh.flood_strikes",
        u64::from(ssh.flood_strikes),
        1,
        1_000_000,
    )?;
    c.range("ssh.write_stall_secs", ssh.write_stall_secs, 1, 3_600)?;
    Ok(())
}

/// Validate a merged configuration.
pub(crate) fn validate(config: &Config, sources: &Sources) -> Result<(), ConfigError> {
    let c = Check { sources };
    let ssh = &config.ssh;
    c.port("ssh.listen", ssh.listen)?;
    c.range("ssh.auth_timeout_secs", ssh.auth_timeout_secs, 1, 3_600)?;
    c.range("ssh.idle_timeout_secs", ssh.idle_timeout_secs, 1, 86_400)?;
    c.range(
        "ssh.keepalive_interval_secs",
        ssh.keepalive_interval_secs,
        1,
        3_600,
    )?;
    let max_connections = u64::try_from(ssh.max_connections).unwrap_or(u64::MAX);
    c.range("ssh.max_connections", max_connections, 1, 1_000_000)?;
    c.range(
        "ssh.max_auth_attempts",
        u64::from(ssh.max_auth_attempts),
        1,
        100,
    )?;
    c.range(
        "ssh.auth_rejection_millis",
        ssh.auth_rejection_millis,
        0,
        60_000,
    )?;
    session_limits(&c, ssh)?;
    if let Some(path) = &ssh.host_key {
        c.path("ssh.host_key", path, false)?;
    }

    let wr = &config.waiting_room;
    c.range(
        "waiting_room.ip_burst",
        u64::from(wr.ip_burst),
        1,
        1_000_000,
    )?;
    c.range(
        "waiting_room.ip_interval_secs",
        wr.ip_interval_secs,
        1,
        86_400,
    )?;
    c.range(
        "waiting_room.key_burst",
        u64::from(wr.key_burst),
        1,
        1_000_000,
    )?;
    c.range(
        "waiting_room.key_interval_secs",
        wr.key_interval_secs,
        1,
        86_400,
    )?;
    c.range(
        "waiting_room.max_form_submissions",
        u64::from(wr.max_form_submissions),
        1,
        1_000,
    )?;
    let tracked = u64::try_from(wr.max_tracked_sources).unwrap_or(u64::MAX);
    c.range("waiting_room.max_tracked_sources", tracked, 1, 1_000_000)?;

    let telnet = &config.telnet;
    c.port("telnet.listen", telnet.listen)?;
    if telnet.allow_login && !telnet.enabled {
        return Err(c.fail(
            "telnet.allow_login",
            "needs telnet.enabled = true; login over telnet is a second opt-in (D27)",
        ));
    }
    if telnet.enabled && clashes(ssh.listen, telnet.listen) {
        return Err(c.fail(
            "telnet.listen",
            "is the same port as ssh.listen; telnet needs its own",
        ));
    }

    datasource(&c, config)?;
    blobs(&c, config)
}

/// Whether two listen addresses would fight for the same port.
fn clashes(a: SocketAddr, b: SocketAddr) -> bool {
    a.port() == b.port() && (a.ip() == b.ip() || a.ip().is_unspecified() || b.ip().is_unspecified())
}

fn datasource(c: &Check<'_>, config: &Config) -> Result<(), ConfigError> {
    let ds = &config.datasource;
    match ds.kind {
        DatasourceKind::Sqlite => {
            let need = "datasource.kind = \"postgres\"";
            c.only_with("datasource.url", need)?;
            c.only_with("datasource.tls_root_cert", need)?;
            if ds.tls_mode != TlsMode::Require {
                return Err(c.fail("datasource.tls_mode", format!("is only used when {need}")));
            }
            if ds.iam_auth {
                return Err(c.fail("datasource.iam_auth", format!("is only used when {need}")));
            }
            if let Some(path) = &ds.path {
                c.path("datasource.path", path, false)?;
            }
        }
        DatasourceKind::Postgres => {
            c.only_with("datasource.path", "datasource.kind = \"sqlite\"")?;
            let Some(url) = &ds.url else {
                return Err(c.fail(
                    "datasource.url",
                    "is required when datasource.kind = \"postgres\"",
                ));
            };
            let text = url.expose();
            let rest = text
                .strip_prefix("postgres://")
                .or_else(|| text.strip_prefix("postgresql://"));
            // The message never includes the URL: it may hold a password.
            let Some(rest) = rest else {
                return Err(c.fail(
                    "datasource.url",
                    "must start with postgres:// or postgresql://",
                ));
            };
            let after_userinfo = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
            if after_userinfo.is_empty() || after_userinfo.starts_with(['/', '?', '#']) {
                return Err(c.fail("datasource.url", "has no host"));
            }
            if let Some(path) = &ds.tls_root_cert {
                c.path("datasource.tls_root_cert", path, false)?;
                if !path.is_file() {
                    return Err(c.fail("datasource.tls_root_cert", "the file does not exist"));
                }
            }
        }
    }
    Ok(())
}

fn blobs(c: &Check<'_>, config: &Config) -> Result<(), ConfigError> {
    let b = &config.blobs;
    match b.backend {
        BlobBackend::Local => {
            let need = "blobs.backend = \"s3\"";
            c.only_with("blobs.endpoint", need)?;
            c.only_with("blobs.bucket", need)?;
            c.only_with("blobs.region", need)?;
            if let Some(dir) = &b.dir {
                c.path("blobs.dir", dir, true)?;
            }
        }
        BlobBackend::S3 => {
            c.only_with("blobs.dir", "blobs.backend = \"local\"")?;
            let Some(bucket) = &b.bucket else {
                return Err(c.fail("blobs.bucket", "is required when blobs.backend = \"s3\""));
            };
            if bucket.is_empty()
                || bucket
                    .chars()
                    .any(|ch| ch.is_whitespace() || ch.is_control())
            {
                return Err(c.fail("blobs.bucket", "must be a bucket name without spaces"));
            }
            if let Some(endpoint) = &b.endpoint {
                if !(endpoint.starts_with("https://") || endpoint.starts_with("http://")) {
                    return Err(c.fail("blobs.endpoint", "must start with https:// or http://"));
                }
                // Credentials in the URL would be persisted in the file; they come from the
                // environment (D30).
                let host_part = endpoint.split("://").nth(1).unwrap_or_default();
                if host_part.split('/').next().is_some_and(|a| a.contains('@')) {
                    return Err(c.fail("blobs.endpoint", "must not contain credentials"));
                }
            }
            if b.region.as_deref().is_some_and(str::is_empty) {
                return Err(c.fail("blobs.region", "must not be empty"));
            }
        }
    }
    Ok(())
}

/// Where `datasource.iam_auth` came from, for the "not built yet" error.
pub(crate) fn iam_auth_origin(sources: &Sources) -> Source {
    sources.get("datasource.iam_auth")
}
