//! Config tests: layering, errors, redaction, and that the example and the docs match the code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use russh_bbs_core::store::RegistrationMode;

use super::keys::{self, KEYS};
use super::{
    Cli, CliValue, Config, ConfigError, DatasourceKind, Dirs, FileInput, Loaded, SecretUrl, Source,
    Sources, assemble,
};
use clap::Parser as _;

const FILE: &str = "/etc/bbs/config.toml";

fn env(pairs: &[(&str, &str)]) -> Vec<(String, OsString)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
        .collect()
}

fn from_file(text: &str) -> Result<super::Assembled, ConfigError> {
    let input = FileInput {
        path: Path::new(FILE),
        text,
        loose_permissions: false,
    };
    assemble(Some(input), &[], &[])
}

fn flags(args: &[&str]) -> Vec<CliValue> {
    let mut line = vec!["russh-bbs"];
    line.extend_from_slice(args);
    Cli::try_parse_from(line).expect("valid flags").overrides()
}

fn message(result: Result<super::Assembled, ConfigError>) -> String {
    result.expect_err("expected an error").to_string()
}

#[test]
fn defaults_only() {
    let got = assemble(None, &[], &[]).expect("defaults are valid");
    assert_eq!(got.config, Config::default());
    assert!(!got.sources.is_set("ssh.listen"));
    assert_eq!(got.config.registration_mode, RegistrationMode::Invite);
    assert_eq!(got.config.ssh.listen.to_string(), "0.0.0.0:2222");
    assert!(got.config.ssh.allow_password_login);
    assert!(!got.config.telnet.enabled && !got.config.telnet.allow_login);
    assert_eq!(got.config.datasource.kind, DatasourceKind::Sqlite);
    // The defaults reproduce the transport limits of D22 and D37 exactly.
    assert_eq!(got.config.limits(), crate::ssh::Limits::default());
    assert_eq!(got.warnings, Vec::<String>::new());
}

#[test]
fn file_overrides_defaults() {
    let got = from_file("[ssh]\nlisten = \"127.0.0.1:2200\"\nmax_connections = 7\n").expect("ok");
    assert_eq!(got.config.ssh.listen.port(), 2200);
    assert_eq!(got.config.ssh.max_connections, 7);
    // Untouched keys keep their defaults.
    assert_eq!(got.config.ssh.idle_timeout_secs, 600);
    assert_eq!(got.sources.get("ssh.listen"), Source::File(FILE.into()));
    assert_eq!(got.sources.get("ssh.idle_timeout_secs"), Source::Default);
}

#[test]
fn env_overrides_file() {
    let input = FileInput {
        path: Path::new(FILE),
        text: "[ssh]\nlisten = \"127.0.0.1:2200\"\nmax_connections = 7\n",
        loose_permissions: false,
    };
    let env = env(&[("RUSSH_BBS_LISTEN", "127.0.0.1:2300")]);
    let got = assemble(Some(input), &env, &[]).expect("ok");
    assert_eq!(got.config.ssh.listen.port(), 2300);
    assert_eq!(got.config.ssh.max_connections, 7);
    assert_eq!(
        got.sources.get("ssh.listen"),
        Source::Env("RUSSH_BBS_LISTEN")
    );
    assert_eq!(
        got.sources.get("ssh.max_connections"),
        Source::File(FILE.into())
    );
}

#[test]
fn cli_overrides_env() {
    let input = FileInput {
        path: Path::new(FILE),
        text: "[ssh]\nlisten = \"127.0.0.1:2200\"\n",
        loose_permissions: false,
    };
    let env = env(&[("RUSSH_BBS_LISTEN", "127.0.0.1:2300")]);
    let flags = flags(&[
        "--listen",
        "127.0.0.1:2400",
        "--db",
        "/tmp/x.db",
        "--ip-burst",
        "3",
    ]);
    let got = assemble(Some(input), &env, &flags).expect("ok");
    assert_eq!(got.config.ssh.listen.port(), 2400);
    assert_eq!(got.sources.get("ssh.listen"), Source::Flag("--listen"));
    assert_eq!(got.config.datasource.path, Some(PathBuf::from("/tmp/x.db")));
    assert_eq!(got.config.waiting_room.ip_burst, 3);
}

