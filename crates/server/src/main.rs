//! The russh-bbs server binary: load the configuration, load the host key, start the SSH server.
//! Everything testable lives in the library; this file only wires it together.

use std::sync::Arc;

use anyhow::Context as _;
use clap::Parser;
use russh_bbs_core::limits::SystemClock;
use russh_bbs_core::scheduler::{Scheduler, SchedulerStore};
use russh_bbs_core::store::{SettingsStore, set_registration_mode};
use russh_bbs_server::config::{Cli, Loaded, ResolvedDatasource};
use russh_bbs_server::doors::{self, Doors};
use russh_bbs_server::feeds::{
    FetchLimits, Guard, GuardedFetcher, Roots, RssOut, RssOutConfig, SystemResolver, load_ca_file,
    proxy_in_environment,
};
use russh_bbs_server::founder;
use russh_bbs_server::sftp::{ClamdScanner, ClamdTarget, FileServices, Scanner};
use russh_bbs_server::shutdown;
use russh_bbs_server::ssh::{Backend, HostKey, ServerConfig, SshServer};
use russh_bbs_server::telnet::{TelnetConfig, TelnetServer};
use russh_bbs_server::ui::Ui;
use russh_bbs_store::{Close, PostgresStore, SqliteStore};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let loaded = Loaded::from_process(&cli).context("invalid configuration")?;
    for warning in &loaded.warnings {
        warn!("{warning}");
    }
    // `Config`'s Debug redacts the datasource URL, so this is safe to log.
    info!(file = ?loaded.file, config = ?loaded.config, "configuration loaded");
    let paths = loaded.paths()?;

    // Sysop theme files live in `themes/` beside the host key (M3-A); no config key is needed.
    let themes_dir = paths.host_key.parent().map(|dir| dir.join("themes"));
    // Sysop art files live in `art/` beside them (UI-3), also without a config key.
    let art_dir = paths.host_key.parent().map(|dir| dir.join("art"));
    let key_path = paths.host_key.clone();
    let host_key = tokio::task::spawn_blocking(move || HostKey::load_or_create(&key_path))
        .await
        .context("host key task panicked")??;
    // The fingerprint is public: the sysop publishes it so users can verify the first connect.
    info!(path = %paths.host_key.display(), fingerprint = %host_key.fingerprint(), "host key ready");

    // The blob store (D30, M4-F1): built now so a bad directory or bucket is a boot error. The
    // file areas and the SFTP subsystem (M4-F2) read and write through it.
    let blobs = russh_bbs_server::blobs::open(&loaded.config.blobs, paths.blobs_dir.as_deref())
        .await
        .context("opening the blob store")?;
    info!(backend = blobs.backend_name(), "blob store ready");
    let files = Arc::new(FileServices {
        blobs,
        scanner: scanner_of(&loaded.config.files),
        sftp: loaded.config.files.sftp,
    });
    info!(
        sftp = files.sftp,
        scanner = match files.scanner {
            Scanner::None(_) => "none (uploads are stored unscanned)",
            Scanner::Clamd(_) => "clamd",
        },
        "file areas ready"
    );

    // The datasource is chosen by configuration and never falls back (D6, D38): a Postgres
    // that cannot be reached or verified is a boot error, not a SQLite file.
    match &paths.datasource {
        ResolvedDatasource::Sqlite(path) => {
            let store = SqliteStore::open(path)
                .await
                .with_context(|| format!("opening database {}", path.display()))?;
            info!(backend = "sqlite", path = %path.display(), "database ready");
            serve(
                store,
                &loaded,
                &host_key,
                &paths.mail_key,
                themes_dir.as_deref(),
                art_dir.as_deref(),
                files,
            )
            .await?;
        }
        ResolvedDatasource::Postgres(settings) => {
            // The error text never contains the password (see `PostgresSettings`).
            let store = PostgresStore::open(settings)
                .await
                .context("opening the postgres datasource (TLS is required)")?;
            info!(backend = "postgres", tls = true, "database ready");
            serve(
                store,
                &loaded,
                &host_key,
                &paths.mail_key,
                themes_dir.as_deref(),
                art_dir.as_deref(),
                files,
            )
            .await?;
        }
    }
    Ok(())
}

