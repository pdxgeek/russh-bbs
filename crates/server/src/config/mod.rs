//! Configuration: one TOML file, `RUSSH_BBS_*` environment overrides and command-line flags,
//! parsed into the typed [`Config`] and validated at boot. See `docs/config.md` for every key.
//!
//! Precedence, lowest to highest: **built-in defaults < config file < environment < command
//! line**. Unknown keys, bad values and out-of-range numbers are errors that name the key and
//! the source. A missing *default* config file means "all defaults"; a missing file that was
//! asked for with `--config` or `RUSSH_BBS_CONFIG` is an error. A datasource URL is a
//! [`SecretUrl`] and is redacted wherever it is printed.

mod cli;
mod error;
mod keys;
mod layers;
mod model;
mod paths;
mod secret;
#[cfg(test)]
mod tests;
mod validate;

use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use russh_bbs_core::store::RegistrationMode;
use russh_bbs_store::{PostgresSettings, TlsMode as StoreTlsMode};

pub use cli::Cli;
pub use error::{ConfigError, Source};
pub use layers::{Assembled, CliValue, FileInput, Sources, assemble};
pub use model::{
    BlobBackend, BlobsConfig, Config, DatasourceConfig, DatasourceKind, MailConfig, SshConfig,
    TelnetConfig, TlsMode, WaitingRoomConfig,
};
pub use paths::{CONTAINER_CONFIG, Dirs};
pub use secret::SecretUrl;

/// Every environment variable this crate reads starts with this.
pub(crate) const ENV_PREFIX: &str = "RUSSH_BBS_";
/// The environment variable that names the config file.
pub const CONFIG_ENV: &str = "RUSSH_BBS_CONFIG";

/// A configuration ready to boot with.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// The settings.
    pub config: Config,
    /// Where each came from.
    pub sources: Sources,
    /// Non-fatal things the sysop should know (settings that do nothing yet, loose file modes).
    pub warnings: Vec<String>,
    /// The config file that was used, if any.
    pub file: Option<PathBuf>,
    dirs: Dirs,
}

/// Where the board's data lives, after defaults are applied.
#[derive(Debug, Clone)]
pub enum ResolvedDatasource {
    /// A SQLite file.
    Sqlite(PathBuf),
    /// A Postgres server, always over TLS (D31). `Debug` hides the URL.
    Postgres(PostgresSettings),
}

/// File locations and the datasource after defaults are applied.
#[derive(Debug, Clone)]
pub struct ResolvedPaths {
    /// The SSH host key.
    pub host_key: PathBuf,
    /// The mail key file (M3-I, D90).
    pub mail_key: PathBuf,
    /// The database to open.
    pub datasource: ResolvedDatasource,
    /// The blob directory, for `blobs.backend = "local"`; `None` for `s3` (M4-F1).
    pub blobs_dir: Option<PathBuf>,
}

impl Loaded {
    /// Load for a real start: process environment, real directories, backend must be built.
    pub fn from_process(cli: &Cli) -> Result<Self, ConfigError> {
        let env = std::env::vars_os()
            .filter_map(|(k, v)| Some((k.into_string().ok()?, v)))
            .collect::<Vec<_>>();
        Self::load(cli.config.clone(), &env, &cli.overrides(), Dirs::from_env())
    }

    /// Find and read the file, merge the layers, validate, and check the backend is built.
    pub fn load(
        config_flag: Option<PathBuf>,
        env: &[(String, OsString)],
        cli: &[CliValue],
        dirs: Dirs,
    ) -> Result<Self, ConfigError> {
        let named = config_flag.or_else(|| {
            env.iter()
                .find(|(k, _)| k == CONFIG_ENV)
                .map(|(_, v)| PathBuf::from(v))
                .filter(|p| !p.as_os_str().is_empty())
        });
        let file = read_config_file(named, &dirs)?;
        let assembled = assemble(
            file.as_ref().map(|f| FileInput {
                path: &f.path,
                text: &f.text,
                loose_permissions: f.loose_permissions,
            }),
            env,
            cli,
        )?;
        let loaded = Self {
            config: assembled.config,
            sources: assembled.sources,
            warnings: assembled.warnings,
            file: file.map(|f| f.path),
            dirs,
        };
        loaded.ensure_backend_built()?;
        Ok(loaded)
    }

    /// Selecting something that is not built is a boot error, never a silent fallback. Both
    /// backends are built (M1-G); what is parsed but not built is IAM token authentication.
    fn ensure_backend_built(&self) -> Result<(), ConfigError> {
        let ds = &self.config.datasource;
        if ds.kind == DatasourceKind::Postgres && ds.iam_auth {
            return Err(ConfigError::IamAuthNotBuilt {
                origin: validate::iam_auth_origin(&self.sources),
            });
        }
        Ok(())
    }

