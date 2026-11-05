//! The registry of every setting: its dotted key, environment variable, command-line flag and
//! kind. It drives environment parsing, unknown-key detection and the docs test. A test applies
//! every entry to [`super::Config`], so a key missing from the struct (or the registry) fails CI.

/// How an environment value is turned into a TOML value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Text, taken as is (paths, addresses, enum names, URLs).
    Str,
    /// A whole number.
    Int,
    /// `true` or `false`.
    Bool,
}

/// One setting.
#[derive(Debug)]
pub(crate) struct KeyDef {
    /// Dotted key: `ssh.listen`, or a bare name for top-level keys.
    pub(crate) path: &'static str,
    /// Environment variable.
    pub(crate) env: &'static str,
    /// Command-line flag, if there is one. Secrets never have one (argv is visible in `ps`).
    pub(crate) flag: Option<&'static str>,
    /// Value type.
    pub(crate) kind: Kind,
    /// The value is a secret: errors never echo it.
    pub(crate) secret: bool,
    /// A valid example value, used by tests.
    #[allow(dead_code)] // read only by the test that applies every key
    pub(crate) sample: &'static str,
}

const fn key(
    path: &'static str,
    env: &'static str,
    flag: Option<&'static str>,
    kind: Kind,
    sample: &'static str,
) -> KeyDef {
    KeyDef {
        path,
        env,
        flag,
        kind,
        secret: false,
        sample,
    }
}

use Kind::{Bool, Int, Str};

