//! SQLite harness for the shared suite, plus the checks that need the SQLite file: PRAGMAs,
//! WAL, file modes, corrupt rows, cascades, and databases created by earlier code.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;

use russh_bbs_core::access::{MemberDirectory, Slug};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountState, AccountStore, NewAccount, StoreError};
use russh_bbs_core::terminal_settings::{TerminalSettings, TerminalSettingsStore};
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Row as _, SqlitePool};
use tempfile::TempDir;

use super::{MIGRATOR, SqliteStore};
use crate::suite::{self, Harness, fingerprint};

struct SqliteHarness {
    dir: TempDir,
    store: SqliteStore,
}

impl SqliteHarness {
    fn path(&self) -> std::path::PathBuf {
        self.dir.path().join("bbs.db")
    }
}

impl Harness for SqliteHarness {
    type Store = SqliteStore;

    async fn fresh() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
        Self { dir, store }
    }

    fn store(&self) -> &SqliteStore {
        &self.store
    }

    async fn reopen(&mut self) {
        self.store.close().await;
        self.store = SqliteStore::open(&self.path()).await.unwrap();
    }

    async fn close(&self) {
        self.store.close().await;
    }

    fn set_clock(&mut self, clock: std::sync::Arc<dyn russh_bbs_core::audit::WallClock>) {
        self.store = self.store.clone().with_clock(clock);
    }

    async fn raw(&self, sql: &str) -> Result<u64, String> {
        sqlx::query(sql)
            .execute(self.store.pool())
            .await
            .map(|done| done.rows_affected())
            .map_err(|error| error.to_string())
    }

    async fn break_audit_inserts(&self) {
        self.raw(
            "CREATE TRIGGER audit_insert_broken BEFORE INSERT ON audit_log
             BEGIN SELECT RAISE(ABORT, 'audit insert broken'); END",
        )
        .await
        .unwrap();
    }
}

suite::shared_suite!(shared, SqliteHarness, allow(unused));

async fn temp_store() -> (TempDir, SqliteStore) {
    let h = SqliteHarness::fresh().await;
    (h.dir, h.store)
}

/// SHA-384 of every landed migration, in version order, as hex. **Append only.** A migration
/// that has landed on a branch others build from must never change; add a new numbered file
/// instead. This test is the "no editing landed migrations" rule from AGENTS.md.
const LANDED_CHECKSUMS: &[(i64, &str)] = &[
    (
        1,
        "3c94eba4f5f0a5b6c23bf6bacc296ad5b992162c0e67b9fb9ae8638a6abd4356909a1ee095bb45837bf12943936fdb2b",
    ),
    (
        2,
        "3ae29f30adad2c56599553cbe66dd207e6d14f1c51259c1b14390438eb3223e1865afc5e5df34e89700fba52ba1f0b66",
    ),
    (
        3,
        "6addf0696c99af4807e315c720136c0e847dc6f1709f1a9657b55a51bdf5bb40a0f4ec23c2becd98740e33a41157b05d",
    ),
    (
        4,
        "dc0b92309b186cd248415cffe0fe6c407afea360a22ed0525bcc652242ddaae8d39f41ac2235f05d8a1cb832214f0f9d",
    ),
];

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
    assert_eq!(
        tables,
        [
            "access_grant",
            "account_keys",
            "account_logins",
            "accounts",
            "audit_log",
            "group_closure",
            "group_member",
            "member_group",
            "role",
            "role_permission",
            "settings",
            "terminal_settings"
        ]
    );
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
        matches!(result, Err(crate::error::OpenError::Migrate(_))),
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