#[test]
fn unknown_file_keys_are_rejected_by_name() {
    let msg = message(from_file("[ssh]\nlistn = \"x\"\n"));
    assert!(msg.contains("ssh.listn") && msg.contains(FILE), "{msg}");
    let msg = message(from_file("[sshh]\nlisten = \"x\"\n"));
    assert!(msg.contains("sshh") && msg.contains(FILE), "{msg}");
    let msg = message(from_file("bogus = 1\n"));
    assert!(msg.contains("bogus"), "{msg}");
}

#[test]
fn unknown_environment_variable_is_rejected_by_name() {
    let err = assemble(None, &env(&[("RUSSH_BBS_LISTN", "x")]), &[]).expect_err("rejected");
    assert!(err.to_string().contains("RUSSH_BBS_LISTN"), "{err}");
    // Variables without the prefix are not ours; RUSSH_BBS_CONFIG is read elsewhere.
    assemble(
        None,
        &env(&[("HOME", "/x"), ("RUSSH_BBS_CONFIG", "/y")]),
        &[],
    )
    .expect("ignored");
}

#[test]
fn kubernetes_service_links_are_ignored_with_a_warning_naming_them() {
    let links = [
        ("RUSSH_BBS_PORT", "tcp://10.0.0.1:2222"),
        ("RUSSH_BBS_PORT_2222_TCP", "tcp://10.0.0.1:2222"),
        ("RUSSH_BBS_PORT_2222_TCP_ADDR", "10.0.0.1"),
        ("RUSSH_BBS_SERVICE_HOST", "10.0.0.1"),
        ("RUSSH_BBS_SERVICE_PORT", "2222"),
        ("RUSSH_BBS_SERVICE_PORT_SSH", "2222"),
    ];
    let ok = assemble(None, &env(&links), &[]).expect("service links do not stop boot");
    for (name, _) in links {
        assert!(
            ok.warnings.iter().any(|w| w.contains(name)),
            "no warning names {name}: {:?}",
            ok.warnings
        );
    }
    // A real setting still wins, and a typo next to a link is still an error.
    let err = assemble(
        None,
        &env(&[("RUSSH_BBS_PORT", "x"), ("RUSSH_BBS_LISTN", "x")]),
        &[],
    )
    .expect_err("typo still rejected");
    assert!(err.to_string().contains("RUSSH_BBS_LISTN"), "{err}");
    assert!(assemble(None, &env(&[("RUSSH_BBS_SERVICE_PORTS", "x")]), &[]).is_err());
}

#[test]
fn bad_values_name_the_key_and_the_source() {
    let msg = message(from_file("[ssh]\nmax_connections = \"lots\"\n"));
    assert!(
        msg.contains("ssh.max_connections") && msg.contains(FILE),
        "{msg}"
    );

    let err = assemble(None, &env(&[("RUSSH_BBS_MAX_CONNECTIONS", "lots")]), &[]).expect_err("bad");
    let msg = err.to_string();
    assert!(
        msg.contains("ssh.max_connections") && msg.contains("RUSSH_BBS_MAX_CONNECTIONS"),
        "{msg}"
    );

    let flags = flags(&["--idle-timeout-secs=-4"]);
    let msg = assemble(None, &[], &flags).expect_err("bad").to_string();
    assert!(
        msg.contains("ssh.idle_timeout_secs") && msg.contains("--idle-timeout-secs"),
        "{msg}"
    );

    // Out of range, from the file.
    let msg = message(from_file("[ssh]\nmax_connections = 0\n"));
    assert!(
        msg.contains("ssh.max_connections") && msg.contains("between 1 and"),
        "{msg}"
    );

    let msg = message(from_file("registration_mode = \"wide-open\"\n"));
    assert!(
        msg.contains("registration_mode") && msg.contains("invite"),
        "{msg}"
    );
}

#[test]
fn a_flag_that_is_out_of_range_names_the_flag() {
    let msg = assemble(None, &[], &flags(&["--key-burst", "0"]))
        .expect_err("bad")
        .to_string();
    assert!(
        msg.contains("waiting_room.key_burst") && msg.contains("--key-burst"),
        "{msg}"
    );
}

