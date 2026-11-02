-- M2-H (Postgres counterpart of the SQLite 0006 migration, D24): resource limits per role with a per-member override, and per-day usage counters
-- (docs/access.md "Resource limits", D62). Forward-only: once merged this file is never edited.
-- No limit columns on `role` (access.md plan section 7): limits have their own tables, keyed
-- by `role.name` and `accounts.id`. Numbers and names only; no secrets.
--
-- `value` NULL means unlimited. A role with no row for a kind has no value for it (the resolver
-- skips it); `sysop` is unlimited in code whatever these rows say.

CREATE TABLE role_limit (
    role  TEXT NOT NULL REFERENCES role (name) ON DELETE CASCADE,
    kind  TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 32),
    value BIGINT CHECK (value IS NULL OR value >= 0),
    PRIMARY KEY (role, kind)
);

-- One account's override of a kind. It replaces the roles' values for that account.
CREATE TABLE member_limit (
    account_id BIGINT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 32),
    value      BIGINT CHECK (value IS NULL OR value >= 0),
    PRIMARY KEY (account_id, kind)
);

-- What an account has used, per UTC day (`day` = days since 1970-01-01), so a restart does not
-- reset it. Minutes are stored as seconds. Rows older than 62 days are deleted at sign-in.
CREATE TABLE limit_usage (
    account_id BIGINT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 32),
    day        BIGINT NOT NULL,
    amount     BIGINT NOT NULL DEFAULT 0 CHECK (amount >= 0),
    PRIMARY KEY (account_id, kind, day)
);

-- The permission to edit limits: `sysop` (mandatory) and `cosysop`.
INSERT INTO role_permission (role, permission) VALUES
    ('sysop', 'limits.edit'),
    ('cosysop', 'limits.edit');

-- Conservative defaults for the built-in roles (listed in docs/access.md and docs/config.md).
-- NULL is unlimited. `waiting` gets none: it never signs in.
INSERT INTO role_limit (role, kind, value) VALUES
    ('member', 'minutes_per_day', 60),
    ('member', 'calls_per_day', 5),
    ('member', 'max_sessions', 2),
    ('member', 'posts_per_hour', 20),
    ('member', 'upload_bytes_per_day', 10485760),
    ('member', 'upload_bytes_per_file', 5242880),
    ('member', 'mail_per_day', 20),
    ('member', 'invites_per_month', 2),
    ('trusted', 'minutes_per_day', 120),
    ('trusted', 'calls_per_day', 10),
    ('trusted', 'max_sessions', 3),
    ('trusted', 'posts_per_hour', 40),
    ('trusted', 'upload_bytes_per_day', 52428800),
    ('trusted', 'upload_bytes_per_file', 10485760),
    ('trusted', 'mail_per_day', 50),
    ('trusted', 'invites_per_month', 5),
    ('moderator', 'minutes_per_day', 240),
    ('moderator', 'calls_per_day', 20),
    ('moderator', 'max_sessions', 4),
    ('moderator', 'posts_per_hour', 100),
    ('moderator', 'upload_bytes_per_day', 104857600),
    ('moderator', 'upload_bytes_per_file', 26214400),
    ('moderator', 'mail_per_day', 100),
    ('moderator', 'invites_per_month', 10),
    ('cosysop', 'minutes_per_day', NULL),
    ('cosysop', 'calls_per_day', NULL),
    ('cosysop', 'max_sessions', 8),
    ('cosysop', 'posts_per_hour', 200),
    ('cosysop', 'upload_bytes_per_day', 1073741824),
    ('cosysop', 'upload_bytes_per_file', 104857600),
    ('cosysop', 'mail_per_day', 500),
    ('cosysop', 'invites_per_month', 50),
    ('sysop', 'minutes_per_day', NULL),
    ('sysop', 'calls_per_day', NULL),
    ('sysop', 'max_sessions', NULL),
    ('sysop', 'posts_per_hour', NULL),
    ('sysop', 'upload_bytes_per_day', NULL),
    ('sysop', 'upload_bytes_per_file', NULL),
    ('sysop', 'mail_per_day', NULL),
    ('sysop', 'invites_per_month', NULL);
