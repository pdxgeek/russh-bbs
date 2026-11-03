//! Postgres harness for the shared suite, plus the checks that are Postgres-only: TLS, URL
//! handling, error text, migrations and corrupt rows (D24, D31, D32).
//!
//! Tests that need a server are `#[ignore]`d so a plain `cargo test --workspace` needs no
//! Postgres; CI runs them with `-- --ignored` and **fails** (it does not skip) if the variables
//! below are missing:
//!
//! - `RUSSH_BBS_TEST_POSTGRES_URL`: a `postgres://` URL of a role that may `CREATE DATABASE`.
//!   Each test makes its own database from it and drops it afterwards. The URL's own `sslmode`
//!   and `sslrootcert` apply, so the same suite runs against a plain server and a TLS server.
//!   (`PGlite` through `pglite-socket` was tried and does not work with a pooled sqlx client:
//!   see D44.)
//! - `RUSSH_BBS_TEST_POSTGRES_TLS_URL` and `RUSSH_BBS_TEST_POSTGRES_ROOT_CERT`: a server that
//!   **requires** TLS (non-TLS clients refused) and the PEM that signs its certificate; used by
//!   the production [`PostgresStore::open`] path. The tests that migrate make a scratch database
//!   on it (created over TLS, dropped afterwards) rather than touching the shared one.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::str::FromStr as _;
use std::sync::atomic::{AtomicU32, Ordering};

use russh_bbs_core::store::{AccountStore, SettingKey, SettingsStore, StoreError};
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::postgres::PgConnectOptions;
use sqlx::{Connection as _, Executor as _, PgConnection};

use super::{MIGRATOR, PostgresSettings, PostgresStore, TlsMode};
use crate::error::OpenError;
use crate::suite::{self, Harness};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("{name} is not set; these tests need a Postgres server (see postgres/tests.rs)")
    })
}

/// A scratch database on the server named by `RUSSH_BBS_TEST_POSTGRES_URL`, dropped at the end.
pub(super) struct PgHarness {
    admin: PgConnectOptions,
    options: PgConnectOptions,
    name: String,
    store: PostgresStore,
}

/// `url` with its database name replaced by `database`, keeping the rest (credentials,
/// host, query).
fn url_with_database(url: &str, database: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap();
    let (authority, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let query = tail
        .split_once('?')
        .map_or(String::new(), |(_, q)| format!("?{q}"));
    format!("{scheme}://{authority}/{database}{query}")
}

fn settings_for_tests(url: &str) -> PostgresSettings {
    PostgresSettings::new(url, TlsMode::Require, None)
}

async fn open_test_database(options: PgConnectOptions, url: &str) -> PostgresStore {
    PostgresStore::open_with(options, &settings_for_tests(url))
        .await
        .unwrap()
}

impl Harness for PgHarness {
    type Store = PostgresStore;

    async fn fresh() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let url = env("RUSSH_BBS_TEST_POSTGRES_URL");
        let admin = PgConnectOptions::from_str(&url).unwrap();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let name = format!(
            "bbs_test_{}_{}_{nanos}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let mut conn = PgConnection::connect_with(&admin).await.unwrap();
        // The name is generated above from digits and underscores only.
        conn.execute(format!("CREATE DATABASE {name}").as_str())
            .await
            .unwrap();
        conn.close().await.unwrap();
        let options = admin.clone().database(&name);
        let store = open_test_database(options.clone(), &url).await;
        Self {
            admin,
            options,
            name,
            store,
        }
    }

    fn store(&self) -> &PostgresStore {
        &self.store
    }

    async fn reopen(&mut self) {
        self.store.close().await;
        let url = env("RUSSH_BBS_TEST_POSTGRES_URL");
        self.store = open_test_database(self.options.clone(), &url).await;
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

    async fn hex(&self, column: &str) -> Vec<String> {
        self.texts(&format!(
            "SELECT encode({column}, 'hex') FROM mail_message ORDER BY id"
        ))
        .await
    }

    async fn break_audit_inserts(&self) {
        // A constraint no new row can satisfy; NOT VALID skips the (empty) existing rows.
        self.raw(
            "ALTER TABLE audit_log ADD CONSTRAINT audit_insert_broken CHECK (false) NOT VALID",
        )
        .await
        .unwrap();
    }
}

impl Drop for PgHarness {
    fn drop(&mut self) {
        // Drop cannot be async: finish the cleanup on a short-lived thread with its own runtime.
        let (admin, name) = (self.admin.clone(), self.name.clone());
        let _ = std::thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async {
                if let Ok(mut conn) = PgConnection::connect_with(&admin).await {
                    let _ = conn
                        .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
                        .await;
                }
            });
        })
        .join();
    }
}

