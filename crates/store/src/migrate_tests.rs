//! Migration behaviour: applied once, re-run is a no-op, landed migrations cannot be edited,
//! and the connection pragmas the design relies on are on.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;

use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Row as _, SqlitePool};

use crate::SqliteStore;
use crate::sqlite::MIGRATOR;
use crate::tests::temp_store;

/// SHA-384 of every landed migration, in version order, as hex. **Append only.** A migration
/// that has landed on a branch others build from must never change; add a new numbered file
/// instead. This test is the "no editing landed migrations" rule from AGENTS.md.
const LANDED_CHECKSUMS: &[(i64, &str)] = &[(
    1,
    "3c94eba4f5f0a5b6c23bf6bacc296ad5b992162c0e67b9fb9ae8638a6abd4356909a1ee095bb45837bf12943936fdb2b",
)];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, b| {
        write!(out, "{b:02x}").unwrap();
        out
    })
}

#[test]
fn landed_migrations_are_unchanged() {
    let actual: Vec<(i64, String)> = MIGRATOR
        .iter()
        .map(|m| (m.version, hex(&m.checksum)))
        .collect();
    let expected: Vec<(i64, String)> = LANDED_CHECKSUMS
        .iter()
        .map(|(v, c)| (*v, (*c).to_owned()))
        .collect();
    assert_eq!(
        actual, expected,
        "a landed migration changed, or a new one was added without recording its checksum"
    );
}

#[test]
fn migration_versions_are_strictly_increasing() {
    let versions: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
    assert!(versions.windows(2).all(|w| w[0] < w[1]), "{versions:?}");
}

#[tokio::test]
async fn a_fresh_database_gets_every_migration() {
    let (_dir, store) = temp_store().await;
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(store.pool())
            .await
            .unwrap();
    let expected: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
    assert_eq!(versions, expected);
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE '\\_%' ESCAPE '\\' \
         AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(tables, ["account_keys", "accounts", "settings"]);
}

/// Version and install time of every applied migration, as text.
async fn installed(store: &SqliteStore) -> Vec<(i64, String)> {
    sqlx::query("SELECT version, CAST(installed_on AS TEXT) FROM _sqlx_migrations ORDER BY version")
        .map(|r: sqlx::sqlite::SqliteRow| (r.get(0), r.get(1)))
        .fetch_all(store.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn reopening_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bbs.db");
    let first = SqliteStore::open(&path).await.unwrap();
    let before = installed(&first).await;
    first.close().await;

    let second = SqliteStore::open(&path).await.unwrap();
    let after = installed(&second).await;
    assert_eq!(before, after, "no migration may be re-applied");
}

#[tokio::test]
async fn data_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bbs.db");
    let store = SqliteStore::open(&path).await.unwrap();
    russh_bbs_core::store::SettingsStore::set_setting(
        &store,
        russh_bbs_core::store::SettingKey::FounderClaimed,
        "true",
    )
    .await
    .unwrap();
    store.close().await;
    let again = SqliteStore::open(&path).await.unwrap();
    let value = russh_bbs_core::store::SettingsStore::get_setting(
        &again,
        russh_bbs_core::store::SettingKey::FounderClaimed,
    )
    .await
    .unwrap();
    assert_eq!(value.as_deref(), Some("true"));
}

async fn scratch_pool(path: &std::path::Path) -> SqlitePool {
    SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn editing_an_applied_migration_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let migrations = dir.path().join("migrations");
    std::fs::create_dir(&migrations).unwrap();
    std::fs::write(migrations.join("0001_a.sql"), "CREATE TABLE a (x INTEGER);").unwrap();
    let pool = scratch_pool(&dir.path().join("t.db")).await;

    Migrator::new(migrations.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();

    std::fs::write(
        migrations.join("0001_a.sql"),
        "CREATE TABLE a (x INTEGER, y TEXT);",
    )
    .unwrap();
    let result = Migrator::new(migrations.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await;
    assert!(
        matches!(result, Err(MigrateError::VersionMismatch(1))),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_database_from_a_newer_binary_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bbs.db");
    let store = SqliteStore::open(&path).await.unwrap();
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (9999, 'from the future', 1, x'00', 0)",
    )
    .execute(store.pool())
    .await
    .unwrap();
    store.close().await;
    let result = SqliteStore::open(&path).await;
    assert!(
        matches!(result, Err(crate::OpenError::Migrate(_))),
        "{result:?}"
    );
}

#[tokio::test]
async fn wal_and_foreign_keys_are_on() {
    let (_dir, store) = temp_store().await;
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(mode, "wal");
    let fks: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(fks, 1);
}

#[tokio::test]
async fn foreign_keys_are_enforced_on_every_pooled_connection() {
    let (_dir, store) = temp_store().await;
    // Hold several connections at once so the pool opens more than one.
    let mut held = Vec::new();
    for _ in 0..3 {
        held.push(store.pool().acquire().await.unwrap());
    }
    for conn in &mut held {
        let fks: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut **conn)
            .await
            .unwrap();
        assert_eq!(fks, 1);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn the_database_file_and_new_directory_are_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let path = data.join("bbs.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let dir_mode = std::fs::metadata(&data).unwrap().permissions().mode() & 0o777;
    assert_eq!(file_mode, 0o600);
    assert_eq!(dir_mode, 0o700);
    store.close().await;
}
