//! The russh-bbs server binary: load the configuration, load the host key, start the SSH server.
//! Everything testable lives in the library; this file only wires it together.

use std::sync::Arc;

use anyhow::Context as _;
use clap::Parser;
use russh_bbs_core::limits::SystemClock;
use russh_bbs_core::scheduler::{Scheduler, SchedulerStore};
use russh_bbs_core::store::{SettingsStore, set_registration_mode};
use russh_bbs_server::config::{Cli, Loaded, ResolvedDatasource};
use russh_bbs_server::founder;
use russh_bbs_server::shutdown;
use russh_bbs_server::ssh::{Backend, HostKey, ServerConfig, SshServer};
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
    let key_path = paths.host_key.clone();
    let host_key = tokio::task::spawn_blocking(move || HostKey::load_or_create(&key_path))
        .await
        .context("host key task panicked")??;
    // The fingerprint is public: the sysop publishes it so users can verify the first connect.
    info!(path = %paths.host_key.display(), fingerprint = %host_key.fingerprint(), "host key ready");

    // The datasource is chosen by configuration and never falls back (D6, D38): a Postgres
    // that cannot be reached or verified is a boot error, not a SQLite file.
    match &paths.datasource {
        ResolvedDatasource::Sqlite(path) => {
            let store = SqliteStore::open(path)
                .await
                .with_context(|| format!("opening database {}", path.display()))?;
            info!(backend = "sqlite", path = %path.display(), "database ready");
            serve(store, &loaded, &host_key, themes_dir.as_deref()).await?;
        }
        ResolvedDatasource::Postgres(settings) => {
            // The error text never contains the password (see `PostgresSettings`).
            let store = PostgresStore::open(settings)
                .await
                .context("opening the postgres datasource (TLS is required)")?;
            info!(backend = "postgres", tls = true, "database ready");
            serve(store, &loaded, &host_key, themes_dir.as_deref()).await?;
        }
    }
    Ok(())
}

/// Run the board on `store` until a shutdown signal. Everything past store selection is
/// backend-independent (core traits only).
async fn serve<S: Backend + SettingsStore + SchedulerStore + Close + Clone>(
    store: S,
    loaded: &Loaded,
    host_key: &HostKey,
    themes_dir: Option<&std::path::Path>,
) -> anyhow::Result<()> {
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
    .context("building the screens")?;
    // The store decides who is a member; an unknown key or a store error is Restricted.
    let server = SshServer::start_with_ui(
        &config,
        host_key,
        Arc::new(store.clone()),
        Arc::new(founder),
        Arc::new(ui),
    )
    .await?;

    // Expired invite requests and denial cooldowns are deleted on a timer as well as whenever
    // the queue is used (M2-E, D72).
    let purge = tokio::spawn(russh_bbs_server::sysop::queue::purge_every(
        store.clone(),
        russh_bbs_server::sysop::queue::PURGE_INTERVAL,
    ));

    // The event scheduler (M3-C, D79). Later slices register their jobs here, before it starts:
    // `scheduler.register(job)?` (see the `russh_bbs_core::scheduler` docs). It has none yet.
    let scheduler = Scheduler::new(store.clone(), Arc::new(SystemClock));
    let (stop_scheduler, scheduler_stopped) = tokio::sync::oneshot::channel::<()>();
    let scheduler = tokio::spawn(async move {
        scheduler
            .run(async {
                // A dropped sender also means stop.
                let _ = scheduler_stopped.await;
            })
            .await;
    });

    shutdown::wait_for_signal()
        .await
        .context("waiting for shutdown signal")?;
    info!("shutting down");
    purge.abort();
    // Stop the scheduler before the store closes, so no job is left writing to a closed pool.
    // `run` returns at once (it drops a tick in progress), so this wait is short.
    let _ = stop_scheduler.send(());
    if let Err(error) = scheduler.await {
        warn!(%error, "the scheduler task ended abnormally");
    }
    // Closes the sessions, then the store (SQLite checkpoints its WAL, Postgres closes the pool).
    shutdown::graceful(server, &store, shutdown::SHUTDOWN_TIMEOUT).await;
    Ok(())
}