#[tokio::test]
async fn corrupt_stored_values_are_reported_not_trusted() {
    let (_dir, store) = temp_store().await;
    let alice = store
        .create_account(NewAccount {
            username: sanitize_username("alice").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    store
        .attach_fingerprint(alice.id, &fingerprint(1))
        .await
        .unwrap();
    // Bypass the CHECK constraint the way a future/buggy writer might: rename a username to
    // something the sanitizer rejects.
    sqlx::query("UPDATE accounts SET username = 'bad name'")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store.account_by_fingerprint(&fingerprint(1)).await;
    assert!(matches!(result, Err(StoreError::Corrupt(_))), "{result:?}");
    assert!(
        !store.is_member(&fingerprint(1)).await,
        "corrupt row must fail closed"
    );
}

async fn account(store: &SqliteStore, name: &str) -> russh_bbs_core::store::Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn a_row_the_code_could_not_have_written_is_corrupt_not_a_default() {
    let (_dir, store) = temp_store().await;
    let alice = account(&store, "alice").await;
    // The CHECK constraints stop most bad values, so write the ones they allow but core rejects.
    sqlx::query(
        "INSERT INTO terminal_settings (account_id, width_override, color, pause) VALUES (?, 5, 1, 1)",
    )
    .bind(alice.id.as_raw())
    .execute(store.pool())
    .await
    .unwrap();
    let result = store.terminal_settings(alice.id).await;
    assert!(matches!(result, Err(StoreError::Corrupt(_))), "{result:?}");
}

#[tokio::test]
async fn the_database_refuses_an_unknown_tier_or_flag() {
    let (_dir, store) = temp_store().await;
    let alice = account(&store, "alice").await;
    for (color, tier) in [(2, "ascii"), (1, "emoji")] {
        let result = sqlx::query(
            "INSERT INTO terminal_settings (account_id, color, pause, icon_tier) VALUES (?, ?, 1, ?)",
        )
        .bind(alice.id.as_raw())
        .bind(color)
        .bind(tier)
        .execute(store.pool())
        .await;
        assert!(result.is_err(), "color={color} tier={tier}");
    }
}

#[tokio::test]
async fn deleting_an_account_deletes_its_settings_and_logins() {
    let (_dir, store) = temp_store().await;
    let alice = account(&store, "alice").await;
    store
        .set_terminal_settings(alice.id, &TerminalSettings::default().safe_fallback())
        .await
        .unwrap();
    store.record_login(alice.id).await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE id = ?")
        .bind(alice.id.as_raw())
        .execute(store.pool())
        .await
        .unwrap();
    for table in ["terminal_settings", "account_logins"] {
        let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
}

/// sqlx records a migration's version and SHA-384 checksum, not its path. So a database made by
/// the code before the restructure (migrations in a flat `migrations/`) must open with the new
/// layout (`migrations/sqlite/`) and re-apply nothing. This builds such a database the old way:
/// the same SQL text, loaded from a flat directory by a runtime `Migrator`, which is exactly
/// what the old embedded `migrate!` recorded. `landed_migrations_are_unchanged` pins that this
/// text hashes to the checksums the old code recorded.
#[tokio::test]
async fn a_database_made_before_the_restructure_still_opens_and_keeps_its_data() {
    let dir = tempfile::tempdir().unwrap();
    let old_layout = dir.path().join("migrations");
    std::fs::create_dir(&old_layout).unwrap();
    std::fs::write(
        old_layout.join("0001_initial.sql"),
        include_str!("../../migrations/sqlite/0001_initial.sql"),
    )
    .unwrap();
    std::fs::write(
        old_layout.join("0002_terminal_settings.sql"),
        include_str!("../../migrations/sqlite/0002_terminal_settings.sql"),
    )
    .unwrap();
    let path = dir.path().join("old.db");
    let pool = scratch_pool(&path).await;
    Migrator::new(old_layout.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO accounts (username, username_skeleton, role, state) \
         VALUES ('alice', 'alice', 'sysop', 'active')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE settings SET value = 'open' WHERE key = 'registration_mode'")
        .execute(&pool)
        .await
        .unwrap();
    let recorded = |pool: SqlitePool| async move {
        sqlx::query("SELECT version, hex(checksum), CAST(installed_on AS TEXT) FROM _sqlx_migrations ORDER BY version")
            .map(|r: sqlx::sqlite::SqliteRow| (r.get::<i64, _>(0), r.get::<String, _>(1), r.get::<String, _>(2)))
            .fetch_all(&pool)
            .await
            .unwrap()
    };
    let before = recorded(pool.clone()).await;
    assert_eq!(before.len(), 2);
    pool.close().await;

    // The new code opens it: no VersionMismatch, the two landed migrations are not re-applied
    // (same install times), the M2-B and M2-C migrations are applied on top, data intact.
    let store = SqliteStore::open(&path).await.unwrap();
    let after = recorded(store.pool().clone()).await;
    assert_eq!(after[..2], before[..]);
    assert_eq!(after.len(), 4);
    // The M1 sysop (legacy role column) is backfilled with a `sysop` grant (M2-B).
    let sysops: Vec<String> = sqlx::query_scalar(
        "SELECT a.username FROM access_grant g JOIN accounts a ON a.id = g.account_id \
         WHERE g.role = 'sysop'",
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(sysops, ["alice"]);
    let mode = russh_bbs_core::store::registration_mode(&store)
        .await
        .unwrap();
    assert_eq!(mode, russh_bbs_core::store::RegistrationMode::Open);
    let name: String = sqlx::query_scalar("SELECT username FROM accounts")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(name, "alice");
}

/// Things only SQLite can be asked: an `INSERT OR REPLACE` that reuses an id deletes the old row
/// without firing the delete trigger, so a third trigger refuses any insert that reuses an id.
#[tokio::test]
async fn audit_log_refuses_replace_and_upsert_by_id() {
    use russh_bbs_core::audit::{Action, Actor, AuditEvent, AuditQuery, AuditStore, Target};

    let (_dir, store) = temp_store().await;
    let original =
        AuditEvent::new(Actor::System, Action::GroupCreated, Target::None).detail("orig");
    let id = store.append(&original).await.unwrap();
    for sql in [
        format!(
            "INSERT OR REPLACE INTO audit_log (id, at, actor_kind, action) \
             VALUES ({}, 1, 'system', 'group.deleted')",
            id.as_raw()
        ),
        format!(
            "REPLACE INTO audit_log (id, at, actor_kind, action) \
             VALUES ({}, 1, 'system', 'group.deleted')",
            id.as_raw()
        ),
        format!(
            "INSERT INTO audit_log (id, at, actor_kind, action) \
             VALUES ({}, 1, 'system', 'group.deleted') \
             ON CONFLICT (id) DO UPDATE SET action = excluded.action",
            id.as_raw()
        ),
    ] {
        let error = sqlx::query(&sql).execute(store.pool()).await.unwrap_err();
        assert!(error.to_string().contains("append-only"), "{sql}: {error}");
    }
    let rows = store.page(&AuditQuery::default()).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event, original);
}

/// A row the code could not have written is `Corrupt`, never skipped or guessed.
#[tokio::test]
async fn a_hand_written_audit_row_with_an_unknown_action_is_corrupt() {
    use russh_bbs_core::audit::{AuditQuery, AuditStore};
    use russh_bbs_core::store::StoreError;

    let (_dir, store) = temp_store().await;
    sqlx::query("INSERT INTO audit_log (at, actor_kind, action) VALUES (1, 'system', 'made.up')")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(matches!(
        store.page(&AuditQuery::default()).await,
        Err(StoreError::Corrupt(_))
    ));
}