suite::shared_suite!(
    shared,
    PgHarness,
    ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"
);

// ---- migrations ----------------------------------------------------------------------------

/// SHA-384 of every landed Postgres migration, in version order, as hex. **Append only**, like
/// the SQLite list (D36): a landed migration never changes; add a new numbered file.
const LANDED_CHECKSUMS: &[(i64, &str)] = &[
    (
        1,
        "33e7a11a42ec4f22a6feb4da840f9140885b0207c390894f8e18196622b0bb6ca56235fe1686fbcf84b724b5e8e559e5",
    ),
    (
        2,
        "fd0ced89bb0415a28f3e7fa2127bd74aa224284f800c5a8acc8b101db8f6f82e39e877c39b4b4f6bd5564a63186c0078",
    ),
    (
        3,
        "30d33c5c10004f05324ac2943ad60731fc2d2779a11faa408b3eda531081b5b90148958a1a603a2c94521b21ab0b6a64",
    ),
    (
        4,
        "dd825623b1277a602de93a586ff964de23e6295be36aed810538127136ba3f2f47dce6db449d36269da7a0beb7031c60",
    ),
    (
        5,
        "e3af2b2e1262291ab3dee3895958b454bea8b70a27b980a12b23caea35ef22055bb6dc92843281d81d021b60fd5e4555",
    ),
    (
        6,
        "5d9c76a2f3ac95187ed3db3b92b68e8b7df7f99aba86d74c9dfe49070b5f7f2e6f963f1308a6bda9b8b0ef5b4c766219",
    ),
    (
        7,
        "5654596047e96eaadd400d261dbf2d19995517d82017170a86288e5a0c161f106541d82001114c50f1c698e79cc5c569",
    ),
    (
        9,
        "28447d9f438d504c5f8d870b39f034c8c4cff5e0f42a0208263440b523e1f18ac33c74e6a76214cb0440611fc002190b",
    ),
    (
        10,
        "811fc48a6f813dc2484a306dac74298bd38b3667b56bc045e25b161dbd21e8b8737f26ca259bc7299e22e2718c1d2a09",
    ),
    (
        11,
        "3ea2880883ca6615d935e49fd8456e48618497e2e76b12657a34072de4e72b1587060bdc0a444dad372781f02f68f176",
    ),
    (
        12,
        "a39aa2c7404ec81315b9e22042454f03fe621cc79f3c129b5015e48ecfe98e56d614885a06132317bfcbd0e723ad85ab",
    ),
    (
        13,
        "31a11d940be480e6aaba7035846a0dfeb1a5f4f6f83989a9a5cd85e3fc8c7f3031adf1553e54b45a8916fe1ef2f42bdc",
    ),
    (
        14,
        "d7431d8dc7edc3cf9a7b0dee7eecac83acecbc3e9ac5a12e47c81793d4aadc983cbfbd4faa830c1d85e0914353cd0101",
    ),
    (
        15,
        "21e6ad1b69d8308df778871a702503e51c9a34b514cf78e2b2de9ace41ee80f21c0c8747e1eedf9b453523742a772b08",
    ),
    (
        16,
        "4af9d1ae47a3d9d594b956ed1ba64b3b346fb579f6ed8ae2d134cbde9534683c43e3b92ffa4560dfd8778395e7b99c95",
    ),
    (
        17,
        "5b4ee1c5b6a7e2c4015c834058588b0405bc384e2026b4bd51356c85f10b10652985327c1dab51be85a1d5539b795f3c",
    ),
    (
        18,
        "1039fe718eb3826b1d0e4a7509adbf20e9ae9f8ac51ba561ee0b86cf7de44d2ec40f9b12e1f3e63d21af8a0872ddc01f",
    ),
    (
        19,
        "248826a55d5250344e181f36ded00023212e7d3a76f48164fa06c4215be1895b1fd57c765bd05a67298ba2feba1e2a9f",
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
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
async fn a_fresh_database_gets_every_migration_and_only_these_tables() {
    let h = PgHarness::fresh().await;
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(h.store().pool())
            .await
            .unwrap();
    let expected: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
    assert_eq!(versions, expected);
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables \
         WHERE table_schema = current_schema() AND table_name NOT LIKE '\\_%' ORDER BY 1",
    )
    .fetch_all(h.store().pool())
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
            "mail_message",
            "mail_report",
            "member_group",
            "member_limit",
            "poll",
            "poll_option",
            "poll_vote",
            "post",
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

/// An M1-era database (migrations 0001 and 0002 only, with a sysop and a member) opens with the
/// M2-B migration applied on top, and the sysop is backfilled with a `sysop` grant.
#[tokio::test]
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
async fn an_m1_database_gets_the_access_migration_and_a_sysop_grant() {
    let h = PgHarness::fresh().await;
    let mut conn = PgConnection::connect_with(&h.options).await.unwrap();
    conn.execute("DROP SCHEMA public CASCADE").await.unwrap();
    conn.execute("CREATE SCHEMA public").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("0001_initial.sql"),
        include_str!("../../migrations/postgres/0001_initial.sql"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("0002_terminal_settings.sql"),
        include_str!("../../migrations/postgres/0002_terminal_settings.sql"),
    )
    .unwrap();
    Migrator::new(dir.path())
        .await
        .unwrap()
        .run(&mut conn)
        .await
        .unwrap();
    conn.execute(
        "INSERT INTO accounts (username, username_skeleton, role, state) VALUES \
         ('alice', 'alice', 'sysop', 'active'), ('bob', 'bob', 'member', 'active')",
    )
    .await
    .unwrap();
    conn.close().await.unwrap();

    let url = env("RUSSH_BBS_TEST_POSTGRES_URL");
    let store = open_test_database(h.options.clone(), &url).await;
    let sysops: Vec<String> = sqlx::query_scalar(
        "SELECT a.username FROM access_grant g JOIN accounts a ON a.id = g.account_id \
         WHERE g.role = 'sysop'",
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert_eq!(sysops, ["alice"]);
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(store.pool())
            .await
            .unwrap();
    let expected: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
    assert_eq!(versions, expected);
}

#[tokio::test]
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
async fn reopening_is_a_no_op() {
    let mut h = PgHarness::fresh().await;
    let installed = |store: PostgresStore| async move {
        sqlx::query_as::<_, (i64, String)>(
            "SELECT version, installed_on::text FROM _sqlx_migrations ORDER BY version",
        )
        .fetch_all(store.pool())
        .await
        .unwrap()
    };
    let before = installed(h.store().clone()).await;
    h.reopen().await;
    assert_eq!(before, installed(h.store().clone()).await);
}

#[tokio::test]
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
async fn editing_an_applied_migration_is_refused() {
    let h = PgHarness::fresh().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("0001_a.sql"), "CREATE TABLE a (x INTEGER);").unwrap();
    // A scratch schema keeps this set apart from the store's own `_sqlx_migrations`.
    let mut conn = PgConnection::connect_with(&h.options).await.unwrap();
    conn.execute("CREATE SCHEMA scratch").await.unwrap();
    conn.execute("SET search_path = scratch").await.unwrap();
    let migrator = Migrator::new(dir.path()).await.unwrap();
    migrator.run_direct(&mut conn).await.unwrap();

    std::fs::write(
        dir.path().join("0001_a.sql"),
        "CREATE TABLE a (x INTEGER, y TEXT);",
    )
    .unwrap();
    let result = Migrator::new(dir.path())
        .await
        .unwrap()
        .run_direct(&mut conn)
        .await;
    assert!(
        matches!(result, Err(MigrateError::VersionMismatch(1))),
        "{result:?}"
    );
}

#[tokio::test]
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
async fn a_database_from_a_newer_binary_is_refused() {
    let h = PgHarness::fresh().await;
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (9999, 'from the future', true, '\\x00', 0)",
    )
    .execute(h.store().pool())
    .await
    .unwrap();
    h.store().close().await;
    let url = env("RUSSH_BBS_TEST_POSTGRES_URL");
    let result = PostgresStore::open_with(h.options.clone(), &settings_for_tests(&url)).await;
    assert!(matches!(result, Err(OpenError::Migrate(_))), "{result:?}");
}

// ---- corrupt rows --------------------------------------------------------------------------

#[tokio::test]
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
async fn corrupt_stored_values_are_reported_not_trusted() {
    use russh_bbs_core::access::MemberDirectory as _;
    use russh_bbs_core::access::Slug;
    use russh_bbs_core::sanitize::sanitize_username;
    use russh_bbs_core::store::{AccountState, NewAccount};
    let h = PgHarness::fresh().await;
    let store = h.store();
    let alice = store
        .create_account(NewAccount {
            username: sanitize_username("alice").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    let key = suite::fingerprint(1);
    store.attach_fingerprint(alice.id, &key).await.unwrap();
    sqlx::query("UPDATE accounts SET username = 'bad name'")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store.account_by_fingerprint(&key).await;
    assert!(matches!(result, Err(StoreError::Corrupt(_))), "{result:?}");
    assert!(!store.is_member(&key).await, "corrupt row must fail closed");
}

#[tokio::test]
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
async fn a_terminal_setting_the_code_could_not_have_written_is_corrupt_not_a_default() {
    use russh_bbs_core::access::Slug;
    use russh_bbs_core::sanitize::sanitize_username;
    use russh_bbs_core::store::{AccountState, NewAccount};
    use russh_bbs_core::terminal_settings::TerminalSettingsStore as _;
    let h = PgHarness::fresh().await;
    let store = h.store();
    let alice = store
        .create_account(NewAccount {
            username: sanitize_username("alice").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    // The CHECK allows 5; core's range (20-500) does not.
    sqlx::query(
        "INSERT INTO terminal_settings (account_id, width_override, color, pause) \
         VALUES ($1, 5, true, true)",
    )
    .bind(alice.id.as_raw())
    .execute(store.pool())
    .await
    .unwrap();
    let result = store.terminal_settings(alice.id).await;
    assert!(matches!(result, Err(StoreError::Corrupt(_))), "{result:?}");
}

#[tokio::test]
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
async fn the_database_refuses_an_unknown_tier() {
    use russh_bbs_core::access::Slug;
    use russh_bbs_core::sanitize::sanitize_username;
    use russh_bbs_core::store::{AccountState, NewAccount};
    let h = PgHarness::fresh().await;
    let store = h.store();
    let alice = store
        .create_account(NewAccount {
            username: sanitize_username("alice").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    let result = sqlx::query(
        "INSERT INTO terminal_settings (account_id, color, pause, icon_tier) \
         VALUES ($1, true, true, 'emoji')",
    )
    .bind(alice.id.as_raw())
    .execute(store.pool())
    .await;
    assert!(result.is_err());
}

#[tokio::test]
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
async fn deleting_an_account_deletes_its_dependent_rows() {
    use russh_bbs_core::access::Slug;
    use russh_bbs_core::sanitize::sanitize_username;
    use russh_bbs_core::store::{AccountState, NewAccount};
    use russh_bbs_core::terminal_settings::{TerminalSettings, TerminalSettingsStore as _};
    let h = PgHarness::fresh().await;
    let store = h.store();
    let alice = store
        .create_account(NewAccount {
            username: sanitize_username("alice").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    store
        .attach_fingerprint(alice.id, &suite::fingerprint(1))
        .await
        .unwrap();
    store
        .set_terminal_settings(alice.id, &TerminalSettings::default().safe_fallback())
        .await
        .unwrap();
    store.record_login(alice.id).await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE id = $1")
        .bind(alice.id.as_raw())
        .execute(store.pool())
        .await
        .unwrap();
    for table in ["terminal_settings", "account_logins", "account_keys"] {
        let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
}

#[tokio::test]
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
async fn timestamps_are_integer_unix_seconds_set_by_the_database() {
    use russh_bbs_core::access::Slug;
    use russh_bbs_core::sanitize::sanitize_username;
    use russh_bbs_core::store::{AccountState, NewAccount};
    let h = PgHarness::fresh().await;
    let alice = h
        .store()
        .create_account(NewAccount {
            username: sanitize_username("alice").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let created = u64::try_from(alice.created_at.as_raw()).unwrap();
    assert!(
        created.abs_diff(now) < 120,
        "created {created} vs now {now}"
    );
}

// ---- URL handling and secrets: no server needed ---------------------------------------------

const SECRET: &str = "hunter2-very-secret";

fn secret_url(extra: &str) -> String {
    format!("postgres://bbs:{SECRET}@db.example:5432/bbs{extra}")
}

#[test]
fn debug_output_hides_the_url() {
    let settings = PostgresSettings::new(secret_url(""), TlsMode::VerifyFull, None);
    let text = format!("{settings:?}");
    assert!(
        !text.contains(SECRET) && !text.contains("db.example"),
        "{text}"
    );
}

#[test]
fn tls_is_forced_whatever_the_url_says() {
    // No sslmode in the URL: the configured mode is applied.
    let options = PostgresSettings::new(secret_url(""), TlsMode::VerifyFull, None)
        .connect_options()
        .unwrap();
    assert!(matches!(
        options.get_ssl_mode(),
        sqlx::postgres::PgSslMode::VerifyFull
    ));
    // The same mode in the URL is fine.
    PostgresSettings::new(secret_url("?sslmode=require"), TlsMode::Require, None)
        .connect_options()
        .unwrap();
    // A weaker or different mode in the URL is refused, not overridden silently.
    for weaker in ["disable", "allow", "prefer", "verify-ca"] {
        let result = PostgresSettings::new(
            secret_url(&format!("?sslmode={weaker}")),
            TlsMode::VerifyFull,
            None,
        )
        .connect_options();
        let Err(OpenError::PostgresSettings(message)) = result else {
            panic!("sslmode={weaker} should be refused");
        };
        assert!(
            message.contains("tls_mode") && !message.contains(SECRET),
            "{message}"
        );
    }
}

#[test]
fn a_unix_socket_is_refused_because_tls_is_required() {
    let result = PostgresSettings::new(
        format!("postgres://bbs:{SECRET}@localhost/bbs?host=/var/run/postgresql"),
        TlsMode::Require,
        None,
    )
    .connect_options();
    assert!(
        matches!(result, Err(OpenError::PostgresSettings(_))),
        "{result:?}"
    );
}

#[test]
fn a_malformed_url_gives_a_settings_error_without_the_password() {
    for bad in [
        format!("postgres://bbs:{SECRET}@db.example:notaport/bbs"),
        format!("postgres://bbs:{SECRET}@db.example:5432/bbs?sslmode=sometimes"),
    ] {
        let error = PostgresSettings::new(bad, TlsMode::Require, None)
            .connect_options()
            .unwrap_err();
        let text = format!("{error} / {error:?}");
        assert!(matches!(error, OpenError::PostgresSettings(_)), "{text}");
        assert!(!text.contains(SECRET), "{text}");
    }
}

#[test]
fn scrub_removes_the_password_from_any_text() {
    let settings = PostgresSettings::new(
        secret_url(&format!("?password={SECRET}2")),
        TlsMode::Require,
        None,
    );
    let text = settings.scrub(&format!("auth failed for {SECRET} and {SECRET}2"));
    assert!(!text.contains(SECRET), "{text}");
    assert_eq!(text, "auth failed for *** and ***");
}

#[tokio::test]
async fn a_bad_host_is_a_clear_boot_error_without_the_password() {
    // Port 1 on localhost: nothing listens, so this fails fast and needs no server.
    let settings = PostgresSettings::new(
        format!("postgres://bbs:{SECRET}@127.0.0.1:1/bbs"),
        TlsMode::Require,
        None,
    );
    let error = PostgresStore::open(&settings).await.unwrap_err();
    let text = format!("{error} / {error:?}");
    assert!(matches!(error, OpenError::PostgresConnect(_)), "{text}");
    assert!(!text.contains(SECRET), "{text}");
}

#[tokio::test]
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
async fn require_never_falls_back_to_plaintext_on_a_server_without_tls() {
    // The test URL must point at a server that does not offer TLS for this to be meaningful;
    // when it does offer TLS the connection succeeds encrypted, which is also correct. What must
    // never happen is a successful *unencrypted* session.
    // Into a scratch database, so the shared one is never migrated (QA M2 L1).
    let h = PgHarness::fresh().await;
    let url = env("RUSSH_BBS_TEST_POSTGRES_URL");
    let bare = url_with_database(url.split('?').next().unwrap(), &h.name);
    match PostgresStore::open(&PostgresSettings::new(bare, TlsMode::Require, None)).await {
        Ok(store) => {
            let encrypted: bool = sqlx::query_scalar(
                "SELECT COALESCE((SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()), false)",
            )
            .fetch_one(store.pool())
            .await
            .unwrap();
            assert!(
                encrypted,
                "a TLS-required open produced a plaintext session"
            );
        }
        Err(error) => assert!(matches!(error, OpenError::PostgresConnect(_)), "{error:?}"),
    }
}

/// Tests that need a Postgres that **requires** TLS. CI runs them in the `postgres-tls` job;
/// the plain-Postgres job skips this module with `--skip tls::`.
mod tls {
    use super::*;

    fn tls_settings(mode: TlsMode, with_root: bool) -> PostgresSettings {
        tls_settings_for(&env("RUSSH_BBS_TEST_POSTGRES_TLS_URL"), mode, with_root)
    }

    fn tls_settings_for(url: &str, mode: TlsMode, with_root: bool) -> PostgresSettings {
        let root: Option<PathBuf> =
            with_root.then(|| PathBuf::from(env("RUSSH_BBS_TEST_POSTGRES_ROOT_CERT")));
        PostgresSettings::new(url, mode, root)
    }

    /// A scratch database on the TLS-required server, created over TLS and dropped at the end,
    /// so these tests never migrate the shared `postgres` database (QA M2 L1).
    struct TlsScratch {
        admin: PgConnectOptions,
        name: String,
        url: String,
    }

    impl TlsScratch {
        async fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let base = env("RUSSH_BBS_TEST_POSTGRES_TLS_URL");
            let admin = PgConnectOptions::from_str(&base)
                .unwrap()
                .ssl_mode(sqlx::postgres::PgSslMode::Require);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos();
            let name = format!(
                "bbs_tls_test_{}_{}_{nanos}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let mut conn = PgConnection::connect_with(&admin).await.unwrap();
            // The name is generated above from digits and underscores only.
            conn.execute(format!("CREATE DATABASE {name}").as_str())
                .await
                .unwrap();
            conn.close().await.unwrap();
            let url = super::url_with_database(&base, &name);
            Self { admin, name, url }
        }

        fn settings(&self, mode: TlsMode, with_root: bool) -> PostgresSettings {
            tls_settings_for(&self.url, mode, with_root)
        }
    }

    impl Drop for TlsScratch {
        fn drop(&mut self) {
            let (admin, name) = (self.admin.clone(), self.name.clone());
            let _ = std::thread::spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async {
                    if let Ok(mut conn) = PgConnection::connect_with(&admin).await {
                        let _ = conn
                            .execute(
                                format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str(),
                            )
                            .await;
                    }
                });
            })
            .join();
        }
    }

    #[tokio::test]
    #[ignore = "needs a TLS-required Postgres: set RUSSH_BBS_TEST_POSTGRES_TLS_URL and _ROOT_CERT"]
    async fn verify_full_with_the_root_cert_connects_over_tls_and_migrates() {
        let scratch = TlsScratch::new().await;
        let store = PostgresStore::open(&scratch.settings(TlsMode::VerifyFull, true))
            .await
            .unwrap();
        let encrypted: bool =
            sqlx::query_scalar("SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert!(encrypted, "the session must be encrypted");
        assert_eq!(
            store
                .get_setting(SettingKey::RegistrationMode)
                .await
                .unwrap()
                .as_deref(),
            Some("invite")
        );
        store.close().await;
    }

    #[tokio::test]
    #[ignore = "needs a TLS-required Postgres: set RUSSH_BBS_TEST_POSTGRES_TLS_URL and _ROOT_CERT"]
    async fn require_connects_over_tls_without_a_root_cert() {
        let scratch = TlsScratch::new().await;
        let store = PostgresStore::open(&scratch.settings(TlsMode::Require, false))
            .await
            .unwrap();
        let encrypted: bool =
            sqlx::query_scalar("SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert!(encrypted);
        store.close().await;
    }

    #[tokio::test]
    #[ignore = "needs a TLS-required Postgres: set RUSSH_BBS_TEST_POSTGRES_TLS_URL and _ROOT_CERT"]
    async fn verify_full_without_the_root_cert_refuses_an_untrusted_certificate() {
        let url = env("RUSSH_BBS_TEST_POSTGRES_TLS_URL");
        let error = PostgresStore::open(&tls_settings(TlsMode::VerifyFull, false))
            .await
            .unwrap_err();
        let text = format!("{error}");
        assert!(matches!(error, OpenError::PostgresConnect(_)), "{text}");
        assert!(!text.contains(password_of(&url)), "{text}");
    }

    #[tokio::test]
    #[ignore = "needs a TLS-required Postgres: set RUSSH_BBS_TEST_POSTGRES_TLS_URL and _ROOT_CERT"]
    async fn a_wrong_password_is_a_connect_error_that_does_not_show_it() {
        let url = env("RUSSH_BBS_TEST_POSTGRES_TLS_URL");
        let bad = url.replace(password_of(&url), "wrong-password-xyz");
        let root = PathBuf::from(env("RUSSH_BBS_TEST_POSTGRES_ROOT_CERT"));
        let error =
            PostgresStore::open(&PostgresSettings::new(bad, TlsMode::VerifyFull, Some(root)))
                .await
                .unwrap_err();
        let text = format!("{error}");
        assert!(matches!(error, OpenError::PostgresConnect(_)), "{text}");
        assert!(!text.contains("wrong-password-xyz"), "{text}");
    }

    /// The password in a `postgres://user:password@host` URL.
    fn password_of(url: &str) -> &str {
        url.split_once("://")
            .and_then(|(_, rest)| rest.split_once('@'))
            .and_then(|(userinfo, _)| userinfo.split_once(':'))
            .map_or("", |(_, password)| password)
    }
}

/// Things only Postgres can be asked: `TRUNCATE` does not fire row triggers, and an upsert by id
/// is an UPDATE.
#[tokio::test]
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
async fn audit_log_refuses_truncate_and_upsert_by_id() {
    use russh_bbs_core::audit::{Action, Actor, AuditEvent, AuditQuery, AuditStore, Target};

    let h = PgHarness::fresh().await;
    let store = h.store();
    let original =
        AuditEvent::new(Actor::System, Action::GroupCreated, Target::None).detail("orig");
    let id = store.append(&original).await.unwrap();
    for sql in [
        "TRUNCATE audit_log".to_owned(),
        "TRUNCATE audit_log RESTART IDENTITY CASCADE".to_owned(),
        format!(
            "INSERT INTO audit_log (id, at, actor_kind, action) OVERRIDING SYSTEM VALUE \
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

/// Postgres only: `TRUNCATE` does not fire row triggers, and an upsert by id is an UPDATE.
#[tokio::test]
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
async fn credit_ledger_refuses_truncate_and_upsert_by_id() {
    use russh_bbs_core::access::Slug;
    use russh_bbs_core::credits::{Amount, Movement, balance, post};
    use russh_bbs_core::sanitize::sanitize_username;
    use russh_bbs_core::store::{AccountState, AccountStore, NewAccount};

    let h = PgHarness::fresh().await;
    let store = h.store();
    let a = store
        .create_account(NewAccount {
            username: sanitize_username("ann").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    let why = Slug::parse("test-grant").unwrap();
    let posted = post(store, a.id, Movement::Earn(Amount::new(10).unwrap()), &why)
        .await
        .unwrap();
    for sql in [
        "TRUNCATE credit_ledger".to_owned(),
        "TRUNCATE credit_ledger RESTART IDENTITY CASCADE".to_owned(),
        format!(
            "INSERT INTO credit_ledger (id, account_id, at, delta, reason) OVERRIDING SYSTEM VALUE \
             VALUES ({}, 1, 1, 999, 'x') ON CONFLICT (id) DO UPDATE SET delta = excluded.delta",
            posted.id.as_raw()
        ),
    ] {
        let error = sqlx::query(&sql).execute(store.pool()).await.unwrap_err();
        assert!(error.to_string().contains("append-only"), "{sql}: {error}");
    }
    assert_eq!(balance(store, a.id).await.unwrap(), 10);
}

// ---- migration 0007 (M2-D) --------------------------------------------------------------

/// A database as the code before M2-D left it, filled with rows from every earlier slice (the
/// same fixture as SQLite's), then opened by the current code: every row and id survives, the
/// `role` column is gone, `invited_by` is empty, and the foreign keys still act.
#[tokio::test]
#[ignore = "needs a Postgres server: set RUSSH_BBS_TEST_POSTGRES_URL"]
#[allow(clippy::too_many_lines)] // one story: seed, migrate, compare, then use the result
async fn the_invites_migration_keeps_every_row_id_and_dependent_table() {
    use crate::migration_fixture::{DUMPS, KEY_ALICE, NEXT_ACCOUNT_ID, SEED};
    use russh_bbs_core::access::{AccessStore as _, Fingerprint, Slug};
    use russh_bbs_core::sanitize::sanitize_username;
    use russh_bbs_core::store::{AccountState, NewAccount};

    let h = PgHarness::fresh().await;
    let mut conn = PgConnection::connect_with(&h.options).await.unwrap();
    conn.execute("DROP SCHEMA public CASCADE").await.unwrap();
    conn.execute("CREATE SCHEMA public").await.unwrap();
    let mut older = Migrator::DEFAULT;
    older.migrations =
        std::borrow::Cow::Owned(MIGRATOR.iter().filter(|m| m.version < 7).cloned().collect());
    older.run(&mut conn).await.unwrap();
    for sql in SEED {
        conn.execute(*sql).await.unwrap();
    }
    let mut before = Vec::new();
    for (name, sql) in DUMPS {
        let rows: Vec<String> = sqlx::query_scalar(sql).fetch_all(&mut conn).await.unwrap();
        before.push((*name, rows));
    }
    // The forcing check: every table that references `accounts` must have rows in the fixture
    // (a slice that adds one adds a seed row and a read), or the comparison proves nothing
    // about it.
    let referencing: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT c.conrelid::regclass::text FROM pg_constraint c \
         WHERE c.contype = 'f' AND c.confrelid = 'accounts'::regclass ORDER BY 1",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    for table in referencing {
        let rows = before.iter().find(|(name, _)| *name == table);
        assert!(
            rows.is_some_and(|(_, rows)| !rows.is_empty()),
            "table `{table}` references accounts but the migration fixture has no rows for it: \
             add a seed row to migration_fixture::SEED and a read to DUMPS"
        );
    }
    conn.close().await.unwrap();

    let url = env("RUSSH_BBS_TEST_POSTGRES_URL");
    let store = open_test_database(h.options.clone(), &url).await;
    let mut after = Vec::new();
    for (name, sql) in DUMPS {
        let rows: Vec<String> = sqlx::query_scalar(sql)
            .fetch_all(store.pool())
            .await
            .unwrap();
        after.push((*name, rows));
    }
    assert_eq!(
        after, before,
        "a row changed, vanished or was cascaded away"
    );

    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = 'accounts' ORDER BY ordinal_position",
    )
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

    let alice = store
        .account_by_fingerprint(&Fingerprint::parse(KEY_ALICE).unwrap())
        .await
        .unwrap()
        .unwrap();
    let snapshot = store.snapshot(Some(alice.id)).await.unwrap();
    assert_eq!(snapshot.top_role(), Some(&Slug::SYSOP));

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
    let keys: i64 = sqlx::query_scalar("SELECT count(*) FROM account_keys")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(keys, 2);
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert!(audit >= 3);
}