#[test]
fn port_zero_and_clashing_telnet_port_are_rejected() {
    let msg = message(from_file("[ssh]\nlisten = \"0.0.0.0:0\"\n"));
    assert!(msg.contains("ssh.listen") && msg.contains("port"), "{msg}");
    let msg = message(from_file(
        "[ssh]\nlisten = \"0.0.0.0:2222\"\n[telnet]\nenabled = true\nlisten = \"127.0.0.1:2222\"\n",
    ));
    assert!(msg.contains("telnet.listen"), "{msg}");
}

#[test]
fn telnet_login_needs_telnet_enabled() {
    let msg = message(from_file("[telnet]\nallow_login = true\n"));
    assert!(
        msg.contains("telnet.allow_login") && msg.contains("telnet.enabled"),
        "{msg}"
    );
    let ok = from_file("[telnet]\nenabled = true\nallow_login = true\n").expect("both on");
    assert!(
        ok.warnings.iter().any(|w| w.contains("telnet")),
        "{:?}",
        ok.warnings
    );
}

#[test]
fn telnet_is_off_by_default_and_on_warns_and_reaches_the_limits() {
    let off = from_file("").expect("parses");
    assert!(!off.config.telnet.enabled && !off.config.limits().low_security);
    assert!(!off.warnings.iter().any(|w| w.contains("telnet")));
    let got = from_file("[telnet]\nenabled = true\n").expect("parses");
    let warning = got
        .warnings
        .iter()
        .find(|w| w.contains("telnet.enabled"))
        .expect("a warning");
    assert!(warning.contains("LOW-SECURITY"), "{warning}");
    assert!(got.config.limits().low_security);
}

#[test]
fn password_login_is_on_by_default_and_off_reaches_the_limits_without_a_warning() {
    // D94 (10): ssh is the transport, the account is handle plus password.
    let on = from_file("").expect("parses");
    assert!(on.config.ssh.allow_password_login);
    assert!(on.config.limits().allow_password_login);
    let off = from_file("[ssh]\nallow_password_login = false\n").expect("parses");
    assert!(!off.config.ssh.allow_password_login);
    assert!(!off.config.limits().allow_password_login);
    // It is built now (M3-D): the "does nothing yet" warning is gone.
    assert!(
        !off.warnings
            .iter()
            .any(|w| w.contains("allow_password_login"))
    );
}

