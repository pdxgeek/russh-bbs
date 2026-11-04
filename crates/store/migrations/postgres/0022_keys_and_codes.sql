-- FIX-M3-11: the Postgres counterpart of SQLite's 0022 (D24: one migration set per backend). Same
-- tables and constraints, Postgres-native types. Forward-only: once merged this file is never
-- edited (see src/postgres/tests.rs). See the SQLite file for what each table is for.
ALTER TABLE account_keys ADD COLUMN last_used_at BIGINT;

CREATE TABLE login_code (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind        TEXT   NOT NULL CHECK (kind IN ('link', 'recovery')),
    code_hash   TEXT   NOT NULL UNIQUE CHECK (length(code_hash) = 64),
    account_id  BIGINT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    issued_by   BIGINT REFERENCES accounts (id) ON DELETE SET NULL,
    created_at  BIGINT NOT NULL,
    expires_at  BIGINT NOT NULL CHECK (expires_at > created_at),
    redeemed_at BIGINT,
    voided_at   BIGINT
);
CREATE INDEX login_code_by_account ON login_code (account_id, kind, created_at);

CREATE TABLE key_notice (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account_id  BIGINT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    kind        TEXT   NOT NULL CHECK (kind IN
                    ('key_added', 'key_removed', 'key_linked', 'device_linked', 'recovered')),
    fingerprint TEXT,
    at          BIGINT NOT NULL,
    seen_at     BIGINT
);
CREATE INDEX key_notice_unseen ON key_notice (account_id, id) WHERE seen_at IS NULL;

INSERT INTO role_permission (role, permission) VALUES
    ('member',    'keys.manage'),
    ('trusted',   'keys.manage'),
    ('moderator', 'keys.manage'),
    ('cosysop',   'keys.manage'),
    ('sysop',     'keys.manage'),
    ('waiting',   'keys.redeem'),
    ('sysop',     'keys.redeem'),
    ('cosysop',   'keys.recover'),
    ('sysop',     'keys.recover');
