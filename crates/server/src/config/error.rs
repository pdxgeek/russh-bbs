//! Config errors. Every error names the key and where its value came from, and none of them
//! contains a secret: a datasource URL is never echoed (see [`super::SecretUrl`]).

use std::fmt;
use std::path::PathBuf;

/// Where a setting's value came from. Later sources override earlier ones:
/// default < file < environment < command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The built-in default.
    Default,
    /// The TOML config file at this path.
    File(PathBuf),
    /// This environment variable.
    Env(&'static str),
    /// This command-line flag (written with its leading dashes).
    Flag(&'static str),
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => f.write_str("the built-in default"),
            Self::File(path) => write!(f, "config file {}", path.display()),
            Self::Env(name) => write!(f, "environment variable {name}"),
            Self::Flag(flag) => write!(f, "command-line flag {flag}"),
        }
    }
}

/// Why the configuration could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A config file was named with `--config` or `RUSSH_BBS_CONFIG` and does not exist.
    #[error("config file {} does not exist (it was named explicitly)", .path.display())]
    MissingFile {
        /// The path that was asked for.
        path: PathBuf,
    },
    /// The file exists but could not be read.
    #[error("cannot read config file {}: {}", .path.display(), .cause)]
    Read {
        /// The file.
        path: PathBuf,
        /// The I/O error.
        cause: std::io::Error,
    },
    /// The file is not valid TOML. Only the message and line are shown, never the source text,
    /// because a line may hold a secret.
    #[error("config file {} is not valid TOML{}: {}", .path.display(), line_note(*.line), .message)]
    Syntax {
        /// The file.
        path: PathBuf,
        /// One-based line of the problem, when known.
        line: Option<usize>,
        /// The parser's message.
        message: String,
    },
    /// A key that no setting has, in the file.
    #[error("unknown key `{key}` in {origin}")]
    UnknownKey {
        /// The dotted key as written.
        key: String,
        /// Where it was found.
        origin: Source,
    },
    /// An environment variable with the `RUSSH_BBS_` prefix that no setting reads.
    #[error("unknown environment variable {name}: no setting reads it")]
    UnknownEnv {
        /// The variable name.
        name: String,
    },
    /// A value that is the wrong type, out of range, or inconsistent with another setting.
    #[error("invalid value for `{key}` from {origin}: {message}")]
    Invalid {
        /// The dotted key.
        key: String,
        /// Where the value came from.
        origin: Source,
        /// What is wrong and what is allowed. Never contains the value of a secret key.
        message: String,
    },
    /// `datasource.iam_auth` is on, but IAM token authentication is not built (D31: parsed, not
    /// built). Refused so a sysop never believes tokens are in use while a password is.
    #[error(
        "datasource.iam_auth = true (from {origin}) is set, but IAM token authentication is not \
         built yet; it is parsed and refused rather than ignored. Put the database password in \
         the datasource.url, or turn iam_auth off. The server will not fall back to a different \
         authentication on its own."
    )]
    IamAuthNotBuilt {
        /// Where `iam_auth = true` came from.
        origin: Source,
    },
    /// No data directory can be worked out and none was configured.
    #[error(
        "cannot choose a default location for `{key}`: neither XDG_DATA_HOME nor HOME is set; \
         set `{key}` explicitly"
    )]
    NoDataDir {
        /// The key that needs a path.
        key: &'static str,
    },
}

fn line_note(line: Option<usize>) -> String {
    line.map(|l| format!(" at line {l}")).unwrap_or_default()
}

impl ConfigError {
    pub(crate) fn invalid(key: &str, origin: &Source, message: impl Into<String>) -> Self {
        Self::Invalid {
            key: key.to_owned(),
            origin: origin.clone(),
            message: message.into(),
        }
    }
}