#[test]
fn paths_must_be_sane() {
    let msg = message(from_file("[ssh]\nhost_key = \"\"\n"));
    assert!(
        msg.contains("ssh.host_key") && msg.contains("empty"),
        "{msg}"
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let text = format!("[ssh]\nhost_key = {:?}\n", dir.path().display().to_string());
    let msg = message(from_file(&text));
    assert!(
        msg.contains("ssh.host_key") && msg.contains("directory"),
        "{msg}"
    );
}

#[test]
fn options_for_the_other_backend_are_rejected() {
    let msg = message(from_file("[datasource]\nurl = \"postgres://u:pw@h/db\"\n"));
    assert!(
        msg.contains("datasource.url") && msg.contains("postgres"),
        "{msg}"
    );
    assert!(!msg.contains("pw@"), "{msg}");
    let msg = message(from_file(
        "[datasource]\nkind = \"postgres\"\npath = \"/x.db\"\nurl = \"postgres://h/d\"\n",
    ));
    assert!(msg.contains("datasource.path"), "{msg}");
    let msg = message(from_file("[datasource]\nkind = \"postgres\"\n"));
    assert!(
        msg.contains("datasource.url") && msg.contains("required"),
        "{msg}"
    );
    let msg = message(from_file(
        "[datasource]\nkind = \"postgres\"\nurl = \"mysql://u:pw@h/db\"\n",
    ));
    assert!(
        msg.contains("datasource.url") && !msg.contains("pw"),
        "{msg}"
    );
    let msg = message(from_file("[datasource]\nkind = \"mysql\"\n"));
    assert!(msg.contains("datasource.kind"), "{msg}");
    let msg = message(from_file(
        "[datasource]\nkind = \"postgres\"\nurl = \"postgres://h/d\"\ntls_mode = \"disable\"\n",
    ));
    assert!(msg.contains("datasource.tls_mode"), "{msg}");
}

#[test]
fn s3_needs_a_bucket_and_no_credentials() {
    let msg = message(from_file("[blobs]\nbackend = \"s3\"\n"));
    assert!(msg.contains("blobs.bucket"), "{msg}");
    let msg = message(from_file(
        "[blobs]\nbackend = \"s3\"\nbucket = \"b\"\nendpoint = \"https://key:secret@s3.example\"\n",
    ));
    assert!(
        msg.contains("blobs.endpoint") && !msg.contains("secret"),
        "{msg}"
    );
    let ok = from_file("[blobs]\nbackend = \"s3\"\nbucket = \"b\"\nregion = \"eu-west-1\"\n")
        .expect("ok");
    assert!(!ok.warnings.iter().any(|w| w.contains("blobs")), "https needs no warning");
    let plain = from_file(
        "[blobs]\nbackend = \"s3\"\nbucket = \"b\"\nendpoint = \"http://127.0.0.1:9000\"\n",
    )
    .expect("ok");
    assert!(plain.warnings.iter().any(|w| w.contains("blobs.endpoint is plain http")));
    let msg = message(from_file("[blobs]\nbucket = \"b\"\n"));
    assert!(msg.contains("blobs.bucket"), "{msg}");
}

#[test]
fn blob_limits_and_s3_only_settings_are_checked() {
    let d = Config::default().blobs;
    assert_eq!((d.max_object_bytes, d.multipart_threshold_bytes), (268_435_456, 16_777_216));
    assert!(!d.path_style && d.ca_file.is_none());
    // The S3-only settings are refused with the local backend.
    for line in ["path_style = true", "ca_file = \"/etc/hostname\"", "multipart_threshold_bytes = 8388608"] {
        let msg = message(from_file(&format!("[blobs]\n{line}\n")));
        assert!(msg.contains("only used when"), "{line}: {msg}");
    }
    let s3 = "[blobs]\nbackend = \"s3\"\nbucket = \"b\"\n";
    let msg = message(from_file(&format!("{s3}multipart_threshold_bytes = 1048576\n")));
    assert!(msg.contains("blobs.multipart_threshold_bytes"), "{msg}");
    let msg = message(from_file(&format!("{s3}max_object_bytes = 0\n")));
    assert!(msg.contains("blobs.max_object_bytes"), "{msg}");
    let msg = message(from_file(&format!(
        "{s3}max_object_bytes = 1099511627776\nmultipart_threshold_bytes = 8388608\n"
    )));
    assert!(msg.contains("blobs.max_object_bytes") && msg.contains("10,000"), "{msg}");
    let msg = message(from_file(&format!("{s3}ca_file = \"/nonexistent/ca.pem\"\n")));
    assert!(msg.contains("blobs.ca_file"), "{msg}");
    let ok = from_file(&format!(
        "{s3}path_style = true\nca_file = \"/etc/hostname\"\nmax_object_bytes = 1048576\n"
    ))
    .expect("ok");
    assert!(ok.config.blobs.path_style);
}

// ---- postgres ------------------------------------------------------------------------------

fn load(env_pairs: &[(&str, &str)], dirs: Dirs) -> Result<Loaded, ConfigError> {
    Loaded::load(None, &env(env_pairs), &[], dirs)
}

#[test]
fn selecting_postgres_is_accepted_and_resolves_to_settings_that_hide_the_password() {
    let loaded = load(
        &[
            ("RUSSH_BBS_DATASOURCE_KIND", "postgres"),
            (
                "RUSSH_BBS_DATASOURCE_URL",
                "postgres://bbs:hunter2@db.example/bbs",
            ),
            ("RUSSH_BBS_DATASOURCE_TLS_MODE", "verify-full"),
        ],
        // No HOME or XDG: postgres needs no data directory for the database.
        Dirs {
            home: None,
            ..Dirs::default()
        },
    )
    .expect("postgres is built");
    let paths = loaded.paths();
    // The host key still needs a location, so only the datasource part is checked here.
    let ds = match paths {
        Err(ConfigError::NoDataDir {
            key: "ssh.host_key",
        }) => {
            let mut with_key = loaded.clone();
            with_key.config.ssh.host_key = Some("/k".into());
            with_key.config.mail.key_file = Some("/m".into());
            with_key.paths().expect("paths").datasource
        }
        other => other.expect("paths").datasource,
    };
    let super::ResolvedDatasource::Postgres(settings) = &ds else {
        panic!("expected postgres, got {ds:?}");
    };
    let shown = format!("{settings:?} {ds:?}");
    assert!(
        !shown.contains("hunter2") && !shown.contains("db.example"),
        "{shown}"
    );
}

#[test]
fn iam_auth_is_refused_at_boot_with_a_clear_error() {
    let err = load(
        &[
            ("RUSSH_BBS_DATASOURCE_KIND", "postgres"),
            (
                "RUSSH_BBS_DATASOURCE_URL",
                "postgres://bbs:hunter2@db.example/bbs",
            ),
            ("RUSSH_BBS_DATASOURCE_IAM_AUTH", "true"),
        ],
        Dirs::default(),
    )
    .expect_err("iam auth is not built");
    let msg = err.to_string();
    assert!(matches!(err, ConfigError::IamAuthNotBuilt { .. }), "{msg}");
    assert!(
        msg.contains("IAM token authentication is not built"),
        "{msg}"
    );
    assert!(msg.contains("RUSSH_BBS_DATASOURCE_IAM_AUTH"), "{msg}");
    assert!(msg.contains("will not fall back"), "{msg}");
    assert!(!msg.contains("hunter2"), "{msg}");
}

#[test]
fn a_postgres_config_without_the_url_fails_validation_before_the_backend_check() {
    let err = load(
        &[("RUSSH_BBS_DATASOURCE_KIND", "postgres")],
        Dirs::default(),
    )
    .expect_err("bad");
    assert!(err.to_string().contains("datasource.url"), "{err}");
}

#[test]
fn feeds_ca_file_must_name_an_existing_file_and_defaults_to_the_platform_store() {
    assert_eq!(Config::default().feeds.ca_file, None);
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("none.pem");
    let err = load(
        &[("RUSSH_BBS_FEEDS_CA_FILE", missing.to_str().unwrap())],
        Dirs::default(),
    )
    .expect_err("a missing file is refused at boot");
    assert!(err.to_string().contains("feeds.ca_file"), "{err}");
    let present = dir.path().join("ca.pem");
    std::fs::write(&present, "x").unwrap();
    let loaded = load(
        &[("RUSSH_BBS_FEEDS_CA_FILE", present.to_str().unwrap())],
        Dirs::default(),
    )
    .expect("an existing file is accepted");
    assert_eq!(
        loaded.config.feeds.ca_file.as_deref(),
        Some(present.as_path())
    );
}

// ---- redaction -----------------------------------------------------------------------------

const SECRET_URL: &str = "postgres://bbs:hunter2@db.example:5432/bbs?sslmode=require";

fn postgres_config() -> Config {
    let text = format!("[datasource]\nkind = \"postgres\"\nurl = \"{SECRET_URL}\"\n");
    from_file(&text).expect("valid postgres settings").config
}

#[test]
fn the_password_is_redacted_in_debug() {
    let config = postgres_config();
    assert_eq!(
        config.datasource.url.as_ref().map(SecretUrl::expose),
        Some(SECRET_URL)
    );
    for text in [
        format!("{config:?}"),
        format!("{config:#?}"),
        format!("{:?}", config.datasource),
    ] {
        assert!(!text.contains("hunter2"), "{text}");
        assert!(text.contains("bbs:***@db.example"), "{text}");
    }
}

/// Collects everything tracing writes.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("capture lock").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn the_password_is_redacted_in_logs() {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_ansi(false)
        .finish();
    let config = postgres_config();
    // Exactly what main.rs logs.
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(file = ?Some(FILE), config = ?config, "configuration loaded");
        tracing::info!(url = %config.datasource.url.as_ref().expect("url"), "display too");
    });
    let logged = String::from_utf8(capture.0.lock().expect("lock").clone()).expect("utf8");
    assert!(logged.contains("configuration loaded"), "{logged}");
    assert!(logged.contains("db.example"), "{logged}");
    assert!(!logged.contains("hunter2"), "{logged}");
}

