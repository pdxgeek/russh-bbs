//! A database as the code before M2-D left it, and a way to read all of it back, shared by both
//! backends' tests of migration `0007_invites` (which drops the `accounts.role` column and
//! adds the invite tables). The fixture carries rows from every earlier slice: M1 accounts
//! (a claimed sysop, a member, a pending and a suspended account), their keys, terminal settings
//! and login times; M2-B roles, grants (account, permission and scoped), a custom group with a
//! member and its closure; M2-C audit rows; board settings. It also burns an account id, because
//! ids are never reused (D35).
//!
//! The SQL is portable across SQLite and Postgres on purpose: no explicit ids on identity
//! columns, `CAST(.. AS TEXT)` and `||` for the dump.

/// Rows to insert after migrations `< 7` have run. Uses the legacy `role` column.
pub(crate) const SEED: &[&str] = &[
    "INSERT INTO accounts (username, username_skeleton, role, state, created_at) VALUES \
     ('alice', 'alice', 'sysop', 'active', 100), \
     ('bob', 'bob', 'member', 'active', 200), \
     ('carol', 'carol', 'member', 'pending', 300), \
     ('dave', 'dave', 'member', 'suspended', 400), \
     ('ghost', 'ghost', 'member', 'active', 500)",
    "DELETE FROM accounts WHERE username = 'ghost'",
    "INSERT INTO account_keys (fingerprint, account_id, created_at) VALUES \
     ('SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA', 1, 100), \
     ('SHA256:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB', 2, 200), \
     ('SHA256:DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD', 4, 400)",
    "INSERT INTO terminal_settings (account_id, color, pause, icon_tier) \
     SELECT 1, true, false, 'ascii' WHERE true",
    "INSERT INTO account_logins (account_id, last_login_at, previous_login_at) VALUES (2, 900, 800)",
    "INSERT INTO access_grant (account_id, role) VALUES (1, 'sysop'), (2, 'moderator')",
    "INSERT INTO access_grant (account_id, permission) VALUES (2, 'invites.view_tree')",
    "INSERT INTO access_grant (account_id, role, resource_kind, resource_id) \
     VALUES (3, 'member', 'forum', 7)",
    "INSERT INTO member_group (name, kind) VALUES ('crew', 'custom')",
    "INSERT INTO group_member (group_id, account_id) \
     SELECT id, 2 FROM member_group WHERE name = 'crew'",
    "INSERT INTO group_closure (group_id, account_id) \
     SELECT id, 2 FROM member_group WHERE name = 'crew'",
    "INSERT INTO profiles (account_id, display_name, pronouns, location, about, website, updated_at) \
     VALUES (2, 'Bobby', 'they/them', 'Portland', 'hello there', 'https://example.org', 950)",
    "INSERT INTO member_limit (account_id, kind, value) VALUES (2, 'invites_per_month', 9), (2, 'mail_per_day', NULL)",
    "INSERT INTO limit_usage (account_id, kind, day, amount) VALUES (2, 'calls_per_day', 20000, 3), (1, 'minutes_per_day', 20000, 1800)",
    "INSERT INTO audit_log (at, actor_kind, actor_id, action, target_kind, target_ref, detail) VALUES \
     (100, 'account', 1, 'founder.claimed', 'account', '1', 'role sysop'), \
     (200, 'system', NULL, 'account.created', 'account', '2', 'role member state active'), \
     (210, 'account', 1, 'grant.added', 'account', '2', 'role moderator')",
    "UPDATE settings SET value = 'apply' WHERE key = 'registration_mode'",
    "INSERT INTO settings (key, value) VALUES ('waiting_room_banner', 'hello')",
];