/// Run the board on `store` until a shutdown signal. Everything past store selection is
/// backend-independent (core traits only).
/// The board's name, accent, defaults and limits (UI-2); the same checks the config ran.
fn board_of(loaded: &Loaded) -> anyhow::Result<russh_bbs_server::ui::Board> {
    russh_bbs_server::ui::Board::from_config(&loaded.config.board)
        .context("reading the [board] settings")
}

/// The scanner `[files]` names (M4-F2). Validation already refused a half-set clamd section.
fn scanner_of(config: &russh_bbs_server::config::FilesConfig) -> Scanner {
    use russh_bbs_server::config::ScanBackend;
    match (config.scan, &config.clamd_address, &config.clamd_socket) {
        (ScanBackend::Clamd, Some(address), _) => {
            Scanner::Clamd(ClamdScanner::new(ClamdTarget::Tcp(address.clone())))
        }
        (ScanBackend::Clamd, None, Some(socket)) => {
            Scanner::Clamd(ClamdScanner::new(ClamdTarget::Unix(socket.clone())))
        }
        _ => Scanner::None(russh_bbs_core::files::NoScan),
    }
}

/// The sysop's widget settings (UI-10); the same checks the config ran.
fn widgets_of(loaded: &Loaded) -> anyhow::Result<russh_bbs_server::ui::widgets::WidgetSettings> {
    russh_bbs_server::ui::widgets::WidgetSettings::from_config(&loaded.config.widgets)
        .context("reading the [widgets] settings")
}

