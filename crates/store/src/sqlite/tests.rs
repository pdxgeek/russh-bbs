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
use sqlx::{Connection as _, Row as _, SqlitePool};
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

    async fn texts(&self, sql: &str) -> Vec<String> {
        sqlx::query_scalar::<_, String>(sql)
            .fetch_all(self.store.pool())
            .await
            .unwrap()
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
    (
        5,
        "9ecd872b24dcef690f2a366d025b4566df941aa81d8e816b99e8f285cee5c39ffb8dffbdce35b432a7f7213de73f4c8d",
    ),
    (
        6,
        "b6b8bb05d697513a30a254b0f1aee5c2579dacd330f66952b872398680fa94f7e58a94fbc8cbccfa511be5ada8098f30",
    ),
    (
        7,
        "0c98a1383826cbf44f87f15e0bf4a091bbc78a72ee98ee0841911afec3497e5e43368afdb59526c15de1355dfd351939",
    ),
    (
        9,
        "33521e762ad7c2b971076b86f65768d45d20cb7838b60f36e55644d2b06592275391d74741bc9409d2b6887bf2b5ac6a",
    ),
    (
        10,
        "9c2d7b7dc4e0aaf578cc59e9b6bbd4ef2f816f0d703a4d4e4262f44b51e038e742b77057a8628e4416ffcbde7e443a96",
    ),
    (
        11,
        "03772cd89de1810db7a6441f13e4c7b8b47cf7444dbd796fc247c4390fd5c1263a41c2813820f5eee94da9042efd06ee",
    ),
    (
        12,
        "b24288ca59bb18b97335404fc067291e0f7ae6207d35ae7d431b64fd6bd112b8c04113266db08b2626e14bcf0bedac6f",
    ),
    (
        13,
        "4472b15b07fabcefe71576b3f788c86ad5ddf1207cb42d344e0d779294d650c5469f9fd85e30159cf284e853e1e15118",
    ),
    (
        14,
        "d098408186779f2898926d5fdd73dd5cbf2e5b88f35aaf9fcb54087613ab5aaf633dafbb4130f35c8404a9f337853b86",
    ),
    (
        15,
        "54e726f76f6cfdedbb5dea6469c4ec93ec0523d39393f85cd7689d6f3424ccb2a0802419e396554723086a49a35700f9",
    ),
    (
        16,
        "f97841a321ab60625b96b1219b74ea61f541e6b7884deb2b97d7576de852bc2578c84beabb099e8bab42ae7b1594f346",
    ),
    (
        17,
        "7e0d54c599f9671827a93667c557dde65b6e5da37c7079372c1a778a94242b040c496b88534fd5a39b5e0a42e685b48e",
    ),
    (
        18,
        "c9781cb099a6dc0df8c13bd4db68078944be4cd1efaa05f054d4d9ecbe89e51a62a3b2e6b0d208ed38d57d02ad56247e",
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
            "account_password",
            "accounts",
            "audit_log",
            "board",
            "board_public",
            "chat_room",
            "community_line",
            "credit_ledger",
            "draft",
            "feed",
            "feed_item",
            "group_closure",
            "group_member",
            "invite",
            "invite_request",
            "invite_request_vouch",
            "last_caller",
            "limit_usage",
            "member_group",
            "member_limit",
            "poll",
            "poll_option",
            "poll_vote",
            "post",
            "post_fts",
            "post_fts_config",
            "post_fts_data",
            "post_fts_docsize",
            "post_fts_idx",
            "profiles",
            "registration_day",
            "request_denial",
            "role",
            "role_limit",
            "role_permission",
            "scheduler_job",
            "settings",
            "terminal_settings",
            "thread"
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
    // (same install times), the later migrations (M2-B, M2-C, M2-F) are applied on top, data intact.
    let store = SqliteStore::open(&path).await.unwrap();
    let after = recorded(store.pool().clone()).await;
    assert_eq!(after[..2], before[..]);
    assert_eq!(after.len(), MIGRATOR.iter().count());
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
    let rows =
        russh_bbs_core::audit::known_events(store.page(&AuditQuery::default()).await.unwrap());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event, original);
}