    /// The host key and database paths, with defaults filled in.
    pub fn paths(&self) -> Result<ResolvedPaths, ConfigError> {
        let host_key = match &self.config.ssh.host_key {
            Some(p) => p.clone(),
            None => self
                .dirs
                .data_path(paths::HOST_KEY_FILE)
                .ok_or(ConfigError::NoDataDir {
                    key: "ssh.host_key",
                })?,
        };
        let mail_key = match &self.config.mail.key_file {
            Some(p) => p.clone(),
            None => self
                .dirs
                .data_path(paths::MAIL_KEY_FILE)
                .ok_or(ConfigError::NoDataDir {
                    key: "mail.key_file",
                })?,
        };
        let ds = &self.config.datasource;
        let datasource = match ds.kind {
            DatasourceKind::Sqlite => ResolvedDatasource::Sqlite(match &ds.path {
                Some(p) => p.clone(),
                None => self
                    .dirs
                    .data_path(paths::DB_FILE)
                    .ok_or(ConfigError::NoDataDir {
                        key: "datasource.path",
                    })?,
            }),
            DatasourceKind::Postgres => {
                // Validation guarantees the URL is present for postgres.
                let url = ds.url.as_ref().ok_or_else(|| ConfigError::Invalid {
                    key: "datasource.url".into(),
                    origin: self.sources.get("datasource.url"),
                    message: "is required when datasource.kind = \"postgres\"".into(),
                })?;
                ResolvedDatasource::Postgres(PostgresSettings::new(
                    url.expose(),
                    match ds.tls_mode {
                        TlsMode::Require => StoreTlsMode::Require,
                        TlsMode::VerifyCa => StoreTlsMode::VerifyCa,
                        TlsMode::VerifyFull => StoreTlsMode::VerifyFull,
                    },
                    ds.tls_root_cert.clone(),
                ))
            }
        };
        let blobs_dir = match (self.config.blobs.backend, &self.config.blobs.dir) {
            (BlobBackend::S3, _) => None,
            (BlobBackend::Local, Some(p)) => Some(p.clone()),
            (BlobBackend::Local, None) => Some(
                self.dirs
                    .data_path(paths::BLOBS_DIR)
                    .ok_or(ConfigError::NoDataDir { key: "blobs.dir" })?,
            ),
        };
        Ok(ResolvedPaths {
            host_key,
            mail_key,
            datasource,
            blobs_dir,
        })
    }

    /// The registration mode to write to the store at boot, if the sysop set one in the file,
    /// environment or flags. `None` leaves the stored value alone (decision D38).
    #[must_use]
    pub fn registration_mode_override(&self) -> Option<RegistrationMode> {
        self.sources
            .is_set("registration_mode")
            .then_some(self.config.registration_mode)
    }
}

/// A config file that was found and read.
struct ConfigFile {
    path: PathBuf,
    text: String,
    loose_permissions: bool,
}

/// An explicit path must exist; otherwise the first default location that exists is used, and
/// none existing means "all defaults".
fn read_config_file(
    named: Option<PathBuf>,
    dirs: &Dirs,
) -> Result<Option<ConfigFile>, ConfigError> {
    if let Some(path) = named {
        return match read_one(&path) {
            Ok(file) => Ok(Some(file)),
            Err(e) if e.kind() == ErrorKind::NotFound => Err(ConfigError::MissingFile { path }),
            Err(cause) => Err(ConfigError::Read { path, cause }),
        };
    }
    for path in dirs.default_config_paths() {
        match read_one(&path) {
            Ok(file) => return Ok(Some(file)),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(cause) => return Err(ConfigError::Read { path, cause }),
        }
    }
    Ok(None)
}

fn read_one(path: &Path) -> std::io::Result<ConfigFile> {
    let text = std::fs::read_to_string(path)?;
    Ok(ConfigFile {
        loose_permissions: is_loose(path),
        path: path.to_path_buf(),
        text,
    })
}

/// Whether the file is readable by group or others (Unix only).
fn is_loose(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o044 != 0)
}

/// Settings that lower the board's security, so the sysop is not misled.
fn unused_warnings(config: &Config) -> Vec<String> {
    let mut out = Vec::new();
    if config.telnet.enabled {
        out.push(
            "telnet.enabled is on: the board is in LOW-SECURITY MODE, and telnet sessions are \
             not encrypted (D27)"
                .to_owned(),
        );
    }
    if config.blobs.backend == BlobBackend::S3
        && config
            .blobs
            .endpoint
            .as_deref()
            .is_some_and(|e| e.starts_with("http://"))
    {
        out.push(
            "blobs.endpoint is plain http: uploads and the access keys cross the network \
             unencrypted; use https unless the store is on this host (D30)"
                .to_owned(),
        );
    }
    out
}