#[allow(
    clippy::too_many_lines,
    reason = "the boot sequence, in order; splitting it hides the order"
)]
async fn serve<S: Backend + SettingsStore + SchedulerStore + Close + Clone>(
    store: S,
    loaded: &Loaded,
    host_key: &HostKey,
    mail_key_path: &std::path::Path,
    themes_dir: Option<&std::path::Path>,
    art_dir: Option<&std::path::Path>,
    files: Arc<FileServices>,
) -> anyhow::Result<()> {
    // Before anything listens, so a stop signal at any point after "listening" is handled.
    let mut signals = shutdown::Signals::install().context("installing the signal handlers")?;

    // A registration mode set in the file, environment or flags wins over the stored value
    // (decision D38); when none is set the stored value stands.
    if let Some(mode) = loaded.registration_mode_override() {
        set_registration_mode(&store, mode)
            .await
            .context("applying registration_mode from the configuration")?;
        info!(
            mode = mode.as_str(),
            "registration mode set from configuration"
        );
    }

    // The mail key (D78, D90): created only on a fresh install, never replaced. A missing, damaged
    // or wrong key file with mail in the database stops the boot here, before anything listens.
    let mail_keys = russh_bbs_server::mail::load_keys(&store, mail_key_path)
        .await
        .context("loading the mail key")?;
    // The path and the key's id are not secrets; the key itself is never logged.
    info!(path = %mail_key_path.display(), key_id = %mail_keys.active(), "mail key ready");

    // First run: print the one-time setup token to the console (D41).
    let founder = founder::start(&store, &mut std::io::stdout()).await?;

    let config = ServerConfig {
        listen: loaded.config.ssh.listen,
        limits: loaded.config.limits(),
    };
    // The shipped screens and themes, plus the sysop's theme files from `themes/` beside the
    // host key (M3-A). A bad theme file is skipped with a warning and never stops the boot.
    let ui = match themes_dir {
        Some(dir) => Ui::standard_with_themes(dir),
        None => Ui::standard(),
    }
    .context("building the screens")?
    .with_board(board_of(loaded)?)
    .with_widgets(widgets_of(loaded)?);
    let ui = match art_dir {
        Some(dir) => ui.with_art_dir(dir),
        None => ui,
    }
    .with_mail_keys(Arc::new(mail_keys))
    .with_files(files)
    // The doors built into this board (M4-D0): the example coin toss until a game replaces it.
    .with_doors(
        Doors::builtin()
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("building the doors")?,
    );
    // The sysop's menu file and pages (UI-6): `menus.toml` and `pages/` beside the host key. A
    // missing file is the built-in menu; a bad one is logged and refused, and the board boots.
    let ui = match themes_dir.and_then(std::path::Path::parent) {
        Some(root) => ui.with_menu_dir(root),
        None => ui,
    };
    // Theme packs (UI-7): `packs/` beside the host key holds exported and importable packs; an
    // import installs into `themes/` and `art/`. Only when all three directories are known.
    let ui = match (themes_dir, art_dir) {
        (Some(themes), Some(art)) => match themes.parent() {
            Some(root) => ui.with_pack_dirs(russh_bbs_server::ui::PackDirs {
                packs: root.join("packs"),
                themes: themes.to_path_buf(),
                art: art.to_path_buf(),
            }),
            None => ui,
        },
        _ => ui,
    };
    // The store decides who is a member; an unknown key or a store error is Restricted.
    let ui = Arc::new(ui);
    let board_doors = ui.doors().clone();
    let server = SshServer::start_with_ui(
        &config,
        host_key,
        Arc::new(store.clone()),
        Arc::new(founder),
        Arc::clone(&ui),
    )
    .await?;

    // The optional telnet listener (M3-H, D27): off by default, and then no port is bound. It
    // shares the SSH server's session registry, so the session limits span both transports.
    let telnet = if loaded.config.telnet.enabled {
        let telnet_config = TelnetConfig {
            listen: loaded.config.telnet.listen,
            allow_login: loaded.config.telnet.allow_login,
            limits: loaded.config.limits(),
        };
        Some(
            TelnetServer::start(
                &telnet_config,
                Arc::new(store.clone()),
                server.registry(),
                ui,
            )
            .await?,
        )
    } else {
        None
    };

    // Expired invite requests and denial cooldowns are deleted on a timer as well as whenever
    // the queue is used (M2-E, D72).
    let purge = tokio::spawn(russh_bbs_server::sysop::queue::purge_every(
        store.clone(),
        russh_bbs_server::sysop::queue::PURGE_INTERVAL,
    ));

    // The event scheduler (M3-C, D79). Later slices register their jobs here, before it starts:
    // `scheduler.register(job)?` (see the `russh_bbs_core::scheduler` docs).
    let mut scheduler = Scheduler::new(store.clone(), Arc::new(SystemClock));
    // One daily-tick job per door that has days (M4-D0): idempotent per day, catching up after
    // downtime with a cap.
    let door_ticks = doors::register_ticks(&mut scheduler, &store, &board_doors)
        .context("scheduling the doors' daily ticks")?;
    info!(jobs = door_ticks, "door ticks scheduled");
    if loaded.config.feeds.enabled {
        register_feed_job(
            &mut scheduler,
            &store,
            loaded.config.feeds.ca_file.as_deref(),
        )?;
    }
    let rss_out = start_rss_out(loaded, &store).await?;
    let (stop_scheduler, scheduler_stopped) = tokio::sync::oneshot::channel::<()>();
    let scheduler = tokio::spawn(async move {
        scheduler
            .run(async {
                // A dropped sender also means stop.
                let _ = scheduler_stopped.await;
            })
            .await;
    });

    signals
        .wait()
        .await
        .context("waiting for shutdown signal")?;
    info!("shutting down");
    // `abort` only requests the stop: wait until the task is really gone, or it can still be
    // writing when the store's final checkpoint runs (FIX-M4-1).
    purge.abort();
    let _ = purge.await;
    // Stop the scheduler before the store closes, so no job is left writing to a closed pool.
    // `run` returns at once (it drops a tick in progress), so this wait is short.
    let _ = stop_scheduler.send(());
    if let Err(error) = scheduler.await {
        warn!(%error, "the scheduler task ended abnormally");
    }
    if let Some(telnet) = telnet {
        telnet.shutdown().await;
    }
    if let Some(rss_out) = rss_out {
        rss_out.shutdown().await;
    }
    // Closes the sessions, then the store (SQLite checkpoints its WAL, Postgres closes the pool).
    shutdown::graceful(server, &store, shutdown::SHUTDOWN_TIMEOUT).await;
    Ok(())
}