/// SQLite only: an `INSERT OR REPLACE` that reuses an id deletes the old row without firing
/// the delete trigger, so the ledger has the same third trigger as the audit log (migration 0011).
#[tokio::test]
async fn credit_ledger_refuses_replace_and_upsert_by_id() {
    use russh_bbs_core::access::Slug;
    use russh_bbs_core::credits::{Amount, Movement, balance, post};
    use russh_bbs_core::sanitize::sanitize_username;
    use russh_bbs_core::store::{AccountState, AccountStore, NewAccount};

    let (_dir, store) = temp_store().await;
    let a = store
        .create_account(NewAccount {
            username: sanitize_username("ann").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    let why = Slug::parse("test-grant").unwrap();
    let posted = post(&store, a.id, Movement::Earn(Amount::new(10).unwrap()), &why)
        .await
        .unwrap();
    for sql in [
        format!(
            "INSERT OR REPLACE INTO credit_ledger (id, account_id, at, delta, reason) \
             VALUES ({}, 1, 1, 999, 'x')",
            posted.id.as_raw()
        ),
        format!(
            "REPLACE INTO credit_ledger (id, account_id, at, delta, reason) \
             VALUES ({}, 1, 1, 999, 'x')",
            posted.id.as_raw()
        ),
        format!(
            "INSERT INTO credit_ledger (id, account_id, at, delta, reason) \
             VALUES ({}, 1, 1, 999, 'x') ON CONFLICT (id) DO UPDATE SET delta = excluded.delta",
            posted.id.as_raw()
        ),
    ] {
        let error = sqlx::query(&sql).execute(store.pool()).await.unwrap_err();
        assert!(error.to_string().contains("append-only"), "{sql}: {error}");
    }
    assert_eq!(balance(&store, a.id).await.unwrap(), 10);
}

// ---- migration 0007: the accounts rebuild (M2-D) -----------------------------------------

/// Migrations below 0007, as a migrator: the database as the code before M2-D left it.
fn before_invites() -> Migrator {
    let mut older = Migrator::DEFAULT;
    older.migrations =
        std::borrow::Cow::Owned(MIGRATOR.iter().filter(|m| m.version < 7).cloned().collect());
    older
}

async fn dump(pool: &SqlitePool) -> Vec<(&'static str, Vec<String>)> {
    let mut out = Vec::new();
    for (name, sql) in crate::migration_fixture::DUMPS {
        let rows: Vec<String> = sqlx::query_scalar(sql).fetch_all(pool).await.unwrap();
        out.push((*name, rows));
    }
    out
}

/// A database at the last pre-0007 version, filled with rows from every earlier slice. The
/// connection has foreign keys ON, as the pool of the code that wrote these rows did.
async fn seeded_before_invites(path: &std::path::Path) -> Vec<(&'static str, Vec<String>)> {
    let pool = scratch_pool(path).await;
    before_invites().run(&pool).await.unwrap();
    for sql in crate::migration_fixture::SEED {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    let before = dump(&pool).await;
    // The forcing check: every table that references `accounts` must have rows in the fixture,
    // or the comparison below says nothing about it. A slice that adds such a table (profiles,
    // limits, ...) must add a row to `migration_fixture::SEED` and a read to `DUMPS`.
    let referencing: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT m.name FROM sqlite_master m, pragma_foreign_key_list(m.name) f \
         WHERE m.type = 'table' AND f.\"table\" = 'accounts' ORDER BY m.name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    for table in referencing {
        let rows = before.iter().find(|(name, _)| *name == table);
        assert!(
            rows.is_some_and(|(_, rows)| !rows.is_empty()),
            "table `{table}` references accounts but the migration fixture has no rows for it: \
             add a seed row to migration_fixture::SEED and a read to DUMPS, so the accounts \
             rebuild is proven not to cascade into it"
        );
    }
    pool.close().await;
    before
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one story: seed, migrate, compare, then use the result
async fn the_accounts_rebuild_keeps_every_row_id_and_dependent_table() {
    use crate::migration_fixture::{KEY_ALICE, KEY_BOB, KEY_DAVE, NEXT_ACCOUNT_ID};
    use russh_bbs_core::access::{AccessStore as _, Fingerprint};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m2c.db");
    let before = seeded_before_invites(&path).await;
    let count = |table: &str| before.iter().find(|(n, _)| *n == table).unwrap().1.len();
    // The fixture really has something in every dependent table, or the comparison proves nothing.
    assert_eq!(count("accounts"), 4);
    for table in [
        "account_keys",
        "terminal_settings",
        "account_logins",
        "access_grant",
        "group_member",
        "group_closure",
        "audit_log",
    ] {
        assert!(count(table) > 0, "{table} is empty in the fixture");
    }
    assert!(count("access_grant") >= 4, "grants of every shape");

    let store = SqliteStore::open(&path).await.unwrap();
    let after = dump(store.pool()).await;
    assert_eq!(
        after, before,
        "a row changed, vanished or was cascaded away"
    );

    // The new shape: no `role`, an `invited_by` that is empty for everyone made before.
    let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('accounts')")
        .fetch_all(store.pool())
        .await
        .unwrap();
    assert_eq!(
        columns,
        [
            "id",
            "username",
            "username_skeleton",
            "state",
            "created_at",
            "invited_by"
        ]
    );
    let invited: i64 =
        sqlx::query_scalar("SELECT count(*) FROM accounts WHERE invited_by IS NOT NULL")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(invited, 0);

    // Nothing dangles, foreign keys are on for every pooled connection, and the tables that
    // reference `accounts` reference the new one (they were never rebuilt, so they still name it).
    let dangling = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(store.pool())
        .await
        .unwrap();
    assert!(dangling.is_empty());
    let on: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(on, 1);
    for table in [
        "account_keys",
        "terminal_settings",
        "account_logins",
        "access_grant",
        "group_member",
        "group_closure",
        "invite",
    ] {
        let parents: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT \"table\" FROM pragma_foreign_key_list('{table}')"
        ))
        .fetch_all(store.pool())
        .await
        .unwrap();
        assert!(
            parents.iter().any(|p| p == "accounts"),
            "{table}: {parents:?}"
        );
        assert!(
            !parents.iter().any(|p| p.ends_with("_new")),
            "{table}: {parents:?}"
        );
    }
    let sql: String = sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name = 'accounts'")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert!(!sql.contains("accounts_new"), "{sql}");

    // The old accounts still work.
    let account = |key: &str| {
        let store = store.clone();
        let key = Fingerprint::parse(key).unwrap();
        async move { store.account_by_fingerprint(&key).await.unwrap().unwrap() }
    };
    let (alice, bob, dave) = (
        account(KEY_ALICE).await,
        account(KEY_BOB).await,
        account(KEY_DAVE).await,
    );
    let top = |id| {
        let store = store.clone();
        async move { store.snapshot(Some(id)).await.unwrap().top_role().cloned() }
    };
    assert_eq!(top(alice.id).await, Some(Slug::SYSOP));
    assert_eq!(
        top(bob.id).await.map(|r| r.as_str().to_owned()).as_deref(),
        Some("moderator")
    );
    assert_eq!(
        top(dave.id).await,
        Some(Slug::WAITING),
        "suspended still waits"
    );

    // Ids are never reused: the next account follows the burned `ghost` id.
    let next = store
        .create_account(NewAccount {
            username: sanitize_username("erin").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    assert_eq!(next.id.as_raw(), NEXT_ACCOUNT_ID);

    // The foreign keys still act: deleting bob takes his own rows and nothing else.
    sqlx::query("DELETE FROM accounts WHERE username = 'bob'")
        .execute(store.pool())
        .await
        .unwrap();
    for (table, left) in [
        ("account_keys", 2),
        ("account_logins", 0),
        ("group_member", 0),
        ("group_closure", 0),
    ] {
        let rows: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert!(rows >= left, "{table}");
    }
    let alice_keys: i64 =
        sqlx::query_scalar("SELECT count(*) FROM account_keys WHERE account_id = 1")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(alice_keys, 1);
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert!(audit >= 3, "audit rows are not touched by account deletion");
}

/// Why the migrations run on a connection with foreign keys off: with them on, the rebuild's
/// `DROP TABLE accounts` deletes every row first and every `ON DELETE CASCADE` follows. This
/// runs the migration SQL by hand on a connection with foreign keys on, as the pool would have,
/// and shows the damage, so nobody "simplifies" `migrate` back to the pool.
#[tokio::test]
async fn the_rebuild_would_destroy_data_with_foreign_keys_on() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unsafe.db");
    seeded_before_invites(&path).await;
    let pool = scratch_pool(&path).await; // foreign keys ON, like every pooled connection
    let sql: String = include_str!("../../migrations/sqlite/0007_invites.sql")
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    // Statement by statement, in one transaction: what sqlx does with a migration.
    let mut tx = pool.begin().await.unwrap();
    for statement in sql.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        sqlx::query(statement).execute(&mut *tx).await.unwrap();
    }
    let keys: i64 = sqlx::query_scalar("SELECT count(*) FROM account_keys")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        keys, 0,
        "the cascade deleted the keys: this is what foreign_keys=OFF prevents"
    );
    tx.rollback().await.unwrap();
}

/// The check at the end of the migrations is not decoration: a database with a row whose foreign
/// key points nowhere is refused, not served.
#[tokio::test]
async fn a_database_left_with_a_dangling_foreign_key_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dangling.db");
    seeded_before_invites(&path).await;
    // Plant an orphan the way a buggy migration would have: foreign keys off.
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .foreign_keys(false);
    let mut conn = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO account_keys (fingerprint, account_id) \
         VALUES ('SHA256:CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC', 999)",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    conn.close().await.unwrap();
    let error = SqliteStore::open(&path).await.unwrap_err();
    assert!(
        matches!(error, crate::OpenError::ForeignKeys { rows: 1 }),
        "{error:?}"
    );
}