#[test]
fn errors_never_quote_the_source_line() {
    // A syntax error on the line that holds the password must not print that line.
    let text = format!("[datasource]\nurl = \"{SECRET_URL}\"\nurl = \"{SECRET_URL}\"\n");
    let msg = message(from_file(&text));
    assert!(!msg.contains("hunter2"), "{msg}");
    assert!(msg.contains("line"), "{msg}");
    // A wrong type for the secret key does not echo the value either.
    let msg = message(from_file("[datasource]\nurl = 5\n"));
    assert!(
        msg.contains("datasource.url") && msg.contains("string"),
        "{msg}"
    );
}

#[test]
fn a_loose_config_file_with_a_url_warns() {
    let input = FileInput {
        path: Path::new(FILE),
        text: &format!("[datasource]\nkind = \"postgres\"\nurl = \"{SECRET_URL}\"\n"),
        loose_permissions: true,
    };
    let got = assemble(Some(input), &[], &[]).expect("valid");
    assert!(
        got.warnings
            .iter()
            .any(|w| w.contains("chmod 600") && !w.contains("hunter2"))
    );
}

// ---- files, paths, registration ------------------------------------------------------------

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .expect("write file");
    path
}

#[test]
fn a_missing_default_file_means_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dirs = Dirs {
        xdg_config_home: Some(dir.path().to_path_buf()),
        home: Some(dir.path().to_path_buf()),
        ..Dirs::default()
    };
    let loaded = Loaded::load(None, &[], &[], dirs).expect("defaults");
    assert_eq!(loaded.config, Config::default());
    // /data/config.toml is also a default location; in a build sandbox it does not exist.
    if !Path::new(super::CONTAINER_CONFIG).exists() {
        assert!(loaded.file.is_none());
    }
}