/// The production feed fetcher: system DNS, the platform CA roots, the SSRF guard on every hop.
///
/// A build with the `qa-loopback-feeds` feature (never a release build) also reads
/// `QA_FEED_ALLOW`, a comma-separated list of exact `ip:port` addresses the guard
/// should let through, so a local test server can be exercised over real `ssh`.
fn feed_fetcher(roots: &Roots) -> GuardedFetcher<SystemResolver> {
    #[cfg(feature = "qa-loopback-feeds")]
    let guard = {
        let allowed = std::env::var("QA_FEED_ALLOW")
            .unwrap_or_default()
            .split(',')
            .filter_map(|part| part.trim().parse::<std::net::SocketAddr>().ok())
            .collect::<Vec<_>>();
        if !allowed.is_empty() {
            warn!(
                ?allowed,
                "QA BUILD: the feed guard lets these addresses through"
            );
        }
        Guard::new().allowing(allowed)
    };
    #[cfg(not(feature = "qa-loopback-feeds"))]
    let guard = Guard::new();
    GuardedFetcher::with_roots(SystemResolver, guard, FetchLimits::default(), roots)
}

/// RSS in (M3-G, D89): one job reads the feeds that are due, through the SSRF-guarded fetcher.
/// Only registered when `feeds.enabled`; otherwise the server makes no outbound request for feeds.
fn register_feed_job<S>(
    scheduler: &mut Scheduler<S>,
    store: &S,
    ca_file: Option<&std::path::Path>,
) -> anyhow::Result<()>
where
    S: russh_bbs_core::feeds::FeedStore + SchedulerStore + Clone + 'static,
{
    let proxies = proxy_in_environment();
    if !proxies.is_empty() {
        warn!(
            variables = ?proxies,
            "a proxy is set in the environment and feeds ignore it: a proxy resolves names \
             itself, so the SSRF guard could not check the address it connects to. Feeds \
             connect directly and fail if the network only allows a proxy"
        );
    }
    // `feeds.ca_file` replaces the platform store; an unusable file is a boot error, never a
    // silent fall back to the platform store (D92).
    let roots = match ca_file {
        Some(path) => Roots::Only(
            load_ca_file(path)
                .with_context(|| format!("reading feeds.ca_file {}", path.display()))?,
        ),
        None => Roots::Platform,
    };
    let fetcher = Arc::new(feed_fetcher(&roots));
    scheduler
        .register(
            russh_bbs_server::feeds::job(store.clone(), fetcher)
                .context("building the feed job")?,
        )
        .context("registering the feed job")?;
    Ok(())
}

/// RSS out (M3-G): a small HTTP listener for boards the sysop marked public. Off by default;
/// then no port is bound.
async fn start_rss_out<S>(loaded: &Loaded, store: &S) -> anyhow::Result<Option<RssOut>>
where
    S: Backend + Clone,
{
    let feeds = &loaded.config.feeds;
    if !feeds.rss_out_enabled {
        return Ok(None);
    }
    // Validation guarantees the base address when this is on.
    let config = RssOutConfig {
        listen: feeds.rss_out_listen,
        base_url: feeds.rss_out_base_url.clone().unwrap_or_default(),
    };
    Ok(Some(
        RssOut::start(&config, Arc::new(store.clone()))
            .await
            .context("starting RSS out")?,
    ))
}
