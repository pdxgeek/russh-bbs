//! Default file locations, derived from the environment (injectable for tests).

use std::path::PathBuf;

/// The environment variables default paths depend on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dirs {
    /// `XDG_DATA_HOME`, used only when absolute.
    pub xdg_data_home: Option<PathBuf>,
    /// `XDG_CONFIG_HOME`, used only when absolute.
    pub xdg_config_home: Option<PathBuf>,
    /// `HOME`.
    pub home: Option<PathBuf>,
}

/// Where the container image keeps its config (decision D25).
pub const CONTAINER_CONFIG: &str = "/data/config.toml";

/// Name of the host key file inside the default data directory.
pub(crate) const HOST_KEY_FILE: &str = "host_ed25519_key";
/// Name of the database file inside the default data directory.
pub(crate) const DB_FILE: &str = "russh-bbs.db";

impl Dirs {
    /// Read the real environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            xdg_data_home: std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            home: std::env::var_os("HOME").map(PathBuf::from),
        }
    }

    /// `name` inside the default data directory: `$XDG_DATA_HOME/russh-bbs/`, else
    /// `$HOME/.local/share/russh-bbs/`. `None` if neither can be worked out.
    #[must_use]
    pub fn data_path(&self, name: &str) -> Option<PathBuf> {
        let base = self
            .xdg_data_home
            .clone()
            .filter(|p| p.is_absolute())
            .or_else(|| self.home.as_ref().map(|h| h.join(".local").join("share")))?;
        Some(base.join("russh-bbs").join(name))
    }

    /// Where to look for a config file when none was named, in order: the XDG config path
    /// (`$XDG_CONFIG_HOME/russh-bbs/config.toml`, else `$HOME/.config/russh-bbs/config.toml`),
    /// then `/data/config.toml` for the container.
    #[must_use]
    pub fn default_config_paths(&self) -> Vec<PathBuf> {
        let base = self
            .xdg_config_home
            .clone()
            .filter(|p| p.is_absolute())
            .or_else(|| self.home.as_ref().map(|h| h.join(".config")));
        let mut paths: Vec<PathBuf> = base
            .into_iter()
            .map(|b| b.join("russh-bbs").join("config.toml"))
            .collect();
        paths.push(PathBuf::from(CONTAINER_CONFIG));
        paths
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::path::PathBuf;

    use super::{DB_FILE, Dirs, HOST_KEY_FILE};

    fn dirs(data: Option<&str>, home: Option<&str>) -> Dirs {
        Dirs {
            xdg_data_home: data.map(PathBuf::from),
            xdg_config_home: None,
            home: home.map(PathBuf::from),
        }
    }

    #[test]
    fn xdg_data_home_wins_when_absolute() {
        let path = dirs(Some("/xdg"), Some("/home/u")).data_path(HOST_KEY_FILE);
        assert_eq!(path, Some(PathBuf::from("/xdg/russh-bbs/host_ed25519_key")));
    }

    #[test]
    fn falls_back_to_home_local_share() {
        let path = dirs(None, Some("/home/u")).data_path(HOST_KEY_FILE);
        assert_eq!(
            path,
            Some(PathBuf::from(
                "/home/u/.local/share/russh-bbs/host_ed25519_key"
            ))
        );
    }

    #[test]
    fn relative_xdg_is_ignored() {
        let path = dirs(Some("rel"), Some("/home/u")).data_path(HOST_KEY_FILE);
        assert_eq!(
            path,
            Some(PathBuf::from(
                "/home/u/.local/share/russh-bbs/host_ed25519_key"
            ))
        );
    }

    #[test]
    fn the_database_lives_beside_the_host_key() {
        let path = dirs(Some("/xdg"), None).data_path(DB_FILE);
        assert_eq!(path, Some(PathBuf::from("/xdg/russh-bbs/russh-bbs.db")));
    }

    #[test]
    fn no_env_means_no_default() {
        assert_eq!(dirs(None, None).data_path(HOST_KEY_FILE), None);
    }

    #[test]
    fn config_search_order_is_xdg_then_container() {
        let d = Dirs {
            xdg_config_home: Some("/xc".into()),
            home: Some("/home/u".into()),
            ..Dirs::default()
        };
        assert_eq!(
            d.default_config_paths(),
            vec![
                PathBuf::from("/xc/russh-bbs/config.toml"),
                PathBuf::from("/data/config.toml")
            ]
        );
        let d = Dirs {
            home: Some("/home/u".into()),
            ..Dirs::default()
        };
        assert_eq!(
            d.default_config_paths()[0],
            PathBuf::from("/home/u/.config/russh-bbs/config.toml")
        );
    }
}