#[test]
fn a_default_file_is_found_under_the_xdg_config_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("russh-bbs")).expect("mkdir");
    write(
        &dir.path().join("russh-bbs"),
        "config.toml",
        "[ssh]\nmax_connections = 9\n",
    );
    let dirs = Dirs {
        xdg_config_home: Some(dir.path().to_path_buf()),
        ..Dirs::default()
    };
    let loaded = Loaded::load(None, &[], &[], dirs).expect("loaded");
    assert_eq!(loaded.config.ssh.max_connections, 9);
}

#[test]
fn a_missing_explicit_file_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("nope.toml");
    let err = Loaded::load(Some(missing.clone()), &[], &[], Dirs::default()).expect_err("missing");
    assert!(matches!(err, ConfigError::MissingFile { .. }), "{err}");
    assert!(err.to_string().contains("nope.toml"), "{err}");
    // The same through the environment variable.
    let env = env(&[("RUSSH_BBS_CONFIG", &missing.display().to_string())]);
    let err = Loaded::load(None, &env, &[], Dirs::default()).expect_err("missing");
    assert!(matches!(err, ConfigError::MissingFile { .. }), "{err}");
}

#[test]
fn the_config_flag_beats_the_environment_variable_and_the_file_is_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let a = write(dir.path(), "a.toml", "[ssh]\nmax_connections = 11\n");
    let b = write(dir.path(), "b.toml", "[ssh]\nmax_connections = 22\n");
    let env = env(&[("RUSSH_BBS_CONFIG", &b.display().to_string())]);
    let loaded = Loaded::load(Some(a.clone()), &env, &[], Dirs::default()).expect("loaded");
    assert_eq!(loaded.config.ssh.max_connections, 11);
    assert_eq!(loaded.file, Some(a));
}

#[test]
fn registration_mode_applies_to_the_store_only_when_set_explicitly() {
    let default = assemble(None, &[], &[]).expect("ok");
    let loaded = Loaded {
        config: default.config,
        sources: default.sources,
        warnings: vec![],
        file: None,
        dirs: Dirs::default(),
    };
    assert_eq!(loaded.registration_mode_override(), None);

    let dir = tempfile::tempdir().expect("tempdir");
    let from_env = load(
        &[("RUSSH_BBS_REGISTRATION_MODE", "apply")],
        Dirs {
            xdg_config_home: Some(dir.path().to_path_buf()),
            ..Dirs::default()
        },
    )
    .expect("ok");
    assert_eq!(
        from_env.registration_mode_override(),
        Some(RegistrationMode::Apply)
    );
}