/// Every table that existed before M2-D, read in a fixed order as `|`-joined text. The
/// `accounts` dump leaves out `role` (dropped) and `invited_by` (new), so it must be equal
/// before and after.
pub(crate) const DUMPS: &[(&str, &str)] = &[
    (
        "accounts",
        "SELECT CAST(id AS TEXT) || '|' || username || '|' || username_skeleton || '|' || state \
         || '|' || CAST(created_at AS TEXT) FROM accounts ORDER BY id",
    ),
    (
        "account_keys",
        "SELECT fingerprint || '|' || CAST(account_id AS TEXT) || '|' || CAST(created_at AS TEXT) \
         FROM account_keys ORDER BY fingerprint",
    ),
    (
        "terminal_settings",
        "SELECT CAST(account_id AS TEXT) || '|' || COALESCE(icon_tier, '-') \
         FROM terminal_settings ORDER BY account_id",
    ),
    (
        "account_logins",
        "SELECT CAST(account_id AS TEXT) || '|' || CAST(last_login_at AS TEXT) || '|' \
         || COALESCE(CAST(previous_login_at AS TEXT), '-') FROM account_logins ORDER BY account_id",
    ),
    (
        "access_grant",
        "SELECT CAST(id AS TEXT) || '|' || COALESCE(CAST(account_id AS TEXT), '-') || '|' \
         || COALESCE(CAST(group_id AS TEXT), '-') || '|' || COALESCE(role, '-') || '|' \
         || COALESCE(permission, '-') || '|' || resource_kind || '|' || CAST(resource_id AS TEXT) \
         FROM access_grant ORDER BY id",
    ),
    (
        "member_group",
        "SELECT CAST(id AS TEXT) || '|' || name || '|' || kind FROM member_group ORDER BY id",
    ),
    (
        "group_member",
        "SELECT CAST(group_id AS TEXT) || '|' || COALESCE(CAST(account_id AS TEXT), '-') || '|' \
         || COALESCE(CAST(child_group_id AS TEXT), '-') FROM group_member ORDER BY 1",
    ),
    (
        "group_closure",
        "SELECT CAST(group_id AS TEXT) || '|' || CAST(account_id AS TEXT) \
         FROM group_closure ORDER BY 1",
    ),
    (
        "profiles",
        "SELECT CAST(account_id AS TEXT) || '|' || display_name || '|' || pronouns || '|' \
         || location || '|' || about || '|' || website || '|' || CAST(updated_at AS TEXT) \
         FROM profiles ORDER BY account_id",
    ),
    (
        "member_limit",
        "SELECT CAST(account_id AS TEXT) || '|' || kind || '|' \
         || COALESCE(CAST(value AS TEXT), '-') FROM member_limit ORDER BY 1",
    ),
    (
        "limit_usage",
        "SELECT CAST(account_id AS TEXT) || '|' || kind || '|' || CAST(day AS TEXT) || '|' \
         || CAST(amount AS TEXT) FROM limit_usage ORDER BY 1",
    ),
    (
        "role_limit",
        "SELECT role || '|' || kind || '|' || COALESCE(CAST(value AS TEXT), '-') \
         FROM role_limit ORDER BY 1",
    ),
    (
        "role",
        "SELECT name || '|' || CAST(rank AS TEXT) || '|' || kind FROM role ORDER BY name",
    ),
    (
        "role_permission",
        // Rows that later migrations add are left out (0009 gave `waiting` and `sysop`
        // `invite.request`, 0010 `membership.open`, 0019 `mail.report` and `mail.moderate`): the fixture compares what existed before 0007 with what is there
        // after it. Add the name of a permission here when a migration adds its rows.
        "SELECT role || '|' || permission FROM role_permission \
         WHERE permission NOT IN ('invite.request', 'membership.open', 'mail.report', 'mail.moderate') \
         ORDER BY 1",
    ),
    (
        "settings",
        "SELECT key || '|' || value FROM settings ORDER BY key",
    ),
    (
        "audit_log",
        "SELECT CAST(id AS TEXT) || '|' || CAST(at AS TEXT) || '|' || actor_kind || '|' \
         || COALESCE(CAST(actor_id AS TEXT), '-') || '|' || action || '|' || target_kind || '|' \
         || target_ref || '|' || resource_kind || '|' || CAST(resource_id AS TEXT) || '|' || detail \
         FROM audit_log ORDER BY id",
    ),
];

/// The fingerprints the fixture binds, as `Fingerprint::parse` accepts them.
pub(crate) const KEY_ALICE: &str = "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
/// See [`KEY_ALICE`]: bob, a member holding `moderator` through a grant.
pub(crate) const KEY_BOB: &str = "SHA256:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
/// See [`KEY_ALICE`]: dave, suspended.
pub(crate) const KEY_DAVE: &str = "SHA256:DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD";

/// The first account id that must be handed out after the fixture: the highest ever issued
/// (the burned `ghost` row) plus one.
pub(crate) const NEXT_ACCOUNT_ID: i64 = 6;
