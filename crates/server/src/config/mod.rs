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

pub use cli::Cli;
pub use error::{ConfigError, Source};
pub use layers::{Assembled, CliValue, FileInput, Sources, assemble};
pub use model::{
    BlobBackend, BlobsConfig, Config, DatasourceConfig, DatasourceKind, SshConfig, TelnetConfig,
    TlsMode, WaitingRoomConfig,
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

/// File locations after defaults are applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPaths {
    /// The SSH host key.
    pub host_key: PathBuf,
    /// The SQLite file.
    pub database: PathBuf,
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

    /// Selecting a backend that is not built is a boot error, never a silent fallback.
    /// M1-G removes the `Postgres` arm.
    fn ensure_backend_built(&self) -> Result<(), ConfigError> {
        match self.config.datasource.kind {
            DatasourceKind::Sqlite => Ok(()),
            DatasourceKind::Postgres => Err(ConfigError::PostgresNotBuilt {
                origin: validate::kind_origin(&self.sources),
            }),
        }
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
        let database = match &self.config.datasource.path {
            Some(p) => p.clone(),
            None => self
                .dirs
                .data_path(paths::DB_FILE)
                .ok_or(ConfigError::NoDataDir {
                    key: "datasource.path",
                })?,
        };
        Ok(ResolvedPaths { host_key, database })
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

/// Settings that are turned on but whose feature is not built, so the sysop is not misled.
fn unused_warnings(config: &Config) -> Vec<String> {
    let mut out = Vec::new();
    if config.ssh.allow_password_login {
        out.push(
            "ssh.allow_password_login is on, but password login is not built yet; it has no \
             effect and SSH accepts keys only (D28)"
                .to_owned(),
        );
    }
    if config.telnet.enabled {
        out.push(
            "telnet.enabled is on, but the telnet transport is not built yet; nothing \
             listens on telnet (D27)"
                .to_owned(),
        );
    }
    if config.blobs.backend == BlobBackend::S3 {
        out.push(
            "blobs.backend = \"s3\" is parsed but no blob store exists yet; it has no effect (D30)"
                .to_owned(),
        );
    }
    out
}