#[test]
fn default_paths_are_resolved_and_a_missing_home_is_an_error() {
    let dirs = Dirs {
        xdg_data_home: Some("/xdg".into()),
        ..Dirs::default()
    };
    let loaded = Loaded {
        config: Config::default(),
        sources: Sources::default(),
        warnings: vec![],
        file: None,
        dirs,
    };
    let paths = loaded.paths().expect("paths");
    assert_eq!(
        paths.host_key,
        PathBuf::from("/xdg/russh-bbs/host_ed25519_key")
    );
    assert!(
        matches!(&paths.datasource, super::ResolvedDatasource::Sqlite(p)
            if p == &PathBuf::from("/xdg/russh-bbs/russh-bbs.db")),
        "{:?}",
        paths.datasource
    );

    let none = Loaded {
        dirs: Dirs::default(),
        ..loaded.clone()
    };
    assert!(matches!(none.paths(), Err(ConfigError::NoDataDir { .. })));
    let mut explicit = none;
    explicit.config.ssh.host_key = Some("/k".into());
    explicit.config.mail.key_file = Some("/m".into());
    explicit.config.datasource.path = Some("/d.db".into());
    let explicit = explicit.paths().expect("explicit").datasource;
    assert!(
        matches!(&explicit, super::ResolvedDatasource::Sqlite(p) if p == &PathBuf::from("/d.db")),
        "{explicit:?}"
    );
}

// ---- the example and the docs agree with the code ------------------------------------------

const EXAMPLE: &str = include_str!("../../../../config.example.toml");
const DOCS: &str = include_str!("../../../../docs/config.md");

#[test]
fn the_example_file_parses_and_equals_the_defaults() {
    let got = from_file(EXAMPLE).expect("example parses and validates");
    assert_eq!(got.config, Config::default());
    assert!(got.warnings.is_empty(), "{:?}", got.warnings);
}

#[test]
fn every_key_is_accepted_by_the_typed_config() {
    for def in KEYS {
        let value = match def.kind {
            keys::Kind::Str => toml::Value::String(def.sample.to_owned()),
            keys::Kind::Int => toml::Value::Integer(def.sample.parse().expect("int sample")),
            keys::Kind::Bool => toml::Value::Boolean(def.sample.parse().expect("bool sample")),
        };
        let mut table = toml::Table::new();
        match keys::split(def.path) {
            (None, leaf) => {
                table.insert(leaf.to_owned(), value);
            }
            (Some(section), leaf) => {
                let mut inner = toml::Table::new();
                inner.insert(leaf.to_owned(), value);
                table.insert(section.to_owned(), toml::Value::Table(inner));
            }
        }
        toml::Value::Table(table)
            .try_into::<Config>()
            .unwrap_or_else(|e| panic!("{} is not a field of Config: {e}", def.path));
    }
}

#[test]
fn every_key_is_in_the_example_and_the_docs() {
    for def in KEYS {
        let leaf = keys::split(def.path).1;
        let in_example = EXAMPLE
            .lines()
            .map(|l| l.trim_start_matches('#').trim())
            .any(|l| l.starts_with(&format!("{leaf} =")));
        assert!(
            in_example,
            "{} is missing from config.example.toml",
            def.path
        );
        assert!(
            DOCS.contains(&format!("`{}`", def.path)),
            "{} is missing from docs/config.md",
            def.path
        );
        assert!(
            DOCS.contains(&format!("`{}`", def.env)),
            "{} is missing from docs/config.md",
            def.env
        );
        if let Some(flag) = def.flag {
            assert!(
                DOCS.contains(&format!("`{flag}`")),
                "{flag} is missing from docs/config.md"
            );
        }
    }
    assert!(DOCS.contains("`RUSSH_BBS_CONFIG`") && DOCS.contains("`--config`"));
}

#[test]
fn env_names_and_paths_are_unique() {
    for (i, a) in KEYS.iter().enumerate() {
        for b in &KEYS[i + 1..] {
            assert_ne!(a.path, b.path);
            assert_ne!(a.env, b.env);
        }
        assert!(a.env.starts_with("RUSSH_BBS_"));
        assert!(
            !a.secret || a.flag.is_none(),
            "secrets must not be flags: {}",
            a.path
        );
    }
}