/// Every setting. Environment names that predate this module (`RUSSH_BBS_LISTEN`, `_HOST_KEY`,
/// `_DB`, the timeout, limit and rate names) are kept; new ones are `RUSSH_BBS_<SECTION>_<KEY>`.
pub(crate) static KEYS: &[KeyDef] = &[
    key(
        "registration_mode",
        "RUSSH_BBS_REGISTRATION_MODE",
        None,
        Str,
        "apply",
    ),
    key(
        "ssh.listen",
        "RUSSH_BBS_LISTEN",
        Some("--listen"),
        Str,
        "127.0.0.1:2200",
    ),
    key(
        "ssh.host_key",
        "RUSSH_BBS_HOST_KEY",
        Some("--host-key"),
        Str,
        "/tmp/hk",
    ),
    key(
        "board.name",
        "RUSSH_BBS_BOARD_NAME",
        None,
        Str,
        "Night Owl BBS",
    ),
    key(
        "board.tagline",
        "RUSSH_BBS_BOARD_TAGLINE",
        None,
        Str,
        "After hours",
    ),
    key(
        "board.location",
        "RUSSH_BBS_BOARD_LOCATION",
        None,
        Str,
        "Portland",
    ),
    key("board.since", "RUSSH_BBS_BOARD_SINCE", None, Str, "2026"),
    key(
        "board.accent",
        "RUSSH_BBS_BOARD_ACCENT",
        None,
        Str,
        "#ff00aa",
    ),
    key("board.style", "RUSSH_BBS_BOARD_STYLE", None, Str, "neon"),
    key(
        "board.layout",
        "RUSSH_BBS_BOARD_LAYOUT",
        None,
        Str,
        "dashboard",
    ),
    key(
        "board.styles",
        "RUSSH_BBS_BOARD_STYLES",
        None,
        Str,
        "neon,paper",
    ),
    key(
        "board.layouts",
        "RUSSH_BBS_BOARD_LAYOUTS",
        None,
        Str,
        "classic,wide",
    ),
    key(
        "board.motion",
        "RUSSH_BBS_BOARD_MOTION",
        None,
        Bool,
        "false",
    ),
    key(
        "widgets.hidden",
        "RUSSH_BBS_WIDGETS_HIDDEN",
        None,
        Str,
        "status",
    ),
    key("widgets.on", "RUSSH_BBS_WIDGETS_ON", None, Str, "motd"),
    key("widgets.off", "RUSSH_BBS_WIDGETS_OFF", None, Str, "wall"),
    key(
        "widgets.dashboard",
        "RUSSH_BBS_WIDGETS_DASHBOARD",
        None,
        Str,
        "clock,mail",
    ),
    key(
        "widgets.wide",
        "RUSSH_BBS_WIDGETS_WIDE",
        None,
        Str,
        "identity,online",
    ),
    key(
        "widgets.focus",
        "RUSSH_BBS_WIDGETS_FOCUS",
        None,
        Str,
        "clock",
    ),
    key(
        "widgets.split",
        "RUSSH_BBS_WIDGETS_SPLIT",
        None,
        Str,
        "chat,mail",
    ),
    key(
        "widgets.motd",
        "RUSSH_BBS_WIDGETS_MOTD",
        None,
        Str,
        "Welcome aboard",
    ),
    key(
        "widgets.quotes",
        "RUSSH_BBS_WIDGETS_QUOTES",
        None,
        Str,
        "One|Two",
    ),
    key(
        "mail.key_file",
        "RUSSH_BBS_MAIL_KEY_FILE",
        None,
        Str,
        "/tmp/mk",
    ),
    key(
        "ssh.auth_timeout_secs",
        "RUSSH_BBS_AUTH_TIMEOUT_SECS",
        Some("--auth-timeout-secs"),
        Int,
        "20",
    ),
    key(
        "ssh.idle_timeout_secs",
        "RUSSH_BBS_IDLE_TIMEOUT_SECS",
        Some("--idle-timeout-secs"),
        Int,
        "300",
    ),
    key(
        "ssh.keepalive_interval_secs",
        "RUSSH_BBS_SSH_KEEPALIVE_INTERVAL_SECS",
        None,
        Int,
        "15",
    ),
    key(
        "ssh.max_connections",
        "RUSSH_BBS_MAX_CONNECTIONS",
        Some("--max-connections"),
        Int,
        "100",
    ),
    key(
        "ssh.render_cols",
        "RUSSH_BBS_SSH_RENDER_COLS",
        None,
        Int,
        "132",
    ),
    key(
        "ssh.render_rows",
        "RUSSH_BBS_SSH_RENDER_ROWS",
        None,
        Int,
        "50",
    ),
    key(
        "ssh.max_auth_attempts",
        "RUSSH_BBS_SSH_MAX_AUTH_ATTEMPTS",
        None,
        Int,
        "3",
    ),
    key(
        "ssh.auth_rejection_millis",
        "RUSSH_BBS_SSH_AUTH_REJECTION_MILLIS",
        None,
        Int,
        "500",
    ),
    key(
        "ssh.redraw_interval_millis",
        "RUSSH_BBS_SSH_REDRAW_INTERVAL_MILLIS",
        None,
        Int,
        "100",
    ),
    key(
        "ssh.requests_per_second",
        "RUSSH_BBS_SSH_REQUESTS_PER_SECOND",
        None,
        Int,
        "300",
    ),
    key(
        "ssh.input_bytes_per_second",
        "RUSSH_BBS_SSH_INPUT_BYTES_PER_SECOND",
        None,
        Int,
        "4096",
    ),
    key(
        "ssh.flood_strikes",
        "RUSSH_BBS_SSH_FLOOD_STRIKES",
        None,
        Int,
        "500",
    ),
    key(
        "ssh.write_stall_secs",
        "RUSSH_BBS_SSH_WRITE_STALL_SECS",
        None,
        Int,
        "5",
    ),
    key(
        "ssh.allow_password_login",
        "RUSSH_BBS_SSH_ALLOW_PASSWORD_LOGIN",
        None,
        Bool,
        "false",
    ),
    key(
        "datasource.kind",
        "RUSSH_BBS_DATASOURCE_KIND",
        None,
        Str,
        "sqlite",
    ),
    key(
        "datasource.path",
        "RUSSH_BBS_DB",
        Some("--db"),
        Str,
        "/tmp/bbs.db",
    ),
    KeyDef {
        secret: true,
        ..key(
            "datasource.url",
            "RUSSH_BBS_DATASOURCE_URL",
            None,
            Str,
            "postgres://u:p@h/db",
        )
    },
    key(
        "datasource.tls_mode",
        "RUSSH_BBS_DATASOURCE_TLS_MODE",
        None,
        Str,
        "verify-full",
    ),
    key(
        "datasource.tls_root_cert",
        "RUSSH_BBS_DATASOURCE_TLS_ROOT_CERT",
        None,
        Str,
        "/tmp/ca.pem",
    ),
    key(
        "datasource.iam_auth",
        "RUSSH_BBS_DATASOURCE_IAM_AUTH",
        None,
        Bool,
        "true",
    ),
    key("blobs.backend", "RUSSH_BBS_BLOBS_BACKEND", None, Str, "s3"),
    key("blobs.dir", "RUSSH_BBS_BLOBS_DIR", None, Str, "/tmp/blobs"),
    key(
        "blobs.endpoint",
        "RUSSH_BBS_BLOBS_ENDPOINT",
        None,
        Str,
        "https://s3.example",
    ),
    key("blobs.bucket", "RUSSH_BBS_BLOBS_BUCKET", None, Str, "bbs"),
    key(
        "blobs.region",
        "RUSSH_BBS_BLOBS_REGION",
        None,
        Str,
        "us-east-1",
    ),
    key(
        "blobs.path_style",
        "RUSSH_BBS_BLOBS_PATH_STYLE",
        None,
        Bool,
        "true",
    ),
    key(
        "blobs.ca_file",
        "RUSSH_BBS_BLOBS_CA_FILE",
        None,
        Str,
        "/tmp/ca.pem",
    ),
    key(
        "blobs.max_object_bytes",
        "RUSSH_BBS_BLOBS_MAX_OBJECT_BYTES",
        None,
        Int,
        "1048576",
    ),
    key(
        "blobs.multipart_threshold_bytes",
        "RUSSH_BBS_BLOBS_MULTIPART_THRESHOLD_BYTES",
        None,
        Int,
        "8388608",
    ),
    key(
        "telnet.enabled",
        "RUSSH_BBS_TELNET_ENABLED",
        None,
        Bool,
        "true",
    ),
    key(
        "telnet.allow_login",
        "RUSSH_BBS_TELNET_ALLOW_LOGIN",
        None,
        Bool,
        "true",
    ),
    key(
        "telnet.listen",
        "RUSSH_BBS_TELNET_LISTEN",
        None,
        Str,
        "127.0.0.1:2300",
    ),
    key(
        "feeds.enabled",
        "RUSSH_BBS_FEEDS_ENABLED",
        None,
        Bool,
        "false",
    ),
    key(
        "feeds.rss_out_enabled",
        "RUSSH_BBS_FEEDS_RSS_OUT_ENABLED",
        None,
        Bool,
        "true",
    ),
    key(
        "feeds.rss_out_listen",
        "RUSSH_BBS_FEEDS_RSS_OUT_LISTEN",
        None,
        Str,
        "127.0.0.1:8081",
    ),
    key(
        "feeds.rss_out_base_url",
        "RUSSH_BBS_FEEDS_RSS_OUT_BASE_URL",
        None,
        Str,
        "https://bbs.example/feeds",
    ),
    key(
        "feeds.ca_file",
        "RUSSH_BBS_FEEDS_CA_FILE",
        None,
        Str,
        "/tmp/feeds-ca.pem",
    ),
    key(
        "waiting_room.ip_burst",
        "RUSSH_BBS_IP_BURST",
        Some("--ip-burst"),
        Int,
        "4",
    ),
    key(
        "waiting_room.ip_interval_secs",
        "RUSSH_BBS_IP_INTERVAL_SECS",
        Some("--ip-interval-secs"),
        Int,
        "3",
    ),
    key(
        "waiting_room.key_burst",
        "RUSSH_BBS_KEY_BURST",
        Some("--key-burst"),
        Int,
        "2",
    ),
    key(
        "waiting_room.key_interval_secs",
        "RUSSH_BBS_KEY_INTERVAL_SECS",
        Some("--key-interval-secs"),
        Int,
        "20",
    ),
    key(
        "waiting_room.max_form_submissions",
        "RUSSH_BBS_MAX_FORM_SUBMISSIONS",
        Some("--max-form-submissions"),
        Int,
        "7",
    ),
    key(
        "waiting_room.max_tracked_sources",
        "RUSSH_BBS_WAITING_ROOM_MAX_TRACKED_SOURCES",
        None,
        Int,
        "500",
    ),
];

/// The setting with this dotted key.
pub(crate) fn by_path(path: &str) -> Option<&'static KeyDef> {
    KEYS.iter().find(|k| k.path == path)
}

/// The setting read from this environment variable.
pub(crate) fn by_env(name: &str) -> Option<&'static KeyDef> {
    KEYS.iter().find(|k| k.env == name)
}

/// `path` as `(section, leaf)`; top-level keys have no section.
pub(crate) fn split(path: &str) -> (Option<&str>, &str) {
    match path.split_once('.') {
        Some((section, leaf)) => (Some(section), leaf),
        None => (None, path),
    }
}
